//! Skia paints, blend modes, and gradient shaders. Stops are already dense sRGB.

use ramose_geom::ToSk;
use ramose_ir::{Blend, Cap, Color, Join, Paint as IrPaint, Stop, Stroke};
use skia_safe::gradient::{self, Colors, Gradient, Interpolation};
use skia_safe::paint::{Cap as SkCap, Join as SkJoin, Style};
use skia_safe::runtime_effect::ChildPtr;
use skia_safe::{
    BlendMode, BlurStyle, Color4f, Data, MaskFilter, Matrix, Paint, PathEffect, Point, RuntimeEffect,
    Shader, TileMode,
};

use crate::field::{FieldUse, field_shader};
use crate::RenderError;

pub fn color4(c: Color) -> Color4f {
    Color4f { r: c.r, g: c.g, b: c.b, a: c.a }
}

pub fn blend_mode(blend: Blend) -> BlendMode {
    match blend {
        Blend::Normal => BlendMode::SrcOver,
        Blend::Multiply => BlendMode::Multiply,
        Blend::Screen => BlendMode::Screen,
        Blend::Overlay => BlendMode::Overlay,
        Blend::Darken => BlendMode::Darken,
        Blend::Lighten => BlendMode::Lighten,
        Blend::ColorDodge => BlendMode::ColorDodge,
        Blend::ColorBurn => BlendMode::ColorBurn,
        Blend::HardLight => BlendMode::HardLight,
        Blend::SoftLight => BlendMode::SoftLight,
        Blend::Difference => BlendMode::Difference,
        Blend::Exclusion => BlendMode::Exclusion,
        Blend::Hue => BlendMode::Hue,
        Blend::Saturation => BlendMode::Saturation,
        Blend::Color => BlendMode::Color,
        Blend::Luminosity => BlendMode::Luminosity,
        Blend::Plus => BlendMode::Plus,
        Blend::Erase => BlendMode::DstOut,
    }
}

pub fn css_blend(blend: Blend) -> Option<&'static str> {
    match blend {
        Blend::Normal | Blend::Erase => None,
        Blend::Multiply => Some("multiply"),
        Blend::Screen => Some("screen"),
        Blend::Overlay => Some("overlay"),
        Blend::Darken => Some("darken"),
        Blend::Lighten => Some("lighten"),
        Blend::ColorDodge => Some("color-dodge"),
        Blend::ColorBurn => Some("color-burn"),
        Blend::HardLight => Some("hard-light"),
        Blend::SoftLight => Some("soft-light"),
        Blend::Difference => Some("difference"),
        Blend::Exclusion => Some("exclusion"),
        Blend::Hue => Some("hue"),
        Blend::Saturation => Some("saturation"),
        Blend::Color => Some("color"),
        Blend::Luminosity => Some("luminosity"),
        Blend::Plus => Some("plus-lighter"),
    }
}

pub fn linear_gradient(colors: &[Color4f], pos: &[f32], from: Point, to: Point) -> Result<Shader, RenderError> {
    gradient_between(colors, pos, |grad| {
        gradient::shaders::linear_gradient((from, to), grad, None)
    })
}

pub fn radial_gradient(colors: &[Color4f], pos: &[f32], center: Point, radius: f32) -> Result<Shader, RenderError> {
    gradient_between(colors, pos, |grad| {
        gradient::shaders::radial_gradient((center, radius), grad, None)
    })
}

pub fn sweep_gradient(colors: &[Color4f], pos: &[f32], center: Point, start_deg: f32) -> Result<Shader, RenderError> {
    gradient_between(colors, pos, |grad| {
        gradient::shaders::sweep_gradient(center, (start_deg, start_deg + 360.0), grad, None)
    })
}

fn gradient_between(
    colors: &[Color4f],
    pos: &[f32],
    build: impl FnOnce(&Gradient<'_>) -> Option<Shader>,
) -> Result<Shader, RenderError> {
    let spec = Colors::new(colors, Some(pos), TileMode::Clamp, None);
    let grad = Gradient::new(spec, Interpolation::default());
    build(&grad).ok_or_else(|| RenderError::Skia("gradient".into()))
}

pub fn content_paint(src: &IrPaint, blur: f32, stroke: Option<&Stroke>) -> Result<Paint, RenderError> {
    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    match src {
        IrPaint::Solid(c) => {
            paint.set_color4f(color4(*c), None);
        }
        IrPaint::Linear { from, to, stops } => {
            paint.set_shader(stops_linear(stops, from.to_sk(), to.to_sk())?);
        }
        IrPaint::Radial { center, radius, stops } => {
            paint.set_shader(stops_radial(stops, center.to_sk(), *radius)?);
        }
        IrPaint::Conic { center, start_deg, stops } => {
            paint.set_shader(stops_sweep(stops, center.to_sk(), *start_deg)?);
        }
        IrPaint::Field { field, stops } => {
            paint.set_shader(field_shader(field, FieldUse::Stops(stops))?);
        }
    }
    if let Some(stroke) = stroke {
        paint.set_style(Style::Stroke);
        paint.set_stroke_width(stroke.width);
        paint.set_stroke_cap(match stroke.cap {
            Cap::Round => SkCap::Round,
            Cap::Butt => SkCap::Butt,
            Cap::Square => SkCap::Square,
        });
        paint.set_stroke_join(match stroke.join {
            Join::Round => SkJoin::Round,
            Join::Miter => SkJoin::Miter,
            Join::Bevel => SkJoin::Bevel,
        });
        if !stroke.dash.is_empty() {
            let mut dash = stroke.dash.clone();
            if dash.len() % 2 == 1 {
                dash = dash.repeat(2);
            }
            if let Some(effect) = PathEffect::dash(&dash, 0.0) {
                paint.set_path_effect(effect);
            }
        }
    }
    if blur > 0.0
        && let Some(filter) = MaskFilter::blur(BlurStyle::Normal, blur, true)
    {
        paint.set_mask_filter(filter);
    }
    Ok(paint)
}

/// SkSL for [`dithered`]: the child's premultiplied color moved by under half an 8-bit level per
/// channel, with the offset hashed from the device pixel relative to `u_origin`.
fn dither_sksl() -> String {
    format!(
        "{}\n{}",
        ramose_brush::HASH,
        r#"uniform shader u_src;
uniform float3x3 u_ltd;
uniform float2 u_origin;
half4 main(float2 p) {
    float4 c = float4(u_src.eval(p));
    float3 d = u_ltd * float3(p, 1);
    float n = (hcore(floor(d.xy / d.z) - u_origin) - 0.5) / 255;
    return half4(clamp(c.rgb + n, 0, c.a), c.a);
}
"#
    )
}

thread_local! {
    static DITHER: RuntimeEffect =
        RuntimeEffect::make_for_shader(dither_sksl(), None).expect("dither shader compiles");
}

/// The device pixel of local (0,0) under `ltd`. Dither is hashed relative to it, so two copies
/// of a node a whole number of pixels apart dither alike.
pub fn dither_origin(ltd: &Matrix) -> [f32; 2] {
    let o = ltd.map_point((0.0, 0.0));
    [o.x.round(), o.y.round()]
}

/// `paint` with its shader dithered in place of Skia's dither, which follows the device's 8x8
/// grid. A paint without a shader is a solid color and is left as it is.
pub fn dithered(mut paint: Paint, ltd: &Matrix) -> Result<Paint, RenderError> {
    let Some(src) = paint.shader() else {
        return Ok(paint);
    };
    let bytes: Vec<u8> = mat9(ltd).iter().chain(&dither_origin(ltd)).flat_map(|f| f.to_ne_bytes()).collect();
    let shader = DITHER
        .with(|e| e.make_shader(Data::new_copy(&bytes), &[ChildPtr::from(src)], None))
        .ok_or_else(|| RenderError::Skia("dither shader".into()))?;
    paint.set_shader(shader);
    Ok(paint)
}

/// Column-major 3x3, as SkSL lays out a `float3x3` uniform.
pub fn mat9(m: &Matrix) -> [f32; 9] {
    [
        m.scale_x(), m.skew_y(), m.persp_x(),
        m.skew_x(), m.scale_y(), m.persp_y(),
        m.translate_x(), m.translate_y(), m[8],
    ]
}

fn stops_linear(stops: &[Stop], from: Point, to: Point) -> Result<Shader, RenderError> {
    let (colors, pos) = stop_arrays(stops);
    linear_gradient(&colors, &pos, from, to)
}

fn stops_radial(stops: &[Stop], center: Point, radius: f32) -> Result<Shader, RenderError> {
    let (colors, pos) = stop_arrays(stops);
    radial_gradient(&colors, &pos, center, radius)
}

fn stops_sweep(stops: &[Stop], center: Point, start_deg: f32) -> Result<Shader, RenderError> {
    let (colors, pos) = stop_arrays(stops);
    sweep_gradient(&colors, &pos, center, start_deg)
}

fn stop_arrays(stops: &[Stop]) -> (Vec<Color4f>, Vec<f32>) {
    let colors = stops.iter().map(|s| color4(s.color)).collect();
    let pos = stops.iter().map(|s| s.pos).collect();
    (colors, pos)
}
