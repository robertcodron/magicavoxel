use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::Path;

use mv_core::{EditorProject, Material, MaterialType, Palette, Rgba};
use thiserror::Error;

mod gltf;
mod mesh;
mod placed;
mod scene_io;

pub use gltf::{
    export_gltf, to_glb_bytes, to_gltf, to_gltf_json, GltfExport, GltfMaterial, GltfMesh, GltfNode,
    GltfOptions, GltfPrimitive,
};
pub use mesh::{greedy_quads, GreedyQuad};

#[derive(Debug, Error)]
pub enum FormatError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid .vox header or corrupted file: {0}")]
    InvalidVox(String),
    #[error("Image encoding error: {0}")]
    Image(#[from] image::ImageError),
}

/// Serialize an `EditorProject` into binary MagicaVoxel `.vox` format (version 150)
pub fn to_vox_bytes(project: &EditorProject) -> Vec<u8> {
    let mut children = Vec::new();

    // 1-2. SIZE + XYZI per model, then the scene graph (nTRN / nGRP / nSHP) and LAYR
    scene_io::write_models_and_scene(project, &mut children);

    // 3. RGBA chunk (256 * 4 bytes; in .vox format, palette[0..254] stores colors 1..255)
    let mut rgba_content = Vec::with_capacity(1024);
    for i in 1..=255usize {
        let c = project.palette.colors[i];
        rgba_content.extend_from_slice(&[c.r, c.g, c.b, c.a]);
    }
    rgba_content.extend_from_slice(&[0, 0, 0, 0]);
    write_chunk(&mut children, b"RGBA", &rgba_content, &[]);

    // 4. MATL chunks for materials that differ from the default
    for i in 1..=255usize {
        let mat = &project.materials[i];
        let default = Material {
            id: mat.id,
            ..Default::default()
        };
        if *mat != default {
            write_chunk(&mut children, b"MATL", &matl_content(i as i32, mat), &[]);
        }
    }

    let mut out = Vec::with_capacity(8 + 12 + children.len());
    out.extend_from_slice(b"VOX ");
    out.extend_from_slice(&150u32.to_le_bytes());
    write_chunk(&mut out, b"MAIN", &[], &children);
    out
}

/// Parse binary MagicaVoxel `.vox` bytes into an `EditorProject`
pub fn from_vox_bytes(bytes: &[u8]) -> Result<EditorProject, FormatError> {
    if bytes.len() < 20 || &bytes[0..4] != b"VOX " {
        return Err(FormatError::InvalidVox(
            "Missing 'VOX ' magic header".to_string(),
        ));
    }
    let _version = read_u32_le(bytes, 4)?;
    if &bytes[8..12] != b"MAIN" {
        return Err(FormatError::InvalidVox("Missing 'MAIN' chunk".to_string()));
    }
    let main_content_len = read_u32_le(bytes, 12)? as usize;
    let main_children_len = read_u32_le(bytes, 16)? as usize;

    let mut cursor = 20 + main_content_len;
    let end = (cursor + main_children_len).min(bytes.len());

    let mut project = EditorProject::new_empty(40, 40, 40);
    let mut scene_reader = scene_io::SceneReader::default();

    while cursor + 12 <= end {
        let id = &bytes[cursor..cursor + 4];
        let content_len = read_u32_le(bytes, cursor + 4)? as usize;
        let children_len = read_u32_le(bytes, cursor + 8)? as usize;
        let content_start = cursor + 12;
        let content_end = (content_start + content_len).min(bytes.len());
        let content = &bytes[content_start..content_end];

        match id {
            _ if scene_reader.accept(id, content) => {}
            b"RGBA" if content.len() >= 1020 => {
                let mut pal = Palette::default();
                for i in 0..255usize {
                    let base = i * 4;
                    pal.colors[i + 1] = Rgba::new(
                        content[base],
                        content[base + 1],
                        content[base + 2],
                        content[base + 3],
                    );
                }
                project.palette = pal;
            }
            b"MATL" if content.len() >= 8 => {
                // Ids map to palette indices; 0 (empty voxel) and out-of-range ids are ignored.
                if let Ok((mat_id @ 1..=255, dict)) = parse_vox_dict(content) {
                    apply_matl_dict(&mut project.materials[mat_id as usize], &dict);
                }
            }
            _ => {}
        }

        cursor = content_start + content_len + children_len;
    }

    let scene = scene_reader
        .finish()
        .ok_or_else(|| FormatError::InvalidVox("No model (SIZE/XYZI) found".to_string()))?;
    // Edit the first visible shape's model by default (else the first model).
    let root = scene.root();
    let active_shape = scene
        .shapes_under(root)
        .into_iter()
        .find(|&s| scene.is_visible(s))
        .or_else(|| scene.shapes_under(root).into_iter().next());
    let active = active_shape.and_then(|s| scene.shape_model(s)).unwrap_or(0);
    project.grid = scene.models[active].clone();
    let d = project.grid.dimensions();
    project.selection.resize(d.x, d.y, d.z);
    project.active_model = active;
    project.active_shape = active_shape;
    project.scene = scene;

    Ok(project)
}

pub fn save_vox_file(project: &EditorProject, path: impl AsRef<Path>) -> Result<(), FormatError> {
    let bytes = to_vox_bytes(project);
    let mut file = File::create(path)?;
    file.write_all(&bytes)?;
    Ok(())
}

pub fn load_vox_file(path: impl AsRef<Path>) -> Result<EditorProject, FormatError> {
    let path_ref = path.as_ref();
    let mut file = File::open(path_ref)?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let mut project = from_vox_bytes(&bytes)?;
    if let Some(stem) = path_ref.file_stem().and_then(|s| s.to_str()) {
        project.name = stem.to_string();
    }
    Ok(project)
}

/// Wavefront `.obj` export of the **visible scene** (one `o` object per shape, at its
/// world placement, current animation frame), greedy meshed with vertex colors.
/// Axes are converted from MagicaVoxel (Z up) to OBJ (Y up) with `(x, y, z) -> (x, z, -y)`
/// (a proper rotation: no mirroring, faces stay counter-clockwise from outside).
/// Returns the number of quads.
pub fn export_obj_greedy(
    project: &EditorProject,
    path: impl AsRef<Path>,
) -> Result<usize, FormatError> {
    let shapes = placed::visible_shapes(project);
    let mut quads_of: HashMap<usize, Vec<mesh::GreedyQuad>> = HashMap::new();
    let mut w = BufWriter::new(File::create(path)?);
    writeln!(
        w,
        "# Exported by MagicaVoxel Rust Edition (Greedy Meshing, whole scene)"
    )?;

    let y_up = |p: glam::Vec3| [p.x, p.z, -p.y];
    // The six axis normals, indexed 1..=6.
    let normals: [[i32; 3]; 6] = [
        [-1, 0, 0],
        [1, 0, 0],
        [0, -1, 0],
        [0, 1, 0],
        [0, 0, -1],
        [0, 0, 1],
    ];
    for n in normals {
        let m = y_up(glam::Vec3::new(n[0] as f32, n[1] as f32, n[2] as f32));
        writeln!(w, "vn {} {} {}", m[0], m[1], m[2])?;
    }

    let mut next_vertex = 1usize;
    let mut total = 0usize;
    for shape in &shapes {
        let quads = quads_of
            .entry(shape.model_id)
            .or_insert_with(|| mesh::greedy_quads(shape.grid));
        if quads.is_empty() {
            continue;
        }
        writeln!(w, "o {}", shape.name.replace(char::is_whitespace, "_"))?;
        for q in quads.iter() {
            let col = project.palette.colors[q.color as usize];
            for c in q.corners {
                let v = y_up(shape.world_point(c));
                writeln!(
                    w,
                    "v {:.3} {:.3} {:.3} {:.4} {:.4} {:.4}",
                    v[0],
                    v[1],
                    v[2],
                    col.r as f32 / 255.0,
                    col.g as f32 / 255.0,
                    col.b as f32 / 255.0
                )?;
            }
        }
        for q in quads.iter() {
            let wn = shape.world_normal(q.normal).to_array();
            let n = 1 + normals.iter().position(|&n| n == wn).unwrap_or(0);
            let b = next_vertex;
            // Rotations may mirror (determinant -1): keep faces counter-clockwise.
            let (i1, i3) = if shape.voxel_xf.rotation.is_mirror() {
                (b + 3, b + 1)
            } else {
                (b + 1, b + 3)
            };
            writeln!(w, "f {b}//{n} {i1}//{n} {}//{n} {i3}//{n}", b + 2)?;
            next_vertex += 4;
        }
        total += quads.len();
    }
    w.flush()?;
    Ok(total)
}

/// Stanford `.ply` point cloud of the surface voxels of the **visible scene**, at world
/// voxel coordinates (MagicaVoxel axes, current animation frame). Returns the number
/// of points.
pub fn export_ply_point_cloud(
    project: &EditorProject,
    path: impl AsRef<Path>,
) -> Result<usize, FormatError> {
    let mut points = Vec::new();
    for shape in placed::visible_shapes(project) {
        let grid = shape.grid;
        for z in 0..grid.size_z() {
            for y in 0..grid.size_y() {
                for x in 0..grid.size_x() {
                    let c = grid.get(x, y, z);
                    if c != 0 && !grid.is_interior_solid(x as i32, y as i32, z as i32) {
                        let p = shape
                            .voxel_xf
                            .apply(glam::IVec3::new(x as i32, y as i32, z as i32));
                        points.push((p, project.palette.colors[c as usize]));
                    }
                }
            }
        }
    }

    let mut w = BufWriter::new(File::create(path)?);
    writeln!(w, "ply")?;
    writeln!(w, "format ascii 1.0")?;
    writeln!(
        w,
        "comment Exported by MagicaVoxel Rust Edition (whole scene)"
    )?;
    writeln!(w, "element vertex {}", points.len())?;
    for prop in [
        "float x",
        "float y",
        "float z",
        "uchar red",
        "uchar green",
        "uchar blue",
    ] {
        writeln!(w, "property {prop}")?;
    }
    writeln!(w, "end_header")?;
    for (p, col) in &points {
        writeln!(w, "{} {} {} {} {} {}", p.x, p.y, p.z, col.r, col.g, col.b)?;
    }
    w.flush()?;
    Ok(points.len())
}

/// Export volume as a 2D slice strip PNG of size `(size_x, size_y * size_z)` (`o slice`)
pub fn export_slice_png(
    project: &EditorProject,
    path: impl AsRef<Path>,
) -> Result<(), FormatError> {
    let grid = &project.grid;
    let width = grid.size_x();
    let height = grid.size_y() * grid.size_z();
    let mut img = image::RgbaImage::new(width, height);

    for z in 0..grid.size_z() {
        for y in 0..grid.size_y() {
            for x in 0..grid.size_x() {
                let c = grid.get(x, y, z);
                let rgba = if c == 0 {
                    [0, 0, 0, 0]
                } else {
                    let col = project.palette.colors[c as usize];
                    [col.r, col.g, col.b, col.a]
                };
                let py = (grid.size_z() - 1 - z) * grid.size_y() + y;
                img.put_pixel(x, py, image::Rgba(rgba));
            }
        }
    }
    img.save(path)?;
    Ok(())
}

/// Material property keys written to / read from `MATL` chunks (MagicaVoxel 0.99 naming).
/// Density is `_d` in the official format; `_density` is also accepted on read.
/// `_ior` is handled separately: MagicaVoxel stores `ior - 1` (default `0.3` = IOR 1.3).
fn matl_float_fields(m: &mut Material) -> [(&'static str, &mut f32); 10] {
    [
        ("_weight", &mut m.weight),
        ("_rough", &mut m.rough),
        ("_spec", &mut m.spec),
        ("_att", &mut m.att),
        ("_flux", &mut m.flux),
        ("_emit", &mut m.emit),
        ("_ldr", &mut m.ldr),
        ("_metal", &mut m.metal),
        ("_alpha", &mut m.alpha),
        ("_d", &mut m.density),
    ]
}

fn matl_content(id: i32, mat: &Material) -> Vec<u8> {
    let mut m = *mat;
    // `{}` prints the shortest string that parses back to the exact same f32.
    let mut pairs: Vec<(&str, String)> = vec![
        ("_type", mat.kind.as_str().to_string()),
        ("_ior", format!("{}", mat.ior - 1.0)),
    ];
    for (key, value) in matl_float_fields(&mut m) {
        pairs.push((key, format!("{}", *value)));
    }
    // `_trans` mirrors `_alpha` for readers that only know the older key.
    pairs.push(("_trans", format!("{}", 1.0 - mat.alpha)));

    let mut out = Vec::new();
    out.extend_from_slice(&id.to_le_bytes());
    out.extend_from_slice(&(pairs.len() as i32).to_le_bytes());
    for (k, v) in &pairs {
        write_vox_string(&mut out, k);
        write_vox_string(&mut out, v);
    }
    out
}

fn apply_matl_dict(m: &mut Material, dict: &HashMap<String, String>) {
    if let Some(t) = dict.get("_type") {
        m.kind = MaterialType::from_vox_str(t);
    }
    let parse = |k: &str| dict.get(k).and_then(|s| s.parse::<f32>().ok());
    if let Some(v) = parse("_ior") {
        m.ior = v + 1.0;
    }
    if let Some(v) = parse("_trans") {
        m.alpha = 1.0 - v;
    }
    for (key, field) in matl_float_fields(m) {
        if let Some(v) = parse(key) {
            *field = v;
        }
    }
    if let Some(v) = parse("_density") {
        m.density = v;
    }
}

pub(crate) fn write_chunk(out: &mut Vec<u8>, id: &[u8; 4], content: &[u8], children: &[u8]) {
    out.extend_from_slice(id);
    out.extend_from_slice(&(content.len() as u32).to_le_bytes());
    out.extend_from_slice(&(children.len() as u32).to_le_bytes());
    out.extend_from_slice(content);
    out.extend_from_slice(children);
}

pub(crate) fn write_vox_string(out: &mut Vec<u8>, s: &str) {
    let b = s.as_bytes();
    out.extend_from_slice(&(b.len() as u32).to_le_bytes());
    out.extend_from_slice(b);
}

pub(crate) fn read_u32_le(bytes: &[u8], offset: usize) -> Result<u32, FormatError> {
    if offset + 4 > bytes.len() {
        return Err(FormatError::InvalidVox("Unexpected EOF".to_string()));
    }
    Ok(u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ]))
}

pub(crate) fn read_i32_le(bytes: &[u8], offset: usize) -> Result<i32, FormatError> {
    Ok(read_u32_le(bytes, offset)? as i32)
}

fn parse_vox_dict(content: &[u8]) -> Result<(i32, HashMap<String, String>), FormatError> {
    let id = read_i32_le(content, 0)?;
    let num_pairs = read_i32_le(content, 4)?.max(0) as usize;
    let mut cursor = 8usize;
    let mut map = HashMap::new();
    for _ in 0..num_pairs {
        let (k, next_c) = read_vox_string(content, cursor)?;
        let (v, next_c2) = read_vox_string(content, next_c)?;
        map.insert(k, v);
        cursor = next_c2;
    }
    Ok((id, map))
}

pub(crate) fn read_vox_string(bytes: &[u8], offset: usize) -> Result<(String, usize), FormatError> {
    let len = read_u32_le(bytes, offset)? as usize;
    let start = offset + 4;
    let end = start + len;
    if end > bytes.len() {
        return Err(FormatError::InvalidVox("String out of bounds".to_string()));
    }
    let s = String::from_utf8_lossy(&bytes[start..end]).to_string();
    Ok((s, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vox_roundtrip() {
        let mut proj = EditorProject::default();
        proj.grid.set(5, 6, 7, 42);
        proj.palette.colors[42] = Rgba::new(12, 34, 56, 255);
        proj.materials[42].kind = MaterialType::Metal;
        proj.materials[42].rough = 0.25;

        let bytes = to_vox_bytes(&proj);
        let decoded = from_vox_bytes(&bytes).expect("Failed to decode .vox");

        assert_eq!(decoded.grid.dimensions(), proj.grid.dimensions());
        assert_eq!(decoded.grid.get(5, 6, 7), 42);
        assert_eq!(decoded.palette.colors[42], Rgba::new(12, 34, 56, 255));
        assert_eq!(decoded.materials[42].kind, MaterialType::Metal);
        assert!((decoded.materials[42].rough - 0.25).abs() < 1e-3);
    }

    #[test]
    fn test_vox_material_roundtrip_all_fields() {
        let mut proj = EditorProject::new_empty(8, 8, 8);
        let m = &mut proj.materials[7];
        m.kind = MaterialType::Glass;
        m.weight = 0.8;
        m.rough = 0.05;
        m.spec = 0.7;
        m.ior = 1.52;
        m.att = 0.3;
        m.flux = 1.5;
        m.emit = 0.25;
        m.ldr = 0.4;
        m.metal = 0.6;
        m.alpha = 0.35;
        m.density = 0.9;
        let expected = *m;

        let decoded = from_vox_bytes(&to_vox_bytes(&proj)).expect("decode");
        let got = decoded.materials[7];
        assert_eq!(got.kind, expected.kind);
        for (a, b) in [
            (got.weight, expected.weight),
            (got.rough, expected.rough),
            (got.spec, expected.spec),
            (got.ior, expected.ior),
            (got.att, expected.att),
            (got.flux, expected.flux),
            (got.emit, expected.emit),
            (got.ldr, expected.ldr),
            (got.metal, expected.metal),
            (got.alpha, expected.alpha),
            (got.density, expected.density),
        ] {
            assert!((a - b).abs() < 1e-3, "{a} != {b}");
        }
        // Untouched materials stay default and produce no MATL chunk.
        assert_eq!(decoded.materials[8], proj.materials[8]);
    }

    #[test]
    fn test_vox_reads_official_matl_keys() {
        let mut dict = HashMap::new();
        dict.insert("_type".to_string(), "_glass".to_string());
        dict.insert("_trans".to_string(), "0.75".to_string());
        dict.insert("_d".to_string(), "0.5".to_string());
        dict.insert("_ior".to_string(), "0.3".to_string());
        let mut m = Material::default();
        apply_matl_dict(&mut m, &dict);
        assert_eq!(m.kind, MaterialType::Glass);
        assert!((m.alpha - 0.25).abs() < 1e-6);
        assert!((m.density - 0.5).abs() < 1e-6);
        assert!((m.ior - 1.3).abs() < 1e-6, "_ior is stored as ior - 1");
    }

    use glam::IVec3;
    use mv_core::{Axis, NodeKind, Rotation, Transform, VoxelGrid};

    #[test]
    fn test_vox_multi_model_scene_roundtrip() {
        let mut proj = EditorProject::new_empty(4, 4, 4);
        proj.grid.set(0, 0, 0, 5); // live edit of the active model must be saved
        let mut tree = VoxelGrid::new(3, 3, 8);
        tree.set(1, 1, 7, 9);
        let tree_id = proj.scene.add_model(tree);

        let root = proj.scene.root();
        let group = proj
            .scene
            .add_group(root, Transform::from_translation(IVec3::new(20, -4, 0)))
            .unwrap();
        let s1 = proj
            .scene
            .add_shape(
                group,
                tree_id,
                Transform {
                    rotation: Rotation::rot90(Axis::Z),
                    translation: IVec3::new(1, 2, 3),
                },
            )
            .unwrap();
        let s2 = proj.scene.instantiate(s1).unwrap();
        proj.scene
            .translate_world(s2, IVec3::new(10, 0, 0))
            .unwrap();
        {
            let n = proj.scene.node_mut(s2).unwrap();
            n.name = Some("tree copy".to_string());
            n.hidden = true;
            n.layer_id = 3;
        }
        proj.scene.layers[3].name = "forest".to_string();
        proj.scene.layers[3].hidden = true;
        proj.scene.layers[3].color = [10, 200, 30];
        // An orphan model (no shape) is not written.
        proj.scene.add_model(VoxelGrid::new(2, 2, 2));

        let decoded = from_vox_bytes(&to_vox_bytes(&proj)).expect("decode");
        let sc = &decoded.scene;
        assert_eq!(sc.models.len(), 2);
        assert_eq!(decoded.active_model, 0);
        assert_eq!(decoded.grid.get(0, 0, 0), 5);

        let shapes = sc.shapes_under(sc.root());
        assert_eq!(shapes.len(), 3);
        let original_shapes = proj.scene.shapes_under(proj.scene.root());
        for (&a, &b) in original_shapes.iter().zip(&shapes) {
            assert_eq!(proj.scene.world_transform(a), sc.world_transform(b));
            assert_eq!(proj.scene.world_bounds(a), sc.world_bounds(b));
        }
        // Instancing survives: both tree shapes reference the same model.
        assert_eq!(sc.node(shapes[1]).unwrap().model_id(), Some(1));
        assert_eq!(sc.node(shapes[2]).unwrap().model_id(), Some(1));
        assert_eq!(sc.models[1].get(1, 1, 7), 9);

        let copy = sc.node(shapes[2]).unwrap();
        assert_eq!(copy.name.as_deref(), Some("tree copy"));
        assert!(copy.hidden);
        assert_eq!(copy.layer_id, 3);
        assert_eq!(sc.layers[3].name, "forest");
        assert!(sc.layers[3].hidden);
        assert_eq!(sc.layers[3].color, [10, 200, 30]);
        assert!(matches!(
            sc.node(sc.node(shapes[1]).unwrap().parent.unwrap())
                .unwrap()
                .kind,
            NodeKind::Group { .. }
        ));
    }

    fn dict(pairs: &[(&str, &str)]) -> Vec<u8> {
        let mut out = (pairs.len() as i32).to_le_bytes().to_vec();
        for (k, v) in pairs {
            write_vox_string(&mut out, k);
            write_vox_string(&mut out, v);
        }
        out
    }

    fn ints(v: &[i32]) -> Vec<u8> {
        v.iter().flat_map(|i| i.to_le_bytes()).collect()
    }

    fn trn(
        id: i32,
        attrs: &[(&str, &str)],
        child: i32,
        layer: i32,
        frame: &[(&str, &str)],
    ) -> Vec<u8> {
        let mut c = ints(&[id]);
        c.extend(dict(attrs));
        c.extend(ints(&[child, -1, layer, 1]));
        c.extend(dict(frame));
        c
    }

    /// Hand-built file following the layout MagicaVoxel 0.99 writes, with the chunk order
    /// shuffled (children before parents) to check the reader resolves ids, not order.
    #[test]
    fn test_vox_reads_magicavoxel_scene_layout() {
        let mut body = Vec::new();
        for (size, vox) in [([2i32, 2, 2], [0u8, 0, 0, 7]), ([1, 1, 4], [0, 0, 3, 8])] {
            write_chunk(&mut body, b"SIZE", &ints(&size), &[]);
            let mut x = ints(&[1]);
            x.extend_from_slice(&vox);
            write_chunk(&mut body, b"XYZI", &x, &[]);
        }
        let shp = |id: i32, model: i32| {
            let mut c = ints(&[id]);
            c.extend(dict(&[]));
            c.extend(ints(&[1, model]));
            c.extend(dict(&[]));
            c
        };
        let shp3 = shp(3, 0);
        let shp5 = shp(5, 1);
        write_chunk(&mut body, b"nSHP", &shp5, &[]);
        write_chunk(
            &mut body,
            b"nTRN",
            &trn(
                4,
                &[("_name", "pole")],
                5,
                1,
                &[("_r", "17"), ("_t", "8 0 2")],
            ),
            &[],
        );
        write_chunk(&mut body, b"nSHP", &shp3, &[]);
        write_chunk(
            &mut body,
            b"nTRN",
            &trn(2, &[], 3, 0, &[("_t", "0 0 1")]),
            &[],
        );
        let mut grp = ints(&[1]);
        grp.extend(dict(&[]));
        grp.extend(ints(&[2, 2, 4]));
        write_chunk(&mut body, b"nGRP", &grp, &[]);
        write_chunk(&mut body, b"nTRN", &trn(0, &[], 1, -1, &[]), &[]);
        let mut layr = ints(&[1]);
        layr.extend(dict(&[("_name", "props"), ("_hidden", "1")]));
        layr.extend(ints(&[-1]));
        write_chunk(&mut body, b"LAYR", &layr, &[]);

        let mut file = b"VOX ".to_vec();
        file.extend(ints(&[150]));
        write_chunk(&mut file, b"MAIN", &[], &body);

        let proj = from_vox_bytes(&file).expect("decode");
        let sc = &proj.scene;
        let shapes = sc.shapes_under(sc.root());
        assert_eq!(shapes.len(), 2);
        assert_eq!(sc.models.len(), 2);

        let cube = sc.node(shapes[0]).unwrap();
        assert_eq!(cube.model_id(), Some(0));
        assert_eq!(cube.transform.translation, IVec3::new(0, 0, 1));

        let pole = sc.node(shapes[1]).unwrap();
        assert_eq!(pole.name.as_deref(), Some("pole"));
        assert_eq!(pole.layer_id, 1);
        assert_eq!(pole.transform.rotation, Rotation::from_vox_byte(17));
        assert_eq!(pole.transform.translation, IVec3::new(8, 0, 2));

        assert_eq!(sc.layers[1].name, "props");
        assert!(sc.layers[1].hidden);
        assert!(!sc.is_visible(shapes[1]));
        assert_eq!(proj.active_model, 0, "first visible shape is edited");
        assert_eq!(proj.grid.get(0, 0, 0), 7);
    }

    #[test]
    fn test_vox_cyclic_graph_does_not_hang() {
        let mut body = Vec::new();
        write_chunk(&mut body, b"SIZE", &ints(&[1, 1, 1]), &[]);
        write_chunk(&mut body, b"XYZI", &ints(&[0]), &[]);
        write_chunk(&mut body, b"nTRN", &trn(0, &[], 1, -1, &[]), &[]);
        let mut grp = ints(&[1]);
        grp.extend(dict(&[]));
        grp.extend(ints(&[1, 0])); // group lists the root transform as its child
        write_chunk(&mut body, b"nGRP", &grp, &[]);
        let mut file = b"VOX ".to_vec();
        file.extend(ints(&[150]));
        write_chunk(&mut file, b"MAIN", &[], &body);

        let proj = from_vox_bytes(&file).expect("decode");
        assert_eq!(proj.scene.node_count(), 1);
    }

    #[test]
    fn test_vox_animation_roundtrip() {
        let mut proj = EditorProject::new_empty(3, 3, 3);
        proj.grid.set(0, 0, 0, 1);
        proj.add_frame(false).unwrap();
        proj.grid.set(1, 1, 1, 2);
        proj.add_frame(true).unwrap();
        proj.grid.set(2, 2, 2, 3);
        let shape = proj.active_shape.unwrap();
        proj.scene.node_mut(shape).unwrap().extra_transform_keys =
            vec![(10, Transform::from_translation(IVec3::new(4, 5, 6)))];

        let decoded = from_vox_bytes(&to_vox_bytes(&proj)).expect("decode");
        let sc = &decoded.scene;
        let s = decoded.active_shape.expect("active shape");
        let frames = sc.node(s).unwrap().frame_models().to_vec();
        assert_eq!(frames.len(), 3);
        assert_eq!(sc.models[frames[0]].count_solid(), 1);
        assert_eq!(sc.models[frames[1]].get(1, 1, 1), 2);
        assert_eq!(sc.models[frames[1]].get(0, 0, 0), 1);
        assert_eq!(sc.models[frames[2]].count_solid(), 1);
        assert_eq!(sc.models[frames[2]].get(2, 2, 2), 3);
        assert_eq!(
            sc.node(s).unwrap().extra_transform_keys,
            vec![(10, Transform::from_translation(IVec3::new(4, 5, 6)))]
        );
    }

    #[test]
    fn test_vox_sparse_model_keys_hold_until_next_key() {
        let mut body = Vec::new();
        for c in [1u8, 2] {
            write_chunk(&mut body, b"SIZE", &ints(&[1, 1, 1]), &[]);
            let mut x = ints(&[1]);
            x.extend_from_slice(&[0, 0, 0, c]);
            write_chunk(&mut body, b"XYZI", &x, &[]);
        }
        write_chunk(&mut body, b"nTRN", &trn(0, &[], 1, -1, &[]), &[]);
        let mut grp = ints(&[1]);
        grp.extend(dict(&[]));
        grp.extend(ints(&[1, 2]));
        write_chunk(&mut body, b"nGRP", &grp, &[]);
        write_chunk(&mut body, b"nTRN", &trn(2, &[], 3, 0, &[]), &[]);
        let mut shp = ints(&[3]);
        shp.extend(dict(&[]));
        shp.extend(ints(&[2, 0]));
        shp.extend(dict(&[("_f", "0")]));
        shp.extend(ints(&[1]));
        shp.extend(dict(&[("_f", "3")]));
        write_chunk(&mut body, b"nSHP", &shp, &[]);
        let mut file = b"VOX ".to_vec();
        file.extend(ints(&[150]));
        write_chunk(&mut file, b"MAIN", &[], &body);

        let proj = from_vox_bytes(&file).expect("decode");
        let s = proj.active_shape.unwrap();
        assert_eq!(proj.scene.node(s).unwrap().frame_models(), &[0, 0, 0, 1]);
    }

    #[test]
    fn test_loaded_files_start_from_plain_materials() {
        // A file whose MATL for index 19 only sets the type must not inherit the demo
        // project's emissive preset on that index.
        let mut proj = EditorProject::new_empty(2, 2, 2);
        proj.grid.set(0, 0, 0, 19);
        proj.materials[20].kind = MaterialType::Metal; // forces MATL chunks to exist
        let decoded = from_vox_bytes(&to_vox_bytes(&proj)).unwrap();
        assert_eq!(
            decoded.materials[19],
            Material {
                id: 19,
                ..Default::default()
            }
        );
        assert_eq!(decoded.materials[79].kind, MaterialType::Diffuse);
        assert_eq!(decoded.materials[217].kind, MaterialType::Diffuse);
    }

    /// Parse `v` and `f` lines of an OBJ written by `export_obj_greedy`:
    /// per object, its vertices and quads (vertex indices + normal index).
    fn read_obj(
        text: &str,
    ) -> (
        Vec<[f32; 3]>,
        Vec<[f32; 3]>,
        Vec<(String, Vec<([usize; 4], usize)>)>,
    ) {
        let (mut verts, mut normals, mut objects) = (Vec::new(), Vec::new(), Vec::new());
        for line in text.lines() {
            let mut it = line.split_whitespace();
            match it.next() {
                Some("v") => {
                    let v: Vec<f32> = it.take(3).map(|x| x.parse().unwrap()).collect();
                    verts.push([v[0], v[1], v[2]]);
                }
                Some("vn") => {
                    let v: Vec<f32> = it.map(|x| x.parse().unwrap()).collect();
                    normals.push([v[0], v[1], v[2]]);
                }
                Some("o") => objects.push((it.next().unwrap().to_string(), Vec::new())),
                Some("f") => {
                    let parts: Vec<(usize, usize)> = it
                        .map(|p| {
                            let (v, n) = p.split_once("//").unwrap();
                            (v.parse().unwrap(), n.parse().unwrap())
                        })
                        .collect();
                    let quad = [parts[0].0, parts[1].0, parts[2].0, parts[3].0];
                    objects.last_mut().unwrap().1.push((quad, parts[0].1));
                }
                _ => {}
            }
        }
        (verts, normals, objects)
    }

    fn export_obj_text(proj: &EditorProject) -> String {
        let path = std::env::temp_dir().join(format!(
            "mv-rs-obj-{}-{:?}.obj",
            std::process::id(),
            std::thread::current().id()
        ));
        export_obj_greedy(proj, &path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        text
    }

    #[test]
    fn test_obj_export_is_a_rotation_not_a_mirror() {
        // A voxel at the back (high Y) and the top (high Z) of a 2x4x3 model, which is
        // centered on its node (half size = (1, 2, 1)): world cell (-1, 1, 1).
        let mut proj = EditorProject::new_empty(2, 4, 3);
        proj.grid.set(0, 3, 2, 5);
        let (verts, _, objects) = read_obj(&export_obj_text(&proj));
        assert_eq!((verts.len(), objects.len()), (24, 1));
        // MagicaVoxel up (Z in 1..2) becomes OBJ up (Y); depth Y in 1..2 becomes -Z.
        assert!(verts.iter().all(|v| (1.0..=2.0).contains(&v[1])));
        assert!(verts.iter().all(|v| (-2.0..=-1.0).contains(&v[2])));
    }

    #[test]
    fn test_obj_exports_the_whole_scene_with_outward_faces() {
        // Edited model + a mirrored, rotated instance and a hidden one.
        let mut proj = EditorProject::new_empty(3, 2, 4);
        proj.grid.full(7);
        proj.grid.set(0, 0, 3, 0); // asymmetric
        let root = proj.scene.root();
        let base = proj.scene.shapes_under(root)[0];
        let mirrored = proj.scene.instantiate(base).unwrap();
        {
            let n = proj.scene.node_mut(mirrored).unwrap();
            n.transform.rotation = Rotation::from_vox_byte(0b0010100);
            n.transform.translation = IVec3::new(10, -3, 2);
            n.name = Some("mirrored copy".into());
        }
        let hidden = proj.scene.instantiate(base).unwrap();
        proj.scene.node_mut(hidden).unwrap().hidden = true;
        assert!(proj
            .scene
            .node(mirrored)
            .unwrap()
            .transform
            .rotation
            .is_mirror());

        let (verts, normals, objects) = read_obj(&export_obj_text(&proj));
        assert_eq!(objects.len(), 2, "hidden shape skipped");
        assert_eq!(objects[1].0, "mirrored_copy");
        let to_mv = |v: [f32; 3]| [v[0], -v[2], v[1]]; // OBJ (x, y, z) -> MagicaVoxel
        for ((_, faces), shape) in objects.iter().zip([base, mirrored]) {
            let (lo, hi) = proj.scene.world_bounds(shape).unwrap();
            let (mut elo, mut ehi) = ([f32::MAX; 3], [f32::MIN; 3]);
            for (quad, n) in faces {
                let p: Vec<[f32; 3]> = quad.iter().map(|&i| verts[i - 1]).collect();
                for v in &p {
                    let m = to_mv(*v);
                    for k in 0..3 {
                        elo[k] = elo[k].min(m[k]);
                        ehi[k] = ehi[k].max(m[k]);
                    }
                }
                // Counter-clockwise from outside: geometric normal along the `vn`.
                let e1 = [p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]];
                let e2 = [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]];
                let g = [
                    e1[1] * e2[2] - e1[2] * e2[1],
                    e1[2] * e2[0] - e1[0] * e2[2],
                    e1[0] * e2[1] - e1[1] * e2[0],
                ];
                let vn = normals[n - 1];
                assert!(
                    g[0] * vn[0] + g[1] * vn[1] + g[2] * vn[2] > 0.0,
                    "face winding"
                );
            }
            assert_eq!(elo, lo.as_vec3().to_array());
            assert_eq!(ehi, (hi + IVec3::ONE).as_vec3().to_array());
        }
    }

    #[test]
    fn test_ply_exports_world_voxel_positions() {
        let mut proj = EditorProject::new_empty(2, 2, 2);
        proj.grid.set(1, 1, 1, 3);
        let root = proj.scene.root();
        let s = proj.scene.shapes_under(root)[0];
        let copy = proj.scene.instantiate(s).unwrap();
        proj.scene
            .translate_world(copy, IVec3::new(5, 0, 0))
            .unwrap();
        let path = std::env::temp_dir().join(format!("mv-rs-ply-{}.ply", std::process::id()));
        assert_eq!(export_ply_point_cloud(&proj, &path).unwrap(), 2);
        let text = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        // Voxel (1,1,1) of a 2³ model centered on its node -> world (0,0,0) and (5,0,0).
        assert!(text.contains("\n0 0 0 "), "{text}");
        assert!(text.contains("\n5 0 0 "), "{text}");
    }
}
