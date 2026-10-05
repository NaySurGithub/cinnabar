//! Post-pass WGSL against the host sandbox prelude (`scene`, `param`, `blur`, `bloom`, ...).

pub const BOSS_AURA: &str = r#"
fn effect(uv: vec2<f32>) -> vec3<f32> {
    let base = scene(uv);
    let center = vec3<f32>(param(0u), param(1u) + 1.4, param(2u));
    let p = world_to_uv(center);
    let top = world_to_uv(center + vec3<f32>(0.0, param(7u), 0.0));
    if p.z <= 0.0 || top.z <= 0.0 {
        return base;
    }
    let aspect = vec2<f32>(frame.resolution.x * frame.resolution.w, 1.0);
    let radius = max(length((top.xy - p.xy) * aspect), 0.01);
    let d = length((uv - p.xy) * aspect) / radius;
    let angle = atan2(uv.y - p.y, (uv.x - p.x) * aspect.x);
    let t = seconds();
    let flame = 0.75 + 0.25 * sin(angle * 7.0 + t * 5.0) * sin(angle * 3.0 - t * 3.0);
    let glow = exp(-d * d * 1.6) * flame;
    let ring = exp(-pow((d - 1.0 - 0.08 * sin(t * 6.0)) / 0.12, 2.0)) * 0.6;
    return base + vec3<f32>(param(4u), param(5u), param(6u)) * (glow * 0.55 + ring) * param(3u);
}
"#;

pub const BEAM_BLOOM: &str = r#"
fn segment_distance(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-6), 0.0, 1.0);
    return length(pa - ba * h);
}

fn effect(uv: vec2<f32>) -> vec3<f32> {
    let base = scene(uv);
    let a = world_to_uv(vec3<f32>(param(0u), param(1u), param(2u)));
    let b = world_to_uv(vec3<f32>(param(4u), param(5u), param(6u)));
    if a.z <= 0.0 || b.z <= 0.0 {
        return base;
    }
    let px = frame.resolution.xy;
    let d = segment_distance(uv * px, a.xy * px, b.xy * px);
    let mask = exp(-pow(d / param(7u), 2.0));
    if mask < 0.01 {
        return base;
    }
    return base + bloom(uv, param(7u) * 0.35, 0.55) * mask * param(3u) * 2.5;
}
"#;

pub const WARP: &str = r#"
fn effect(uv: vec2<f32>) -> vec3<f32> {
    let t = seconds();
    let aspect = vec2<f32>(frame.resolution.x * frame.resolution.w, 1.0);
    var offset = vec2<f32>(sin(t * 71.0 + param(6u)), cos(t * 53.0 + param(6u) * 1.7))
        * param(5u) * frame.resolution.zw;
    let projected = world_to_uv(vec3<f32>(param(2u), param(3u), param(4u)));
    let center = select(vec2<f32>(0.5), projected.xy, projected.z > 0.0);
    let rel = (uv - center) * aspect;
    let r = length(rel);
    let wave = exp(-pow((r - param(0u) * 1.6) / 0.06, 2.0)) * param(1u);
    offset += normalize(rel + vec2<f32>(1e-5)) / aspect * wave * 0.03;
    offset += vec2<f32>(sin(uv.y * 60.0 + t * 9.0), cos(uv.x * 45.0 + t * 7.0)) * 0.002 * param(7u);
    var color = scene(uv + offset);
    color.r = mix(color.r, scene(uv + offset * 1.6).r, clamp(wave, 0.0, 1.0));
    let centered = (uv - vec2<f32>(0.5)) * aspect;
    let lanes = atan2(centered.y, centered.x) * 40.0;
    let lane = fract(sin(floor(lanes) * 91.7) * 43758.5);
    let flicker = step(0.5, fract(lane * 13.0 + t * 6.0));
    let streak = step(0.82, lane) * flicker * smoothstep(0.35, 0.9, length(centered))
        * (1.0 - abs(fract(lanes) - 0.5) * 2.0);
    return mix(color, vec3<f32>(1.0), clamp(streak * param(8u), 0.0, 0.85));
}
"#;

pub const GRADE: &str = r#"
fn effect(uv: vec2<f32>) -> vec3<f32> {
    let low = param(0u);
    let death = param(1u);
    var color = scene(uv);
    if death > 0.0 {
        color = mix(color, blur(uv, 2.0 + 6.0 * death), min(death * 1.5, 1.0));
    }
    color = mix(color, vec3<f32>(luminance(color)), clamp(low * 0.85 + death, 0.0, 1.0));
    let aspect = vec2<f32>(frame.resolution.x * frame.resolution.w, 1.0);
    let edge = length((uv - vec2<f32>(0.5)) * aspect);
    let vignette = smoothstep(0.35, 0.95, edge) * (low * (0.55 + 0.35 * param(4u)) + death * 0.6);
    color = mix(color, vec3<f32>(0.3, 0.0, 0.0), clamp(vignette, 0.0, 0.9));
    let l = luminance(color);
    color = mix(color, vec3<f32>(l * 1.4, l * 0.25, l * 0.2), death * 0.85);
    if param(2u) > 0.5 {
        if param(3u) < 0.5 {
            color = vec3<f32>(1.0) - min(color, vec3<f32>(1.0));
        } else {
            color = vec3<f32>(step(0.35, luminance(color)));
        }
    }
    return color;
}
"#;
