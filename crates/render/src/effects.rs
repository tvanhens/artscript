//! Effect graph as Skia image filters.
//!
//! `None` means the layer's own content (Skia's null filter input). Named results remember that
//! same choice so a later `Named` input can point at the untouched source.

use std::collections::HashMap;

use artscript_ir::{Color, Effect, EffectInput, EffectOp};
use skia_safe::{
    BlendMode, Color4f, ColorMatrix, Data, ImageFilter, Matrix, Paint, PictureRecorder, Point, Rect,
    RuntimeEffect, TileMode, image_filters,
};
use skia_safe::runtime_effect::RuntimeShaderBuilder;

use crate::draw::{DrawCtx, draw_node};
use crate::paint::color4;
use crate::RenderError;

pub struct FxEnv<'a> {
    pub ctx: &'a DrawCtx<'a>,
    pub world: Matrix,
}

pub fn chain(effects: &[Effect], env: &FxEnv<'_>) -> Result<Option<ImageFilter>, RenderError> {
    let mut current: Option<ImageFilter> = None;
    let mut named: HashMap<String, Option<ImageFilter>> = HashMap::new();
    for effect in effects {
        let input = resolve(&effect.input, &current, &named, env)?;
        let result = apply(&effect.op, input, &named, env)?;
        if let Some(name) = &effect.name {
            named.insert(name.clone(), result.clone());
        }
        current = result;
    }
    Ok(current)
}

fn resolve(
    input: &EffectInput,
    current: &Option<ImageFilter>,
    named: &HashMap<String, Option<ImageFilter>>,
    env: &FxEnv<'_>,
) -> Result<Option<ImageFilter>, RenderError> {
    match input {
        EffectInput::Previous => Ok(current.clone()),
        EffectInput::Source => Ok(None),
        EffectInput::Named(name) => Ok(named.get(name).cloned().unwrap_or_else(|| Some(image_filters::empty()))),
        EffectInput::Node(path) => node_filter(path, env),
    }
}

fn apply(
    op: &EffectOp,
    input: Option<ImageFilter>,
    named: &HashMap<String, Option<ImageFilter>>,
    env: &FxEnv<'_>,
) -> Result<Option<ImageFilter>, RenderError> {
    match op {
        EffectOp::Blur { sigma } => {
            if *sigma <= 0.0 {
                return Ok(input);
            }
            Ok(Some(req(
                "blur",
                image_filters::blur((*sigma, *sigma), TileMode::Decal, input, None),
            )?))
        }
        EffectOp::Shadow { dx, dy, blur, color } => Ok(Some(req(
            "shadow",
            image_filters::drop_shadow((*dx, *dy), (*blur, *blur), color4(*color), None, input, None),
        )?)),
        EffectOp::Glow { radius, color } => Ok(Some(req(
            "glow",
            image_filters::drop_shadow((0.0, 0.0), (*radius, *radius), color4(*color), None, input, None),
        )?)),
        EffectOp::Inner { dx, dy, blur, color } => inner(*dx, *dy, *blur, *color, input),
        EffectOp::Rim { width, dir, color } => rim(*width, *dir, *color, input),
        EffectOp::Outline { width, color } => outline(*width, *color, input),
        EffectOp::Grain { strength, scale, seed } => grain(*strength, *scale, *seed, input),
        EffectOp::Adjust { hue, sat, light } => adjust(*hue, *sat, *light, input),
        EffectOp::Merge { inputs, alphas } => merge(inputs, alphas, &input, named, env),
    }
}

fn inner(dx: f32, dy: f32, blur: f32, color: Color, input: Option<ImageFilter>) -> Result<Option<ImageFilter>, RenderError> {
    // CSS inset: offset (0, 6) darkens the top, because the shifted alpha leaves the top uncovered.
    let c = color4(color);
    let band = shader_on(
        EDGE_SKSL,
        "child",
        &[("off", &[dx, dy]), ("color", &[c.r, c.g, c.b, c.a])],
        input.clone(),
        dx.abs().max(dy.abs()),
        false,
    )?;
    let band = maybe_blur(band, blur)?;
    Ok(Some(req(
        "inner composite",
        image_filters::blend(BlendMode::SrcATop, input, band, None),
    )?))
}

fn rim(width: f32, dir: Point, color: Color, input: Option<ImageFilter>) -> Result<Option<ImageFilter>, RenderError> {
    if width <= 0.0 {
        return Ok(input);
    }
    let len = (dir.x * dir.x + dir.y * dir.y).sqrt();
    let (dx, dy) = if len == 0.0 { (1.0, 1.0) } else { (dir.x / len, dir.y / len) };
    // Two inward offsets, weighted like pen.rim_light, so the band is brightest at the edge.
    let far = (-dx * width, -dy * width);
    let near = (-dx * width * 0.45, -dy * width * 0.45);
    let c = color4(color);
    let band = shader_on(
        RIM_SKSL,
        "child",
        &[
            ("offFar", &[far.0, far.1]),
            ("offNear", &[near.0, near.1]),
            ("color", &[c.r, c.g, c.b, c.a]),
        ],
        input.clone(),
        width,
        false,
    )?;
    let band = maybe_blur(band, width * 0.4)?;
    Ok(Some(req(
        "rim composite",
        image_filters::blend(BlendMode::SrcATop, input, band, None),
    )?))
}

fn outline(width: f32, color: Color, input: Option<ImageFilter>) -> Result<Option<ImageFilter>, RenderError> {
    if width <= 0.0 {
        return Ok(input);
    }
    // Skia's dilate is a square kernel. A disc of samples keeps the silhouette round at 45 degrees.
    let c = color4(color);
    let grown = shader_on(
        OUTLINE_SKSL,
        "child",
        &[("radius", &[width]), ("color", &[c.r, c.g, c.b, c.a])],
        input.clone(),
        width,
        false,
    )?;
    let grown = maybe_blur(grown, 0.45)?;
    Ok(Some(req(
        "outline composite",
        image_filters::blend(BlendMode::SrcOver, grown, input, None),
    )?))
}

fn grain(strength: f32, scale: f32, seed: u32, input: Option<ImageFilter>) -> Result<Option<ImageFilter>, RenderError> {
    if strength == 0.0 {
        return Ok(input);
    }
    let (lo, hi) = ((seed & 0xffff) as f32, (seed >> 16) as f32);
    let scale = if scale == 0.0 { 0.0001 } else { scale };
    Ok(Some(shader_on(
        GRAIN_SKSL,
        "child",
        &[("strength", &[strength]), ("scale", &[scale]), ("seed", &[lo, hi])],
        input,
        0.0,
        true,
    )?))
}

fn adjust(hue: f32, sat: f32, light: f32, input: Option<ImageFilter>) -> Result<Option<ImageFilter>, RenderError> {
    let effect = color_effect(ADJUST_SKSL)?;
    let mut bytes = Vec::with_capacity(12);
    for v in [hue, sat, light] {
        bytes.extend_from_slice(&v.to_ne_bytes());
    }
    let filter = effect
        .make_color_filter(Data::new_copy(&bytes), None)
        .ok_or_else(|| RenderError::Skia("adjust filter".into()))?;
    Ok(Some(req("adjust", image_filters::color_filter(filter, input, None))?))
}

fn merge(
    inputs: &[EffectInput],
    alphas: &[f32],
    current: &Option<ImageFilter>,
    named: &HashMap<String, Option<ImageFilter>>,
    env: &FxEnv<'_>,
) -> Result<Option<ImageFilter>, RenderError> {
    let mut layers = Vec::with_capacity(inputs.len());
    for (i, input) in inputs.iter().enumerate() {
        let resolved = resolve(input, current, named, env)?;
        let alpha = alphas.get(i).copied().unwrap_or(1.0);
        layers.push(scale_alpha(resolved, alpha)?);
    }
    if layers.is_empty() {
        return Ok(Some(image_filters::empty()));
    }
    Ok(Some(req("merge", image_filters::merge(layers, None))?))
}

fn maybe_blur(input: ImageFilter, sigma: f32) -> Result<ImageFilter, RenderError> {
    if sigma <= 0.0 {
        return Ok(input);
    }
    req("effect blur", image_filters::blur((sigma, sigma), TileMode::Decal, input, None))
}

fn scale_alpha(input: Option<ImageFilter>, alpha: f32) -> Result<Option<ImageFilter>, RenderError> {
    if (alpha - 1.0).abs() < 1.0e-6 {
        return Ok(input);
    }
    let matrix = ColorMatrix::new(
        1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, alpha, 0.0,
    );
    let filter = skia_safe::color_filters::matrix(&matrix, None);
    Ok(Some(req("alpha", image_filters::color_filter(filter, input, None))?))
}

fn node_filter(path: &str, env: &FxEnv<'_>) -> Result<Option<ImageFilter>, RenderError> {
    let Some(node) = env.ctx.index.node(path) else {
        return Ok(Some(image_filters::empty()));
    };
    let parent = env.ctx.index.parent_world(path);
    let Some(inv) = env.world.invert() else {
        return Ok(Some(image_filters::empty()));
    };
    let place = inv * parent;
    let local = node.bounds().unwrap_or_else(|| Rect::from_wh(1.0, 1.0));
    let (mut cull, _) = place.map_rect(local);
    cull.outset((64.0, 64.0));
    if cull.is_empty() {
        cull = Rect::from_xywh(-1.0, -1.0, 2.0, 2.0);
    }
    let mut recorder = PictureRecorder::new();
    let canvas = recorder.begin_recording(cull, false);
    canvas.concat(&place);
    draw_node(canvas, node, env.ctx, parent)?;
    let Some(picture) = recorder.finish_recording_as_picture(None) else {
        return Ok(Some(image_filters::empty()));
    };
    req("node picture", image_filters::picture(picture, None)).map(Some)
}

fn shader_on(
    sksl: &str,
    child: &str,
    uniforms: &[(&str, &[f32])],
    input: Option<ImageFilter>,
    sample_radius: f32,
    restrict: bool,
) -> Result<ImageFilter, RenderError> {
    let effect = RuntimeEffect::make_for_shader(sksl, None).map_err(RenderError::Skia)?;
    let mut builder = RuntimeShaderBuilder::new(effect);
    for (name, data) in uniforms {
        builder
            .set_uniform_float(name, data)
            .map_err(|err| RenderError::Skia(err.to_string()))?;
    }
    req(
        "runtime shader",
        image_filters::runtime_shader_with_options(&builder, sample_radius, child, input, restrict),
    )
}

fn color_effect(sksl: &str) -> Result<RuntimeEffect, RenderError> {
    RuntimeEffect::make_for_color_filter(sksl, None).map_err(RenderError::Skia)
}

fn req(what: &str, filter: Option<ImageFilter>) -> Result<ImageFilter, RenderError> {
    filter.ok_or_else(|| RenderError::Skia(what.into()))
}

const EDGE_SKSL: &str = r#"
uniform shader child;
uniform float2 off;
uniform float4 color;
half4 main(float2 p) {
    half a = child.eval(p).a;
    half s = child.eval(p - off).a;
    half cov = a * (1.0 - s) * color.a;
    return half4(color.rgb * cov, cov);
}
"#;

const RIM_SKSL: &str = r#"
uniform shader child;
uniform float2 offFar;
uniform float2 offNear;
uniform float4 color;
half4 main(float2 p) {
    half a = child.eval(p).a;
    half far = a * (1.0 - child.eval(p - offFar).a);
    half near = a * (1.0 - child.eval(p - offNear).a);
    half band = 0.6 * far + 0.4 * near;
    half cov = band * color.a;
    return half4(color.rgb * cov, cov);
}
"#;

const OUTLINE_SKSL: &str = r#"
uniform shader child;
uniform float radius;
uniform float4 color;
half4 main(float2 p) {
    half m = child.eval(p).a;
    float step = max(radius / 8.0, 0.5);
    for (int j = -8; j <= 8; j++) {
        for (int i = -8; i <= 8; i++) {
            float2 o = float2(float(i), float(j)) * step;
            if (dot(o, o) <= radius * radius) {
                m = max(m, child.eval(p + o).a);
            }
        }
    }
    for (int k = 0; k < 24; k++) {
        float ang = float(k) * 0.26179938779;
        float2 o = float2(cos(ang), sin(ang)) * radius;
        m = max(m, child.eval(p + o).a);
    }
    half cov = m * color.a;
    return half4(color.rgb * cov, cov);
}
"#;

const GRAIN_SKSL: &str = r#"
uniform shader child;
uniform float strength;
uniform float scale;
uniform float2 seed;

float hash21(float2 p) {
    float h = dot(p, float2(127.1, 311.7));
    return fract(sin(h) * 43758.5453123);
}
float2 grad2(float2 cell, float2 sd) {
    float a = hash21(cell + sd) * 6.28318530718;
    return float2(cos(a), sin(a));
}
float perlin2(float2 p, float2 sd) {
    float2 i = floor(p);
    float2 f = fract(p);
    float2 u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    float n00 = dot(grad2(i, sd), f);
    float n10 = dot(grad2(i + float2(1.0, 0.0), sd), f - float2(1.0, 0.0));
    float n01 = dot(grad2(i + float2(0.0, 1.0), sd), f - float2(0.0, 1.0));
    float n11 = dot(grad2(i + float2(1.0, 1.0), sd), f - float2(1.0, 1.0));
    float n = mix(mix(n00, n10, u.x), mix(n01, n11, u.x), u.y);
    return n * 1.41421356;
}
half3 overlay3(half3 cb, half3 cs) {
    half3 lo = 2.0 * cs * cb;
    half3 hi = 1.0 - 2.0 * (1.0 - cs) * (1.0 - cb);
    return half3(cb.r < 0.5 ? lo.r : hi.r, cb.g < 0.5 ? lo.g : hi.g, cb.b < 0.5 ? lo.b : hi.b);
}
half4 main(float2 p) {
    half4 src = child.eval(p);
    if (src.a <= 0.001) {
        return src;
    }
    float n = perlin2(p / scale, seed) * 0.5
        + perlin2(p / (scale * 2.0), seed + float2(17.0, 3.0)) * 0.25
        + perlin2(p / (scale * 4.0), seed + float2(43.0, 9.0)) * 0.125;
    n = n / 0.875;
    float gray = clamp(0.5 + 0.5 * n, 0.0, 1.0);
    float ga = min(1.0, 2.0 * strength);
    half3 straight = src.rgb / src.a;
    half3 mixed = mix(straight, overlay3(straight, half3(gray)), ga);
    return half4(mixed * src.a, src.a);
}
"#;

const ADJUST_SKSL: &str = r#"
uniform float hue;
uniform float sat;
uniform float light;

float to_linear(float u) {
    return u <= 0.04045 ? u / 12.92 : pow((u + 0.055) / 1.055, 2.4);
}
float to_srgb(float u) {
    return u <= 0.0031308 ? 12.92 * u : 1.055 * pow(max(u, 0.0), 1.0 / 2.4) - 0.055;
}
float3 linear_rgb(float3 c) {
    return float3(to_linear(c.r), to_linear(c.g), to_linear(c.b));
}
float3 oklab_of(float3 lin) {
    float l = 0.4122214708 * lin.r + 0.5363325363 * lin.g + 0.0514459929 * lin.b;
    float m = 0.2119034982 * lin.r + 0.6806995451 * lin.g + 0.1073969566 * lin.b;
    float s = 0.0883024619 * lin.r + 0.2817188376 * lin.g + 0.6299787005 * lin.b;
    float l3 = sign(l) * pow(abs(l), 1.0 / 3.0);
    float m3 = sign(m) * pow(abs(m), 1.0 / 3.0);
    float s3 = sign(s) * pow(abs(s), 1.0 / 3.0);
    return float3(
        0.2104542553 * l3 + 0.7936177850 * m3 - 0.0040720468 * s3,
        1.9779984951 * l3 - 2.4285922050 * m3 + 0.4505937099 * s3,
        0.0259040371 * l3 + 0.7827717662 * m3 - 0.8086757660 * s3
    );
}
float3 linear_of_oklab(float L, float a, float b) {
    float l = L + 0.3963377774 * a + 0.2158037573 * b;
    float m = L - 0.1055613458 * a - 0.0638541728 * b;
    float s = L - 0.0894841775 * a - 1.2914855480 * b;
    l = l * l * l;
    m = m * m * m;
    s = s * s * s;
    return float3(
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s
    );
}
bool fits(float L, float C, float H) {
    float hr = radians(H);
    float3 lin = linear_of_oklab(L, C * cos(hr), C * sin(hr));
    return lin.r >= -0.0001 && lin.r <= 1.0001
        && lin.g >= -0.0001 && lin.g <= 1.0001
        && lin.b >= -0.0001 && lin.b <= 1.0001;
}
half4 main(half4 c) {
    if (c.a <= 0.0) {
        return c;
    }
    float3 lab = oklab_of(linear_rgb(c.rgb));
    float L = clamp(lab.x + light, 0.0, 1.0);
    float C = max(length(lab.yz) * (1.0 + sat), 0.0);
    float H = degrees(atan(lab.z, lab.y)) + hue;
    if (!fits(L, C, H)) {
        float lo = 0.0;
        float hi = C;
        for (int i = 0; i < 12; i++) {
            float mid = 0.5 * (lo + hi);
            if (fits(L, mid, H)) { lo = mid; } else { hi = mid; }
        }
        C = lo;
    }
    float hr = radians(H);
    float3 lin = linear_of_oklab(L, C * cos(hr), C * sin(hr));
    return half4(
        clamp(to_srgb(clamp(lin.r, 0.0, 1.0)), 0.0, 1.0),
        clamp(to_srgb(clamp(lin.g, 0.0, 1.0)), 0.0, 1.0),
        clamp(to_srgb(clamp(lin.b, 0.0, 1.0)), 0.0, 1.0),
        c.a
    );
}
"#;

pub fn white_paint() -> Paint {
    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    paint.set_dither(true);
    paint.set_color4f(Color4f { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }, None);
    paint
}
