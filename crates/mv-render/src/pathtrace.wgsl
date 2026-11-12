struct PathTraceParams {
    cam_eye: vec4<f32>,
    cam_forward: vec4<f32>,
    cam_right: vec4<f32>,
    cam_up: vec4<f32>,
    sun_dir: vec4<f32>,
    sky_color: vec4<f32>,
    ground_color: vec4<f32>,
    grid_size: vec4<u32>,      // x = instance count, y = point light count, z = voxel_shape
    resolution: vec4<u32>,     // width, height, sample_index, max_bounces
    lens_post: vec4<f32>,      // fov_half_tan, aperture, focus_dist, exposure
    post_flags: vec4<f32>,     // vignette, fog_density, sun_intensity, flags_bits (bit0=aces, bit1=ground, bit2=sky_bg)
    extra_params: vec4<f32>,   // x = bloom, y = voxel_shape, z = unused, w = unused
};

struct GpuMaterial {
    albedo: vec4<f32>,         // r, g, b, alpha
    props0: vec4<f32>,         // kind, rough, spec, ior
    props1: vec4<f32>,         // emit, metal, density, att
};

struct GpuPointLight {
    pos_radius: vec4<f32>,     // xyz = position, w = radius
    col_intensity: vec4<f32>,  // rgb = color, w = intensity
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
@group(0) @binding(6) var<storage, read> point_lights: array<GpuPointLight>;

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

struct ShapeHitResult {
    hit: bool,
    t: f32,
    normal: vec3<f32>,
};

fn intersect_sphere_shape(ro: vec3<f32>, rd: vec3<f32>, map_pos: vec3<i32>, t_enter: f32, t_exit: f32) -> ShapeHitResult {
    var res: ShapeHitResult;
    res.hit = false;
    res.t = t_exit;
    res.normal = vec3<f32>(0.0, 0.0, 1.0);

    let center = vec3<f32>(map_pos) + vec3<f32>(0.5);
    let oc = ro - center;
    let b = dot(oc, rd);
    let c = dot(oc, oc) - 0.25;
    let disc = b * b - c;
    if (disc < 0.0) {
        return res;
    }
    let s = sqrt(disc);
    let t0 = -b - s;
    let t1 = -b + s;
    var t = -1.0;
    if (t0 >= t_enter - 1e-4 && t0 <= t_exit + 1e-4 && t0 > 0.0) {
        t = t0;
    } else if (t1 >= t_enter - 1e-4 && t1 <= t_exit + 1e-4 && t1 > 0.0) {
        t = t1;
    }
    if (t > 0.0) {
        res.hit = true;
        res.t = t;
        res.normal = normalize(ro + rd * t - center);
    }
    return res;
}

fn intersect_cylinder_shape(ro: vec3<f32>, rd: vec3<f32>, map_pos: vec3<i32>, t_enter: f32, t_exit: f32) -> ShapeHitResult {
    var res: ShapeHitResult;
    res.hit = false;
    res.t = t_exit;
    res.normal = vec3<f32>(0.0, 0.0, 1.0);

    let c_xy = vec2<f32>(f32(map_pos.x) + 0.5, f32(map_pos.y) + 0.5);
    let oc = ro.xy - c_xy;
    let z_min = f32(map_pos.z);
    let z_max = z_min + 1.0;

    let a = dot(rd.xy, rd.xy);
    if (a > 1e-6) {
        let b = dot(oc, rd.xy);
        let c = dot(oc, oc) - 0.25;
        let disc = b * b - a * c;
        if (disc >= 0.0) {
            let s = sqrt(disc);
            let t_side0 = (-b - s) / a;
            let t_side1 = (-b + s) / a;
            var t_cand = t_side0;
            if (t_cand < t_enter - 1e-4 || t_cand > t_exit + 1e-4 || t_cand <= 0.0) {
                t_cand = t_side1;
            }
            if (t_cand >= t_enter - 1e-4 && t_cand <= t_exit + 1e-4 && t_cand > 0.0) {
                let z = ro.z + rd.z * t_cand;
                if (z >= z_min - 1e-4 && z <= z_max + 1e-4) {
                    res.hit = true;
                    res.t = t_cand;
                    let p_xy = ro.xy + rd.xy * t_cand;
                    res.normal = normalize(vec3<f32>(p_xy - c_xy, 0.0));
                }
            }
        }
    }

    if (abs(rd.z) > 1e-6) {
        let t_top = (z_max - ro.z) / rd.z;
        if (t_top >= t_enter - 1e-4 && t_top <= t_exit + 1e-4 && t_top > 0.0) {
            let p_xy = ro.xy + rd.xy * t_top;
            if (dot(p_xy - c_xy, p_xy - c_xy) <= 0.25) {
                if (!res.hit || t_top < res.t) {
                    res.hit = true;
                    res.t = t_top;
                    res.normal = vec3<f32>(0.0, 0.0, 1.0);
                }
            }
        }
        let t_bot = (z_min - ro.z) / rd.z;
        if (t_bot >= t_enter - 1e-4 && t_bot <= t_exit + 1e-4 && t_bot > 0.0) {
            let p_xy = ro.xy + rd.xy * t_bot;
            if (dot(p_xy - c_xy, p_xy - c_xy) <= 0.25) {
                if (!res.hit || t_bot < res.t) {
                    res.hit = true;
                    res.t = t_bot;
                    res.normal = vec3<f32>(0.0, 0.0, -1.0);
                }
            }
        }
    }

    return res;
}

fn intersect_clay_shape(ro: vec3<f32>, rd: vec3<f32>, map_pos: vec3<i32>, t_enter: f32, t_exit: f32) -> ShapeHitResult {
    var res: ShapeHitResult;
    res.hit = false;
    res.t = t_exit;
    res.normal = vec3<f32>(0.0, 0.0, 1.0);

    let center = vec3<f32>(map_pos) + vec3<f32>(0.5);
    let b = vec3<f32>(0.38);
    let r = 0.12;
    var t = max(t_enter, 0.0);
    for (var s = 0u; s < 6u; s = s + 1u) {
        let p = ro + rd * t;
        let q = p - center;
        let q_abs = abs(q);
        let d_box = max(q_abs - b, vec3<f32>(0.0));
        let dist = length(d_box) - r;
        if (dist < 0.003) {
            res.hit = true;
            res.t = t;
            if (length(d_box) > 1e-4) {
                res.normal = normalize(sign(q) * d_box);
            } else {
                var flat = vec3<f32>(0.0);
                if (q_abs.x >= q_abs.y && q_abs.x >= q_abs.z) {
                    flat.x = sign(q.x);
                } else if (q_abs.y >= q_abs.z) {
                    flat.y = sign(q.y);
                } else {
                    flat.z = sign(q.z);
                }
                res.normal = flat;
            }
            return res;
        }
        t += max(dist, 0.005);
        if (t > t_exit + 1e-4) {
            break;
        }
    }
    return res;
}

fn intersect_lego_shape(ro: vec3<f32>, rd: vec3<f32>, map_pos: vec3<i32>, t_enter: f32, t_exit: f32, has_stud: bool, entry_n: vec3<f32>) -> ShapeHitResult {
    var res: ShapeHitResult;
    res.hit = true;
    res.t = max(t_enter, 0.0);
    res.normal = entry_n;

    if (has_stud) {
        let z_max = f32(map_pos.z) + 1.0;
        let c_xy = vec2<f32>(f32(map_pos.x) + 0.5, f32(map_pos.y) + 0.5);
        let r_stud = 0.28;
        let oc = ro.xy - c_xy;
        let a = dot(rd.xy, rd.xy);
        if (a > 1e-6) {
            let b = dot(oc, rd.xy);
            let c = dot(oc, oc) - r_stud * r_stud;
            let disc = b * b - a * c;
            if (disc >= 0.0) {
                let s = sqrt(disc);
                let t_cand = (-b - s) / a;
                if (t_cand > 0.0 && t_cand < res.t) {
                    let z = ro.z + rd.z * t_cand;
                    if (z >= z_max && z <= z_max + 0.18) {
                        res.t = t_cand;
                        res.normal = normalize(vec3<f32>(ro.xy + rd.xy * t_cand - c_xy, 0.0));
                    }
                }
            }
        }
        if (abs(rd.z) > 1e-6) {
            let t_cap = (z_max + 0.18 - ro.z) / rd.z;
            if (t_cap > 0.0 && t_cap < res.t) {
                let p_xy = ro.xy + rd.xy * t_cap;
                if (dot(p_xy - c_xy, p_xy - c_xy) <= r_stud * r_stud) {
                    res.t = t_cap;
                    res.normal = vec3<f32>(0.0, 0.0, 1.0);
                }
            }
        }
    }
    return res;
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

    let shape_mode = params.grid_size.z;
    var t_cur = t_start;
    for (var i: u32 = 0u; i < 800u; i = i + 1u) {
        if (any(map_pos < vec3<i32>(0)) || any(map_pos >= size) || t_cur > max_dist) {
            break;
        }
        let t_next = min(t_max.x, min(t_max.y, t_max.z));
        let c = get_voxel(size, base, map_pos);
        if (c != 0u) {
            if (shape_mode == 0u) {
                // Cube
                result.hit = true;
                result.t = t_cur;
                result.voxel_color = c;
                result.normal = n.x * inst.inv_r0.xyz + n.y * inst.inv_r1.xyz + n.z * inst.inv_r2.xyz;
                result.pos = ro_w + rd_w * t_cur;
                break;
            } else if (shape_mode == 2u) {
                // Sphere
                let s_hit = intersect_sphere_shape(ro, rd, map_pos, t_cur, t_next);
                if (s_hit.hit) {
                    result.hit = true;
                    result.t = s_hit.t;
                    result.voxel_color = c;
                    result.normal = s_hit.normal.x * inst.inv_r0.xyz + s_hit.normal.y * inst.inv_r1.xyz + s_hit.normal.z * inst.inv_r2.xyz;
                    result.pos = ro_w + rd_w * s_hit.t;
                    break;
                }
            } else if (shape_mode == 3u) {
                // Cylinder
                let cyl_hit = intersect_cylinder_shape(ro, rd, map_pos, t_cur, t_next);
                if (cyl_hit.hit) {
                    result.hit = true;
                    result.t = cyl_hit.t;
                    result.voxel_color = c;
                    result.normal = cyl_hit.normal.x * inst.inv_r0.xyz + cyl_hit.normal.y * inst.inv_r1.xyz + cyl_hit.normal.z * inst.inv_r2.xyz;
                    result.pos = ro_w + rd_w * cyl_hit.t;
                    break;
                }
            } else if (shape_mode == 4u) {
                // Clay
                let clay_hit = intersect_clay_shape(ro, rd, map_pos, t_cur, t_next);
                if (clay_hit.hit) {
                    result.hit = true;
                    result.t = clay_hit.t;
                    result.voxel_color = c;
                    result.normal = clay_hit.normal.x * inst.inv_r0.xyz + clay_hit.normal.y * inst.inv_r1.xyz + clay_hit.normal.z * inst.inv_r2.xyz;
                    result.pos = ro_w + rd_w * clay_hit.t;
                    break;
                }
            } else if (shape_mode == 1u) {
                // Lego
                let has_stud = get_voxel(size, base, map_pos + vec3<i32>(0, 0, 1)) == 0u;
                let l_hit = intersect_lego_shape(ro, rd, map_pos, t_cur, t_next, has_stud, n);
                if (l_hit.hit) {
                    result.hit = true;
                    result.t = l_hit.t;
                    result.voxel_color = c;
                    result.normal = l_hit.normal.x * inst.inv_r0.xyz + l_hit.normal.y * inst.inv_r1.xyz + l_hit.normal.z * inst.inv_r2.xyz;
                    result.pos = ro_w + rd_w * l_hit.t;
                    break;
                }
            }
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

    // Midnight starry night sky: subtle stars when looking up at night
    if (params.sky_color.z > params.sky_color.x * 2.0 && params.sky_color.x < 0.1 && rd.z > 0.05) {
        let star_p = floor(rd * 160.0);
        let star_hash = pcg_hash(u32(abs(star_p.x) * 73856.0) ^ u32(abs(star_p.y) * 19349.0) ^ u32(abs(star_p.z) * 83492.0));
        if ((star_hash & 0x3FFu) == 42u) {
            sky += vec3<f32>(0.9, 0.95, 1.0) * (f32((star_hash >> 16u) & 0xFFu) / 255.0 * 2.0);
        }
    }
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

        // Direct Point Light Next-Event Estimation
        let light_count = params.grid_size.y;
        for (var li: u32 = 0u; li < light_count; li = li + 1u) {
            let pl = point_lights[li];
            let to_light = pl.pos_radius.xyz - hit.pos;
            let dist = length(to_light);
            let radius = pl.pos_radius.w;
            if (dist < radius && dist > 1e-4) {
                let l_dir = to_light / dist;
                let l_ndotl = max(dot(n, l_dir), 0.0);
                if (l_ndotl > 0.0) {
                    let shadow_hit = trace_scene(hit.pos + n * 0.02, l_dir, dist - 0.04, 0u);
                    if (!shadow_hit.hit || shadow_hit.t >= dist - 0.05) {
                        let atten = pl.col_intensity.w / (1.0 + dist * dist);
                        let cutoff = clamp(1.0 - dist / radius, 0.0, 1.0);
                        let smooth_cutoff = cutoff * cutoff * (3.0 - 2.0 * cutoff);
                        let direct_weight = select(1.0, 1.0 - metalness * 0.7, kind == 1u);
                        let light_rad = pl.col_intensity.xyz * (atten * smooth_cutoff * l_ndotl * direct_weight);
                        radiance += throughput * albedo * light_rad;
                    }
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

    // HDR Bloom filter post-processing
    let bloom_str = params.extra_params.x;
    if (bloom_str > 0.001) {
        var bloom_sum = vec3<f32>(0.0);
        var total_w = 0.0;
        let offsets = array<vec2<i32>, 16>(
            vec2<i32>(-6, 0), vec2<i32>(6, 0), vec2<i32>(0, -6), vec2<i32>(0, 6),
            vec2<i32>(-4, -4), vec2<i32>(4, -4), vec2<i32>(-4, 4), vec2<i32>(4, 4),
            vec2<i32>(-10, 0), vec2<i32>(10, 0), vec2<i32>(0, -10), vec2<i32>(0, 10),
            vec2<i32>(-2, 0), vec2<i32>(2, 0), vec2<i32>(0, -2), vec2<i32>(0, 2)
        );
        let weights = array<f32, 16>(
            0.12, 0.12, 0.12, 0.12,
            0.08, 0.08, 0.08, 0.08,
            0.04, 0.04, 0.04, 0.04,
            0.18, 0.18, 0.18, 0.18
        );
        for (var tap = 0u; tap < 16u; tap = tap + 1u) {
            let sx = clamp(i32(gid.x) + offsets[tap].x, 0, i32(width) - 1);
            let sy = clamp(i32(gid.y) + offsets[tap].y, 0, i32(height) - 1);
            let s_idx = u32(sy) * width + u32(sx);
            let s_col = accum_buffer[s_idx].xyz * exposure;
            let lum = dot(s_col, vec3<f32>(0.2126, 0.7152, 0.0722));
            if (lum > 1.0) {
                let bright = s_col * (lum - 1.0) / lum;
                bloom_sum += bright * weights[tap];
            }
            total_w += weights[tap];
        }
        color += (bloom_sum / total_w) * (bloom_str * 2.5);
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

