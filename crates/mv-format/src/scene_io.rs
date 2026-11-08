//! Multi-model `.vox` support: `SIZE`/`XYZI` pairs plus the scene graph chunks
//! `nTRN` (transform), `nGRP` (group), `nSHP` (shape) and `LAYR` (layer).
//!
//! Chunk layouts (MagicaVoxel `.vox` extension spec):
//! - `nTRN`: node id, attributes dict (`_name`, `_hidden`), child node id, reserved (-1),
//!   layer id, frame count, then one dict per frame (`_r` rotation byte, `_t` "x y z").
//! - `nGRP`: node id, attributes dict, child count, child node ids.
//! - `nSHP`: node id, attributes dict, model count, then (model id, dict) per model.
//! - `LAYR`: layer id, attributes dict (`_name`, `_hidden`, `_color` "r g b"), reserved (-1).
//!
//! Only the first animation frame of transforms and shapes is kept for now.

use std::collections::{BTreeMap, HashMap, HashSet};

use glam::IVec3;
use mv_core::{EditorProject, NodeId, NodeKind, Rotation, Scene, SceneLayer, Transform, VoxelGrid};

use crate::{read_i32_le, read_vox_string, write_chunk, write_vox_string, FormatError};

/// Guards against malicious/cyclic graphs.
const MAX_GRAPH_DEPTH: usize = 64;

type Dict = HashMap<String, String>;

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

/// Append `SIZE`/`XYZI` for every model referenced by the scene, then the scene graph
/// and layers. The active model is taken from `project.grid` (the live edited copy).
pub(crate) fn write_models_and_scene(project: &EditorProject, out: &mut Vec<u8>) {
    let scene = &project.scene;

    // Models referenced by at least one shape, in model-id order.
    let mut referenced: Vec<usize> = scene
        .nodes()
        .filter_map(|(_, n)| n.model_id())
        .filter(|&m| project.model(m).is_some())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let has_graph = !referenced.is_empty();
    if !has_graph {
        // Degenerate scene without shapes: still save the edited grid as a plain model.
        referenced.push(project.active_model);
    }
    let remap: BTreeMap<usize, i32> = referenced
        .iter()
        .enumerate()
        .map(|(new, &old)| (old, new as i32))
        .collect();

    for &m in &referenced {
        let grid = project.model(m).unwrap_or(&project.grid);
        write_model(grid, out);
    }
    if !has_graph {
        return;
    }

    let mut next_id: i32 = 0;
    emit_node(scene, scene.root(), true, &remap, &mut next_id, out);

    for (i, layer) in scene.layers.iter().enumerate() {
        let mut content = Vec::new();
        content.extend_from_slice(&(i as i32).to_le_bytes());
        let mut attrs = vec![("_name", layer.name.clone())];
        if layer.hidden {
            attrs.push(("_hidden", "1".to_string()));
        }
        let [r, g, b] = layer.color;
        attrs.push(("_color", format!("{r} {g} {b}")));
        write_dict(&mut content, &attrs);
        content.extend_from_slice(&(-1i32).to_le_bytes());
        write_chunk(out, b"LAYR", &content, &[]);
    }
}

fn write_model(grid: &VoxelGrid, out: &mut Vec<u8>) {
    let d = grid.dimensions();
    let mut size = Vec::with_capacity(12);
    for v in [d.x, d.y, d.z] {
        size.extend_from_slice(&v.to_le_bytes());
    }
    write_chunk(out, b"SIZE", &size, &[]);

    let mut xyzi = vec![0u8; 4];
    let mut count: u32 = 0;
    for z in 0..d.z {
        for y in 0..d.y {
            for x in 0..d.x {
                let c = grid.get(x, y, z);
                if c != 0 {
                    xyzi.extend_from_slice(&[x as u8, y as u8, z as u8, c]);
                    count += 1;
                }
            }
        }
    }
    xyzi[0..4].copy_from_slice(&count.to_le_bytes());
    write_chunk(out, b"XYZI", &xyzi, &[]);
}

fn write_dict(out: &mut Vec<u8>, pairs: &[(&str, String)]) {
    out.extend_from_slice(&(pairs.len() as i32).to_le_bytes());
    for (k, v) in pairs {
        write_vox_string(out, k);
        write_vox_string(out, v);
    }
}

/// Emit `nTRN` + its child (`nGRP` or `nSHP`) for a node, recursively. Returns the
/// `nTRN` id, which is what a parent group lists as its child.
fn emit_node(
    scene: &Scene,
    id: NodeId,
    is_root: bool,
    remap: &BTreeMap<usize, i32>,
    next_id: &mut i32,
    out: &mut Vec<u8>,
) -> i32 {
    let node = scene.node(id).expect("emit_node called on a live node");
    let trn_id = *next_id;
    let child_id = trn_id + 1;
    *next_id += 2;

    // nTRN
    let mut trn = Vec::new();
    trn.extend_from_slice(&trn_id.to_le_bytes());
    let mut attrs = Vec::new();
    if let Some(name) = &node.name {
        attrs.push(("_name", name.clone()));
    }
    if node.hidden {
        attrs.push(("_hidden", "1".to_string()));
    }
    write_dict(&mut trn, &attrs);
    trn.extend_from_slice(&child_id.to_le_bytes());
    trn.extend_from_slice(&(-1i32).to_le_bytes());
    let layer: i32 = if is_root { -1 } else { node.layer_id as i32 };
    trn.extend_from_slice(&layer.to_le_bytes());
    trn.extend_from_slice(&1i32.to_le_bytes());
    let mut frame = Vec::new();
    let xf = node.transform;
    if xf.rotation != Rotation::IDENTITY {
        frame.push(("_r", xf.rotation.to_vox_byte().to_string()));
    }
    if xf.translation != IVec3::ZERO {
        let t = xf.translation;
        frame.push(("_t", format!("{} {} {}", t.x, t.y, t.z)));
    }
    write_dict(&mut trn, &frame);
    write_chunk(out, b"nTRN", &trn, &[]);

    match &node.kind {
        NodeKind::Group { children } => {
            // Children first reserve their ids; the group chunk lists them.
            let mut body = Vec::new();
            let child_trn_ids: Vec<i32> = children
                .iter()
                .map(|&c| emit_node(scene, c, false, remap, next_id, &mut body))
                .collect();
            let mut grp = Vec::new();
            grp.extend_from_slice(&child_id.to_le_bytes());
            write_dict(&mut grp, &[]);
            grp.extend_from_slice(&(child_trn_ids.len() as i32).to_le_bytes());
            for c in child_trn_ids {
                grp.extend_from_slice(&c.to_le_bytes());
            }
            write_chunk(out, b"nGRP", &grp, &[]);
            out.extend_from_slice(&body);
        }
        NodeKind::Shape { model_id } => {
            let mut shp = Vec::new();
            shp.extend_from_slice(&child_id.to_le_bytes());
            write_dict(&mut shp, &[]);
            shp.extend_from_slice(&1i32.to_le_bytes());
            shp.extend_from_slice(&remap[model_id].to_le_bytes());
            write_dict(&mut shp, &[]);
            write_chunk(out, b"nSHP", &shp, &[]);
        }
    }
    trn_id
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

struct TrnChunk {
    attrs: Dict,
    child: i32,
    layer: i32,
    frame: Dict,
}

struct GrpChunk {
    children: Vec<i32>,
}

/// Collects model and scene chunks while `from_vox_bytes` walks the file.
#[derive(Default)]
pub(crate) struct SceneReader {
    models: Vec<VoxelGrid>,
    pending_size: Option<(u32, u32, u32)>,
    trns: HashMap<i32, TrnChunk>,
    grps: HashMap<i32, GrpChunk>,
    shps: HashMap<i32, i32>,
    layers: BTreeMap<i32, Dict>,
}

impl SceneReader {
    /// Handle a chunk if it belongs to models or the scene graph. Malformed scene
    /// chunks are skipped (the file still loads, possibly with fewer nodes).
    pub(crate) fn accept(&mut self, id: &[u8], content: &[u8]) -> bool {
        match id {
            b"SIZE" => self.read_size(content),
            b"XYZI" => self.read_xyzi(content),
            b"nTRN" => {
                let _ = self.read_trn(content);
            }
            b"nGRP" => {
                let _ = self.read_grp(content);
            }
            b"nSHP" => {
                let _ = self.read_shp(content);
            }
            b"LAYR" => {
                if let Ok((id, attrs, _)) = read_id_and_dict(content) {
                    self.layers.insert(id, attrs);
                }
            }
            _ => return false,
        }
        true
    }

    fn read_size(&mut self, content: &[u8]) {
        if content.len() >= 12 {
            let dim = |o: usize| {
                read_i32_le(content, o)
                    .map_or(1, |v| v.clamp(1, mv_core::MAX_GRID_DIM as i32) as u32)
            };
            self.pending_size = Some((dim(0), dim(4), dim(8)));
        }
    }

    fn read_xyzi(&mut self, content: &[u8]) {
        if content.len() < 4 {
            return;
        }
        let (sx, sy, sz) = self.pending_size.take().unwrap_or((40, 40, 40));
        let mut grid = VoxelGrid::new(sx, sy, sz);
        let declared = read_i32_le(content, 0).unwrap_or(0).max(0) as usize;
        let available = (content.len() - 4) / 4;
        for v in content[4..4 + declared.min(available) * 4].chunks_exact(4) {
            grid.set(v[0] as u32, v[1] as u32, v[2] as u32, v[3]);
        }
        self.models.push(grid);
    }

    fn read_trn(&mut self, content: &[u8]) -> Result<(), FormatError> {
        let (id, attrs, mut c) = read_id_and_dict(content)?;
        let child = read_i32_le(content, c)?;
        let layer = read_i32_le(content, c + 8)?;
        let frames = read_i32_le(content, c + 12)?;
        c += 16;
        let frame = if frames > 0 {
            read_dict(content, c)?.0
        } else {
            Dict::new()
        };
        self.trns.insert(
            id,
            TrnChunk {
                attrs,
                child,
                layer,
                frame,
            },
        );
        Ok(())
    }

    fn read_grp(&mut self, content: &[u8]) -> Result<(), FormatError> {
        let (id, _attrs, c) = read_id_and_dict(content)?;
        let n = read_i32_le(content, c)?.max(0) as usize;
        let children = (0..n)
            .map(|i| read_i32_le(content, c + 4 + i * 4))
            .collect::<Result<Vec<_>, _>>()?;
        self.grps.insert(id, GrpChunk { children });
        Ok(())
    }

    fn read_shp(&mut self, content: &[u8]) -> Result<(), FormatError> {
        let (id, _attrs, c) = read_id_and_dict(content)?;
        let n = read_i32_le(content, c)?;
        if n > 0 {
            // First model only (later ones are animation frames).
            self.shps.insert(id, read_i32_le(content, c + 4)?);
        }
        Ok(())
    }

    /// Build the scene. Returns `None` if the file contained no model at all.
    pub(crate) fn finish(self) -> Option<Scene> {
        if self.models.is_empty() {
            return None;
        }
        let mut scene = Scene::new();
        for (id, attrs) in &self.layers {
            let Ok(i) = usize::try_from(*id) else {
                continue;
            };
            if i >= 256 {
                continue;
            }
            while scene.layers.len() <= i {
                let n = scene.layers.len();
                scene.layers.push(SceneLayer {
                    name: n.to_string(),
                    color: [255, 255, 255],
                    hidden: false,
                });
            }
            let layer = &mut scene.layers[i];
            if let Some(name) = attrs.get("_name") {
                layer.name = name.clone();
            }
            layer.hidden = is_true(attrs.get("_hidden"));
            if let Some(c) = attrs.get("_color").and_then(|s| parse_triplet(s)) {
                layer.color = [c.x as u8, c.y as u8, c.z as u8];
            }
        }

        let model_count = self.models.len();
        for m in &self.models {
            scene.add_model(m.clone());
        }

        let root_trn = self.trns.get(&0).map(|_| 0).or_else(|| {
            // Fallback: a transform that no group references.
            let referenced: HashSet<i32> = self
                .grps
                .values()
                .flat_map(|g| g.children.iter().copied())
                .collect();
            self.trns
                .keys()
                .copied()
                .filter(|k| !referenced.contains(k))
                .min()
        });

        match root_trn {
            Some(root_id) => {
                let root = scene.root();
                let mut visited = HashSet::new();
                self.build_node(&mut scene, root, root_id, true, 0, &mut visited);
            }
            None => {
                // No scene graph (single-model or pre-0.99 file). Several models there are
                // animation frames: keep frame 0 visible and the others as hidden shapes so
                // they are preserved on save without overlapping on screen.
                let root = scene.root();
                for m in 0..model_count {
                    if let Ok(id) = scene.add_shape(root, m, Transform::IDENTITY) {
                        if m > 0 {
                            if let Some(n) = scene.node_mut(id) {
                                n.name = Some(format!("frame {m}"));
                                n.hidden = true;
                            }
                        }
                    }
                }
            }
        }
        Some(scene)
    }

    /// Attach the node described by `trn_id` under scene group `parent`. The root
    /// transform is merged into the scene's root group instead of creating a node.
    fn build_node(
        &self,
        scene: &mut Scene,
        parent: NodeId,
        trn_id: i32,
        is_root: bool,
        depth: usize,
        visited: &mut HashSet<i32>,
    ) {
        if depth > MAX_GRAPH_DEPTH || !visited.insert(trn_id) {
            return;
        }
        let Some(trn) = self.trns.get(&trn_id) else {
            return;
        };
        let transform = parse_transform(&trn.frame);

        let node_id = if let Some(grp) = self.grps.get(&trn.child) {
            let id = if is_root {
                let root = scene.root();
                if let Some(n) = scene.node_mut(root) {
                    n.transform = transform;
                }
                root
            } else {
                match scene.add_group(parent, transform) {
                    Ok(id) => id,
                    Err(_) => return,
                }
            };
            for &c in &grp.children {
                self.build_node(scene, id, c, false, depth + 1, visited);
            }
            id
        } else if let Some(&model) = self.shps.get(&trn.child) {
            let Ok(model) = usize::try_from(model) else {
                return;
            };
            // A root that is directly a shape still lives under the root group.
            match scene.add_shape(parent, model, transform) {
                Ok(id) => id,
                Err(_) => return,
            }
        } else {
            return;
        };

        if let Some(node) = scene.node_mut(node_id) {
            node.name = trn.attrs.get("_name").cloned();
            node.hidden = is_true(trn.attrs.get("_hidden"));
            node.layer_id = usize::try_from(trn.layer).unwrap_or(0);
        }
    }
}

fn parse_transform(frame: &Dict) -> Transform {
    Transform {
        rotation: frame
            .get("_r")
            .and_then(|s| s.trim().parse::<u8>().ok())
            .map_or(Rotation::IDENTITY, Rotation::from_vox_byte),
        translation: frame
            .get("_t")
            .and_then(|s| parse_triplet(s))
            .unwrap_or(IVec3::ZERO),
    }
}

fn parse_triplet(s: &str) -> Option<IVec3> {
    let mut it = s.split_whitespace().map(|p| p.parse::<i32>().ok());
    let v = IVec3::new(it.next()??, it.next()??, it.next()??);
    Some(v)
}

fn is_true(v: Option<&String>) -> bool {
    matches!(v.map(|s| s.trim()), Some("1") | Some("true"))
}

fn read_dict(bytes: &[u8], offset: usize) -> Result<(Dict, usize), FormatError> {
    let n = read_i32_le(bytes, offset)?.max(0) as usize;
    let mut cursor = offset + 4;
    let mut map = Dict::new();
    for _ in 0..n {
        let (k, c1) = read_vox_string(bytes, cursor)?;
        let (v, c2) = read_vox_string(bytes, c1)?;
        map.insert(k, v);
        cursor = c2;
    }
    Ok((map, cursor))
}

fn read_id_and_dict(bytes: &[u8]) -> Result<(i32, Dict, usize), FormatError> {
    let id = read_i32_le(bytes, 0)?;
    let (dict, next) = read_dict(bytes, 4)?;
    Ok((id, dict, next))
}
