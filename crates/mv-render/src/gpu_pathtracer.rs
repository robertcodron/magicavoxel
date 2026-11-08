use bytemuck::{Pod, Zeroable};
use glam::{Vec2, Vec3};
use mv_core::{EditorProject, MaterialType};
use rayon::prelude::*;
use std::hash::{Hash, Hasher};

use crate::{dda_raycast_grid, DisplaySettings, OrbitCamera};

#[derive(Debug, Clone)]
pub struct PathTracerSettings {
    pub max_bounces: u32,
    pub target_samples: u32,
    pub samples_per_frame: u32,
    pub paused: bool,
    pub aperture: f32,
    pub focus_dist: f32,
    pub exposure: f32,
    pub vignette: f32,
    pub use_aces: bool,
    pub fog_density: f32,
    pub sun_intensity: f32,
    pub sky_color: [f32; 3],
    pub ground_color: [f32; 3],
}

impl Default for PathTracerSettings {
    fn default() -> Self {
        Self {
            max_bounces: 5,
            target_samples: 512,
            samples_per_frame: 2,
            paused: false,
            aperture: 0.08,
            focus_dist: 65.0,
            exposure: 1.15,
            vignette: 0.25,
            use_aces: true,
            fog_density: 0.0015,
            sun_intensity: 1.4,
            sky_color: [0.52, 0.68, 0.92],
            ground_color: [0.24, 0.26, 0.29],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuPathTraceParams {
    cam_eye: [f32; 4],
    cam_forward: [f32; 4],
    cam_right: [f32; 4],
    cam_up: [f32; 4],
    sun_dir: [f32; 4],
    sky_color: [f32; 4],
    ground_color: [f32; 4],
    grid_size: [u32; 4],
    resolution: [u32; 4],
    lens_post: [f32; 4],
    post_flags: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuMaterial {
    albedo: [f32; 4],
    props0: [f32; 4], // kind, rough, spec, ior
    props1: [f32; 4], // emit, metal, density, att
}

struct WgpuBackend {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    params_buf: wgpu::Buffer,
    materials_buf: wgpu::Buffer,
    voxels_buf: Option<(wgpu::Buffer, usize)>,
    accum_buf: Option<(wgpu::Buffer, u32, u32)>,
    output_buf: Option<(wgpu::Buffer, u32, u32)>,
    staging_buf: Option<(wgpu::Buffer, u32, u32)>,
}

pub struct PathTracerEngine {
    wgpu_backend: Option<WgpuBackend>,
    cpu_accum: Vec<Vec3>,
    pub current_samples: u32,
    last_width: u32,
    last_height: u32,
    last_scene_hash: u64,
    last_camera_hash: u64,
}

impl Default for PathTracerEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl PathTracerEngine {
    pub fn new() -> Self {
        let wgpu_backend = pollster::block_on(Self::init_wgpu());
        Self {
            wgpu_backend,
            cpu_accum: Vec::new(),
            current_samples: 0,
            last_width: 0,
            last_height: 0,
            last_scene_hash: 0,
            last_camera_hash: 0,
        }
    }

    pub fn is_gpu_accelerated(&self) -> bool {
        self.wgpu_backend.is_some()
    }

    pub fn reset_accumulation(&mut self) {
        self.current_samples = 0;
    }

    async fn init_wgpu() -> Option<WgpuBackend> {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await?;

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("mv-render-pathtracer-device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    memory_hints: wgpu::MemoryHints::Performance,
                },
                None,
            )
            .await
            .ok()?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("pathtrace.wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("pathtrace.wgsl").into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pathtrace_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pathtrace_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("pathtrace_compute_pipeline"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });

        let params_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("params_buf"),
            size: std::mem::size_of::<GpuPathTraceParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let materials_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("materials_buf"),
            size: (256 * std::mem::size_of::<GpuMaterial>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Some(WgpuBackend {
            device,
            queue,
            pipeline,
            bind_group_layout,
            params_buf,
            materials_buf,
            voxels_buf: None,
            accum_buf: None,
            output_buf: None,
            staging_buf: None,
        })
    }

    pub fn render_progressive(
        &mut self,
        project: &EditorProject,
        camera: &OrbitCamera,
        display: &DisplaySettings,
        pt: &PathTracerSettings,
        width: usize,
        height: usize,
        out_rgba: &mut Vec<u8>,
    ) {
        let w = width.max(1) as u32;
        let h = height.max(1) as u32;

        let scene_hash = compute_scene_hash(project);
        let cam_hash = compute_camera_and_settings_hash(camera, display, pt);

        let scene_changed = scene_hash != self.last_scene_hash;
        let cam_changed = cam_hash != self.last_camera_hash
            || w != self.last_width
            || h != self.last_height;

        if scene_changed || cam_changed {
            self.current_samples = 0;
            self.last_scene_hash = scene_hash;
            self.last_camera_hash = cam_hash;
            self.last_width = w;
            self.last_height = h;
        }

        let should_step = !pt.paused && self.current_samples < pt.target_samples;
        let steps = if self.current_samples == 0 {
            1
        } else if should_step {
            pt.samples_per_frame.clamp(1, 8)
        } else {
            0
        };

        if self.wgpu_backend.is_some() {
            self.render_wgpu(project, camera, display, pt, w, h, scene_changed, steps, out_rgba);
        } else {
            self.render_cpu_fallback(project, camera, display, pt, w as usize, h as usize, steps, out_rgba);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_wgpu(
        &mut self,
        project: &EditorProject,
        camera: &OrbitCamera,
        display: &DisplaySettings,
        pt: &PathTracerSettings,
        w: u32,
        h: u32,
        upload_voxels: bool,
        steps: u32,
        out_rgba: &mut Vec<u8>,
    ) {
        let backend = self.wgpu_backend.as_mut().unwrap();

        // 1. Upload packed voxels & materials when changed or uninitialized
        let raw_voxels = project.grid.raw_data();
        let packed_len = raw_voxels.len().div_ceil(4).max(1);
        let need_new_voxel_buf = backend
            .voxels_buf
            .as_ref()
            .map(|(_, len)| *len != packed_len)
            .unwrap_or(true);

        if need_new_voxel_buf || upload_voxels || self.current_samples == 0 {
            let mut packed = vec![0u32; packed_len];
            for (i, &b) in raw_voxels.iter().enumerate() {
                packed[i >> 2] |= (b as u32) << ((i & 3) * 8);
            }
            let byte_slice = bytemuck::cast_slice(&packed);
            if need_new_voxel_buf {
                let buf = backend.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("voxels_packed_buf"),
                    size: byte_slice.len() as u64,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                backend.queue.write_buffer(&buf, 0, byte_slice);
                backend.voxels_buf = Some((buf, packed_len));
            } else if let Some((buf, _)) = &backend.voxels_buf {
                backend.queue.write_buffer(buf, 0, byte_slice);
            }

            let gpu_mats = build_gpu_materials(project);
            backend
                .queue
                .write_buffer(&backend.materials_buf, 0, bytemuck::cast_slice(&gpu_mats));
        }

        // 2. Ensure resolution buffers exist
        let pixel_count = (w * h) as u64;
        let need_res_bufs = backend
            .accum_buf
            .as_ref()
            .map(|(_, bw, bh)| *bw != w || *bh != h)
            .unwrap_or(true);

        if need_res_bufs {
            let accum_buf = backend.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("accum_buf"),
                size: pixel_count * 16,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let output_buf = backend.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("output_rgba_buf"),
                size: pixel_count * 4,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            let staging_buf = backend.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("staging_rgba_buf"),
                size: pixel_count * 4,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            backend.accum_buf = Some((accum_buf, w, h));
            backend.output_buf = Some((output_buf, w, h));
            backend.staging_buf = Some((staging_buf, w, h));
        }

        if steps == 0 && out_rgba.len() == (w * h * 4) as usize {
            return;
        }

        let (eye, forward, right, up) = camera.basis();
        let sun = display.sun_direction();
        let d = project.grid.dimensions();
        let half_tan = (camera.fov_deg.to_radians() * 0.5).tan();
        let mut flags_bits = 0u32;
        if pt.use_aces {
            flags_bits |= 1;
        }
        if display.show_ground {
            flags_bits |= 2;
        }
        if display.show_background {
            flags_bits |= 4;
        }

        let bind_group = backend.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pathtrace_bg"),
            layout: &backend.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: backend.params_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: backend.voxels_buf.as_ref().unwrap().0.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: backend.materials_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: backend.accum_buf.as_ref().unwrap().0.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: backend.output_buf.as_ref().unwrap().0.as_entire_binding(),
                },
            ],
        });

        let wg_x = w.div_ceil(8);
        let wg_y = h.div_ceil(8);

        for _ in 0..steps.max(1) {
            let params = GpuPathTraceParams {
                cam_eye: [eye.x, eye.y, eye.z, 1.0],
                cam_forward: [forward.x, forward.y, forward.z, 0.0],
                cam_right: [right.x, right.y, right.z, 0.0],
                cam_up: [up.x, up.y, up.z, 0.0],
                sun_dir: [sun.x, sun.y, sun.z, 0.0],
                sky_color: [pt.sky_color[0], pt.sky_color[1], pt.sky_color[2], 1.0],
                ground_color: [pt.ground_color[0], pt.ground_color[1], pt.ground_color[2], 1.0],
                grid_size: [d.x, d.y, d.z, 0],
                resolution: [w, h, self.current_samples, pt.max_bounces],
                lens_post: [half_tan, pt.aperture, pt.focus_dist, pt.exposure],
                post_flags: [pt.vignette, pt.fog_density, pt.sun_intensity, flags_bits as f32],
            };
            backend
                .queue
                .write_buffer(&backend.params_buf, 0, bytemuck::bytes_of(&params));

            let mut encoder = backend
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("pathtrace_encoder"),
                });
            {
                let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("pathtrace_pass"),
                    timestamp_writes: None,
                });
                cpass.set_pipeline(&backend.pipeline);
                cpass.set_bind_group(0, &bind_group, &[]);
                cpass.dispatch_workgroups(wg_x, wg_y, 1);
            }
            backend.queue.submit(Some(encoder.finish()));
            self.current_samples += 1;
        }

        // Copy output_buf -> staging_buf and read back
        let mut copy_enc = backend
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("pathtrace_readback"),
            });
        let byte_size = (w * h * 4) as u64;
        copy_enc.copy_buffer_to_buffer(
            &backend.output_buf.as_ref().unwrap().0,
            0,
            &backend.staging_buf.as_ref().unwrap().0,
            0,
            byte_size,
        );
        backend.queue.submit(Some(copy_enc.finish()));

        let staging_slice = backend.staging_buf.as_ref().unwrap().0.slice(..);
        staging_slice.map_async(wgpu::MapMode::Read, |_| {});
        backend.device.poll(wgpu::Maintain::Wait);

        {
            let mapped = staging_slice.get_mapped_range();
            out_rgba.resize(byte_size as usize, 0);
            out_rgba.copy_from_slice(&mapped);
        }
        backend.staging_buf.as_ref().unwrap().0.unmap();
    }

    #[allow(clippy::too_many_arguments)]
    fn render_cpu_fallback(
        &mut self,
        project: &EditorProject,
        camera: &OrbitCamera,
        display: &DisplaySettings,
        pt: &PathTracerSettings,
        width: usize,
        height: usize,
        steps: u32,
        out_rgba: &mut Vec<u8>,
    ) {
        let total = width * height;
        if self.cpu_accum.len() != total || self.current_samples == 0 {
            self.cpu_accum.clear();
            self.cpu_accum.resize(total, Vec3::ZERO);
        }
        out_rgba.resize(total * 4, 0);
        if steps == 0 {
            return;
        }

        let aspect = width as f32 / height.max(1) as f32;
        let sun_dir = display.sun_direction();
        let grid = &project.grid;

        for _ in 0..steps {
            let sample_idx = self.current_samples;
            let weight = 1.0 / (sample_idx + 1) as f32;

            self.cpu_accum
                .par_chunks_mut(width)
                .enumerate()
                .for_each(|(py, row)| {
                    for (px, acc) in row.iter_mut().enumerate() {
                        let seed = ((py * width + px) as u32)
                            ^ sample_idx.wrapping_mul(1664525).wrapping_add(1013904223);
                        let jx = (mv_core::voxel::hash_u32(seed) & 0xFFFF) as f32 / 65535.0 - 0.5;
                        let jy = (mv_core::voxel::hash_u32(seed.wrapping_add(1)) & 0xFFFF) as f32
                            / 65535.0
                            - 0.5;
                        let ndc_x = ((px as f32 + 0.5 + jx) / width as f32) * 2.0 - 1.0;
                        let ndc_y = 1.0 - ((py as f32 + 0.5 + jy) / height as f32) * 2.0;
                        let (ray_o, ray_d) = camera.ray_for_ndc(Vec2::new(ndc_x, ndc_y), aspect);

                        let rad = if let Some(hit) = dda_raycast_grid(grid, ray_o, ray_d, 1000.0) {
                            let c = grid.get(hit.voxel.x as u32, hit.voxel.y as u32, hit.voxel.z as u32);
                            let col = project.palette.colors[c as usize];
                            let mat = project.materials[c as usize];
                            let albedo = Vec3::new(
                                col.r as f32 / 255.0,
                                col.g as f32 / 255.0,
                                col.b as f32 / 255.0,
                            );
                            if mat.kind == MaterialType::Emit || mat.emit > 0.01 {
                                albedo * (mat.emit.max(1.0) * 3.0)
                            } else {
                                let n = Vec3::new(
                                    hit.normal.x as f32,
                                    hit.normal.y as f32,
                                    hit.normal.z as f32,
                                );
                                let ndotl = n.dot(sun_dir).max(0.0);
                                albedo * (0.35 + ndotl * pt.sun_intensity)
                            }
                        } else {
                            Vec3::from_array(pt.sky_color)
                        };

                        if sample_idx == 0 {
                            *acc = rad;
                        } else {
                            *acc = acc.lerp(rad, weight);
                        }
                    }
                });

            self.current_samples += 1;
        }

        for (i, &acc) in self.cpu_accum.iter().enumerate() {
            let mut c = acc * pt.exposure;
            if pt.use_aces {
                c = (c * (2.51 * c + Vec3::splat(0.03)))
                    / (c * (2.43 * c + Vec3::splat(0.59)) + Vec3::splat(0.14));
            }
            c = c.clamp(Vec3::ZERO, Vec3::ONE).powf(1.0 / 2.2);
            let base = i * 4;
            out_rgba[base] = (c.x * 255.0) as u8;
            out_rgba[base + 1] = (c.y * 255.0) as u8;
            out_rgba[base + 2] = (c.z * 255.0) as u8;
            out_rgba[base + 3] = 255;
        }
    }
}

fn build_gpu_materials(project: &EditorProject) -> [GpuMaterial; 256] {
    std::array::from_fn(|i| {
        let col = project.palette.colors[i];
        let m = project.materials[i];
        let kind_id = match m.kind {
            MaterialType::Diffuse => 0.0,
            MaterialType::Metal => 1.0,
            MaterialType::Glass => 2.0,
            MaterialType::Emit => 3.0,
            MaterialType::Blend => 4.0,
            MaterialType::Cloud => 5.0,
        };
        GpuMaterial {
            albedo: [
                col.r as f32 / 255.0,
                col.g as f32 / 255.0,
                col.b as f32 / 255.0,
                m.alpha,
            ],
            props0: [kind_id, m.rough, m.spec, m.ior],
            props1: [m.emit, m.metal, m.density, m.att],
        }
    })
}

fn compute_scene_hash(project: &EditorProject) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    project.grid.dimensions().to_array().hash(&mut hasher);
    project.grid.raw_data().hash(&mut hasher);
    for i in 1..=255 {
        let c = project.palette.colors[i];
        [c.r, c.g, c.b, c.a].hash(&mut hasher);
        let m = project.materials[i];
        (m.kind as u8).hash(&mut hasher);
        m.rough.to_bits().hash(&mut hasher);
        m.spec.to_bits().hash(&mut hasher);
        m.ior.to_bits().hash(&mut hasher);
        m.emit.to_bits().hash(&mut hasher);
        m.metal.to_bits().hash(&mut hasher);
        m.density.to_bits().hash(&mut hasher);
    }
    hasher.finish()
}

fn compute_camera_and_settings_hash(
    cam: &OrbitCamera,
    display: &DisplaySettings,
    pt: &PathTracerSettings,
) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    cam.target.x.to_bits().hash(&mut hasher);
    cam.target.y.to_bits().hash(&mut hasher);
    cam.target.z.to_bits().hash(&mut hasher);
    cam.distance.to_bits().hash(&mut hasher);
    cam.yaw_deg.to_bits().hash(&mut hasher);
    cam.pitch_deg.to_bits().hash(&mut hasher);
    cam.fov_deg.to_bits().hash(&mut hasher);
    display.sun_azimuth_deg.to_bits().hash(&mut hasher);
    display.sun_elevation_deg.to_bits().hash(&mut hasher);
    display.show_ground.hash(&mut hasher);
    display.show_background.hash(&mut hasher);
    pt.max_bounces.hash(&mut hasher);
    pt.aperture.to_bits().hash(&mut hasher);
    pt.focus_dist.to_bits().hash(&mut hasher);
    pt.exposure.to_bits().hash(&mut hasher);
    pt.vignette.to_bits().hash(&mut hasher);
    pt.use_aces.hash(&mut hasher);
    pt.fog_density.to_bits().hash(&mut hasher);
    pt.sun_intensity.to_bits().hash(&mut hasher);
    for c in pt.sky_color {
        c.to_bits().hash(&mut hasher);
    }
    for c in pt.ground_color {
        c.to_bits().hash(&mut hasher);
    }
    hasher.finish()
}

