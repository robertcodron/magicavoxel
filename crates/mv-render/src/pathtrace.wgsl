struct PathTraceParams {
    cam_eye: vec4<f32>,
    cam_forward: vec4<f32>,
    cam_right: vec4<f32>,
    cam_up: vec4<f32>,
    sun_dir: vec4<f32>,
    sky_color: vec4<f32>,
    ground_color: vec4<f32>,
    grid_size: vec4<u32>,      // x = instance count
    resolution: vec4<u32>,     // width, height, sample_index, max_bounces
    lens_post: vec4<f32>,      // fov_half_tan, aperture, focus_dist, exposure
    post_flags: vec4<f32>,     // vignette, fog_density, sun_intensity, flags_bits (bit0=aces, bit1=ground, bit2=sky_bg)
};

struct GpuMaterial {
    albedo: vec4<f32>,         // r, g, b, alpha
    props0: vec4<f32>,         // kind, rough, spec, ior
    props1: vec4<f32>,         // emit, metal, density, att
};

@group(0) @binding(0) var<uniform> params: PathTraceParams;
@group(0) @binding(1) var<storage, read> voxels_packed: array<u32>;
@group(0) @binding(2) var<storage, read> materials: array<GpuMaterial>;
@group(0) @binding(3) var<storage, read_write> accum_buffer: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read_write> output_rgba: array<u32>;

// One placed shape of the scene (two-level structure: instances -> model grids).
// local = inv_rot * (world - offset); world normal = inv_rot^T * local normal.
struct GpuInstance {
    inv_r0: vec4<f32>,
    inv_r1: vec4<f32>,
    inv_r2: vec4<f32>,
    offset: vec4<f32>,
    bmin: vec4<f32>,           // world AABB
    bmax: vec4<f32>,
    size: vec4<u32>,           // model size xyz, w = first word in voxels_packed
};
@group(0) @binding(5) var<storage, read> instances: array<GpuInstance>;

const PI: f32 = 3.14159265359;

fn pcg_hash(input: u32) -> u32 {
    let state = input * 747796405u + 2891336453u;
    let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
    return (word >> 22u) ^ word;
}

fn rand_f32(state: ptr<function, u32>) -> f32 {
    *state = pcg_hash(*state);
    return f32(*state & 0x00FFFFFFu) / 16777216.0;
}

fn sample_disk(u1: f32, u2: f32) -> vec2<f32> {
    let r = sqrt(u1);
    let theta = 2.0 * PI * u2;
    return vec2<f32>(r * cos(theta), r * sin(theta));
}

fn cosine_sample_hemisphere(n: vec3<f32>, u1: f32, u2: f32) -> vec3<f32> {
    let r = sqrt(u1);
    let phi = 2.0 * PI * u2;
    let x = r * cos(phi);
    let y = r * sin(phi);
    let z = sqrt(max(0.0, 1.0 - u1));

    let up = select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 0.0, 1.0), abs(n.z) < 0.999);
    let tangent = normalize(cross(up, n));
    let bitangent = cross(n, tangent);
    return normalize(tangent * x + bitangent * y + n * z);
}

fn get_voxel(size: vec3<i32>, base: u32, p: vec3<i32>) -> u32 {
    if (any(p < vec3<i32>(0)) || any(p >= size)) {
        return 0u;
    }
    let idx = u32(p.x + p.y * size.x + p.z * size.x * size.y);
    let word = voxels_packed[base + (idx >> 2u)];
    let shift = (idx & 3u) * 8u;
    return (word >> shift) & 0xFFu;
}

struct HitResult {
    hit: bool,
    is_ground: bool,
    t: f32,
    voxel_color: u32,
    normal: vec3<f32>,
    pos: vec3<f32>,
};

fn intersect_aabb(ro: vec3<f32>, rd: vec3<f32>, bmin: vec3<f32>, bmax: vec3<f32>) -> vec3<f32> {
    let inv_d = 1.0 / select(rd, vec3<f32>(1e-6), abs(rd) < vec3<f32>(1e-6));
    let t0 = (bmin - ro) * inv_d;
    let t1 = (bmax - ro) * inv_d;
    let tsmaller = min(t0, t1);
    let tbigger = max(t0, t1);
    let tmin = max(tsmaller.x, max(tsmaller.y, tsmaller.z));
    let tmax = min(tbigger.x, min(tbigger.y, tbigger.z));
    var entry_axis = 2;
    if (tsmaller.x > tsmaller.y && tsmaller.x > tsmaller.z) {
        entry_axis = 0;
    } else if (tsmaller.y > tsmaller.z) {
        entry_axis = 1;
    }
    return vec3<f32>(tmin, tmax, f32(entry_axis));
}

/// DDA through one instance's model grid, in the instance's local space. Returns the
/// hit with `t` along the (unit length) local ray, which equals the world distance.
fn trace_instance(inst: GpuInstance, ro_w: vec3<f32>, rd_w: vec3<f32>, max_dist: f32) -> HitResult {
    var result: HitResult;
    result.hit = false;
    result.is_ground = false;
    result.t = max_dist;
    result.voxel_color = 0u;
    result.normal = vec3<f32>(0.0, 0.0, 1.0);
    result.pos = ro_w;

    let rel = ro_w - inst.offset.xyz;
    let ro = vec3<f32>(dot(inst.inv_r0.xyz, rel), dot(inst.inv_r1.xyz, rel), dot(inst.inv_r2.xyz, rel));
    let rd = vec3<f32>(dot(inst.inv_r0.xyz, rd_w), dot(inst.inv_r1.xyz, rd_w), dot(inst.inv_r2.xyz, rd_w));
    let size = vec3<i32>(inst.size.xyz);
    let base = inst.size.w;

    let box_hit = intersect_aabb(ro, rd, vec3<f32>(0.0), vec3<f32>(size));
    let t_enter = box_hit.x;
    let t_exit = box_hit.y;
    if (t_enter > t_exit || t_exit <= 0.0 || t_enter >= max_dist) {
        return result;
    }

    let t_start = max(t_enter, 0.0);
    let p_start = ro + rd * (t_start + 1e-4);
    var map_pos = clamp(vec3<i32>(floor(p_start)), vec3<i32>(0), size - vec3<i32>(1));
    let step = select(vec3<i32>(-1), vec3<i32>(1), rd >= vec3<f32>(0.0));
    let inv_abs = 1.0 / max(abs(rd), vec3<f32>(1e-6));
    let next_b = vec3<f32>(map_pos) + select(vec3<f32>(0.0), vec3<f32>(1.0), rd >= vec3<f32>(0.0));
    var t_max = vec3<f32>(t_start) + abs(next_b - p_start) * inv_abs;
    let t_delta = inv_abs;

    var n = vec3<f32>(0.0);
    let axis = i32(box_hit.z);
    if (axis == 0) {
        n = vec3<f32>(-f32(step.x), 0.0, 0.0);
    } else if (axis == 1) {
        n = vec3<f32>(0.0, -f32(step.y), 0.0);
    } else {
        n = vec3<f32>(0.0, 0.0, -f32(step.z));
    }

    var t_cur = t_start;
    for (var i: u32 = 0u; i < 800u; i = i + 1u) {
        if (any(map_pos < vec3<i32>(0)) || any(map_pos >= size) || t_cur > max_dist) {
            break;
        }
        let c = get_voxel(size, base, map_pos);
        if (c != 0u) {
            result.hit = true;
            result.t = t_cur;
            result.voxel_color = c;
            result.normal = n.x * inst.inv_r0.xyz + n.y * inst.inv_r1.xyz + n.z * inst.inv_r2.xyz;
            result.pos = ro_w + rd_w * t_cur;
            break;
        }
        if (t_max.x < t_max.y) {
            if (t_max.x < t_max.z) {
                map_pos.x += step.x;
                t_cur = t_max.x;
                t_max.x += t_delta.x;
                n = vec3<f32>(-f32(step.x), 0.0, 0.0);
            } else {
                map_pos.z += step.z;
                t_cur = t_max.z;
                t_max.z += t_delta.z;
                n = vec3<f32>(0.0, 0.0, -f32(step.z));
            }
        } else if (t_max.y < t_max.z) {
            map_pos.y += step.y;
            t_cur = t_max.y;
            t_max.y += t_delta.y;
            n = vec3<f32>(0.0, -f32(step.y), 0.0);
        } else {
            map_pos.z += step.z;
            t_cur = t_max.z;
            t_max.z += t_delta.z;
            n = vec3<f32>(0.0, 0.0, -f32(step.z));
        }
    }
    return result;
}

/// Nearest hit over every instance (world AABB culling first), then the ground plane.
fn trace_scene(ro: vec3<f32>, rd: vec3<f32>, max_dist: f32, ignore_color: u32) -> HitResult {
    var result: HitResult;
    result.hit = false;
    result.is_ground = false;
    result.t = max_dist;
    result.voxel_color = 0u;
    result.normal = vec3<f32>(0.0, 0.0, 1.0);
    result.pos = ro;

    let count = params.grid_size.x;
    for (var k: u32 = 0u; k < count; k = k + 1u) {
        let inst = instances[k];
        let box = intersect_aabb(ro, rd, inst.bmin.xyz, inst.bmax.xyz);
        if (box.x > box.y || box.y <= 0.0 || box.x >= result.t) {
            continue;
        }
        let h = trace_instance(inst, ro, rd, result.t);
        if (h.hit && h.t < result.t && h.voxel_color != ignore_color) {
            result = h;
        }
    }

    let flags = u32(params.post_flags.w);
    let show_ground = (flags & 2u) != 0u;
    if (show_ground && rd.z < -1e-5 && ro.z > 0.0) {
        let tg = -ro.z / rd.z;
        if (tg > 1e-4 && tg < result.t && tg < 800.0) {
            result.hit = true;
            result.is_ground = true;
            result.t = tg;
            result.voxel_color = 0u;
            result.normal = vec3<f32>(0.0, 0.0, 1.0);
            result.pos = ro + rd * tg;
        }
    }

    return result;
}

fn evaluate_sky(rd: vec3<f32>) -> vec3<f32> {
    let sun_dir = normalize(params.sun_dir.xyz);
    let sun_int = params.post_flags.z;
    let z_t = clamp(rd.z * 0.5 + 0.5, 0.0, 1.0);
    let horizon = params.sky_color.xyz * 0.85;
    let zenith = params.sky_color.xyz * 1.35 + vec3<f32>(0.05, 0.10, 0.22);
    var sky = mix(horizon, zenith, pow(z_t, 0.7));

    let sun_cos = max(dot(rd, sun_dir), 0.0);
    sky += vec3<f32>(1.0, 0.92, 0.78) * pow(sun_cos, 256.0) * (sun_int * 3.5);
    sky += vec3<f32>(1.0, 0.85, 0.65) * pow(sun_cos, 12.0) * (sun_int * 0.22);
    return sky;
}

fn aces_filmic(x: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

fn schlick_fresnel(cos_theta: f32, ior: f32) -> f32 {
    var r0 = (1.0 - ior) / (1.0 + ior);
    r0 = r0 * r0;
    return r0 + (1.0 - r0) * pow(clamp(1.0 - cos_theta, 0.0, 1.0), 5.0);
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let width = params.resolution.x;
    let height = params.resolution.y;
    if (gid.x >= width || gid.y >= height) {
        return;
    }

    let sample_idx = params.resolution.z;
    let max_bounces = max(params.resolution.w, 2u);
    let pixel_idx = gid.y * width + gid.x;

    var rng = pcg_hash(pixel_idx ^ pcg_hash(sample_idx * 1664525u + 1013904223u));

    // Anti-aliasing subpixel jitter
    let jx = rand_f32(&rng) - 0.5;
    let jy = rand_f32(&rng) - 0.5;

    let aspect = f32(width) / f32(max(height, 1u));
    let ndc_x = ((f32(gid.x) + 0.5 + jx) / f32(width)) * 2.0 - 1.0;
    let ndc_y = 1.0 - ((f32(gid.y) + 0.5 + jy) / f32(height)) * 2.0;

    let half_tan = params.lens_post.x;
    let aperture = params.lens_post.y;
    let focus_dist = max(params.lens_post.z, 1.0);

    var ray_o = params.cam_eye.xyz;
    var ray_d = normalize(
        params.cam_forward.xyz
        + params.cam_right.xyz * (ndc_x * aspect * half_tan)
        + params.cam_up.xyz * (ndc_y * half_tan)
    );

    // Physical Thin-Lens Depth of Field
    if (aperture > 1e-4) {
        let focal_point = ray_o + ray_d * focus_dist;
        let lens_uv = sample_disk(rand_f32(&rng), rand_f32(&rng)) * aperture;
        ray_o = ray_o + params.cam_right.xyz * lens_uv.x + params.cam_up.xyz * lens_uv.y;
        ray_d = normalize(focal_point - ray_o);
    }

    var throughput = vec3<f32>(1.0);
    var radiance = vec3<f32>(0.0);
    let sun_dir = normalize(params.sun_dir.xyz);
    let sun_int = params.post_flags.z;
    let fog_density = params.post_flags.y;

    for (var bounce: u32 = 0u; bounce < max_bounces; bounce = bounce + 1u) {
        let hit = trace_scene(ray_o, ray_d, 1200.0, 0u);
        if (!hit.hit) {
            let flags = u32(params.post_flags.w);
            if (bounce == 0u && (flags & 4u) == 0u) {
                radiance += vec3<f32>(0.08, 0.09, 0.11);
            } else {
                radiance += throughput * evaluate_sky(ray_d);
            }
            break;
        }

        // Exponential fog attenuation on primary ray
        if (bounce == 0u && fog_density > 1e-5) {
            let fog_factor = 1.0 - exp(-hit.t * fog_density);
            let sky_fog = evaluate_sky(ray_d);
            radiance += throughput * sky_fog * fog_factor;
            throughput *= (1.0 - fog_factor);
        }

        var albedo = params.ground_color.xyz;
        var kind: u32 = 0u;
        var rough: f32 = 0.7;
        var spec: f32 = 0.1;
        var ior: f32 = 1.45;
        var emit: f32 = 0.0;
        var metalness: f32 = 0.0;
        var density: f32 = 0.0;

        if (!hit.is_ground) {
            let mat = materials[hit.voxel_color];
            albedo = mat.albedo.xyz;
            kind = u32(mat.props0.x + 0.5);
            rough = clamp(mat.props0.y, 0.02, 1.0);
            spec = mat.props0.z;
            ior = max(mat.props0.w, 1.01);
            emit = mat.props1.x;
            metalness = mat.props1.y;
            density = mat.props1.z;
        }

        // Emissive material (`_emit`)
        if (kind == 3u || emit > 0.01) {
            let power = max(emit, 1.0) * 3.5;
            radiance += throughput * albedo * power;
            break;
        }

        let n = hit.normal;

        // Dielectric Glass (`_glass`)
        if (kind == 2u) {
            let cos_i = clamp(abs(dot(-ray_d, n)), 0.0, 1.0);
            let fresnel = schlick_fresnel(cos_i, ior);
            let rand_Dir = cosine_sample_hemisphere(n, rand_f32(&rng), rand_f32(&rng));
            if (rand_f32(&rng) < fresnel) {
                // Specular reflection off glass surface
                let refl = normalize(reflect(ray_d, n) + rand_Dir * (rough * 0.25));
                ray_o = hit.pos + n * 0.015;
                ray_d = refl;
            } else {
                // Refraction + Beer-Lambert tint through glass voxel
                let eta = 1.0 / ior;
                let refr = refract(ray_d, n, eta);
                let next_d = select(reflect(ray_d, n), refr, length(refr) > 0.01);
                throughput *= mix(vec3<f32>(1.0), albedo, 0.65);
                // Step past the current glass voxel to avoid immediate self-re-hit
                ray_o = hit.pos + ray_d * 0.85;
                ray_d = normalize(next_d + rand_Dir * (rough * 0.15));
            }
            continue;
        }

        // Cloud / Volumetric Scattering Media (`_media`)
        if (kind == 5u) {
            let scatter_prob = clamp(0.35 + density * 0.5, 0.15, 0.9);
            if (rand_f32(&rng) < scatter_prob) {
                let iso_dir = cosine_sample_hemisphere(
                    select(-ray_d, ray_d, rand_f32(&rng) < 0.5),
                    rand_f32(&rng),
                    rand_f32(&rng)
                );
                throughput *= albedo * 0.92;
                ray_o = hit.pos + ray_d * 0.5;
                ray_d = iso_dir;
                continue;
            }
        }

        // Direct Sun Next-Event Estimation (with soft shadow cone jitter)
        let sun_jitter = cosine_sample_hemisphere(sun_dir, rand_f32(&rng), rand_f32(&rng));
        let shadow_dir = normalize(mix(sun_dir, sun_jitter, 0.035));
        let ndotl = max(dot(n, shadow_dir), 0.0);
        if (ndotl > 0.0) {
            let shadow_hit = trace_scene(hit.pos + n * 0.02, shadow_dir, 400.0, 0u);
            if (!shadow_hit.hit) {
                let direct_weight = select(1.0, 1.0 - metalness * 0.7, kind == 1u);
                radiance += throughput * albedo * vec3<f32>(1.0, 0.96, 0.88) * (ndotl * sun_int * 0.85 * direct_weight);
            } else if (!shadow_hit.is_ground) {
                // Translucent shadow through glass voxels
                let occ_mat = materials[shadow_hit.voxel_color];
                if (u32(occ_mat.props0.x + 0.5) == 2u) {
                    radiance += throughput * albedo * occ_mat.albedo.xyz * (ndotl * sun_int * 0.45);
                }
            }
        }

        // Indirect bounce: Metal (`kind == 1u`) vs Diffuse/Plastic
        let diffuse_dir = cosine_sample_hemisphere(n, rand_f32(&rng), rand_f32(&rng));
        if (kind == 1u) {
            let refl_dir = reflect(ray_d, n);
            ray_d = normalize(mix(refl_dir, diffuse_dir, rough * rough));
            throughput *= mix(vec3<f32>(1.0), albedo, max(metalness, 0.75));
        } else {
            let fresnel = schlick_fresnel(max(dot(-ray_d, n), 0.0), ior) * spec * 0.25;
            if (rand_f32(&rng) < fresnel) {
                let refl_dir = reflect(ray_d, n);
                ray_d = normalize(mix(refl_dir, diffuse_dir, rough));
            } else {
                ray_d = diffuse_dir;
                throughput *= albedo;
            }
        }
        ray_o = hit.pos + n * 0.02;

        // Russian roulette termination after bounce 3
        if (bounce >= 3u) {
            let p = clamp(max(throughput.x, max(throughput.y, throughput.z)), 0.05, 0.95);
            if (rand_f32(&rng) > p) {
                break;
            }
            throughput /= p;
        }
    }

    // Clamp fireflies
    radiance = min(radiance, vec3<f32>(15.0));

    // Temporal progressive accumulation
    let prev = accum_buffer[pixel_idx].xyz;
    let weight = 1.0 / f32(sample_idx + 1u);
    let accumulated = select(mix(prev, radiance, weight), radiance, sample_idx == 0u);
    accum_buffer[pixel_idx] = vec4<f32>(accumulated, 1.0);

    // Post-processing: Exposure, Vignette, ACES Filmic, sRGB Gamma
    let exposure = params.lens_post.w;
    let vignette_str = params.post_flags.x;
    let flags = u32(params.post_flags.w);
    let use_aces = (flags & 1u) != 0u;

    var color = accumulated * exposure;
    if (vignette_str > 0.001) {
        let uv_dist = length(vec2<f32>(ndc_x, ndc_y) * 0.7071);
        let vig = clamp(1.0 - uv_dist * uv_dist * vignette_str, 0.1, 1.0);
        color *= vig;
    }

    if (use_aces) {
        color = aces_filmic(color);
    } else {
        color = clamp(color, vec3<f32>(0.0), vec3<f32>(1.0));
    }

    // Linear to sRGB gamma
    color = pow(color, vec3<f32>(1.0 / 2.2));

    let r8 = u32(clamp(color.x * 255.0 + 0.5, 0.0, 255.0));
    let g8 = u32(clamp(color.y * 255.0 + 0.5, 0.0, 255.0));
    let b8 = u32(clamp(color.z * 255.0 + 0.5, 0.0, 255.0));
    output_rgba[pixel_idx] = r8 | (g8 << 8u) | (b8 << 16u) | (255u << 24u);
}

