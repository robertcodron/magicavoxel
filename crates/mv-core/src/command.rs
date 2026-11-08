use glam::{IVec3, Vec3};

use crate::project::EditorProject;
use crate::voxel::{Axis, AxisFlags, VoxelGrid};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandOutcome {
    Message(String),
    ExportRequested { format: String, batch_dir: bool },
}

/// Execute a MagicaVoxel console command line (`mv_commands.html`)
pub fn execute_console_command(project: &mut EditorProject, raw_line: &str) -> Result<CommandOutcome, String> {
    let line = raw_line.trim();
    if line.is_empty() {
        return Ok(CommandOutcome::Message(String::new()));
    }

    let tokens: Vec<&str> = line.split_whitespace().collect();
    let cmd = tokens[0].to_ascii_lowercase();
    let args = &tokens[1..];

    match cmd.as_str() {
        "help" | "?" => Ok(CommandOutcome::Message(
            "Commands: sel_all, sel_none, sel_inv, copy, cut, paste, size [x y z], shrink, zero, full, fill, inv, flip [xyz], loop [axis offset], scale [xyz factor], x2, rot [axis deg], rot90 [axis], repeat [xyz factor], mir [axis], dia [axis], dil [axis], ero [axis], flood [idx], noise [seed scale min max], rand [min max], maze [edge], shear [axis s1 s2], pal [mask|fill|bw|mac|sort], log, xs [-n iter] [wave|sphere|torus|gyroid|pyramid|checker|life], o [vox|obj|ply|slice]"
                .to_string(),
        )),

        "sel_none" => {
            project.selection.clear();
            Ok(CommandOutcome::Message("Selection cleared".to_string()))
        }
        "sel_all" => {
            project.selection.select_all_occupied(project.grid.raw_data());
            Ok(CommandOutcome::Message(format!(
                "Selected {} voxels",
                project.selection.count_selected()
            )))
        }
        "sel_inv" => {
            project.selection.invert_occupied(project.grid.raw_data());
            Ok(CommandOutcome::Message(format!(
                "Inverted selection ({} selected)",
                project.selection.count_selected()
            )))
        }
        "copy" => {
            let count = project.copy_selected();
            Ok(CommandOutcome::Message(format!("Copied {} voxels", count)))
        }
        "cut" => {
            project.record_undo();
            let count = project.cut_selected();
            Ok(CommandOutcome::Message(format!("Cut {} voxels", count)))
        }
        "paste" => {
            project.record_undo();
            let count = project.paste_clipboard();
            Ok(CommandOutcome::Message(format!("Pasted {} voxels", count)))
        }
        "size" => {
            if args.is_empty() {
                return Err("Usage: size <x> [y z]".to_string());
            }
            let (sx, sy, sz) = if args.len() == 1 {
                let s: u32 = args[0].parse().map_err(|_| "Invalid size number")?;
                (s, s, s)
            } else if args.len() >= 3 {
                let x: u32 = args[0].parse().map_err(|_| "Invalid X size")?;
                let y: u32 = args[1].parse().map_err(|_| "Invalid Y size")?;
                let z: u32 = args[2].parse().map_err(|_| "Invalid Z size")?;
                (x, y, z)
            } else {
                return Err("Usage: size <x> <y> <z>".to_string());
            };
            project.record_undo();
            project.resize_canvas(sx, sy, sz);
            Ok(CommandOutcome::Message(format!(
                "Resized model to {}x{}x{}",
                project.grid.size_x(),
                project.grid.size_y(),
                project.grid.size_z()
            )))
        }
        "shrink" => {
            project.record_undo();
            if project.grid.shrink() {
                let d = project.grid.dimensions();
                project.selection.resize(d.x, d.y, d.z);
                Ok(CommandOutcome::Message(format!(
                    "Shrunk model to {}x{}x{}",
                    d.x, d.y, d.z
                )))
            } else {
                Ok(CommandOutcome::Message("Model is empty; cannot shrink".to_string()))
            }
        }
        "zero" => {
            project.record_undo();
            project.grid.clear();
            project.selection.clear();
            Ok(CommandOutcome::Message("Cleared all voxels".to_string()))
        }
        "full" => {
            project.record_undo();
            let c = project.palette.selected.max(1);
            project.grid.full(c);
            Ok(CommandOutcome::Message(format!(
                "Filled full volume with color #{}",
                c
            )))
        }
        "fill" => {
            project.record_undo();
            let c = project.palette.selected.max(1);
            project.grid.fill_solid(c);
            Ok(CommandOutcome::Message(format!(
                "Painted all solid voxels with color #{}",
                c
            )))
        }
        "inv" => {
            project.record_undo();
            let c = project.palette.selected.max(1);
            project.grid.invert(c);
            Ok(CommandOutcome::Message("Inverted solid and empty space".to_string()))
        }
        "flip" => {
            let mut flags = AxisFlags::NONE;
            for &a in args {
                let f = AxisFlags::from_token(a);
                flags.x |= f.x;
                flags.y |= f.y;
                flags.z |= f.z;
            }
            if !flags.any() {
                flags.x = true;
            }
            project.record_undo();
            project.grid.flip(flags);
            Ok(CommandOutcome::Message(format!(
                "Flipped model (x:{}, y:{}, z:{})",
                flags.x, flags.y, flags.z
            )))
        }
        "loop" => {
            if args.is_empty() {
                return Err("Usage: loop [axis] [offset] (e.g. loop z 1 x -2)".to_string());
            }
            let mut delta = IVec3::ZERO;
            let mut i = 0;
            while i < args.len() {
                let axes = AxisFlags::from_token(args[i]);
                if axes.any() && i + 1 < args.len() {
                    let offset: i32 = args[i + 1].parse().map_err(|_| "Invalid loop offset")?;
                    if axes.x {
                        delta.x += offset;
                    }
                    if axes.y {
                        delta.y += offset;
                    }
                    if axes.z {
                        delta.z += offset;
                    }
                    i += 2;
                } else {
                    return Err("Usage: loop [axis] [offset]".to_string());
                }
            }
            project.record_undo();
            project.grid.loop_shift(delta);
            Ok(CommandOutcome::Message(format!(
                "Loop shifted model by ({}, {}, {})",
                delta.x, delta.y, delta.z
            )))
        }
        "scale" => {
            if args.is_empty() {
                return Err("Usage: scale [xyz] <factor>".to_string());
            }
            let factors = parse_xyz_factors(args, 1.0)?;
            project.record_undo();
            project.grid.scale(factors);
            let d = project.grid.dimensions();
            project.selection.resize(d.x, d.y, d.z);
            Ok(CommandOutcome::Message(format!(
                "Scaled model to {}x{}x{}",
                d.x, d.y, d.z
            )))
        }
        "x2" => {
            project.record_undo();
            project.grid.scale(Vec3::splat(2.0));
            let d = project.grid.dimensions();
            project.selection.resize(d.x, d.y, d.z);
            Ok(CommandOutcome::Message(format!(
                "Doubled model size to {}x{}x{}",
                d.x, d.y, d.z
            )))
        }
        "rot90" => {
            let axis = args
                .first()
                .map(|s| parse_single_axis(s))
                .unwrap_or(Axis::Z);
            project.record_undo();
            project.grid.rot90(axis);
            let d = project.grid.dimensions();
            project.selection.resize(d.x, d.y, d.z);
            Ok(CommandOutcome::Message(format!("Rotated 90° around {:?}", axis)))
        }
        "rot" => {
            if args.len() < 2 {
                return Err("Usage: rot <x|y|z> <degrees>".to_string());
            }
            let axis = parse_single_axis(args[0]);
            let deg: f32 = args[1].parse().map_err(|_| "Invalid angle in degrees")?;
            project.record_undo();
            project.grid.rotate_degrees(axis, deg);
            let d = project.grid.dimensions();
            project.selection.resize(d.x, d.y, d.z);
            Ok(CommandOutcome::Message(format!(
                "Rotated {}° around {:?}",
                deg, axis
            )))
        }
        "repeat" => {
            if args.is_empty() {
                return Err("Usage: repeat [xyz] <factor>".to_string());
            }
            let factors = parse_xyz_factors(args, 1.0)?;
            project.record_undo();
            project.grid.repeat(factors);
            let d = project.grid.dimensions();
            project.selection.resize(d.x, d.y, d.z);
            Ok(CommandOutcome::Message(format!(
                "Repeated model to {}x{}x{}",
                d.x, d.y, d.z
            )))
        }
        "mir" => {
            let mut flags = AxisFlags::NONE;
            for &a in args {
                let f = AxisFlags::from_token(a);
                flags.x |= f.x;
                flags.y |= f.y;
                flags.z |= f.z;
            }
            if !flags.any() {
                flags.x = true;
            }
            project.record_undo();
            project.grid.mirror_symmetry(flags);
            Ok(CommandOutcome::Message(format!(
                "Applied mirror symmetry (x:{}, y:{}, z:{})",
                flags.x, flags.y, flags.z
            )))
        }
        "dia" => {
            let mut flags = AxisFlags::NONE;
            for &a in args {
                let f = AxisFlags::from_token(a);
                flags.x |= f.x;
                flags.y |= f.y;
                flags.z |= f.z;
            }
            if !flags.any() {
                flags.z = true;
            }
            project.record_undo();
            project.grid.diagonal_symmetry(flags);
            Ok(CommandOutcome::Message(format!(
                "Applied diagonal symmetry (x:{}, y:{}, z:{})",
                flags.x, flags.y, flags.z
            )))
        }
        "dil" => {
            let spec = args.join("");
            project.record_undo();
            project.grid.dilate(&spec);
            Ok(CommandOutcome::Message("Applied dilation".to_string()))
        }
        "ero" => {
            let spec = args.join("");
            project.record_undo();
            project.grid.erode(&spec);
            Ok(CommandOutcome::Message("Applied erosion".to_string()))
        }
        "flood" => {
            let idx: u8 = args
                .first()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            project.record_undo();
            project.grid.flood(idx);
            if idx == 0 {
                Ok(CommandOutcome::Message(
                    "Removed all interior invisible voxels".to_string(),
                ))
            } else {
                Ok(CommandOutcome::Message(format!(
                    "Flooded interior cavities with color #{}",
                    idx
                )))
            }
        }
        "noise" => {
            let seed: u32 = args.first().and_then(|s| s.parse().ok()).unwrap_or(123);
            let scale: f32 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(0.08);
            let min_v: f32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.35);
            let max_v: f32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(0.65);
            project.record_undo();
            let c = project.palette.selected.max(1);
            project.grid.generate_noise(seed, scale, min_v, max_v, c);
            Ok(CommandOutcome::Message(format!(
                "Generated 3D noise (seed={}, scale={}, range=[{}, {}])",
                seed, scale, min_v, max_v
            )))
        }
        "rand" => {
            let min_c: u8 = args
                .first()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1);
            let max_c: u8 = args
                .get(1)
                .and_then(|s| s.parse().ok())
                .unwrap_or(16);
            project.record_undo();
            project.grid.rand_paint(min_c, max_c, 0x9E37_79B9);
            Ok(CommandOutcome::Message(format!(
                "Randomly painted solid voxels in range [{}..={}]",
                min_c, max_c
            )))
        }
        "maze" => {
            let edge: u32 = args.first().and_then(|s| s.parse().ok()).unwrap_or(6);
            project.record_undo();
            let c = project.palette.selected.max(1);
            project.grid.generate_maze(edge, c);
            Ok(CommandOutcome::Message(format!(
                "Generated 3D maze with edge length {}",
                edge
            )))
        }
        "shear" => {
            if args.len() < 3 {
                return Err("Usage: shear <x|y|z> <scale1> <scale2>".to_string());
            }
            let axis = parse_single_axis(args[0]);
            let s1: f32 = args[1].parse().map_err(|_| "Invalid shear factor 1")?;
            let s2: f32 = args[2].parse().map_err(|_| "Invalid shear factor 2")?;
            project.record_undo();
            project.grid.shear(axis, s1, s2);
            Ok(CommandOutcome::Message(format!(
                "Sheared model along {:?} by ({}, {})",
                axis, s1, s2
            )))
        }
        "pal" => {
            let sub = args.first().map(|s| s.to_ascii_lowercase()).unwrap_or_default();
            project.record_undo();
            match sub.as_str() {
                "mask" => {
                    let used = project.grid.used_colors();
                    project.palette.mask_unused(&used);
                    Ok(CommandOutcome::Message("Masked unused palette swatches".to_string()))
                }
                "fill" => {
                    let idx: u8 = args
                        .get(1)
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(project.palette.selected);
                    project.palette.fill_from_index(idx.max(1));
                    Ok(CommandOutcome::Message(format!(
                        "Filled palette with swatch #{}",
                        idx
                    )))
                }
                "bw" => {
                    project.palette.generate_bw();
                    Ok(CommandOutcome::Message("Generated greyscale palette".to_string()))
                }
                "mac" => {
                    project.palette.generate_mac();
                    Ok(CommandOutcome::Message("Restored default MagicaVoxel palette".to_string()))
                }
                "sort" => {
                    let spec = args.get(1).copied().unwrap_or("hsv");
                    project.palette.sort_by_spec(spec);
                    Ok(CommandOutcome::Message(format!(
                        "Sorted palette by '{}'",
                        spec
                    )))
                }
                _ => Err("Usage: pal <mask|fill [idx]|bw|mac|sort [+-hsvrgb]>".to_string()),
            }
        }
        "log" => {
            let d = project.grid.dimensions();
            let solid = project.grid.count_solid();
            let hist = project.grid.color_histogram();
            let used_swatches = hist.iter().skip(1).filter(|&&c| c > 0).count();
            Ok(CommandOutcome::Message(format!(
                "Volume: {}x{}x{} | Solid Voxels: {} | Used Colors: {}",
                d.x, d.y, d.z, solid, used_swatches
            )))
        }
        "xs" => {
            if args.is_empty() {
                return Err("Usage: xs [-n iterations] <shader_name> [arg0 arg1 ...]".to_string());
            }
            let mut idx = 0;
            let mut iterations = 1u32;
            while idx < args.len() && args[idx].starts_with('-') {
                match args[idx] {
                    "-n" if idx + 1 < args.len() => {
                        iterations = args[idx + 1]
                            .parse::<u32>()
                            .unwrap_or(1)
                            .clamp(1, 64);
                        idx += 2;
                    }
                    "-prev" | "-cur" => {
                        idx += 1;
                    }
                    _ => {
                        idx += 1;
                    }
                }
            }
            if idx >= args.len() {
                return Err("Missing voxel shader name".to_string());
            }
            let shader_name = args[idx].to_ascii_lowercase();
            let shader_args: Vec<f32> = args[idx + 1..]
                .iter()
                .filter_map(|s| s.parse::<f32>().ok())
                .collect();

            project.record_undo();
            let c = project.palette.selected.max(1);
            for iter in 0..iterations {
                run_voxel_shader(&mut project.grid, &shader_name, &shader_args, c, iter)?;
            }
            Ok(CommandOutcome::Message(format!(
                "Executed voxel shader '{}' ({} iteration(s))",
                shader_name, iterations
            )))
        }
        "o" | "odir" => {
            let fmt = args.first().map(|s| s.to_ascii_lowercase()).unwrap_or_else(|| "vox".to_string());
            Ok(CommandOutcome::ExportRequested {
                format: fmt,
                batch_dir: cmd == "odir",
            })
        }
        _ => Err(format!("Unknown command '{}'. Type 'help' for available commands.", cmd)),
    }
}

fn parse_single_axis(s: &str) -> Axis {
    match s.chars().next().unwrap_or('z').to_ascii_lowercase() {
        'x' => Axis::X,
        'y' => Axis::Y,
        _ => Axis::Z,
    }
}

fn parse_xyz_factors(args: &[&str], default_val: f32) -> Result<Vec3, String> {
    if args.len() == 1 {
        let v: f32 = args[0].parse().map_err(|_| "Invalid numeric factor")?;
        return Ok(Vec3::splat(v));
    }
    let mut out = Vec3::splat(default_val);
    let mut i = 0;
    while i < args.len() {
        let axes = AxisFlags::from_token(args[i]);
        if axes.any() && i + 1 < args.len() {
            let val: f32 = args[i + 1].parse().map_err(|_| "Invalid numeric factor")?;
            if axes.x {
                out.x = val;
            }
            if axes.y {
                out.y = val;
            }
            if axes.z {
                out.z = val;
            }
            i += 2;
        } else if let Ok(val) = args[i].parse::<f32>() {
            out = Vec3::splat(val);
            i += 1;
        } else {
            return Err("Invalid axis/factor arguments".to_string());
        }
    }
    Ok(out)
}

/// Built-in Voxel Shaders (`xs [shader_name] [args...]`)
fn run_voxel_shader(
    grid: &mut VoxelGrid,
    shader: &str,
    args: &[f32],
    base_color: u8,
    iter: u32,
) -> Result<(), String> {
    let sx = grid.size_x() as f32;
    let sy = grid.size_y() as f32;
    let sz = grid.size_z() as f32;
    let cx = sx * 0.5;
    let cy = sy * 0.5;
    let cz = sz * 0.5;

    match shader {
        "wave" | "sine" => {
            let freq = args.first().copied().unwrap_or(0.22);
            let amp = args.get(1).copied().unwrap_or(sz * 0.25);
            let phase = iter as f32 * 0.6;
            grid.clear();
            for y in 0..grid.size_y() {
                for x in 0..grid.size_x() {
                    let dx = x as f32 - cx;
                    let dy = y as f32 - cy;
                    let r = (dx * dx + dy * dy).sqrt();
                    let h = (cz + (r * freq - phase).sin() * amp).round() as i32;
                    for z in 0..=h.clamp(0, grid.size_z() as i32 - 1) {
                        let col = base_color.wrapping_add((z as u8) % 12).max(1);
                        grid.set(x, y, z as u32, col);
                    }
                }
            }
            Ok(())
        }
        "sphere" => {
            let radius = args
                .first()
                .copied()
                .unwrap_or(sx.min(sy).min(sz) * 0.45);
            let r2 = radius * radius;
            grid.clear();
            for z in 0..grid.size_z() {
                for y in 0..grid.size_y() {
                    for x in 0..grid.size_x() {
                        let dx = x as f32 + 0.5 - cx;
                        let dy = y as f32 + 0.5 - cy;
                        let dz = z as f32 + 0.5 - cz;
                        if dx * dx + dy * dy + dz * dz <= r2 {
                            grid.set(x, y, z, base_color);
                        }
                    }
                }
            }
            Ok(())
        }
        "torus" => {
            let major = args.first().copied().unwrap_or(sx.min(sy) * 0.30);
            let minor = args.get(1).copied().unwrap_or(sx.min(sy) * 0.12);
            grid.clear();
            for z in 0..grid.size_z() {
                for y in 0..grid.size_y() {
                    for x in 0..grid.size_x() {
                        let dx = x as f32 + 0.5 - cx;
                        let dy = y as f32 + 0.5 - cy;
                        let dz = z as f32 + 0.5 - cz;
                        let q = (dx * dx + dy * dy).sqrt() - major;
                        if q * q + dz * dz <= minor * minor {
                            grid.set(x, y, z, base_color);
                        }
                    }
                }
            }
            Ok(())
        }
        "cylinder" => {
            let radius = args.first().copied().unwrap_or(sx.min(sy) * 0.4);
            let r2 = radius * radius;
            grid.clear();
            for z in 0..grid.size_z() {
                for y in 0..grid.size_y() {
                    for x in 0..grid.size_x() {
                        let dx = x as f32 + 0.5 - cx;
                        let dy = y as f32 + 0.5 - cy;
                        if dx * dx + dy * dy <= r2 {
                            grid.set(x, y, z, base_color);
                        }
                    }
                }
            }
            Ok(())
        }
        "gyroid" => {
            let scale = args.first().copied().unwrap_or(0.22);
            let thickness = args.get(1).copied().unwrap_or(0.35);
            grid.clear();
            for z in 0..grid.size_z() {
                for y in 0..grid.size_y() {
                    for x in 0..grid.size_x() {
                        let gx = x as f32 * scale;
                        let gy = y as f32 * scale;
                        let gz = z as f32 * scale;
                        let val = gx.sin() * gy.cos() + gy.sin() * gz.cos() + gz.sin() * gx.cos();
                        if val.abs() <= thickness {
                            let c = base_color.wrapping_add(((z / 3) as u8) % 8).max(1);
                            grid.set(x, y, z, c);
                        }
                    }
                }
            }
            Ok(())
        }
        "pyramid" => {
            grid.clear();
            let max_h = grid.size_z().min(grid.size_x() / 2).min(grid.size_y() / 2);
            for z in 0..max_h {
                let m = z;
                for y in m..(grid.size_y() - m) {
                    for x in m..(grid.size_x() - m) {
                        grid.set(x, y, z, base_color.wrapping_add((z as u8) % 6).max(1));
                    }
                }
            }
            Ok(())
        }
        "checker" => {
            let step = (args.first().copied().unwrap_or(4.0) as u32).max(1);
            for z in 0..grid.size_z() {
                for y in 0..grid.size_y() {
                    for x in 0..grid.size_x() {
                        if grid.get(x, y, z) != 0 {
                            let parity = ((x / step) + (y / step) + (z / step)) % 2;
                            if parity == 1 {
                                grid.set(x, y, z, base_color);
                            }
                        }
                    }
                }
            }
            Ok(())
        }
        "life" | "ca" => {
            // 3D Cellular Automaton step
            let prev = grid.clone();
            for z in 0..grid.size_z() as i32 {
                for y in 0..grid.size_y() as i32 {
                    for x in 0..grid.size_x() as i32 {
                        let mut neighbors = 0;
                        for dz in -1..=1 {
                            for dy in -1..=1 {
                                for dx in -1..=1 {
                                    if dx == 0 && dy == 0 && dz == 0 {
                                        continue;
                                    }
                                    if prev.get_i32(x + dx, y + dy, z + dz) != 0 {
                                        neighbors += 1;
                                    }
                                }
                            }
                        }
                        let alive = prev.get_i32(x, y, z) != 0;
                        if alive && (4..=9).contains(&neighbors) {
                            grid.set_i32(x, y, z, prev.get_i32(x, y, z));
                        } else if !alive && (5..=6).contains(&neighbors) {
                            grid.set_i32(x, y, z, base_color);
                        } else {
                            grid.set_i32(x, y, z, 0);
                        }
                    }
                }
            }
            Ok(())
        }
        _ => Err(format!(
            "Unknown voxel shader '{}'. Built-ins: wave, sphere, torus, cylinder, gyroid, pyramid, checker, life",
            shader
        )),
    }
}

