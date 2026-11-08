//! Greedy meshing shared by the mesh exporters (OBJ, glTF).
//!
//! For each axis and direction, visible faces of a slice are merged into maximal
//! rectangles of the same color. Quads are in model voxel coordinates (MagicaVoxel
//! axes, Z up), counter-clockwise when seen from outside (right-handed).

use mv_core::VoxelGrid;

#[derive(Debug, Clone, PartialEq)]
pub struct GreedyQuad {
    /// Corners in model space, counter-clockwise seen from the side the normal faces.
    pub corners: [[f32; 3]; 4],
    /// Outward axis-aligned normal.
    pub normal: [i32; 3],
    /// Palette index.
    pub color: u8,
}

pub fn greedy_quads(grid: &VoxelGrid) -> Vec<GreedyQuad> {
    let dims = [
        grid.size_x() as i32,
        grid.size_y() as i32,
        grid.size_z() as i32,
    ];
    let mut quads = Vec::new();

    for d in 0..3usize {
        // (u, v, d) is a cyclic permutation of (x, y, z): u × v = +d.
        let u = (d + 1) % 3;
        let v = (d + 2) % 3;
        let du = dims[u] as usize;
        let dv = dims[v] as usize;
        let mut mask = vec![0u8; du * dv];

        for dir in [-1i32, 1] {
            let mut normal = [0i32; 3];
            normal[d] = dir;
            for slice in 0..dims[d] {
                for j in 0..dv {
                    for i in 0..du {
                        let mut pos = [0i32; 3];
                        pos[d] = slice;
                        pos[u] = i as i32;
                        pos[v] = j as i32;
                        let c = grid.get_i32(pos[0], pos[1], pos[2]);
                        let mut npos = pos;
                        npos[d] += dir;
                        mask[i + j * du] = if c != 0 && grid.get_i32(npos[0], npos[1], npos[2]) == 0
                        {
                            c
                        } else {
                            0
                        };
                    }
                }

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
                        'grow: while j + height < dv {
                            for k in 0..width {
                                if mask[i + k + (j + height) * du] != c {
                                    break 'grow;
                                }
                            }
                            height += 1;
                        }
                        for hj in 0..height {
                            for wi in 0..width {
                                mask[i + wi + (j + hj) * du] = 0;
                            }
                        }

                        let plane = if dir > 0 { slice + 1 } else { slice } as f32;
                        let (u0, u1) = (i as f32, (i + width) as f32);
                        let (v0, v1) = (j as f32, (j + height) as f32);
                        let corner = |uu: f32, vv: f32| {
                            let mut p = [0.0f32; 3];
                            p[d] = plane;
                            p[u] = uu;
                            p[v] = vv;
                            p
                        };
                        let corners = if dir > 0 {
                            [
                                corner(u0, v0),
                                corner(u1, v0),
                                corner(u1, v1),
                                corner(u0, v1),
                            ]
                        } else {
                            [
                                corner(u0, v0),
                                corner(u0, v1),
                                corner(u1, v1),
                                corner(u1, v0),
                            ]
                        };
                        quads.push(GreedyQuad {
                            corners,
                            normal,
                            color: c,
                        });
                        i += width;
                    }
                }
            }
        }
    }
    quads
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
        [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
    }

    fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    }

    #[test]
    fn solid_box_gives_six_merged_faces_wound_outward() {
        let mut g = VoxelGrid::new(3, 2, 4);
        g.full(5);
        let quads = greedy_quads(&g);
        assert_eq!(quads.len(), 6, "one rectangle per side");
        let area: f32 = quads
            .iter()
            .map(|q| {
                let c = cross(
                    sub(q.corners[1], q.corners[0]),
                    sub(q.corners[3], q.corners[0]),
                );
                (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt()
            })
            .sum();
        assert_eq!(area, 2.0 * (3.0 * 2.0 + 3.0 * 4.0 + 2.0 * 4.0));
        for q in &quads {
            let n = cross(
                sub(q.corners[1], q.corners[0]),
                sub(q.corners[2], q.corners[0]),
            );
            let dot =
                n[0] * q.normal[0] as f32 + n[1] * q.normal[1] as f32 + n[2] * q.normal[2] as f32;
            assert!(dot > 0.0, "counter-clockwise from outside: {q:?}");
        }
    }

    #[test]
    fn colors_are_not_merged_and_hidden_faces_skipped() {
        let mut g = VoxelGrid::new(2, 1, 1);
        g.set(0, 0, 0, 1);
        g.set(1, 0, 0, 2);
        let quads = greedy_quads(&g);
        // 2 voxels × 6 faces − 2 shared faces.
        assert_eq!(quads.len(), 10);
        assert_eq!(quads.iter().filter(|q| q.color == 1).count(), 5);
    }
}
