//! glTF 2.0 export of the whole scene (`.glb` binary or `.gltf` with an embedded buffer).
//!
//! - Scene graph: every visible group / shape becomes a glTF node with its transform;
//!   a root node converts MagicaVoxel axes (Z up) to glTF axes (Y up) and applies the
//!   export scale.
//! - Instances: one glTF mesh per model, shared by every shape that references it
//!   (the model of the current animation frame).
//! - Materials: one PBR material per palette index used (diffuse, metal, glass as
//!   alpha-blended, emissive with `KHR_materials_emissive_strength` above 1).
//! - Placement matches the in-app renderer: model vertices are centered
//!   (`v - size / 2`) and node matrices are `[R | t + n(R)]`, where `n` shifts negated
//!   axes by one voxel so voxel cells map onto integer world cells.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use glam::Vec3;
use mv_core::{EditorProject, MaterialType, NodeId, NodeKind, Transform};

use crate::mesh::greedy_quads;
use crate::FormatError;

#[derive(Debug, Clone, PartialEq)]
pub struct GltfOptions {
    /// Size of one voxel in glTF units (meters).
    pub scale: f32,
    /// Export hidden objects and objects on hidden layers.
    pub include_hidden: bool,
}

impl Default for GltfOptions {
    fn default() -> Self {
        Self {
            scale: 1.0,
            include_hidden: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Typed document (inspected by tests, then serialized)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct GltfNode {
    pub name: String,
    /// Column-major 4×4 matrix, `None` for identity.
    pub matrix: Option<[f32; 16]>,
    pub children: Vec<usize>,
    pub mesh: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GltfPrimitive {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    pub material: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GltfMesh {
    pub name: String,
    pub primitives: Vec<GltfPrimitive>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GltfMaterial {
    pub name: String,
    /// Linear RGBA.
    pub base_color: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
    pub emissive: [f32; 3],
    pub emissive_strength: Option<f32>,
    pub blend: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GltfExport {
    pub nodes: Vec<GltfNode>,
    /// Index of the root node of the scene.
    pub root: usize,
    pub meshes: Vec<GltfMesh>,
    pub materials: Vec<GltfMaterial>,
}

impl GltfExport {
    pub fn triangle_count(&self) -> usize {
        self.meshes
            .iter()
            .flat_map(|m| &m.primitives)
            .map(|p| p.indices.len() / 3)
            .sum()
    }
}

fn srgb_to_linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Column-major matrix of a scene transform, in continuous voxel space.
fn node_matrix(t: Transform) -> Option<[f32; 16]> {
    if t == Transform::IDENTITY {
        return None;
    }
    let r = t.rotation;
    let col = |v: Vec3| r.apply_f32(v);
    let (cx, cy, cz) = (col(Vec3::X), col(Vec3::Y), col(Vec3::Z));
    let flipped = r.apply_f32(Vec3::ONE);
    let shift = Vec3::new(
        (flipped.x < 0.0) as i32 as f32,
        (flipped.y < 0.0) as i32 as f32,
        (flipped.z < 0.0) as i32 as f32,
    );
    let tr = t.translation.as_vec3() + shift;
    Some([
        cx.x, cx.y, cx.z, 0.0, cy.x, cy.y, cy.z, 0.0, cz.x, cz.y, cz.z, 0.0, tr.x, tr.y, tr.z, 1.0,
    ])
}

struct Builder<'a> {
    project: &'a EditorProject,
    opts: &'a GltfOptions,
    doc: GltfExport,
    mesh_of_model: HashMap<usize, Option<usize>>,
    material_of_color: HashMap<u8, usize>,
}

impl Builder<'_> {
    fn material(&mut self, color: u8) -> usize {
        if let Some(&m) = self.material_of_color.get(&color) {
            return m;
        }
        let c = self.project.palette.colors[color as usize];
        let mat = self.project.materials[color as usize];
        let rgb = [
            srgb_to_linear(c.r),
            srgb_to_linear(c.g),
            srgb_to_linear(c.b),
        ];
        let mut m = GltfMaterial {
            name: format!("palette_{color}"),
            base_color: [rgb[0], rgb[1], rgb[2], 1.0],
            metallic: 0.0,
            roughness: 1.0,
            emissive: [0.0; 3],
            emissive_strength: None,
            blend: false,
        };
        match mat.kind {
            MaterialType::Metal => {
                m.metallic = if mat.metal > 0.0 {
                    mat.metal.min(1.0)
                } else {
                    1.0
                };
                m.roughness = mat.rough.clamp(0.0, 1.0);
            }
            MaterialType::Glass => {
                m.roughness = mat.rough.clamp(0.0, 1.0);
                m.base_color[3] = mat.alpha.clamp(0.0, 1.0);
                m.blend = true;
            }
            MaterialType::Emit => {
                m.emissive = rgb;
                if mat.emit > 1.0 {
                    m.emissive_strength = Some(mat.emit);
                }
            }
            MaterialType::Diffuse | MaterialType::Blend | MaterialType::Cloud => {}
        }
        self.doc.materials.push(m);
        let idx = self.doc.materials.len() - 1;
        self.material_of_color.insert(color, idx);
        idx
    }

    fn mesh(&mut self, model: usize) -> Option<usize> {
        if let Some(&m) = self.mesh_of_model.get(&model) {
            return m;
        }
        let grid = self.project.model(model)?;
        let half = (grid.dimensions() / 2).as_vec3();
        let mut by_color: BTreeMap<u8, GltfPrimitive> = BTreeMap::new();
        for q in greedy_quads(grid) {
            let material = self.material(q.color);
            let prim = by_color.entry(q.color).or_insert_with(|| GltfPrimitive {
                positions: Vec::new(),
                normals: Vec::new(),
                indices: Vec::new(),
                material,
            });
            let base = prim.positions.len() as u32;
            let n = [q.normal[0] as f32, q.normal[1] as f32, q.normal[2] as f32];
            for c in q.corners {
                prim.positions
                    .push([c[0] - half.x, c[1] - half.y, c[2] - half.z]);
                prim.normals.push(n);
            }
            prim.indices
                .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        let result = if by_color.is_empty() {
            None
        } else {
            self.doc.meshes.push(GltfMesh {
                name: format!("model_{model}"),
                primitives: by_color.into_values().collect(),
            });
            Some(self.doc.meshes.len() - 1)
        };
        self.mesh_of_model.insert(model, result);
        result
    }

    /// Add `id` and its visible subtree; returns its glTF node index.
    fn node(&mut self, id: NodeId) -> Option<usize> {
        let scene = &self.project.scene;
        let node = scene.node(id)?;
        let layer_hidden = scene.layers.get(node.layer_id).is_some_and(|l| l.hidden);
        if !self.opts.include_hidden && (node.hidden || layer_hidden) {
            return None;
        }
        let (name, mesh, children) = match &node.kind {
            NodeKind::Group { children } => {
                let children = children.clone();
                let name = node.name.clone().unwrap_or_else(|| {
                    if id == scene.root() {
                        "Scene".to_string()
                    } else {
                        format!("Group {id}")
                    }
                });
                let kids = children.into_iter().filter_map(|c| self.node(c)).collect();
                (name, None, kids)
            }
            NodeKind::Shape { .. } => {
                let model = scene.shape_model(id)?;
                let name = node
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("Model {model}"));
                (name, self.mesh(model), Vec::new())
            }
        };
        let matrix = node_matrix(self.project.scene.node(id)?.transform);
        self.doc.nodes.push(GltfNode {
            name,
            matrix,
            children,
            mesh,
        });
        Some(self.doc.nodes.len() - 1)
    }
}

/// Build the glTF document of the project's scene.
pub fn to_gltf(project: &EditorProject, opts: &GltfOptions) -> GltfExport {
    let mut b = Builder {
        project,
        opts,
        doc: GltfExport {
            nodes: Vec::new(),
            root: 0,
            meshes: Vec::new(),
            materials: Vec::new(),
        },
        mesh_of_model: HashMap::new(),
        material_of_color: HashMap::new(),
    };
    let scene_root = b.node(project.scene.root());
    let s = opts.scale;
    // MagicaVoxel (x, y, z) -> glTF (x, z, -y), scaled.
    b.doc.nodes.push(GltfNode {
        name: format!("{} (MagicaVoxel, Z-up)", project.name),
        matrix: Some([
            s, 0.0, 0.0, 0.0, 0.0, 0.0, -s, 0.0, 0.0, s, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ]),
        children: scene_root.into_iter().collect(),
        mesh: None,
    });
    b.doc.root = b.doc.nodes.len() - 1;
    b.doc
}

// ---------------------------------------------------------------------------
// Serialization
// ---------------------------------------------------------------------------

enum J {
    F(f32),
    U(usize),
    S(String),
    A(Vec<J>),
    O(Vec<(&'static str, J)>),
}

impl J {
    fn write(&self, out: &mut String) {
        match self {
            J::F(v) => {
                let v = if v.is_finite() { *v } else { 0.0 };
                out.push_str(&format!("{v}"));
            }
            J::U(v) => out.push_str(&v.to_string()),
            J::S(s) => {
                out.push('"');
                for ch in s.chars() {
                    match ch {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\r' => out.push_str("\\r"),
                        '\t' => out.push_str("\\t"),
                        c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
                        c => out.push(c),
                    }
                }
                out.push('"');
            }
            J::A(items) => {
                out.push('[');
                for (i, it) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    it.write(out);
                }
                out.push(']');
            }
            J::O(fields) => {
                out.push('{');
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    J::S(k.to_string()).write(out);
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
        }
    }
}

fn floats(v: &[f32]) -> J {
    J::A(v.iter().map(|&f| J::F(f)).collect())
}

const FLOAT: usize = 5126;
const UNSIGNED_INT: usize = 5125;
const ARRAY_BUFFER: usize = 34962;
const ELEMENT_ARRAY_BUFFER: usize = 34963;

/// Serialize to `(json, binary buffer)`. `buffer_uri`: `Some(uri)` for `.gltf`, `None`
/// for the GLB binary chunk.
fn serialize(
    doc: &GltfExport,
    buffer_uri: impl FnOnce(&[u8]) -> Option<String>,
) -> (String, Vec<u8>) {
    let mut bin: Vec<u8> = Vec::new();
    let mut views = Vec::new();
    let mut accessors = Vec::new();
    let mut push_view = |bin: &mut Vec<u8>, bytes: &[u8], target: usize| -> usize {
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        let offset = bin.len();
        bin.extend_from_slice(bytes);
        views.push(J::O(vec![
            ("buffer", J::U(0)),
            ("byteOffset", J::U(offset)),
            ("byteLength", J::U(bytes.len())),
            ("target", J::U(target)),
        ]));
        views.len() - 1
    };

    let mut meshes = Vec::new();
    for mesh in &doc.meshes {
        let mut prims = Vec::new();
        for p in &mesh.primitives {
            let pos_bytes: Vec<u8> = p
                .positions
                .iter()
                .flatten()
                .flat_map(|f| f.to_le_bytes())
                .collect();
            let nrm_bytes: Vec<u8> = p
                .normals
                .iter()
                .flatten()
                .flat_map(|f| f.to_le_bytes())
                .collect();
            let idx_bytes: Vec<u8> = p.indices.iter().flat_map(|i| i.to_le_bytes()).collect();
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for v in &p.positions {
                for k in 0..3 {
                    lo[k] = lo[k].min(v[k]);
                    hi[k] = hi[k].max(v[k]);
                }
            }
            let pv = push_view(&mut bin, &pos_bytes, ARRAY_BUFFER);
            accessors.push(J::O(vec![
                ("bufferView", J::U(pv)),
                ("componentType", J::U(FLOAT)),
                ("count", J::U(p.positions.len())),
                ("type", J::S("VEC3".into())),
                ("min", floats(&lo)),
                ("max", floats(&hi)),
            ]));
            let pos_acc = accessors.len() - 1;
            let nv = push_view(&mut bin, &nrm_bytes, ARRAY_BUFFER);
            accessors.push(J::O(vec![
                ("bufferView", J::U(nv)),
                ("componentType", J::U(FLOAT)),
                ("count", J::U(p.normals.len())),
                ("type", J::S("VEC3".into())),
            ]));
            let nrm_acc = accessors.len() - 1;
            let iv = push_view(&mut bin, &idx_bytes, ELEMENT_ARRAY_BUFFER);
            accessors.push(J::O(vec![
                ("bufferView", J::U(iv)),
                ("componentType", J::U(UNSIGNED_INT)),
                ("count", J::U(p.indices.len())),
                ("type", J::S("SCALAR".into())),
            ]));
            let idx_acc = accessors.len() - 1;
            prims.push(J::O(vec![
                (
                    "attributes",
                    J::O(vec![("POSITION", J::U(pos_acc)), ("NORMAL", J::U(nrm_acc))]),
                ),
                ("indices", J::U(idx_acc)),
                ("material", J::U(p.material)),
                ("mode", J::U(4)),
            ]));
        }
        meshes.push(J::O(vec![
            ("name", J::S(mesh.name.clone())),
            ("primitives", J::A(prims)),
        ]));
    }
    while bin.len() % 4 != 0 {
        bin.push(0);
    }

    let mut uses_emissive_strength = false;
    let materials: Vec<J> = doc
        .materials
        .iter()
        .map(|m| {
            let mut fields = vec![
                ("name", J::S(m.name.clone())),
                (
                    "pbrMetallicRoughness",
                    J::O(vec![
                        ("baseColorFactor", floats(&m.base_color)),
                        ("metallicFactor", J::F(m.metallic)),
                        ("roughnessFactor", J::F(m.roughness)),
                    ]),
                ),
            ];
            if m.emissive != [0.0; 3] {
                fields.push(("emissiveFactor", floats(&m.emissive)));
            }
            if let Some(strength) = m.emissive_strength {
                uses_emissive_strength = true;
                fields.push((
                    "extensions",
                    J::O(vec![(
                        "KHR_materials_emissive_strength",
                        J::O(vec![("emissiveStrength", J::F(strength))]),
                    )]),
                ));
            }
            if m.blend {
                fields.push(("alphaMode", J::S("BLEND".into())));
            }
            J::O(fields)
        })
        .collect();

    let nodes: Vec<J> = doc
        .nodes
        .iter()
        .map(|n| {
            let mut fields = vec![("name", J::S(n.name.clone()))];
            if let Some(m) = n.matrix {
                fields.push(("matrix", floats(&m)));
            }
            if !n.children.is_empty() {
                fields.push((
                    "children",
                    J::A(n.children.iter().map(|&c| J::U(c)).collect()),
                ));
            }
            if let Some(mesh) = n.mesh {
                fields.push(("mesh", J::U(mesh)));
            }
            J::O(fields)
        })
        .collect();

    let mut root = vec![
        (
            "asset",
            J::O(vec![
                ("version", J::S("2.0".into())),
                ("generator", J::S("MagicaVoxel Rust Edition".into())),
            ]),
        ),
        ("scene", J::U(0)),
        (
            "scenes",
            J::A(vec![J::O(vec![("nodes", J::A(vec![J::U(doc.root)]))])]),
        ),
        ("nodes", J::A(nodes)),
    ];
    if !meshes.is_empty() {
        root.push(("meshes", J::A(meshes)));
        root.push(("materials", J::A(materials)));
        root.push(("accessors", J::A(accessors)));
        root.push(("bufferViews", J::A(views)));
        let mut buffer = vec![("byteLength", J::U(bin.len()))];
        if let Some(uri) = buffer_uri(&bin) {
            buffer.push(("uri", J::S(uri)));
        }
        root.push(("buffers", J::A(vec![J::O(buffer)])));
    }
    if uses_emissive_strength {
        root.push((
            "extensionsUsed",
            J::A(vec![J::S("KHR_materials_emissive_strength".into())]),
        ));
    }
    let mut json = String::new();
    J::O(root).write(&mut json);
    (json, bin)
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for k in 0..4 {
            if k <= chunk.len() {
                out.push(T[(n >> (18 - 6 * k) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// GLB container: 12-byte header, JSON chunk (space padded), BIN chunk (zero padded).
pub fn to_glb_bytes(doc: &GltfExport) -> Vec<u8> {
    let (json, bin) = serialize(doc, |_| None);
    let mut json = json.into_bytes();
    while json.len() % 4 != 0 {
        json.push(b' ');
    }
    let has_bin = !bin.is_empty();
    let total = 12 + 8 + json.len() + if has_bin { 8 + bin.len() } else { 0 };
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x4E4F_534Au32.to_le_bytes()); // "JSON"
    out.extend_from_slice(&json);
    if has_bin {
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(&0x004E_4942u32.to_le_bytes()); // "BIN\0"
        out.extend_from_slice(&bin);
    }
    out
}

/// `.gltf` JSON with the buffer embedded as a base64 data URI.
pub fn to_gltf_json(doc: &GltfExport) -> String {
    serialize(doc, |bin| {
        Some(format!(
            "data:application/octet-stream;base64,{}",
            base64(bin)
        ))
    })
    .0
}

/// Export the scene; `.glb` extension writes the binary container, anything else the
/// JSON `.gltf` form.
pub fn export_gltf(
    project: &EditorProject,
    path: impl AsRef<Path>,
    opts: &GltfOptions,
) -> Result<GltfExport, FormatError> {
    let path = path.as_ref();
    let doc = to_gltf(project, opts);
    let binary = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("glb"));
    if binary {
        std::fs::write(path, to_glb_bytes(&doc))?;
    } else {
        std::fs::write(path, to_gltf_json(&doc))?;
    }
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::IVec3;
    use mv_core::{Axis, Rotation, VoxelGrid};

    fn mat_mul(m: &[f32; 16], p: Vec3) -> Vec3 {
        Vec3::new(
            m[0] * p.x + m[4] * p.y + m[8] * p.z + m[12],
            m[1] * p.x + m[5] * p.y + m[9] * p.z + m[13],
            m[2] * p.x + m[6] * p.y + m[10] * p.z + m[14],
        )
    }

    /// World-space (MagicaVoxel axes) bounding box of every mesh instance, walking the
    /// exported node hierarchy below the axis-conversion root.
    fn exported_bounds(doc: &GltfExport) -> Vec<(String, Vec3, Vec3)> {
        fn walk(
            doc: &GltfExport,
            n: usize,
            parent: [f32; 16],
            out: &mut Vec<(String, Vec3, Vec3)>,
        ) {
            let node = &doc.nodes[n];
            let local = node.matrix.unwrap_or([
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ]);
            // parent * local (column-major)
            let mut m = [0.0f32; 16];
            for c in 0..4 {
                for r in 0..4 {
                    m[c * 4 + r] = (0..4).map(|k| parent[k * 4 + r] * local[c * 4 + k]).sum();
                }
            }
            if let Some(mesh) = node.mesh {
                let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
                for p in &doc.meshes[mesh].primitives {
                    for v in &p.positions {
                        let w = mat_mul(&m, Vec3::from_array(*v));
                        lo = lo.min(w);
                        hi = hi.max(w);
                    }
                }
                out.push((node.name.clone(), lo, hi));
            }
            for &c in &node.children {
                walk(doc, c, m, out);
            }
        }
        let identity = [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        let mut out = Vec::new();
        for &c in &doc.nodes[doc.root].children {
            walk(doc, c, identity, &mut out);
        }
        out
    }

    fn scene_project() -> (EditorProject, NodeId, NodeId) {
        let mut proj = EditorProject::new_empty(1, 1, 1);
        proj.scene = mv_core::Scene::new();
        proj.active_model = usize::MAX;
        let mut tower = VoxelGrid::new(3, 4, 6);
        tower.full(10);
        tower.set(0, 0, 5, 20); // a second color
        let m = proj.scene.add_model(tower);
        let root = proj.scene.root();
        let g = proj
            .scene
            .add_group(
                root,
                Transform {
                    rotation: Rotation::rot90(Axis::Z),
                    translation: IVec3::new(10, 0, 0),
                },
            )
            .unwrap();
        let a = proj
            .scene
            .add_shape(g, m, Transform::from_translation(IVec3::new(0, 5, 3)))
            .unwrap();
        let b = proj.scene.instantiate(a).unwrap();
        proj.scene.rotate90(b, Axis::X).unwrap();
        proj.scene
            .translate_world(b, IVec3::new(-20, 0, 0))
            .unwrap();
        proj.scene.node_mut(b).unwrap().name = Some("tilted \"copy\"".to_string());
        (proj, a, b)
    }

    #[test]
    fn hierarchy_instances_and_placement_match_the_scene() {
        let (proj, a, b) = scene_project();
        let doc = to_gltf(&proj, &GltfOptions::default());

        assert_eq!(doc.meshes.len(), 1, "instances share one mesh");
        assert_eq!(doc.meshes[0].primitives.len(), 2, "one primitive per color");
        assert_eq!(doc.materials.len(), 2);
        let root = &doc.nodes[doc.root];
        let scene = &doc.nodes[root.children[0]];
        assert_eq!(scene.name, "Scene");
        let group = &doc.nodes[scene.children[0]];
        assert_eq!(group.children.len(), 2);

        // Exported boxes equal the scene's world boxes (voxel cells [lo, hi + 1]).
        let bounds = exported_bounds(&doc);
        for (i, shape) in [a, b].into_iter().enumerate() {
            let (lo, hi) = proj.scene.world_bounds(shape).unwrap();
            let (_, elo, ehi) = &bounds[i];
            assert_eq!(*elo, lo.as_vec3(), "shape {i} min");
            assert_eq!(*ehi, (hi + IVec3::ONE).as_vec3(), "shape {i} max");
        }
        assert_eq!(bounds[1].0, "tilted \"copy\"");
    }

    #[test]
    fn axis_conversion_scale_hidden_and_materials() {
        let (mut proj, a, _) = scene_project();
        proj.materials[20].kind = MaterialType::Emit;
        proj.materials[20].emit = 3.0;
        proj.materials[10].kind = MaterialType::Glass;
        proj.materials[10].alpha = 0.4;

        let opts = GltfOptions {
            scale: 0.1,
            include_hidden: false,
        };
        let doc = to_gltf(&proj, &opts);
        let m = doc.nodes[doc.root].matrix.unwrap();
        // Z (MagicaVoxel up) maps to +Y (glTF up); Y maps to -Z; scale applied.
        assert_eq!(mat_mul(&m, Vec3::Z), Vec3::new(0.0, 0.1, 0.0));
        assert_eq!(mat_mul(&m, Vec3::Y), Vec3::new(0.0, 0.0, -0.1));

        let emit = doc
            .materials
            .iter()
            .find(|m| m.name == "palette_20")
            .unwrap();
        assert_eq!(emit.emissive_strength, Some(3.0));
        assert!(emit.emissive[0] > 0.0);
        let glass = doc
            .materials
            .iter()
            .find(|m| m.name == "palette_10")
            .unwrap();
        assert!(glass.blend && (glass.base_color[3] - 0.4).abs() < 1e-6);

        proj.scene.node_mut(a).unwrap().hidden = true;
        let doc = to_gltf(&proj, &opts);
        assert_eq!(exported_bounds(&doc).len(), 1, "hidden shape skipped");
        let all = to_gltf(
            &proj,
            &GltfOptions {
                include_hidden: true,
                ..opts
            },
        );
        assert_eq!(exported_bounds(&all).len(), 2);
    }

    #[test]
    fn glb_and_gltf_containers_are_well_formed() {
        let (proj, _, _) = scene_project();
        let doc = to_gltf(&proj, &GltfOptions::default());
        let glb = to_glb_bytes(&doc);
        assert_eq!(&glb[0..4], b"glTF");
        assert_eq!(
            u32::from_le_bytes(glb[8..12].try_into().unwrap()) as usize,
            glb.len()
        );
        let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        assert_eq!(json_len % 4, 0);
        let json = std::str::from_utf8(&glb[20..20 + json_len]).unwrap();
        assert!(json.contains("\"version\":\"2.0\""));
        assert!(json.contains("tilted \\\"copy\\\""), "names are escaped");
        let bin_type = &glb[20 + json_len + 4..20 + json_len + 8];
        assert_eq!(bin_type, b"BIN\0");

        let gltf = to_gltf_json(&doc);
        assert!(gltf.contains("data:application/octet-stream;base64,"));
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
        assert_eq!(
            doc.triangle_count(),
            doc.meshes[0]
                .primitives
                .iter()
                .map(|p| p.indices.len() / 3)
                .sum::<usize>()
        );
    }

    /// Writes reference exports for external validation:
    /// `GLTF_OUT=dir cargo test -p mv-format gltf_reference_files -- --ignored`
    #[test]
    #[ignore]
    fn gltf_reference_files() {
        let Ok(dir) = std::env::var("GLTF_OUT") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        let mut default = EditorProject::default();
        default.materials[19].emit = 2.8; // emissive strength > 1
        export_gltf(
            &default,
            dir.join("default_materials.glb"),
            &GltfOptions::default(),
        )
        .unwrap();
        let (mut proj, a, _) = scene_project();
        // A mirroring rotation (determinant -1) on top of the rotated group.
        proj.scene.node_mut(a).unwrap().transform.rotation = Rotation::from_vox_byte(0b0010100);
        export_gltf(
            &proj,
            dir.join("rotated_instances.gltf"),
            &GltfOptions::default(),
        )
        .unwrap();
    }
}
