//! Heightmap PNG importer for terrain generation.
//!
//! Loads an arbitrary PNG image (grayscale or color luminance), resizes it if needed
//! to fit within MagicaVoxel's 256x256 grid dimensions, and generates a solid terrain
//! voxel volume with elevation-based biome coloring (water, sand, grass, rock, snow).

use image::GenericImageView;
use mv_core::voxel::MAX_GRID_DIM;
use mv_core::{Palette, Rgba, VoxelGrid};

use crate::FormatError;

/// Reference target colors for altitude biomes.
const COLOR_WATER: Rgba = Rgba::new(45, 105, 180, 255);
const COLOR_SAND: Rgba = Rgba::new(215, 195, 145, 255);
const COLOR_DIRT: Rgba = Rgba::new(110, 75, 45, 255);
const COLOR_GRASS: Rgba = Rgba::new(76, 140, 43, 255);
const COLOR_ROCK: Rgba = Rgba::new(120, 120, 125, 255);
const COLOR_SNOW: Rgba = Rgba::new(245, 248, 250, 255);

/// Find the closest palette color index in `1..=255` matching `target`.
fn closest_palette_index(palette: &Palette, target: Rgba) -> u8 {
    let mut best_idx = 1u8;
    let mut best_dist = u64::MAX;
    for i in 1..=255usize {
        let c = palette.colors[i];
        let dr = (c.r as i64) - (target.r as i64);
        let dg = (c.g as i64) - (target.g as i64);
        let db = (c.b as i64) - (target.b as i64);
        let dist = (dr * dr + dg * dg + db * db) as u64;
        if dist < best_dist {
            best_dist = dist;
            best_idx = i as u8;
        }
    }
    best_idx
}

/// Import a PNG heightmap image and generate a terrain `VoxelGrid`.
///
/// - `png_bytes`: Raw binary PNG data.
/// - `max_height`: Maximum height of the generated terrain (clamped to 1..=256).
/// - `palette`: MagicaVoxel palette used to find optimal swatch colors.
pub fn import_heightmap_png(
    png_bytes: &[u8],
    max_height: u32,
    palette: &Palette,
) -> Result<VoxelGrid, FormatError> {
    let img = image::load_from_memory(png_bytes)?;

    let (raw_w, raw_h) = img.dimensions();
    if raw_w == 0 || raw_h == 0 {
        return Err(FormatError::Heightmap("Image dimensions are 0".to_string()));
    }

    // Resize if larger than MagicaVoxel grid limit (256x256)
    let img = if raw_w > MAX_GRID_DIM || raw_h > MAX_GRID_DIM {
        let target_w = raw_w.min(MAX_GRID_DIM);
        let target_h = raw_h.min(MAX_GRID_DIM);
        img.resize(target_w, target_h, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };

    let (width, depth) = img.dimensions();
    let width = width.clamp(1, MAX_GRID_DIM);
    let depth = depth.clamp(1, MAX_GRID_DIM);
    let height = max_height.clamp(1, MAX_GRID_DIM);

    let mut grid = VoxelGrid::new(width, depth, height);

    // Precalculate palette indices for biomes
    let idx_water = closest_palette_index(palette, COLOR_WATER);
    let idx_sand = closest_palette_index(palette, COLOR_SAND);
    let idx_dirt = closest_palette_index(palette, COLOR_DIRT);
    let idx_grass = closest_palette_index(palette, COLOR_GRASS);
    let idx_rock = closest_palette_index(palette, COLOR_ROCK);
    let idx_snow = closest_palette_index(palette, COLOR_SNOW);

    let rgba = img.to_rgba8();

    for y in 0..depth {
        for x in 0..width {
            let pixel = rgba.get_pixel(x, y);

            // Compute normalized luminance in [0.0, 1.0]
            let luma = (0.299 * pixel[0] as f32 + 0.587 * pixel[1] as f32 + 0.114 * pixel[2] as f32)
                / 255.0;

            let target_h = ((luma * (height - 1) as f32).round() as u32).min(height - 1);

            for z in 0..=target_h {
                let rel_alt = z as f32 / (height - 1).max(1) as f32;

                let color = if z == target_h {
                    // Surface voxel: colored according to elevation biome
                    if rel_alt < 0.12 {
                        if target_h == 0 {
                            idx_water
                        } else {
                            idx_sand
                        }
                    } else if rel_alt < 0.50 {
                        idx_grass
                    } else if rel_alt < 0.78 {
                        idx_rock
                    } else {
                        idx_snow
                    }
                } else {
                    // Subsurface voxel: top 2 layers are dirt/subsoil, deeper is rock
                    if z + 2 >= target_h {
                        idx_dirt
                    } else {
                        idx_rock
                    }
                };

                grid.set(x, y, z, color);
            }
        }
    }

    Ok(grid)
}

/// Export a `VoxelGrid` as a 2D grayscale heightmap PNG.
/// Each pixel `(x, y)` corresponds to the normalized altitude `z` of the highest solid voxel.
pub fn export_heightmap_png(grid: &VoxelGrid) -> Result<Vec<u8>, FormatError> {
    let w = grid.size_x();
    let d = grid.size_y();
    let h = grid.size_z();
    let mut img: image::ImageBuffer<image::Luma<u8>, Vec<u8>> = image::ImageBuffer::new(w, d);

    for y in 0..d {
        for x in 0..w {
            let max_solid = (0..h).rev().find(|&z| grid.get(x, y, z) != 0);
            let val = match max_solid {
                Some(z) => {
                    if h <= 1 {
                        255
                    } else {
                        ((z as f32 / (h - 1) as f32) * 255.0).round() as u8
                    }
                }
                None => 0,
            };
            img.put_pixel(x, y, image::Luma([val]));
        }
    }

    let mut bytes = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut bytes),
        image::ImageFormat::Png,
    )?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba as ImgRgba};

    fn make_test_png(w: u32, h: u32, f: impl Fn(u32, u32) -> u8) -> Vec<u8> {
        let img: ImageBuffer<ImgRgba<u8>, Vec<u8>> =
            ImageBuffer::from_fn(w, h, |x, y| {
                let v = f(x, y);
                ImgRgba([v, v, v, 255])
            });

        let mut png_bytes = Vec::new();
        img.write_to(
            &mut std::io::Cursor::new(&mut png_bytes),
            image::ImageFormat::Png,
        )
        .expect("Failed to encode PNG");
        png_bytes
    }

    #[test]
    fn test_heightmap_basic_gradient() {
        let palette = Palette::default();
        // 16x16 image with X-gradient from 0 (black) to 255 (white)
        let png = make_test_png(16, 16, |x, _y| (x * 255 / 15) as u8);

        let grid = import_heightmap_png(&png, 32, &palette).expect("Failed to import heightmap");
        assert_eq!(grid.size_x(), 16);
        assert_eq!(grid.size_y(), 16);
        assert_eq!(grid.size_z(), 32);

        // Column 0 should have low elevation (height 0)
        assert_ne!(grid.get(0, 0, 0), 0);
        assert_eq!(grid.get(0, 0, 1), 0);

        // Column 15 should reach peak elevation (height 31)
        assert_ne!(grid.get(15, 0, 31), 0);

        // Solid columns should be filled from z=0 to target_h
        for z in 0..=31 {
            assert_ne!(grid.get(15, 0, z), 0, "Voxel at z={z} should be solid");
        }

        // Low altitude surface (x=0) vs high altitude surface (x=15)
        let low_c = grid.get(0, 0, 0);
        let peak_c = grid.get(15, 0, 31);
        assert_ne!(low_c, peak_c, "Peak and base should have different biome colors");
    }

    #[test]
    fn test_heightmap_clamp_and_resize() {
        let palette = Palette::default();
        // 300x300 image (exceeds MagicaVoxel 256 limit)
        let png = make_test_png(300, 300, |_x, _y| 128);

        let grid = import_heightmap_png(&png, 400, &palette).expect("Failed to import large heightmap");
        assert!(grid.size_x() <= 256);
        assert!(grid.size_y() <= 256);
        assert_eq!(grid.size_z(), 256); // clamped to MAX_GRID_DIM
    }

    #[test]
    fn test_heightmap_invalid_png_error() {
        let palette = Palette::default();
        let junk = b"not a valid png file header";
        assert!(import_heightmap_png(junk, 32, &palette).is_err());
    }
}
