//! Effect graph as Skia image filters.
//!
//! `None` means the layer's own content (Skia's null filter input). Named results remember that
//! same choice so a later `Named` input can point at the untouched source.
//!
//! An outline is not a filter. A chain that contains one is split around it. The outline fills
//! and strokes the node's filled regions, so earlier effects do not change the path it traces.

use std::collections::HashMap;

use artscript_ir::{
    Clip, Color, Effect, EffectInput, EffectOp, Field, Fill, Knot, Mask, MaskSource, Node, NodeKind,
    Paint as IrPaint, Path, Ribbon, RibbonContour, RibbonMark, Shape, Sides, Stroke, ribbon_reach,
};
use skia_safe::canvas::SaveLayerRec;
use skia_safe::paint::{Cap, Join, Style};
use skia_safe::region::RegionOp;
use skia_safe::runtime_effect::RuntimeShaderBuilder;
use skia_safe::{
    BlendMode, Canvas, Color4f, ColorMatrix, ContourMeasureIter, Data, FilterMode, IRect, Image,
    ImageFilter, ImageInfo, Matrix, MipmapMode, Paint, PathBuilder, PathOp, PictureRecorder, Point,
    Rect, RoundOut, RuntimeEffect, SamplingOptions, TileMode, color_filters, image_filters,
    path_utils, surfaces,
};

use crate::RenderError;
use crate::draw::{DrawCtx, draw_node};
use crate::field::{FieldUse, field_shader};
use crate::gpu::{self, Backend};
use crate::mark::{self, Ink};
use crate::paint::{color4, content_paint};

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
        EffectInput::Named(name) => Ok(named
            .get(name)
            .cloned()
            .unwrap_or_else(|| Some(image_filters::empty()))),
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
        EffectOp::Shadow {
            dx,
            dy,
            blur,
            color,
        } => Ok(Some(req(
            "shadow",
            image_filters::drop_shadow(
                (*dx, *dy),
                (*blur, *blur),
                color4(*color),
                None,
                input,
                None,
            ),
        )?)),
        EffectOp::Glow {
            radius,
            color,
            strength,
        } => glow(*radius, *color, *strength, input),
        EffectOp::Inner {
            dx,
            dy,
            blur,
            color,
        } => inner(*dx, *dy, *blur, *color, input),
        EffectOp::Rim { width, dir, color } => rim(*width, *dir, *color, input),
        EffectOp::Outline { .. } => Ok(input),
        EffectOp::Grain {
            strength,
            scale,
            seed,
        } => grain(*strength, *scale, *seed, input),
        EffectOp::Adjust { hue, sat, light } => adjust(*hue, *sat, *light, input),
        EffectOp::Merge { inputs, alphas } => merge(inputs, alphas, &input, named, env),
    }
}

fn glow(
    radius: f32,
    color: Color,
    strength: f32,
    input: Option<ImageFilter>,
) -> Result<Option<ImageFilter>, RenderError> {
    let halo = req(
        "glow",
        image_filters::drop_shadow_only(
            (0.0, 0.0),
            (radius, radius),
            color4(color),
            None,
            input.clone(),
            None,
        ),
    )?;
    // The matrix works on unpremultiplied color, so it scales only alpha and clamps it to 1.
    let scale = ColorMatrix::new(
        1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0,
        strength, 0.0,
    );
    let halo = req(
        "glow strength",
        image_filters::color_filter(color_filters::matrix(&scale, None), halo, None),
    )?;
    Ok(Some(req(
        "glow",
        image_filters::merge([Some(halo), input], None),
    )?))
}

fn inner(
    dx: f32,
    dy: f32,
    blur: f32,
    color: Color,
    input: Option<ImageFilter>,
) -> Result<Option<ImageFilter>, RenderError> {
    // CSS inset: offset (0, 6) darkens the top, because the shifted alpha leaves the top uncovered.
    let band = tint(uncovered(&input, (dx, dy))?, color)?;
    let band = maybe_blur(band, blur)?;
    Ok(Some(req(
        "inner composite",
        image_filters::blend(BlendMode::SrcATop, input, band, None),
    )?))
}

fn rim(
    width: f32,
    dir: Point,
    color: Color,
    input: Option<ImageFilter>,
) -> Result<Option<ImageFilter>, RenderError> {
    if width <= 0.0 {
        return Ok(input);
    }
    let len = (dir.x * dir.x + dir.y * dir.y).sqrt();
    let (dx, dy) = if len == 0.0 {
        (1.0, 1.0)
    } else {
        (dir.x / len, dir.y / len)
    };
    // Two inward offsets, weighted like pen.rim_light, so the band is brightest at the edge.
    let far = uncovered(&input, (-dx * width, -dy * width))?;
    let near = uncovered(&input, (-dx * width * 0.45, -dy * width * 0.45))?;
    let band = req(
        "rim band",
        image_filters::arithmetic(0.0, 0.6, 0.4, 0.0, true, near, far, None),
    )?;
    let band = maybe_blur(tint(band, color)?, width * 0.4)?;
    Ok(Some(req(
        "rim composite",
        image_filters::blend(BlendMode::SrcATop, input, band, None),
    )?))
}

fn grain(
    strength: f32,
    scale: f32,
    seed: u32,
    input: Option<ImageFilter>,
) -> Result<Option<ImageFilter>, RenderError> {
    if strength == 0.0 {
        return Ok(input);
    }
    let (lo, hi) = ((seed & 0xffff) as f32, (seed >> 16) as f32);
    let scale = if scale == 0.0 { 0.0001 } else { scale };
    let effect = RuntimeEffect::make_for_shader(GRAIN_NOISE_SKSL, None).map_err(RenderError::Skia)?;
    let mut builder = RuntimeShaderBuilder::new(effect);
    for (name, data) in [("scale", &[scale][..]), ("seed", &[lo, hi])] {
        builder
            .set_uniform_float(name, data)
            .map_err(|err| RenderError::Skia(err.to_string()))?;
    }
    let noise = builder
        .make_shader(&Matrix::default())
        .ok_or_else(|| RenderError::Skia("grain noise".into()))?;
    let noise = req("grain noise", image_filters::shader(noise, None))?;
    let overlay = RuntimeEffect::make_for_blender(GRAIN_BLEND_SKSL, None)
        .map_err(RenderError::Skia)?
        .make_blender(Data::new_copy(&strength.to_ne_bytes()), None)
        .ok_or_else(|| RenderError::Skia("grain blender".into()))?;
    let grained = req(
        "grain",
        image_filters::blend(overlay, input.clone(), noise, None),
    )?;
    // Skia cannot tell that the blender keeps transparent pixels transparent, so it would run the
    // endless noise over the whole clip. SrcIn against the input's alpha, raised to opaque wherever
    // the input shows, bounds the result to the input and leaves its pixels as they are.
    let shows = scale_alpha(input, 255.0)?;
    Ok(Some(req(
        "grain bounds",
        image_filters::blend(BlendMode::SrcIn, shows, grained, None),
    )?))
}

fn adjust(
    hue: f32,
    sat: f32,
    light: f32,
    input: Option<ImageFilter>,
) -> Result<Option<ImageFilter>, RenderError> {
    let effect = color_effect(ADJUST_SKSL)?;
    let mut bytes = Vec::with_capacity(12);
    for v in [hue, sat, light] {
        bytes.extend_from_slice(&v.to_ne_bytes());
    }
    let filter = effect
        .make_color_filter(Data::new_copy(&bytes), None)
        .ok_or_else(|| RenderError::Skia("adjust filter".into()))?;
    Ok(Some(req(
        "adjust",
        image_filters::color_filter(filter, input, None),
    )?))
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
    req(
        "effect blur",
        image_filters::blur((sigma, sigma), TileMode::Decal, input, None),
    )
}

fn scale_alpha(input: Option<ImageFilter>, alpha: f32) -> Result<Option<ImageFilter>, RenderError> {
    if (alpha - 1.0).abs() < 1.0e-6 {
        return Ok(input);
    }
    let matrix = ColorMatrix::new(
        1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0,
        alpha, 0.0,
    );
    let filter = skia_safe::color_filters::matrix(&matrix, None);
    Ok(Some(req(
        "alpha",
        image_filters::color_filter(filter, input, None),
    )?))
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
    let index = &env.ctx.index;
    let local = node
        .outer_bounds(&parent, &|p| Some((index.node(p)?, index.parent_world(p))))
        .unwrap_or_else(|| Rect::from_wh(1.0, 1.0));
    let (mut cull, _) = place.map_rect(local);
    cull.outset((2.0, 2.0));
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

/// The input where the input shifted by `off` does not cover it.
fn uncovered(input: &Option<ImageFilter>, off: (f32, f32)) -> Result<ImageFilter, RenderError> {
    // An offset filter would round `off` to whole pixels and step the band along a curve.
    let shifted = req(
        "edge offset",
        image_filters::matrix_transform(
            &Matrix::translate(off),
            SamplingOptions::new(FilterMode::Linear, MipmapMode::None),
            input.clone(),
        ),
    )?;
    req(
        "edge band",
        image_filters::blend(BlendMode::SrcOut, shifted, input.clone(), None),
    )
}

/// `color` at the band's alpha times the color's own.
fn tint(band: ImageFilter, color: Color) -> Result<ImageFilter, RenderError> {
    let filter = color_filters::blend_with_color_space(color4(color), None, BlendMode::SrcIn)
        .ok_or_else(|| RenderError::Skia("band color".into()))?;
    req("band color", image_filters::color_filter(filter, band, None))
}

fn color_effect(sksl: &str) -> Result<RuntimeEffect, RenderError> {
    RuntimeEffect::make_for_color_filter(sksl, None).map_err(RenderError::Skia)
}

fn req(what: &str, filter: Option<ImageFilter>) -> Result<ImageFilter, RenderError> {
    filter.ok_or_else(|| RenderError::Skia(what.into()))
}

const GRAIN_NOISE_SKSL: &str = r#"
uniform float scale;
uniform float2 seed;

float hash21(float2 p) {
    float3 p3 = fract(float3(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
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
half4 main(float2 p) {
    float n = perlin2(p / scale, seed) * 0.5
        + perlin2(p / (scale * 2.0), seed + float2(17.0, 3.0)) * 0.25
        + perlin2(p / (scale * 4.0), seed + float2(43.0, 9.0)) * 0.125;
    // Chibi divides this sum by its peak. The peak on a flat shape is about 0.58,
    // and 0.875 (the weight sum) left the grain at about two thirds of that swing.
    n = n / 0.58;
    return half4(half3(clamp(0.5 + 0.5 * n, 0.0, 1.0)), 1.0);
}
"#;

/// `src` is the opaque noise gray and `dst` the input.
const GRAIN_BLEND_SKSL: &str = r#"
uniform float strength;

half3 overlay3(half3 cb, half3 cs) {
    half3 lo = 2.0 * cs * cb;
    half3 hi = 1.0 - 2.0 * (1.0 - cs) * (1.0 - cb);
    return half3(cb.r < 0.5 ? lo.r : hi.r, cb.g < 0.5 ? lo.g : hi.g, cb.b < 0.5 ? lo.b : hi.b);
}
half4 main(half4 src, half4 dst) {
    if (dst.a <= 0.001) {
        return dst;
    }
    half ga = min(1.0, 2.0 * strength);
    half3 straight = dst.rgb / dst.a;
    half3 mixed = mix(straight, overlay3(straight, src.rgb), ga);
    return half4(mixed * dst.a, dst.a);
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
bool fits(float L, float2 ab) {
    float3 lin = linear_of_oklab(L, ab.x, ab.y);
    return lin.r >= -0.0001 && lin.r <= 1.0001
        && lin.g >= -0.0001 && lin.g <= 1.0001
        && lin.b >= -0.0001 && lin.b <= 1.0001;
}
half4 main(half4 c) {
    if (c.a <= 0.0) {
        return c;
    }
    float3 lab = oklab_of(linear_rgb(c.rgb / c.a));
    float L = clamp(lab.x + light, 0.0, 1.0);
    float hr = radians(hue);
    float2 ab = max(1.0 + sat, 0.0) * float2(
        lab.y * cos(hr) - lab.z * sin(hr),
        lab.y * sin(hr) + lab.z * cos(hr)
    );
    if (!fits(L, ab)) {
        float lo = 0.0;
        float hi = 1.0;
        for (int i = 0; i < 12; i++) {
            float mid = 0.5 * (lo + hi);
            if (fits(L, ab * mid)) { lo = mid; } else { hi = mid; }
        }
        ab *= lo;
    }
    float3 lin = linear_of_oklab(L, ab.x, ab.y);
    float3 rgb = clamp(float3(
        to_srgb(clamp(lin.r, 0.0, 1.0)),
        to_srgb(clamp(lin.g, 0.0, 1.0)),
        to_srgb(clamp(lin.b, 0.0, 1.0))
    ), 0.0, 1.0);
    return half4(rgb * c.a, c.a);
}
"#;

/// One step of an effect chain that contains an outline. Filters are the Skia graph for a run
/// of non-outline ops. An outline is geometry, so a name defined in an earlier run is not
/// visible to a later one.
pub enum LayerStep<'a> {
    Filter(ImageFilter),
    Outline(OutlineStep<'a>),
}

#[derive(Clone, Copy)]
pub struct OutlineStep<'a> {
    pub width: f32,
    pub color: Color,
    pub sides: Sides,
    pub mask: Option<&'a Mask>,
    pub brush: &'a [RibbonMark],
    pub cover_halo_past: Option<f32>,
}

impl OutlineStep<'_> {
    /// Local units the ring may paint past the traced areas on the sides it draws on.
    pub fn reach(&self) -> f32 {
        self.width + ribbon_reach(self.brush)
    }
}

fn paints_past_its_input(effect: &Effect) -> bool {
    matches!(effect.input, EffectInput::Node(_))
        || match &effect.op {
            EffectOp::Blur { .. } | EffectOp::Shadow { .. } | EffectOp::Glow { .. } => true,
            EffectOp::Merge { inputs, .. } => inputs.iter().any(|i| matches!(i, EffectInput::Node(_))),
            EffectOp::Inner { .. }
            | EffectOp::Rim { .. }
            | EffectOp::Adjust { .. }
            | EffectOp::Grain { .. }
            | EffectOp::Outline { .. } => false,
        }
}

/// Splits a chain that contains a positive outline. A chain with none stays on the single
/// image-filter path.
pub fn layer_steps<'a>(
    effects: &'a [Effect],
    env: &FxEnv<'_>,
) -> Result<Option<Vec<LayerStep<'a>>>, RenderError> {
    let outlined = effects
        .iter()
        .any(|e| matches!(e.op, EffectOp::Outline { width, .. } if width > 0.0));
    if !outlined {
        return Ok(None);
    }
    let mut steps = Vec::new();
    let mut start = 0;
    let mut halo = false;
    let mut inked = 0.0_f32;
    for (i, effect) in effects.iter().enumerate() {
        let EffectOp::Outline { width, color, sides, mask, brush } = &effect.op else {
            continue;
        };
        let (width, color, sides, mask) = (*width, *color, *sides, mask.as_deref());
        if width <= 0.0 {
            continue;
        }
        let run = &effects[start..i];
        if !run.is_empty()
            && let Some(filter) = chain(run, env)?
        {
            steps.push(LayerStep::Filter(filter));
            halo |= run.iter().any(paints_past_its_input);
        }
        let step = OutlineStep { width, color, sides, mask, brush, cover_halo_past: halo.then_some(inked) };
        inked = inked.max(step.reach());
        steps.push(LayerStep::Outline(step));
        start = i + 1;
    }
    if start < effects.len()
        && let Some(filter) = chain(&effects[start..], env)?
    {
        steps.push(LayerStep::Filter(filter));
    }
    Ok(Some(steps))
}

pub(crate) struct Region {
    pub path: Path,
    pub coverage: Coverage,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Coverage {
    Opaque,
    Translucent,
}

/// `world` is the outlined node's canvas transform, which its mask's nodes draw under.
pub fn outline_geometry(
    ctx: &DrawCtx<'_>,
    world: Matrix,
    src: Image,
    local_to_px: &Matrix,
    node: &Node,
    step: OutlineStep<'_>,
) -> Result<Image, RenderError> {
    let OutlineStep { width, color, sides, mask, brush, cover_halo_past } = step;
    if width <= 0.0 {
        return Ok(src);
    }
    let regions = silhouette(node, local_to_px, ctx)?;
    if regions.is_empty() {
        return Ok(src);
    }
    let ring = Ring { regions: &regions, local_to_px, width, color, sides, mask, brush, ctx, world, origin: &node.origin };
    let mut under = ring.composite(&src, BlendMode::DstOver)?;
    if let Some(inked) = cover_halo_past {
        let over = ring.composite(&src, BlendMode::SrcOver)?.image_snapshot();
        let outside = ink_area(&regions, inked)?
            .with_transform(local_to_px)
            .with_toggle_inverse_fill_type();
        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        paint.set_blend_mode(BlendMode::Src);
        paint.set_shader(over.to_shader(None, SamplingOptions::default(), None));
        under.canvas().draw_path(&outside, &paint);
    }
    Ok(under.image_snapshot())
}

fn ink_area(regions: &[Region], reach: f32) -> Result<Path, RenderError> {
    let mut grow = Paint::default();
    grow.set_style(Style::Stroke);
    grow.set_stroke_width(reach * 2.0);
    grow.set_stroke_cap(Cap::Round);
    grow.set_stroke_join(Join::Round);
    regions
        .iter()
        .try_fold(Path::default(), |area, region| {
            let area = skia_safe::op(&area, &region.path, PathOp::Union)?;
            if reach <= 0.0 {
                return Some(area);
            }
            let mut band = PathBuilder::new();
            path_utils::fill_path_with_paint(&region.path, &grow, &mut band, None::<&Rect>, None::<Matrix>);
            skia_safe::op(&area, &band.snapshot(), PathOp::Union)
        })
        .ok_or_else(|| RenderError::Skia("outline ink area".into()))
}

struct Ring<'a> {
    regions: &'a [Region],
    local_to_px: &'a Matrix,
    width: f32,
    color: Color,
    sides: Sides,
    /// Keeps the ring's ink where it keeps it. The content under the ring is not masked.
    mask: Option<&'a Mask>,
    brush: &'a [RibbonMark],
    ctx: &'a DrawCtx<'a>,
    world: Matrix,
    origin: &'a str,
}

impl Ring<'_> {
    /// The traced geometry's box, open on every side the outline draws on.
    fn kept_area(&self) -> Rect {
        const FAR: f32 = 1.0e6;
        let mut bounds = *self.regions[0].path.bounds();
        for region in &self.regions[1..] {
            bounds.join(region.path.bounds());
        }
        let edge = |on: bool, at: f32, open: f32| if on { open } else { at };
        Rect::new(
            edge(self.sides.left, bounds.left, -FAR),
            edge(self.sides.top, bounds.top, -FAR),
            edge(self.sides.right, bounds.right, FAR),
            edge(self.sides.bottom, bounds.bottom, FAR),
        )
    }

    fn composite(&self, src: &Image, mode: BlendMode) -> Result<skia_safe::Surface, RenderError> {
        let mut surface = gpu::surface(self.ctx.backend, (src.width(), src.height()))
            .ok_or_else(|| RenderError::Skia("outline geom surface".into()))?;
        let canvas = surface.canvas();
        canvas.draw_image(src, (0.0, 0.0), None);
        let mut layer = Paint::default();
        layer.set_blend_mode(mode);
        layer.set_alpha_f(self.color.a.clamp(0.0, 1.0));
        canvas.save_layer(&SaveLayerRec::default().paint(&layer));
        canvas.concat(self.local_to_px);
        let mut ink = Paint::default();
        ink.set_anti_alias(true);
        ink.set_color4f(Color4f { a: 1.0, ..color4(self.color) }, None);
        ink.set_stroke_cap(Cap::Round);
        ink.set_stroke_join(Join::Round);
        ink.set_stroke_width(self.width * 2.0);
        canvas.save();
        if !self.sides.is_all() {
            canvas.clip_rect(self.kept_area(), None, true);
        }
        if self.brush.is_empty() {
            for region in self.regions {
                ink.set_style(Style::Fill);
                canvas.draw_path(&region.path, &ink);
                ink.set_style(Style::Stroke);
                canvas.draw_path(&region.path, &ink);
            }
        } else {
            self.brushed(canvas, &ink)?;
        }
        canvas.restore();
        ink.set_style(Style::Fill);
        ink.set_blend_mode(BlendMode::DstOut);
        for region in self.regions.iter().filter(|r| r.coverage == Coverage::Translucent) {
            canvas.draw_path(&region.path, &ink);
        }
        if let Some(mask) = self.mask {
            crate::draw::apply_mask(canvas, mask, self.ctx, self.world)?;
        }
        canvas.restore();
        Ok(surface)
    }
}

impl Ring<'_> {
    /// The traced area filled solid, then each of its contours as a closed ribbon of half width
    /// `width` under the brush stages, so the stages see the ring as one stroke: `m.d > 0` is
    /// the half over the area and `m.d < 0` the half that shows. A single region keeps its path
    /// as traced, so `m.s` starts where the shape's contour position does.
    fn brushed(&self, canvas: &Canvas, ink: &Paint) -> Result<(), RenderError> {
        let area = match self.regions {
            [one] => one.path.clone(),
            many => ink_area(many, 0.0)?,
        };
        let mut fill = ink.clone();
        fill.set_style(Style::Fill);
        canvas.draw_path(&area, &fill);
        let px = 1.0 / mark::device_scale(&canvas.local_to_device_as_3x3());
        let (spine, contours) = ring_contours(&area, self.width, px);
        let ribbon = Ribbon { marks: self.brush.to_vec(), contours, taper: [0.0, 0.0] };
        let paint = IrPaint::Solid(Color { a: 1.0, ..self.color });
        mark::draw_ribbon(canvas, &spine, &paint, &ribbon, self.ctx, self.origin)
    }
}

/// Every contour of `area` as a closed polyline that keeps its first point and runs with the
/// area on its right of travel, with the ribbon contour of constant half width `width` along
/// it. `px` is local units per device pixel, the flattening step.
fn ring_contours(area: &Path, width: f32, px: f32) -> (Path, Vec<RibbonContour>) {
    const VOTES: usize = 8;
    let mut spine = PathBuilder::new();
    let mut contours = Vec::new();
    for measure in ContourMeasureIter::new(area, false, Some(10.0 / px)) {
        let len = measure.length();
        if len <= 0.0 {
            continue;
        }
        let inside = (0..VOTES)
            .filter(|k| {
                let s = len * (*k as f32 + 0.5) / VOTES as f32;
                measure.pos_tan(s).is_some_and(|(p, t)| area.contains(p + Point::new(-t.y, t.x) * (0.5 * px)))
            })
            .count();
        let n = ((len / (0.5 * px)).ceil() as usize).max(2);
        let mut pts: Vec<Point> = (0..n).filter_map(|i| measure.pos_tan(len * i as f32 / n as f32)).map(|(p, _)| p).collect();
        if inside * 2 < VOTES {
            pts[1..].reverse();
        }
        spine.move_to(pts[0]);
        for p in &pts[1..] {
            spine.line_to(*p);
        }
        spine.close();
        let knot = |s: f32| Knot { s, half_width: width, pressure: 1.0, angle: 0.0, tilt: 0.0, velocity: 1.0 };
        contours.push(RibbonContour { len, closed: true, knots: vec![knot(0.0), knot(len)] });
    }
    (spine.snapshot(), contours)
}

/// `local_to_px` maps the node's own space to the pixels a field mask or a brush coverage is
/// sampled on.
pub(crate) fn silhouette(node: &Node, local_to_px: &Matrix, ctx: &DrawCtx<'_>) -> Result<Vec<Region>, RenderError> {
    let mut regions = Vec::new();
    gather(node, local_to_px, true, ctx, &mut regions)?;
    Ok(regions)
}

/// `root` is the outlined node. Its transform is already on the canvas, and its opacity
/// tints the finished layer, so neither is baked into the regions. `to_px` maps the parent's
/// space to pixels, or the root's own space.
fn gather(
    node: &Node,
    to_px: &Matrix,
    root: bool,
    ctx: &DrawCtx<'_>,
    out: &mut Vec<Region>,
) -> Result<(), RenderError> {
    let opacity = if root {
        1.0
    } else {
        node.opacity.clamp(0.0, 1.0)
    };
    if opacity <= 0.0 {
        return Ok(());
    }
    let (here, local_to_px) = if root {
        (Matrix::new_identity(), *to_px)
    } else {
        (node.transform, *to_px * node.transform)
    };
    let mut local = Vec::new();
    match &node.kind {
        NodeKind::Shape(shape) => {
            push_shape(shape, &node.origin, &local_to_px, ctx, &mut local)?;
            for stamp in &shape.stamps {
                gather(stamp, &local_to_px, false, ctx, &mut local)?;
            }
            let children = if fill_shows(shape) { &[][..] } else { &shape.children[..] };
            for child in children {
                let mut nested = Vec::new();
                gather(child, &local_to_px, false, ctx, &mut nested)?;
                local.extend(nested.into_iter().filter_map(|region| {
                    skia_safe::op(&region.path, &shape.clip(), PathOp::Intersect)
                        .filter(|p| !p.is_empty())
                        .map(|path| Region { path, coverage: region.coverage })
                }));
            }
        }
        NodeKind::Group { children, .. } => {
            for child in children {
                gather(child, &local_to_px, false, ctx, &mut local)?;
            }
        }
    }
    if let Some(clip) = &node.clip {
        local = local
            .into_iter()
            .filter_map(|region| {
                clip_region(&region.path, clip).map(|path| Region {
                    path,
                    coverage: if clip.feather > 0.0 {
                        Coverage::Translucent
                    } else {
                        region.coverage
                    },
                })
            })
            .collect();
    }
    if let Some(mask) = &node.mask {
        local = mask_regions(local, mask, &local_to_px, ctx)?;
    }
    for region in local {
        out.push(Region {
            path: region.path.with_transform(&here),
            coverage: if opacity < 1.0 {
                Coverage::Translucent
            } else {
                region.coverage
            },
        });
    }
    Ok(())
}

/// A node mask or a thresholded field mask cuts each region to what the mask keeps, marked
/// translucent when the edge is feathered. Any other mask keeps the whole region, marked
/// translucent because the mask can lower its alpha anywhere.
fn mask_regions(
    regions: Vec<Region>,
    mask: &Mask,
    local_to_px: &Matrix,
    ctx: &DrawCtx<'_>,
) -> Result<Vec<Region>, RenderError> {
    let kept = match &mask.source {
        MaskSource::Nodes(nodes) if !mask.invert => {
            let mut kept = Vec::new();
            for node in nodes {
                gather(node, local_to_px, false, ctx, &mut kept)?;
            }
            kept
        }
        MaskSource::Field { field, threshold: Some(t) } => {
            field_kept(field, *t, mask.invert, &regions, local_to_px)?
                .map(|path| Region { path, coverage: Coverage::Opaque })
                .into_iter()
                .collect()
        }
        _ => {
            return Ok(regions
                .into_iter()
                .map(|region| Region { coverage: Coverage::Translucent, ..region })
                .collect());
        }
    };
    Ok(regions
        .iter()
        .flat_map(|region| {
            kept.iter().filter_map(move |k| {
                skia_safe::op(&region.path, &k.path, PathOp::Intersect)
                    .filter(|p| !p.is_empty())
                    .map(|path| Region {
                        path,
                        coverage: if mask.feather <= 0.0
                            && region.coverage == Coverage::Opaque
                            && k.coverage == Coverage::Opaque
                        {
                            Coverage::Opaque
                        } else {
                            Coverage::Translucent
                        },
                    })
            })
        })
        .collect())
}

/// The pixels a thresholded field keeps over `regions`, sampled at the same pixel centers the
/// mask draws on, as a path in local space.
fn field_kept(
    field: &Field,
    threshold: f32,
    invert: bool,
    regions: &[Region],
    local_to_px: &Matrix,
) -> Result<Option<Path>, RenderError> {
    let Some(bounds) = regions.iter().map(|r| *r.path.bounds()).reduce(|mut a, b| {
        a.join(b);
        a
    }) else {
        return Ok(None);
    };
    kept_pixels(bounds, local_to_px, invert, |canvas| {
        let mut paint = Paint::default();
        paint.set_shader(field_shader(
            field,
            FieldUse::Coverage { threshold: Some(threshold), as_luma: false },
        )?);
        canvas.draw_paint(&paint);
        Ok(())
    })
}

/// The pixels a brushed fill covers by more than one half, sampled at the same pixel centers the
/// fill draws on, as a path in local space. `None` when the brush leaves nothing.
fn brush_kept(
    geometry: &Path,
    fill: &Fill,
    origin: &str,
    local_to_px: &Matrix,
    ctx: &DrawCtx<'_>,
) -> Result<Option<Path>, RenderError> {
    let reach = fill.reach();
    let bounds = geometry.compute_tight_bounds().with_outset((reach, reach));
    kept_pixels(bounds, local_to_px, false, |canvas| {
        mark::draw_area(canvas, geometry, fill, ctx, origin, Ink::Coverage)
    })
}

/// The pixels of `bounds` where `draw` leaves alpha above one half, as a path in local space.
fn kept_pixels(
    bounds: Rect,
    local_to_px: &Matrix,
    invert: bool,
    draw: impl FnOnce(&Canvas) -> Result<(), RenderError>,
) -> Result<Option<Path>, RenderError> {
    let Some(px_to_local) = local_to_px.invert() else {
        return Ok(None);
    };
    let area: IRect = local_to_px.map_rect(bounds).0.round_out();
    if area.is_empty() {
        return Ok(None);
    }
    let info = ImageInfo::new_a8(area.size());
    let mut surface = surfaces::raster(&info, None, None)
        .ok_or_else(|| RenderError::Skia("coverage surface".into()))?;
    let canvas = surface.canvas();
    canvas.translate((-area.left as f32, -area.top as f32));
    canvas.concat(local_to_px);
    draw(canvas)?;
    let row = area.width() as usize;
    let mut alpha = vec![0u8; row * area.height() as usize];
    if !surface.read_pixels(&info, &mut alpha, row, (0, 0)) {
        return Err(RenderError::Skia("coverage pixels".into()));
    }
    let mut kept: Vec<skia_safe::Region> = alpha
        .chunks(row)
        .zip(area.top..)
        .map(|(line, y)| {
            let mut region = skia_safe::Region::new();
            let mut x = area.left;
            for run in line.chunk_by(|a, b| (*a > 127) == (*b > 127)) {
                let end = x + run.len() as i32;
                if (run[0] > 127) != invert {
                    region.op_rect(IRect::new(x, y, end, y + 1), RegionOp::Union);
                }
                x = end;
            }
            region
        })
        .collect();
    while kept.len() > 1 {
        let mut rows = kept.into_iter();
        kept = std::iter::from_fn(|| {
            let mut a = rows.next()?;
            if let Some(b) = rows.next() {
                a.op_region(&b, RegionOp::Union);
            }
            Some(a)
        })
        .collect();
    }
    Ok(kept
        .pop()
        .and_then(|region| region.boundary_path())
        .map(|path| path.with_transform(&px_to_local)))
}

/// Fill path, or the coverage a brushed fill leaves, plus the stroke's own filled outline.
fn push_shape(
    shape: &Shape,
    origin: &str,
    local_to_px: &Matrix,
    ctx: &DrawCtx<'_>,
    out: &mut Vec<Region>,
) -> Result<(), RenderError> {
    if let Some(coverage) = shape.fill.as_ref().and_then(|f| coverage(&f.paint))
        && !shape.geometry.is_empty()
    {
        let path = match shape.coverage_clip() {
            Some(fill) => brush_kept(&shape.geometry, fill, origin, local_to_px, ctx)?,
            None => Some(shape.geometry.clone()),
        };
        if let Some(path) = path {
            out.push(Region { path, coverage });
        }
    }
    if let Some(stroke) = &shape.stroke
        && let Some(coverage) = coverage(&stroke.paint)
    {
        let filled = match &stroke.ink {
            Some(ink) => ink.clone(),
            None => stroke_fill(&shape.geometry, stroke)?,
        };
        if !filled.is_empty() {
            out.push(Region { path: filled, coverage });
        }
    }
    Ok(())
}

fn stroke_fill(path: &Path, stroke: &Stroke) -> Result<Path, RenderError> {
    let paint = content_paint(&stroke.paint, 0.0, Some(stroke))?;
    let mut dst = PathBuilder::new();
    path_utils::fill_path_with_paint(path, &paint, &mut dst, None::<&Rect>, None::<Matrix>);
    Ok(dst.snapshot())
}

fn clip_region(path: &Path, clip: &Clip) -> Option<Path> {
    let op = if clip.invert {
        PathOp::Difference
    } else {
        PathOp::Intersect
    };
    skia_safe::op(path, &clip.path, op).filter(|p| !p.is_empty())
}

/// Whether the fill paints anywhere, so the shape's children clip to it instead of adding
/// their own areas.
pub(crate) fn fill_shows(shape: &Shape) -> bool {
    shape.fill.as_ref().and_then(|f| coverage(&f.paint)).is_some()
}

fn coverage(paint: &IrPaint) -> Option<Coverage> {
    let (least, most) = match paint {
        IrPaint::Solid(c) => (c.a, c.a),
        IrPaint::Linear { stops, .. }
        | IrPaint::Radial { stops, .. }
        | IrPaint::Conic { stops, .. }
        | IrPaint::Field { stops, .. } => stops
            .iter()
            .fold((1.0_f32, 0.0_f32), |(least, most), stop| {
                (least.min(stop.color.a), most.max(stop.color.a))
            }),
    };
    if most <= 0.0 {
        None
    } else if least >= 1.0 {
        Some(Coverage::Opaque)
    } else {
        Some(Coverage::Translucent)
    }
}

/// Runs `filter` in `local_to_px` over a device-space image of that same layer.
pub fn apply_image_filter(
    backend: Backend,
    src: Image,
    local_to_px: &Matrix,
    filter: ImageFilter,
) -> Result<Image, RenderError> {
    let mut surface = gpu::surface(backend, (src.width(), src.height()))
        .ok_or_else(|| RenderError::Skia("filter surface".into()))?;
    let canvas = surface.canvas();
    canvas.concat(local_to_px);
    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    paint.set_image_filter(filter);
    canvas.save_layer(&SaveLayerRec::default().paint(&paint));
    canvas.reset_matrix();
    canvas.draw_image(&src, (0.0, 0.0), None);
    canvas.restore();
    Ok(surface.image_snapshot())
}

pub fn white_paint() -> Paint {
    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    paint.set_color4f(
        Color4f {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 1.0,
        },
        None,
    );
    paint
}
