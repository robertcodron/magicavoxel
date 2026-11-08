use std::collections::HashMap;
use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::Path;

use mv_core::{EditorProject, MaterialType, Palette, Rgba, VoxelGrid};
use thiserror::Error;

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
    let grid = &project.grid;
    let mut xyzi_entries: Vec<[u8; 4]> = Vec::new();
    for z in 0..grid.size_z() {
        for y in 0..grid.size_y() {
            for x in 0..grid.size_x() {
                let c = grid.get(x, y, z);
                if c != 0 {
                    xyzi_entries.push([x as u8, y as u8, z as u8, c]);
                }
            }
        }
    }

    let mut children = Vec::new();

    // 1. SIZE chunk (12 bytes)
    let mut size_content = Vec::with_capacity(12);
    size_content.extend_from_slice(&(grid.size_x() as u32).to_le_bytes());
    size_content.extend_from_slice(&(grid.size_y() as u32).to_le_bytes());
    size_content.extend_from_slice(&(grid.size_z() as u32).to_le_bytes());
    write_chunk(&mut children, b"SIZE", &size_content, &[]);

    // 2. XYZI chunk (4 + 4 * N bytes)
    let mut xyzi_content = Vec::with_capacity(4 + xyzi_entries.len() * 4);
    xyzi_content.extend_from_slice(&(xyzi_entries.len() as u32).to_le_bytes());
    for entry in &xyzi_entries {
        xyzi_content.extend_from_slice(entry);
    }
    write_chunk(&mut children, b"XYZI", &xyzi_content, &[]);

    // 3. RGBA chunk (256 * 4 bytes; in .vox format, palette[0..254] stores colors 1..255)
    let mut rgba_content = Vec::with_capacity(1024);
    for i in 1..=255usize {
        let c = project.palette.colors[i];
        rgba_content.extend_from_slice(&[c.r, c.g, c.b, c.a]);
    }
    rgba_content.extend_from_slice(&[0, 0, 0, 0]);
    write_chunk(&mut children, b"RGBA", &rgba_content, &[]);

    // 4. MATL chunks for non-default materials
    for i in 1..=255usize {
        let mat = &project.materials[i];
        if mat.kind != MaterialType::Diffuse || (mat.rough - 0.1).abs() > 1e-4 {
            let mut matl = Vec::new();
            matl.extend_from_slice(&(i as i32).to_le_bytes());
            let pairs = [
                ("_type", mat.kind.as_str().to_string()),
                ("_weight", format!("{:.4}", mat.weight)),
                ("_rough", format!("{:.4}", mat.rough)),
                ("_spec", format!("{:.4}", mat.spec)),
                ("_ior", format!("{:.4}", mat.ior)),
                ("_att", format!("{:.4}", mat.att)),
                ("_flux", format!("{:.4}", mat.flux)),
                ("_emit", format!("{:.4}", mat.emit)),
            ];
            matl.extend_from_slice(&(pairs.len() as i32).to_le_bytes());
            for (k, v) in &pairs {
                write_vox_string(&mut matl, k);
                write_vox_string(&mut matl, v);
            }
            write_chunk(&mut children, b"MATL", &matl, &[]);
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
    let mut pending_size: Option<(u32, u32, u32)> = None;
    let mut loaded_first_model = false;

    while cursor + 12 <= end {
        let id = &bytes[cursor..cursor + 4];
        let content_len = read_u32_le(bytes, cursor + 4)? as usize;
        let children_len = read_u32_le(bytes, cursor + 8)? as usize;
        let content_start = cursor + 12;
        let content_end = (content_start + content_len).min(bytes.len());
        let content = &bytes[content_start..content_end];

        match id {
            b"SIZE" if content.len() >= 12 => {
                let sx = read_u32_le(content, 0)?.clamp(1, 256);
                let sy = read_u32_le(content, 4)?.clamp(1, 256);
                let sz = read_u32_le(content, 8)?.clamp(1, 256);
                pending_size = Some((sx, sy, sz));
            }
            b"XYZI" if content.len() >= 4 && !loaded_first_model => {
                let (sx, sy, sz) = pending_size.unwrap_or((40, 40, 40));
                let mut grid = VoxelGrid::new(sx, sy, sz);
                let num_voxels = read_u32_le(content, 0)? as usize;
                let max_voxels = (content.len() - 4) / 4;
                for i in 0..num_voxels.min(max_voxels) {
                    let base = 4 + i * 4;
                    let x = content[base] as u32;
                    let y = content[base + 1] as u32;
                    let z = content[base + 2] as u32;
                    let c = content[base + 3];
                    grid.set(x, y, z, c);
                }
                project.grid = grid;
                project.selection.resize(sx, sy, sz);
                loaded_first_model = true;
            }
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
                if let Ok((mat_id, dict)) = parse_vox_dict(content) {
                    let idx = (mat_id as usize).clamp(1, 255);
                    let m = &mut project.materials[idx];
                    if let Some(t) = dict.get("_type") {
                        m.kind = MaterialType::from_vox_str(t);
                    }
                    if let Some(v) = dict.get("_weight").and_then(|s| s.parse().ok()) {
                        m.weight = v;
                    }
                    if let Some(v) = dict.get("_rough").and_then(|s| s.parse().ok()) {
                        m.rough = v;
                    }
                    if let Some(v) = dict.get("_spec").and_then(|s| s.parse().ok()) {
                        m.spec = v;
                    }
                    if let Some(v) = dict.get("_ior").and_then(|s| s.parse().ok()) {
                        m.ior = v;
                    }
                    if let Some(v) = dict.get("_emit").and_then(|s| s.parse().ok()) {
                        m.emit = v;
                    }
                    if let Some(v) = dict.get("_flux").and_then(|s| s.parse().ok()) {
                        m.flux = v;
                    }
                }
            }
            _ => {}
        }

        cursor = content_start + content_len + children_len;
    }

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
pub fn export_obj_greedy(project: &EditorProject, path: impl AsRef<Path>) -> Result<usize, FormatError> {
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
pub fn export_ply_point_cloud(project: &EditorProject, path: impl AsRef<Path>) -> Result<usize, FormatError> {
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
pub fn export_slice_png(project: &EditorProject, path: impl AsRef<Path>) -> Result<(), FormatError> {
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

fn write_chunk(out: &mut Vec<u8>, id: &[u8; 4], content: &[u8], children: &[u8]) {
    out.extend_from_slice(id);
    out.extend_from_slice(&(content.len() as u32).to_le_bytes());
    out.extend_from_slice(&(children.len() as u32).to_le_bytes());
    out.extend_from_slice(content);
    out.extend_from_slice(children);
}

fn write_vox_string(out: &mut Vec<u8>, s: &str) {
    let b = s.as_bytes();
    out.extend_from_slice(&(b.len() as u32).to_le_bytes());
    out.extend_from_slice(b);
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Result<u32, FormatError> {
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

fn read_i32_le(bytes: &[u8], offset: usize) -> Result<i32, FormatError> {
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

fn read_vox_string(bytes: &[u8], offset: usize) -> Result<(String, usize), FormatError> {
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
}

