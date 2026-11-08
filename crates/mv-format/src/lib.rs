use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::Path;

use mv_core::{EditorProject, Material, MaterialType, Palette, Rgba};
use thiserror::Error;

mod scene_io;

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

/// Export `VoxelGrid` to Wavefront `.obj` using 2D Greedy Meshing per slice to minimize quad count
pub fn export_obj_greedy(
    project: &EditorProject,
    path: impl AsRef<Path>,
) -> Result<usize, FormatError> {
    let grid = &project.grid;
    let file = File::create(path)?;
    let mut w = BufWriter::new(file);

    writeln!(w, "# Exported by MagicaVoxel Rust Edition (Greedy Meshing)")?;
    writeln!(w, "o {}", project.name)?;

    let mut vertices: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut colors: Vec<Rgba> = Vec::new();
    let mut quads: Vec<([usize; 4], usize)> = Vec::new();

    let dims = [
        grid.size_x() as i32,
        grid.size_y() as i32,
        grid.size_z() as i32,
    ];

    // Sweep along each axis d in 0..3 and sign in [-1, +1]
    for d in 0..3usize {
        let u = (d + 1) % 3;
        let v = (d + 2) % 3;
        let du = dims[u] as usize;
        let dv = dims[v] as usize;

        for &dir in &[-1i32, 1i32] {
            let mut n = [0.0f32; 3];
            n[d] = dir as f32;
            normals.push(n);
            let normal_idx = normals.len();

            for slice in 0..dims[d] {
                let mut mask = vec![0u8; du * dv];
                for j in 0..dv {
                    for i in 0..du {
                        let mut pos = [0i32; 3];
                        pos[d] = slice;
                        pos[u] = i as i32;
                        pos[v] = j as i32;
                        let c = grid.get_i32(pos[0], pos[1], pos[2]);
                        if c != 0 {
                            let mut npos = pos;
                            npos[d] += dir;
                            if grid.get_i32(npos[0], npos[1], npos[2]) == 0 {
                                mask[i + j * du] = c;
                            }
                        }
                    }
                }

                // Greedy 2D rectangle merge on `mask`
                for j in 0..dv {
                    let mut i = 0;
                    while i < du {
                        let c = mask[i + j * du];
                        if c == 0 {
                            i += 1;
                            continue;
                        }
                        let mut width = 1;
                        while i + width < du && mask[i + width + j * du] == c {
                            width += 1;
                        }
                        let mut height = 1;
                        'outer: while j + height < dv {
                            for k in 0..width {
                                if mask[i + k + (j + height) * du] != c {
                                    break 'outer;
                                }
                            }
                            height += 1;
                        }

                        // Clear merged region
                        for hj in 0..height {
                            for wi in 0..width {
                                mask[i + wi + (j + hj) * du] = 0;
                            }
                        }

                        let plane_coord = if dir > 0 { slice + 1 } else { slice } as f32;
                        let u0 = i as f32;
                        let u1 = (i + width) as f32;
                        let v0 = j as f32;
                        let v1 = (j + height) as f32;

                        let make_vert = |uu: f32, vv: f32| -> [f32; 3] {
                            let mut pt = [0.0f32; 3];
                            pt[d] = plane_coord;
                            pt[u] = uu;
                            pt[v] = vv;
                            // Convert MV (X, Y-depth, Z-up) to standard OBJ (X, Z-up -> Y, -Y -> Z)
                            [pt[0], pt[2], pt[1]]
                        };

                        let corners = if dir > 0 {
                            [
                                make_vert(u0, v0),
                                make_vert(u1, v0),
                                make_vert(u1, v1),
                                make_vert(u0, v1),
                            ]
                        } else {
                            [
                                make_vert(u0, v0),
                                make_vert(u0, v1),
                                make_vert(u1, v1),
                                make_vert(u1, v0),
                            ]
                        };

                        let base_v = vertices.len() + 1;
                        let col = project.palette.colors[c as usize];
                        for corner in corners {
                            vertices.push(corner);
                            colors.push(col);
                        }
                        quads.push(([base_v, base_v + 1, base_v + 2, base_v + 3], normal_idx));

                        i += width;
                    }
                }
            }
        }
    }

    for (v, col) in vertices.iter().zip(colors.iter()) {
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
    for n in &normals {
        writeln!(w, "vn {:.3} {:.3} {:.3}", n[0], n[2], n[1])?;
    }
    for (q, n_idx) in &quads {
        writeln!(
            w,
            "f {}//{} {}//{} {}//{} {}//{}",
            q[0], n_idx, q[1], n_idx, q[2], n_idx, q[3], n_idx
        )?;
    }

    Ok(quads.len())
}

/// Export voxel point cloud to Stanford `.ply` format (`o ply`)
pub fn export_ply_point_cloud(
    project: &EditorProject,
    path: impl AsRef<Path>,
) -> Result<usize, FormatError> {
    let grid = &project.grid;
    let mut points = Vec::new();
    for z in 0..grid.size_z() {
        for y in 0..grid.size_y() {
            for x in 0..grid.size_x() {
                let c = grid.get(x, y, z);
                if c != 0 && !grid.is_interior_solid(x as i32, y as i32, z as i32) {
                    points.push((x, y, z, project.palette.colors[c as usize]));
                }
            }
        }
    }

    let file = File::create(path)?;
    let mut w = BufWriter::new(file);
    writeln!(w, "ply")?;
    writeln!(w, "format ascii 1.0")?;
    writeln!(w, "comment Exported by MagicaVoxel Rust Edition")?;
    writeln!(w, "element vertex {}", points.len())?;
    writeln!(w, "property float x")?;
    writeln!(w, "property float y")?;
    writeln!(w, "property float z")?;
    writeln!(w, "property uchar red")?;
    writeln!(w, "property uchar green")?;
    writeln!(w, "property uchar blue")?;
    writeln!(w, "end_header")?;

    for (x, y, z, col) in &points {
        writeln!(w, "{} {} {} {} {} {}", x, y, z, col.r, col.g, col.b)?;
    }
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
}
