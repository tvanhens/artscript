//! Skia paints, blend modes, and gradient shaders. Stops are already dense sRGB.

use artscript_ir::{Blend, Cap, Color, Join, Paint as IrPaint, Stop, Stroke};
use skia_safe::gradient::{self, Colors, Gradient, Interpolation};
use skia_safe::paint::{Cap as SkCap, Join as SkJoin, Style};
use skia_safe::{
    BlendMode, BlurStyle, Color4f, MaskFilter, Paint, PathEffect, Point, Shader, TileMode,
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
    paint.set_dither(true);
    match src {
        IrPaint::Solid(c) => {
            paint.set_color4f(color4(*c), None);
        }
        IrPaint::Linear { from, to, stops } => {
            paint.set_shader(stops_linear(stops, *from, *to)?);
        }
        IrPaint::Radial { center, radius, stops } => {
            paint.set_shader(stops_radial(stops, *center, *radius)?);
        }
        IrPaint::Conic { center, start_deg, stops } => {
            paint.set_shader(stops_sweep(stops, *center, *start_deg)?);
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
                dash.push(dash[dash.len() - 1]);
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
