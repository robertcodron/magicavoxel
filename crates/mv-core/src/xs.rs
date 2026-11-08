//! `xs` voxel shader invocations: option parsing and application to the project,
//! shared by the built-in shaders (`command.rs`) and external GPU shaders (`mv-app`).
//!
//! `xs [-n iterations] [-prev | -cur] <shader> [arg0 arg1 ...]`
//! - no `-prev`/`-cur`: run the iterations in place on the edited model;
//! - `-prev` / `-cur`: generate one new animation frame per iteration, each based on
//!   the previous generated frame / on the current frame (`mv_main.html`, 0.97.4).

use crate::project::EditorProject;
use crate::voxel::VoxelGrid;

/// Names handled by the built-in shaders of `command.rs`.
pub const BUILTIN_SHADERS: &[&str] = &[
    "wave", "sine", "sphere", "torus", "cylinder", "gyroid", "pyramid", "checker", "life", "ca",
];

pub const MAX_XS_ITERATIONS: u32 = 360;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XsMode {
    InPlace,
    Animate { from_previous: bool },
}

#[derive(Debug, Clone, PartialEq)]
pub struct XsInvocation {
    /// Shader name as typed (`sub/name` for subfolders).
    pub shader: String,
    pub args: Vec<f32>,
    pub iterations: u32,
    pub mode: XsMode,
}

impl XsInvocation {
    pub fn is_builtin(&self) -> bool {
        BUILTIN_SHADERS.contains(&self.shader.to_ascii_lowercase().as_str())
    }

    /// Equivalent console command line.
    pub fn to_command(&self) -> String {
        let mut s = String::from("xs");
        if self.iterations != 1 {
            s += &format!(" -n {}", self.iterations);
        }
        match self.mode {
            XsMode::InPlace => {}
            XsMode::Animate {
                from_previous: true,
            } => s += " -prev",
            XsMode::Animate {
                from_previous: false,
            } => s += " -cur",
        }
        s += &format!(" {}", self.shader);
        for a in &self.args {
            s += &format!(" {a}");
        }
        s
    }
}

/// Inputs of one shader iteration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XsStep {
    pub iter: u32,
    /// Animation frame being produced (`iFrame`).
    pub frame: u32,
    /// Total frames of the animation after the run (`iNumFrames`).
    pub num_frames: u32,
}

/// Parse the arguments following `xs`.
pub fn parse_xs(args: &[&str]) -> Result<XsInvocation, String> {
    let mut idx = 0;
    let mut iterations = 1;
    let mut mode = XsMode::InPlace;
    while idx < args.len() && args[idx].starts_with('-') && args[idx].parse::<f32>().is_err() {
        match args[idx] {
            "-n" => {
                let n = args
                    .get(idx + 1)
                    .ok_or("xs -n expects a number of iterations")?;
                iterations = n
                    .parse::<u32>()
                    .map_err(|_| format!("invalid iteration count '{n}'"))?
                    .clamp(1, MAX_XS_ITERATIONS);
                idx += 2;
            }
            "-prev" => {
                mode = XsMode::Animate {
                    from_previous: true,
                };
                idx += 1;
            }
            "-cur" => {
                mode = XsMode::Animate {
                    from_previous: false,
                };
                idx += 1;
            }
            other => return Err(format!("unknown xs option '{other}'")),
        }
    }
    let shader = args
        .get(idx)
        .ok_or("Usage: xs [-n iterations] [-prev|-cur] <shader> [arg0 arg1 ...]")?
        .to_string();
    let args = args[idx + 1..]
        .iter()
        .map(|s| {
            s.parse::<f32>()
                .map_err(|_| format!("invalid shader argument '{s}'"))
        })
        .collect::<Result<_, _>>()?;
    Ok(XsInvocation {
        shader,
        args,
        iterations,
        mode,
    })
}

impl EditorProject {
    /// Apply a shader invocation. `run` transforms a grid for one iteration. Work is done
    /// on copies and committed only if every iteration succeeds, as a single undo step
    /// (voxel undo in place, World undo when frames are generated).
    pub fn run_xs(
        &mut self,
        inv: &XsInvocation,
        mut run: impl FnMut(&mut VoxelGrid, XsStep) -> Result<(), String>,
    ) -> Result<String, String> {
        let frame = self.active_frame_index() as u32;
        let count = self.active_frame_count() as u32;
        match inv.mode {
            XsMode::InPlace => {
                let mut work = self.grid.clone();
                for iter in 0..inv.iterations {
                    run(
                        &mut work,
                        XsStep {
                            iter,
                            frame,
                            num_frames: count,
                        },
                    )?;
                }
                self.record_undo();
                self.grid = work;
                Ok(format!(
                    "Voxel shader '{}' ({} iteration(s))",
                    inv.shader, inv.iterations
                ))
            }
            XsMode::Animate { from_previous } => {
                if self.active_shape.is_none() {
                    return Err(
                        "xs -prev/-cur needs an object opened from the World Editor".to_string()
                    );
                }
                let base = self.grid.clone();
                let total = count + inv.iterations;
                let mut frames: Vec<VoxelGrid> = Vec::with_capacity(inv.iterations as usize);
                for iter in 0..inv.iterations {
                    let mut g = match (from_previous, frames.last()) {
                        (true, Some(prev)) => prev.clone(),
                        _ => base.clone(),
                    };
                    run(
                        &mut g,
                        XsStep {
                            iter,
                            frame: frame + 1 + iter,
                            num_frames: total,
                        },
                    )?;
                    frames.push(g);
                }
                self.record_world_undo();
                for g in frames {
                    self.add_frame_unrecorded(false);
                    self.grid = g;
                }
                Ok(format!(
                    "Voxel shader '{}' generated {} frame(s) ({})",
                    inv.shader,
                    inv.iterations,
                    if from_previous { "-prev" } else { "-cur" }
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_options_and_args() {
        let inv = parse_xs(&["-n", "4", "-cur", "sub/poly", "1", "-2.5"]).unwrap();
        assert_eq!(inv.iterations, 4);
        assert_eq!(
            inv.mode,
            XsMode::Animate {
                from_previous: false
            }
        );
        assert_eq!(inv.shader, "sub/poly");
        assert_eq!(inv.args, vec![1.0, -2.5]);
        assert!(!inv.is_builtin());
        assert_eq!(parse_xs(&["Sphere"]).unwrap().mode, XsMode::InPlace);
        assert!(parse_xs(&["Sphere"]).unwrap().is_builtin());
        assert!(parse_xs(&["-x", "sphere"]).is_err());
        assert!(parse_xs(&["sphere", "abc"]).is_err());
        assert_eq!(
            parse_xs(&["-n", "2", "-prev", "grow", "3"])
                .unwrap()
                .to_command(),
            "xs -n 2 -prev grow 3"
        );
    }

    #[test]
    fn failing_iteration_leaves_project_untouched() {
        let mut proj = EditorProject::new_empty(4, 4, 4);
        proj.grid.set(1, 1, 1, 5);
        let inv = parse_xs(&["-n", "3", "custom"]).unwrap();
        let err = proj.run_xs(&inv, |g, step| {
            g.clear();
            if step.iter == 2 {
                Err("boom".to_string())
            } else {
                Ok(())
            }
        });
        assert_eq!(err, Err("boom".to_string()));
        assert_eq!(proj.grid.get(1, 1, 1), 5);
        assert!(!proj.history.can_undo());

        let inv = parse_xs(&["-n", "2", "-prev", "custom"]).unwrap();
        assert!(proj.run_xs(&inv, |_, _| Err("no".into())).is_err());
        assert_eq!(proj.active_frame_count(), 1);
        assert!(proj.world_undo.is_empty());
    }

    #[test]
    fn animate_passes_frame_numbers() {
        let mut proj = EditorProject::new_empty(4, 4, 4);
        let inv = parse_xs(&["-n", "3", "-prev", "custom"]).unwrap();
        let mut steps = Vec::new();
        proj.run_xs(&inv, |g, step| {
            steps.push(step);
            g.set(step.iter, 0, 0, 1); // accumulates with -prev
            Ok(())
        })
        .unwrap();
        assert_eq!(
            steps
                .iter()
                .map(|s| (s.frame, s.num_frames))
                .collect::<Vec<_>>(),
            vec![(1, 4), (2, 4), (3, 4)]
        );
        assert_eq!(proj.active_frame_count(), 4);
        assert_eq!(
            proj.grid.count_solid(),
            3,
            "last frame built on the previous ones"
        );
    }
}
