//! Qubicle Binary (`.qb`) format import and export.
//!
//! Specification:
//! - Global Header (24 bytes):
//!   - uint32 version: 0x0101 (v1.1.0)
//!   - uint32 colorFormat: 0 = RGBA, 1 = BGRA
//!   - uint32 zAxisOrientation: 0 = Left-handed, 1 = Right-handed
//!   - uint32 compressed: 0 = Uncompressed, 1 = Compressed (RLE)
//!   - uint32 visibilityMaskEncoded: 0 = false, 1 = true
//!   - uint32 numMatrices: number of matrices stored
//!
//! - Per Matrix:
//!   - uint8 nameLength + ASCII string
//!   - uint32 sizeX, sizeY, sizeZ (Qubicle axes: X=Width, Y=Height, Z=Depth)
//!   - int32  posX, posY, posZ
//!   - Voxel data:
//!     - Uncompressed: sizeX * sizeY * sizeZ * 4 bytes (RGBA / BGRA) in Z -> Y -> X order
//!     - Compressed: slice-by-slice RLE using CODEFLAG (2) and NEXTSLICEFLAG (6)

use std::collections::HashMap;

use glam::IVec3;
use mv_core::{EditorProject, Palette, Rgba, Scene, SelectionMask, Transform, VoxelGrid};

use crate::{placed, FormatError};

pub const QB_VERSION: u32 = 0x0101;
pub const COLOR_FORMAT_RGBA: u32 = 0;
pub const COLOR_FORMAT_BGRA: u32 = 1;
pub const CODEFLAG: u32 = 2;
pub const NEXTSLICEFLAG: u32 = 6;

/// Export an `EditorProject` to binary Qubicle `.qb` format (v1.1.0, RGBA, uncompressed).
/// Visible scene shapes are exported as individual Qubicle matrices.
pub fn export_qb(project: &EditorProject) -> Vec<u8> {
    let shapes = placed::visible_shapes(project);

    // If there are no shapes visible, fallback to exporting the active grid
    let matrices_count = if shapes.is_empty() { 1 } else { shapes.len() } as u32;

    let mut out = Vec::new();

    // 1. Header (24 bytes)
    out.extend_from_slice(&QB_VERSION.to_le_bytes());
    out.extend_from_slice(&COLOR_FORMAT_RGBA.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // zAxisOrientation: 0 = Left-handed
    out.extend_from_slice(&0u32.to_le_bytes()); // compressed: 0 = uncompressed
    out.extend_from_slice(&0u32.to_le_bytes()); // visibilityMaskEncoded: 0 = false
    out.extend_from_slice(&matrices_count.to_le_bytes());

    if shapes.is_empty() {
        write_matrix(
            &mut out,
            &project.name,
            &project.grid,
            IVec3::ZERO,
            &project.palette,
        );
    } else {
        for shape in &shapes {
            write_matrix(
                &mut out,
                &shape.name,
                shape.grid,
                shape.voxel_xf.translation,
                &project.palette,
            );
        }
    }

    out
}

fn write_matrix(
    out: &mut Vec<u8>,
    name: &str,
    grid: &VoxelGrid,
    translation: IVec3,
    palette: &Palette,
) {
    // Matrix name: 1 byte length + string (max 255 bytes)
    let name_bytes = name.as_bytes();
    let name_len = name_bytes.len().min(255) as u8;
    out.push(name_len);
    out.extend_from_slice(&name_bytes[..name_len as usize]);

    // Qubicle: X = Width, Y = Height, Z = Depth
    // MagicaVoxel: X = Width, Y = Depth, Z = Height
    let qb_size_x = grid.size_x();
    let qb_size_y = grid.size_z(); // Height
    let qb_size_z = grid.size_y(); // Depth

    out.extend_from_slice(&qb_size_x.to_le_bytes());
    out.extend_from_slice(&qb_size_y.to_le_bytes());
    out.extend_from_slice(&qb_size_z.to_le_bytes());

    // Position mapping: (X -> X, Y -> Z, Z -> Y)
    let qb_pos_x = translation.x;
    let qb_pos_y = translation.z; // Height
    let qb_pos_z = translation.y; // Depth

    out.extend_from_slice(&qb_pos_x.to_le_bytes());
    out.extend_from_slice(&qb_pos_y.to_le_bytes());
    out.extend_from_slice(&qb_pos_z.to_le_bytes());

    // Voxel data in Z -> Y -> X order
    for qz in 0..qb_size_z {
        let mv_y = qz;
        for qy in 0..qb_size_y {
            let mv_z = qy;
            for qx in 0..qb_size_x {
                let mv_x = qx;
                let c = grid.get(mv_x, mv_y, mv_z);
                if c == 0 {
                    out.extend_from_slice(&[0, 0, 0, 0]);
                } else {
                    let col = palette.colors[c as usize];
                    out.extend_from_slice(&[col.r, col.g, col.b, 255]);
                }
            }
        }
    }
}

/// Helper to map RGBA colors into MagicaVoxel 256-color palette
struct PaletteMapper {
    palette: Palette,
    color_map: HashMap<[u8; 3], u8>,
    next_free_slot: usize,
}

impl PaletteMapper {
    fn new() -> Self {
        let palette = Palette::default();
        let mut color_map = HashMap::new();
        // Register default colors
        for i in 1..=255usize {
            let c = palette.colors[i];
            color_map.entry([c.r, c.g, c.b]).or_insert(i as u8);
        }
        Self {
            palette,
            color_map,
            next_free_slot: 1,
        }
    }

    fn map_rgb(&mut self, r: u8, g: u8, b: u8) -> u8 {
        let key = [r, g, b];
        if let Some(&idx) = self.color_map.get(&key) {
            return idx;
        }

        // Try to allocate an unused slot
        while self.next_free_slot <= 255 {
            let idx = self.next_free_slot;
            self.next_free_slot += 1;
            self.palette.colors[idx] = Rgba::new(r, g, b, 255);
            self.color_map.insert(key, idx as u8);
            return idx as u8;
        }

        // Palette full: find closest matching color
        let mut best_idx = 1u8;
        let mut best_dist = u64::MAX;
        for i in 1..=255usize {
            let c = self.palette.colors[i];
            let dr = (c.r as i64) - (r as i64);
            let dg = (c.g as i64) - (g as i64);
            let db = (c.b as i64) - (b as i64);
            let dist = (dr * dr + dg * dg + db * db) as u64;
            if dist < best_dist {
                best_dist = dist;
                best_idx = i as u8;
            }
        }
        self.color_map.insert(key, best_idx);
        best_idx
    }
}

/// Parse binary Qubicle `.qb` bytes into an `EditorProject`.
pub fn import_qb(bytes: &[u8]) -> Result<EditorProject, FormatError> {
    if bytes.len() < 20 {
        return Err(FormatError::InvalidQb(
            "File too small for Qubicle header".to_string(),
        ));
    }

    let version = read_u32_le(bytes, 0)?;
    if version != 0x0101 && (version & 0xFFFF) != 0x0101 && version != 0x00010100 && version != 0x0100 {
        return Err(FormatError::InvalidQb(format!(
            "Unsupported Qubicle version: 0x{version:08X}"
        )));
    }

    let color_format = read_u32_le(bytes, 4)?;
    let _z_axis_orientation = read_u32_le(bytes, 8)?;
    let compressed = read_u32_le(bytes, 12)?;

    // Handle header length: standard 24 bytes vs older 20 bytes
    let (num_matrices, mut cursor) = if bytes.len() >= 24 {
        let val_16 = read_u32_le(bytes, 16)?;
        let val_20 = read_u32_le(bytes, 20)?;
        // If val_20 is reasonable number of matrices (< 4096), assume 24-byte header
        if val_20 <= 4096 && (val_16 <= 1 || val_20 > 0) {
            (val_20 as usize, 24)
        } else {
            (val_16 as usize, 20)
        }
    } else {
        (read_u32_le(bytes, 16)? as usize, 20)
    };

    if num_matrices == 0 {
        return Err(FormatError::InvalidQb("No matrices in file".to_string()));
    }

    let mut mapper = PaletteMapper::new();
    let mut imported_matrices = Vec::new();

    for _ in 0..num_matrices {
        if cursor >= bytes.len() {
            return Err(FormatError::InvalidQb(
                "Unexpected EOF while reading matrix header".to_string(),
            ));
        }

        let name_len = bytes[cursor] as usize;
        cursor += 1;

        if cursor + name_len > bytes.len() {
            return Err(FormatError::InvalidQb(
                "Unexpected EOF reading matrix name".to_string(),
            ));
        }
        let name = String::from_utf8_lossy(&bytes[cursor..cursor + name_len]).into_owned();
        cursor += name_len;

        if cursor + 24 > bytes.len() {
            return Err(FormatError::InvalidQb(
                "Unexpected EOF reading matrix bounds".to_string(),
            ));
        }

        let qb_size_x = read_u32_le(bytes, cursor)?;
        cursor += 4;
        let qb_size_y = read_u32_le(bytes, cursor)?; // Qubicle Height
        cursor += 4;
        let qb_size_z = read_u32_le(bytes, cursor)?; // Qubicle Depth
        cursor += 4;

        let qb_pos_x = read_i32_le(bytes, cursor)?;
        cursor += 4;
        let qb_pos_y = read_i32_le(bytes, cursor)?; // Qubicle Height pos
        cursor += 4;
        let qb_pos_z = read_i32_le(bytes, cursor)?; // Qubicle Depth pos
        cursor += 4;

        // Convert to MagicaVoxel dimensions (Z-up)
        let mv_size_x = qb_size_x.clamp(1, mv_core::voxel::MAX_GRID_DIM);
        let mv_size_y = qb_size_z.clamp(1, mv_core::voxel::MAX_GRID_DIM); // Depth
        let mv_size_z = qb_size_y.clamp(1, mv_core::voxel::MAX_GRID_DIM); // Height

        let mv_pos = IVec3::new(qb_pos_x, qb_pos_z, qb_pos_y);

        let mut grid = VoxelGrid::new(mv_size_x, mv_size_y, mv_size_z);

        if compressed == 0 {
            // Uncompressed voxel data
            for qz in 0..qb_size_z {
                let mv_y = qz;
                for qy in 0..qb_size_y {
                    let mv_z = qy;
                    for qx in 0..qb_size_x {
                        let mv_x = qx;
                        if cursor + 4 > bytes.len() {
                            return Err(FormatError::InvalidQb(
                                "Unexpected EOF in uncompressed voxel data".to_string(),
                            ));
                        }
                        let b0 = bytes[cursor];
                        let b1 = bytes[cursor + 1];
                        let b2 = bytes[cursor + 2];
                        let b3 = bytes[cursor + 3];
                        cursor += 4;

                        let (r, g, b, a) = if color_format == COLOR_FORMAT_BGRA {
                            (b2, b1, b0, b3)
                        } else {
                            (b0, b1, b2, b3)
                        };

                        if a > 0 && mv_x < mv_size_x && mv_y < mv_size_y && mv_z < mv_size_z {
                            let col_idx = mapper.map_rgb(r, g, b);
                            grid.set(mv_x, mv_y, mv_z, col_idx);
                        }
                    }
                }
            }
        } else {
            // RLE Compressed slice-by-slice
            for qz in 0..qb_size_z {
                let mv_y = qz;
                let mut index = 0usize;
                let slice_pixels = (qb_size_x as usize) * (qb_size_y as usize);

                loop {
                    if cursor + 4 > bytes.len() {
                        return Err(FormatError::InvalidQb(
                            "Unexpected EOF in compressed voxel data".to_string(),
                        ));
                    }
                    let token = read_u32_le(bytes, cursor)?;
                    cursor += 4;

                    if token == NEXTSLICEFLAG {
                        break;
                    }

                    if token == CODEFLAG {
                        if cursor + 8 > bytes.len() {
                            return Err(FormatError::InvalidQb(
                                "Unexpected EOF in RLE run".to_string(),
                            ));
                        }
                        let count = read_u32_le(bytes, cursor)? as usize;
                        cursor += 4;
                        let color_u32 = read_u32_le(bytes, cursor)?;
                        cursor += 4;

                        let (r, g, b, a) = decode_color_u32(color_u32, color_format);

                        for _ in 0..count {
                            if index < slice_pixels {
                                let qx = (index % (qb_size_x as usize)) as u32;
                                let qy = (index / (qb_size_x as usize)) as u32;
                                let mv_x = qx;
                                let mv_z = qy;

                                if a > 0 && mv_x < mv_size_x && mv_y < mv_size_y && mv_z < mv_size_z {
                                    let col_idx = mapper.map_rgb(r, g, b);
                                    grid.set(mv_x, mv_y, mv_z, col_idx);
                                }
                            }
                            index += 1;
                        }
                    } else {
                        // Literal color
                        let (r, g, b, a) = decode_color_u32(token, color_format);
                        if index < slice_pixels {
                            let qx = (index % (qb_size_x as usize)) as u32;
                            let qy = (index / (qb_size_x as usize)) as u32;
                            let mv_x = qx;
                            let mv_z = qy;

                            if a > 0 && mv_x < mv_size_x && mv_y < mv_size_y && mv_z < mv_size_z {
                                let col_idx = mapper.map_rgb(r, g, b);
                                grid.set(mv_x, mv_y, mv_z, col_idx);
                            }
                        }
                        index += 1;
                    }
                }
            }
        }

        imported_matrices.push((name, grid, mv_pos));
    }

    if imported_matrices.is_empty() {
        return Err(FormatError::InvalidQb("No models could be decoded".to_string()));
    }

    // Assemble the EditorProject and Scene
    let (first_name, first_grid, first_pos) = imported_matrices.remove(0);
    let mut project = EditorProject::new_empty(
        first_grid.size_x(),
        first_grid.size_y(),
        first_grid.size_z(),
    );
    project.name = first_name.clone();
    project.grid = first_grid.clone();
    project.palette = mapper.palette;

    let first_half = IVec3::new(
        (first_grid.size_x() / 2) as i32,
        (first_grid.size_y() / 2) as i32,
        (first_grid.size_z() / 2) as i32,
    );
    let mut scene = Scene::single(first_grid);
    let root = scene.root();
    if let Some(&shape_id) = scene.shapes_under(root).first() {
        if let Some(node) = scene.node_mut(shape_id) {
            node.name = Some(first_name);
            node.transform = Transform::from_translation(first_pos + first_half);
        }
        project.active_shape = Some(shape_id);
    }

    for (name, grid, pos) in imported_matrices {
        let half = IVec3::new(
            (grid.size_x() / 2) as i32,
            (grid.size_y() / 2) as i32,
            (grid.size_z() / 2) as i32,
        );
        let model_id = scene.add_model(grid);
        if let Ok(shape_id) = scene.add_shape(root, model_id, Transform::from_translation(pos + half)) {
            if let Some(node) = scene.node_mut(shape_id) {
                node.name = Some(name);
            }
        }
    }

    let d = project.grid.dimensions();
    project.selection = SelectionMask::new(d.x, d.y, d.z);
    project.scene = scene;
    project.active_model = 0;

    Ok(project)
}

fn decode_color_u32(val: u32, color_format: u32) -> (u8, u8, u8, u8) {
    let bytes = val.to_le_bytes();
    if color_format == COLOR_FORMAT_BGRA {
        (bytes[2], bytes[1], bytes[0], bytes[3])
    } else {
        (bytes[0], bytes[1], bytes[2], bytes[3])
    }
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Result<u32, FormatError> {
    if offset + 4 > bytes.len() {
        return Err(FormatError::InvalidQb("Unexpected EOF".to_string()));
    }
    Ok(u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ]))
}

fn read_i32_le(bytes: &[u8], offset: usize) -> Result<i32, FormatError> {
    if offset + 4 > bytes.len() {
        return Err(FormatError::InvalidQb("Unexpected EOF".to_string()));
    }
    Ok(i32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qb_roundtrip_single_matrix() {
        let mut proj = EditorProject::new_empty(4, 5, 6);
        proj.name = "single_cube".to_string();
        proj.palette.colors[1] = Rgba::new(255, 0, 0, 255);
        proj.palette.colors[2] = Rgba::new(0, 255, 0, 255);
        proj.palette.colors[3] = Rgba::new(0, 0, 255, 255);

        proj.grid.set(0, 0, 0, 1);
        proj.grid.set(1, 2, 3, 2);
        proj.grid.set(3, 4, 5, 3);
        assert_eq!(proj.grid.count_solid(), 3);

        let qb_bytes = export_qb(&proj);
        assert!(qb_bytes.len() > 24);

        let imported = import_qb(&qb_bytes).expect("Failed to import QB");
        assert_eq!(imported.grid.size_x(), 4);
        assert_eq!(imported.grid.size_y(), 5);
        assert_eq!(imported.grid.size_z(), 6);
        assert_eq!(imported.grid.count_solid(), 3);

        let c1 = imported.grid.get(0, 0, 0);
        let c2 = imported.grid.get(1, 2, 3);
        let c3 = imported.grid.get(3, 4, 5);
        assert_ne!(c1, 0);
        assert_ne!(c2, 0);
        assert_ne!(c3, 0);

        let col1 = imported.palette.colors[c1 as usize];
        let col2 = imported.palette.colors[c2 as usize];
        let col3 = imported.palette.colors[c3 as usize];

        assert_eq!((col1.r, col1.g, col1.b), (255, 0, 0));
        assert_eq!((col2.r, col2.g, col2.b), (0, 255, 0));
        assert_eq!((col3.r, col3.g, col3.b), (0, 0, 255));
    }

    #[test]
    fn test_qb_roundtrip_multi_matrix() {
        let mut proj = EditorProject::new_empty(3, 3, 3);
        proj.grid.set(1, 1, 1, 10);

        let mut second_grid = VoxelGrid::new(2, 2, 2);
        second_grid.set(0, 0, 0, 20);

        let root = proj.scene.root();
        let m2 = proj.scene.add_model(second_grid);
        let s2 = proj
            .scene
            .add_shape(root, m2, Transform::from_translation(IVec3::new(10, 20, 30)))
            .unwrap();
        proj.scene.node_mut(s2).unwrap().name = Some("SecondShape".to_string());

        let qb_bytes = export_qb(&proj);
        let imported = import_qb(&qb_bytes).expect("Failed to import multi-matrix QB");

        assert_eq!(imported.scene.models.len(), 2);
        let shapes = imported.scene.shapes_under(imported.scene.root());
        assert_eq!(shapes.len(), 2);

        let n2 = imported.scene.node(shapes[1]).unwrap();
        assert_eq!(n2.name.as_deref(), Some("SecondShape"));
        assert_eq!(n2.transform.translation, IVec3::new(10, 20, 30));
    }

    #[test]
    fn test_qb_compressed_rle_import() {
        // Construct a small 2x2x2 QB file manually with RLE compression
        let mut bytes = Vec::new();
        // Header
        bytes.extend_from_slice(&0x0101u32.to_le_bytes()); // version
        bytes.extend_from_slice(&COLOR_FORMAT_RGBA.to_le_bytes()); // colorFormat
        bytes.extend_from_slice(&0u32.to_le_bytes()); // zAxisOrientation
        bytes.extend_from_slice(&1u32.to_le_bytes()); // compressed = 1
        bytes.extend_from_slice(&0u32.to_le_bytes()); // visibilityMaskEncoded
        bytes.extend_from_slice(&1u32.to_le_bytes()); // 1 matrix

        // Matrix
        let name = "RleBox";
        bytes.push(name.len() as u8);
        bytes.extend_from_slice(name.as_bytes());

        // sizeX = 2, sizeY = 2 (Height), sizeZ = 2 (Depth)
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&2u32.to_le_bytes());

        // pos
        bytes.extend_from_slice(&0i32.to_le_bytes());
        bytes.extend_from_slice(&0i32.to_le_bytes());
        bytes.extend_from_slice(&0i32.to_le_bytes());

        // Slice 0: 4 voxels run of red (CODEFLAG=2, count=4, color=red)
        bytes.extend_from_slice(&CODEFLAG.to_le_bytes());
        bytes.extend_from_slice(&4u32.to_le_bytes());
        let red = 0xFF0000FFu32; // RGBA: r=255, a=255
        bytes.extend_from_slice(&red.to_le_bytes());
        bytes.extend_from_slice(&NEXTSLICEFLAG.to_le_bytes());

        // Slice 1: 4 voxels empty (literal 0, 4 times)
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&NEXTSLICEFLAG.to_le_bytes());

        let imported = import_qb(&bytes).expect("Failed to import compressed QB");
        assert_eq!(imported.grid.size_x(), 2);
        assert_eq!(imported.grid.size_y(), 2);
        assert_eq!(imported.grid.size_z(), 2);
        // Slice 0 has 4 solid voxels, Slice 1 has 0
        assert_eq!(imported.grid.count_solid(), 4);
    }

    #[test]
    fn test_qb_bgra_color_format() {
        let mut bytes = Vec::new();
        // Header with BGRA format
        bytes.extend_from_slice(&0x0101u32.to_le_bytes());
        bytes.extend_from_slice(&COLOR_FORMAT_BGRA.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes()); // uncompressed
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());

        let name = "BgraTest";
        bytes.push(name.len() as u8);
        bytes.extend_from_slice(name.as_bytes());

        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());

        bytes.extend_from_slice(&0i32.to_le_bytes());
        bytes.extend_from_slice(&0i32.to_le_bytes());
        bytes.extend_from_slice(&0i32.to_le_bytes());

        // Blue in BGRA: Blue byte = 255, Green = 0, Red = 0, Alpha = 255 -> [255, 0, 0, 255]
        bytes.extend_from_slice(&[255, 0, 0, 255]);

        let imported = import_qb(&bytes).expect("Failed to import BGRA QB");
        let c = imported.grid.get(0, 0, 0);
        assert_ne!(c, 0);
        let col = imported.palette.colors[c as usize];
        // Blue should be decoded as (r=0, g=0, b=255)
        assert_eq!((col.r, col.g, col.b), (0, 0, 255));
    }

    #[test]
    fn test_qb_invalid_header_error() {
        let too_short = vec![0u8; 10];
        assert!(matches!(import_qb(&too_short), Err(FormatError::InvalidQb(_))));

        let mut invalid_version = vec![0u8; 30];
        invalid_version[0] = 0x99;
        invalid_version[1] = 0x99;
        assert!(matches!(import_qb(&invalid_version), Err(FormatError::InvalidQb(_))));
    }
}
