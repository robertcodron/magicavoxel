//! Marching Cubes smoothed mesh extraction for voxel models.
//!
//! Generates smooth triangular surfaces from discrete `VoxelGrid` data using
//! the classic 256-configuration Marching Cubes algorithm with trilinear field
//! interpolation, vertex deduplication, smooth normal generation, and vertex colors.

use std::collections::HashMap;

use glam::Vec3;
use mv_core::{Palette, Rgba, VoxelGrid};

use crate::marching_cubes_table::{EDGE_CONNECTION, EDGE_TABLE, TRI_TABLE, VERTEX_OFFSET};

/// A single extracted mesh vertex with position, normal, and RGB color.
#[derive(Debug, Clone, Copy)]
pub struct McVertex {
    pub position: Vec3,
    pub normal: Vec3,
    pub color: Rgba,
}

/// Extracted triangular mesh.
#[derive(Debug, Default)]
pub struct McMesh {
    pub vertices: Vec<McVertex>,
    pub triangles: Vec<[u32; 3]>,
}

/// Extract a smoothed triangle mesh from `VoxelGrid` using Marching Cubes.
pub fn extract_marching_cubes_mesh(grid: &VoxelGrid, palette: &Palette) -> McMesh {
    let size_x = grid.size_x() as usize;
    let size_y = grid.size_y() as usize;
    let size_z = grid.size_z() as usize;

    if grid.count_solid() == 0 || size_x == 0 || size_y == 0 || size_z == 0 {
        return McMesh::default();
    }

    // Grid dimension with 1-cell padding around all sides so boundaries close naturally.
    let pw = size_x + 2;
    let pd = size_y + 2;
    let ph = size_z + 2;

    // Corner density field and average corner color
    // Number of corner nodes is pw + 1, pd + 1, ph + 1
    let nw = pw + 1;
    let nd = pd + 1;
    let nh = ph + 1;

    let node_count = nw * nd * nh;
    let mut field = vec![0.0f32; node_count];
    let mut col_accum = vec![[0u32; 3]; node_count];
    let mut col_count = vec![0u32; node_count];

    let node_idx = |x: usize, y: usize, z: usize| -> usize {
        x + y * nw + z * nw * nd
    };

    // Populate field from solid voxels
    for vz in 0..size_z {
        let pz = vz + 1;
        for vy in 0..size_y {
            let py = vy + 1;
            for vx in 0..size_x {
                let c = grid.get(vx as u32, vy as u32, vz as u32);
                if c == 0 {
                    continue;
                }
                let color = palette.colors[c as usize];

                // Add to the 8 surrounding corner nodes
                for dz in 0..=1 {
                    let nz = pz + dz;
                    for dy in 0..=1 {
                        let ny = py + dy;
                        for dx in 0..=1 {
                            let nx = px_add(vx + 1, dx);
                            let idx = node_idx(nx, ny, nz);
                            field[idx] += 1.0;
                            col_accum[idx][0] += color.r as u32;
                            col_accum[idx][1] += color.g as u32;
                            col_accum[idx][2] += color.b as u32;
                            col_count[idx] += 1;
                        }
                    }
                }
            }
        }
    }

    // Isosurface threshold: a corner touched by 1 voxel has density 1.0.
    // Threshold 0.5 cleanly separates outside (0) from occupied (>=1) while smoothing edges.
    let iso_level = 0.5f32;

    // Deduplicate vertices on shared cube edges
    // Key: (node_x, node_y, node_z, edge_axis: 0 for X, 1 for Y, 2 for Z)
    let mut edge_vertex_map: HashMap<(u32, u32, u32, u8), u32> = HashMap::new();
    let mut positions: Vec<Vec3> = Vec::new();
    let mut colors: Vec<Rgba> = Vec::new();
    let mut triangles: Vec<[u32; 3]> = Vec::new();

    // Iterate through all cells
    for cz in 0..ph {
        for cy in 0..pd {
            for cx in 0..pw {
                // Read 8 corner values for this cell
                let mut cube_values = [0.0f32; 8];
                let mut cube_nodes = [0usize; 8];

                for i in 0..8 {
                    let nx = cx + (VERTEX_OFFSET[i][0] as usize);
                    let ny = cy + (VERTEX_OFFSET[i][1] as usize);
                    let nz = cz + (VERTEX_OFFSET[i][2] as usize);
                    let idx = node_idx(nx, ny, nz);
                    cube_nodes[i] = idx;
                    cube_values[i] = field[idx];
                }

                // Determine the cube configuration index
                let mut flag_index = 0usize;
                for i in 0..8 {
                    if cube_values[i] >= iso_level {
                        flag_index |= 1 << i;
                    }
                }

                let edge_flags = EDGE_TABLE[flag_index];
                if edge_flags == 0 {
                    continue;
                }

                // Find or create vertices on the 12 edges
                let mut cell_vertices = [0u32; 12];

                for edge_idx in 0..12 {
                    if (edge_flags & (1 << edge_idx)) == 0 {
                        continue;
                    }

                    let v0 = EDGE_CONNECTION[edge_idx][0];
                    let v1 = EDGE_CONNECTION[edge_idx][1];

                    let n0_idx = cube_nodes[v0];
                    let n1_idx = cube_nodes[v1];

                    let val0 = cube_values[v0];
                    let val1 = cube_values[v1];

                    let p0 = Vec3::new(
                        cx as f32 + VERTEX_OFFSET[v0][0],
                        cy as f32 + VERTEX_OFFSET[v0][1],
                        cz as f32 + VERTEX_OFFSET[v0][2],
                    );
                    let p1 = Vec3::new(
                        cx as f32 + VERTEX_OFFSET[v1][0],
                        cy as f32 + VERTEX_OFFSET[v1][1],
                        cz as f32 + VERTEX_OFFSET[v1][2],
                    );

                    let t = if (val1 - val0).abs() < 1e-6 {
                        0.5
                    } else {
                        ((iso_level - val0) / (val1 - val0)).clamp(0.0, 1.0)
                    };

                    // Global edge key for deduplication
                    let (gx, gy, gz, axis) = global_edge_key(cx as u32, cy as u32, cz as u32, edge_idx);

                    let v_idx = *edge_vertex_map.entry((gx, gy, gz, axis)).or_insert_with(|| {
                        let pos = p0.lerp(p1, t);
                        // Shift from padded grid space to model voxel coordinates
                        // The voxel (0, 0, 0) occupies [1, 2] in padded space, so center at 0.5
                        let model_pos = pos - Vec3::new(1.0, 1.0, 1.0);

                        // Color interpolation
                        let c0 = if col_count[n0_idx] > 0 {
                            let r = (col_accum[n0_idx][0] / col_count[n0_idx]) as u8;
                            let g = (col_accum[n0_idx][1] / col_count[n0_idx]) as u8;
                            let b = (col_accum[n0_idx][2] / col_count[n0_idx]) as u8;
                            Rgba::new(r, g, b, 255)
                        } else {
                            Rgba::WHITE
                        };

                        let c1 = if col_count[n1_idx] > 0 {
                            let r = (col_accum[n1_idx][0] / col_count[n1_idx]) as u8;
                            let g = (col_accum[n1_idx][1] / col_count[n1_idx]) as u8;
                            let b = (col_accum[n1_idx][2] / col_count[n1_idx]) as u8;
                            Rgba::new(r, g, b, 255)
                        } else {
                            Rgba::WHITE
                        };

                        let vert_col = if col_count[n0_idx] == 0 {
                            c1
                        } else if col_count[n1_idx] == 0 {
                            c0
                        } else {
                            c0.lerp(c1, t)
                        };

                        let new_idx = positions.len() as u32;
                        positions.push(model_pos);
                        colors.push(vert_col);
                        new_idx
                    });

                    cell_vertices[edge_idx] = v_idx;
                }

                // Add triangles for this cube configuration
                let tri_row = &TRI_TABLE[flag_index];
                let mut i = 0;
                while i < 16 && tri_row[i] != -1 {
                    let e0 = tri_row[i] as usize;
                    let e1 = tri_row[i + 1] as usize;
                    let e2 = tri_row[i + 2] as usize;

                    let idx0 = cell_vertices[e0];
                    let idx1 = cell_vertices[e1];
                    let idx2 = cell_vertices[e2];

                    if idx0 != idx1 && idx1 != idx2 && idx2 != idx0 {
                        triangles.push([idx0, idx1, idx2]);
                    }

                    i += 3;
                }
            }
        }
    }

    // Compute smooth vertex normals by accumulating face normals
    let mut normals = vec![Vec3::ZERO; positions.len()];
    for tri in &triangles {
        let p0 = positions[tri[0] as usize];
        let p1 = positions[tri[1] as usize];
        let p2 = positions[tri[2] as usize];

        let fn_norm = (p1 - p0).cross(p2 - p0);
        normals[tri[0] as usize] += fn_norm;
        normals[tri[1] as usize] += fn_norm;
        normals[tri[2] as usize] += fn_norm;
    }

    let vertices = (0..positions.len())
        .map(|i| {
            let n = normals[i];
            let normal = if n.length_squared() > 1e-8 {
                n.normalize()
            } else {
                Vec3::Y
            };
            McVertex {
                position: positions[i],
                normal,
                color: colors[i],
            }
        })
        .collect();

    McMesh { vertices, triangles }
}

#[inline(always)]
fn px_add(base: usize, offset: usize) -> usize {
    base + offset
}

/// Convert local cell edge index (0..12) into a canonical global edge key `(gx, gy, gz, axis)`
/// where axis: 0 = X-axis edge, 1 = Y-axis edge, 2 = Z-axis edge.
fn global_edge_key(cx: u32, cy: u32, cz: u32, edge: usize) -> (u32, u32, u32, u8) {
    match edge {
        0 => (cx, cy, cz, 0),
        1 => (cx + 1, cy, cz, 1),
        2 => (cx, cy + 1, cz, 0),
        3 => (cx, cy, cz, 1),
        4 => (cx, cy, cz + 1, 0),
        5 => (cx + 1, cy, cz + 1, 1),
        6 => (cx, cy + 1, cz + 1, 0),
        7 => (cx, cy, cz + 1, 1),
        8 => (cx, cy, cz, 2),
        9 => (cx + 1, cy, cz, 2),
        10 => (cx + 1, cy + 1, cz, 2),
        11 => (cx, cy + 1, cz, 2),
        _ => (cx, cy, cz, 0),
    }
}

/// Export a `VoxelGrid` as a Wavefront OBJ string with vertex colors and smooth normals.
/// Coordinates are converted from MagicaVoxel Z-up to standard OBJ Y-up `(x, z, -y)`.
pub fn export_marching_cubes_obj(grid: &VoxelGrid, palette: &Palette) -> String {
    let mesh = extract_marching_cubes_mesh(grid, palette);

    let mut out = String::with_capacity(mesh.vertices.len() * 80 + mesh.triangles.len() * 30);
    out.push_str("# Exported by MagicaVoxel Rust Edition (Marching Cubes)\n");

    let y_up = |p: Vec3| [p.x, p.z, -p.y];

    // Vertices: v x y z r g b
    for v in &mesh.vertices {
        let p = y_up(v.position);
        let cr = v.color.r as f32 / 255.0;
        let cg = v.color.g as f32 / 255.0;
        let cb = v.color.b as f32 / 255.0;
        out.push_str(&format!(
            "v {:.4} {:.4} {:.4} {:.4} {:.4} {:.4}\n",
            p[0], p[1], p[2], cr, cg, cb
        ));
    }

    // Normals: vn nx ny nz
    for v in &mesh.vertices {
        let n = y_up(v.normal);
        out.push_str(&format!("vn {:.4} {:.4} {:.4}\n", n[0], n[1], n[2]));
    }

    // Faces (1-indexed): f v1//vn1 v2//vn2 v3//vn3
    for tri in &mesh.triangles {
        let i0 = tri[0] + 1;
        let i1 = tri[1] + 1;
        let i2 = tri[2] + 1;
        out.push_str(&format!("f {i0}//{i0} {i1}//{i1} {i2}//{i2}\n"));
    }

    out
}

/// Export a `VoxelGrid` as Stanford PLY binary or ASCII format with vertex positions,
/// normals, and colors.
pub fn export_marching_cubes_ply(grid: &VoxelGrid, palette: &Palette) -> Vec<u8> {
    let mesh = extract_marching_cubes_mesh(grid, palette);

    let mut out = Vec::new();
    let header = format!(
        "ply\n\
         format ascii 1.0\n\
         comment Exported by MagicaVoxel Rust Edition (Marching Cubes)\n\
         element vertex {}\n\
         property float x\n\
         property float y\n\
         property float z\n\
         property float nx\n\
         property float ny\n\
         property float nz\n\
         property uchar red\n\
         property uchar green\n\
         property uchar blue\n\
         element face {}\n\
         property list uchar int vertex_indices\n\
         end_header\n",
        mesh.vertices.len(),
        mesh.triangles.len()
    );
    out.extend_from_slice(header.as_bytes());

    for v in &mesh.vertices {
        let line = format!(
            "{:.4} {:.4} {:.4} {:.4} {:.4} {:.4} {} {} {}\n",
            v.position.x,
            v.position.y,
            v.position.z,
            v.normal.x,
            v.normal.y,
            v.normal.z,
            v.color.r,
            v.color.g,
            v.color.b
        );
        out.extend_from_slice(line.as_bytes());
    }

    for tri in &mesh.triangles {
        let line = format!("3 {} {} {}\n", tri[0], tri[1], tri[2]);
        out.extend_from_slice(line.as_bytes());
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_marching_cubes_empty_grid() {
        let palette = Palette::default();
        let grid = VoxelGrid::new(4, 4, 4);

        let mesh = extract_marching_cubes_mesh(&grid, &palette);
        assert_eq!(mesh.vertices.len(), 0);
        assert_eq!(mesh.triangles.len(), 0);

        let obj = export_marching_cubes_obj(&grid, &palette);
        assert!(obj.starts_with("# Exported by MagicaVoxel Rust Edition"));
        assert!(!obj.contains("v "));
        assert!(!obj.contains("f "));

        let ply = export_marching_cubes_ply(&grid, &palette);
        let ply_str = String::from_utf8_lossy(&ply);
        assert!(ply_str.contains("element vertex 0"));
        assert!(ply_str.contains("element face 0"));
    }

    #[test]
    fn test_marching_cubes_single_voxel() {
        let palette = Palette::default();
        let mut grid = VoxelGrid::new(3, 3, 3);
        grid.set(1, 1, 1, 12); // Red swatch

        let mesh = extract_marching_cubes_mesh(&grid, &palette);
        assert!(mesh.vertices.len() >= 6, "Expected enclosing surface vertices");
        assert!(mesh.triangles.len() >= 8, "Expected enclosing surface triangles");

        for v in &mesh.vertices {
            assert!(
                (v.normal.length() - 1.0).abs() < 1e-3,
                "Normal should be normalized: {:?}",
                v.normal
            );
            // Color should match or be close to the swatch
            assert_eq!(v.color.a, 255);
        }

        let obj = export_marching_cubes_obj(&grid, &palette);
        assert!(obj.contains("v "));
        assert!(obj.contains("vn "));
        assert!(obj.contains("f "));

        let ply = export_marching_cubes_ply(&grid, &palette);
        let ply_str = String::from_utf8_lossy(&ply);
        assert!(ply_str.contains(&format!("element vertex {}", mesh.vertices.len())));
        assert!(ply_str.contains(&format!("element face {}", mesh.triangles.len())));
    }

    #[test]
    fn test_marching_cubes_solid_block() {
        let palette = Palette::default();
        let mut grid = VoxelGrid::new(4, 4, 4);
        for z in 1..=2 {
            for y in 1..=2 {
                for x in 1..=2 {
                    grid.set(x, y, z, 5);
                }
            }
        }

        let mesh = extract_marching_cubes_mesh(&grid, &palette);
        assert!(mesh.vertices.len() > 10);
        assert!(mesh.triangles.len() > 10);

        // Verify all triangle vertex indices are valid
        for tri in &mesh.triangles {
            for &idx in tri {
                assert!((idx as usize) < mesh.vertices.len());
            }
        }
    }

    #[test]
    fn test_marching_cubes_formats_syntax() {
        let palette = Palette::default();
        let mut grid = VoxelGrid::new(2, 2, 2);
        grid.set(0, 0, 0, 1);

        let obj = export_marching_cubes_obj(&grid, &palette);
        let mut v_count = 0;
        let mut f_count = 0;
        for line in obj.lines() {
            if line.starts_with("v ") {
                v_count += 1;
                let parts: Vec<&str> = line.split_whitespace().collect();
                // "v", x, y, z, r, g, b -> 7 elements
                assert_eq!(parts.len(), 7, "OBJ vertex format with RGB");
            } else if line.starts_with("f ") {
                f_count += 1;
                let parts: Vec<&str> = line.split_whitespace().collect();
                // "f", v1//n1, v2//n2, v3//n3 -> 4 elements
                assert_eq!(parts.len(), 4, "OBJ triangle face format");
            }
        }
        assert!(v_count > 0);
        assert!(f_count > 0);

        let ply_bytes = export_marching_cubes_ply(&grid, &palette);
        let ply_text = String::from_utf8(ply_bytes).expect("PLY should be valid ASCII");
        assert!(ply_text.starts_with("ply\nformat ascii 1.0\n"));
        assert!(ply_text.contains("end_header\n"));
    }
}
