//! Draws a brushed fill: the fill's stages as one chain of runtime shaders over the shape's
//! signed distance field. Stage 0 reads the base paint, each later stage reads the stage before
//! it as `u_prev`, and the last stage is drawn over the node's device bounds plus its reach. Every
//! program here was checked by eval, so a compile or binding failure is an internal error and
//! names the node it came from.

use artscript_brush::Seeds;
use artscript_ir::{AreaMark, Fill, Paint as IrPaint, Path};
use skia_safe::{Canvas, Data, FilterMode, IRect, Matrix, MipmapMode, Paint, RoundOut, RuntimeEffect, SamplingOptions, TileMode, runtime_effect::ChildPtr, shaders};

use crate::RenderError;
use crate::draw::DrawCtx;
use crate::paint::content_paint;

pub(crate) fn draw_area(
    canvas: &Canvas,
    geometry: &Path,
    fill: &Fill,
    ctx: &DrawCtx<'_>,
    origin: &str,
) -> Result<(), RenderError> {
    let ltd = canvas.local_to_device_as_3x3();
    let Some(dtl) = ltd.invert() else { return Ok(()) };
    let px = 1.0 / device_scale(&ltd);
    let reach = fill.reach();
    let local = geometry.compute_tight_bounds().with_outset((reach, reach));
    let rounded: IRect = ltd.map_rect(local).0.round_out();
    // The field keeps a 2 px margin past the drawn area, so `edge` at its border is measured, not clamped.
    let field_rect = rounded.with_outset((2, 2));
    let Some(clip) = canvas.device_clip_bounds() else { return Ok(()) };
    let Some(dev) = IRect::intersect(&clip, &rounded) else { return Ok(()) };
    if dev.is_empty() {
        return Ok(());
    }
    let sdf = ctx
        .sdf
        .borrow_mut()
        .get(geometry, &ltd, field_rect)?
        .to_raw_shader((TileMode::Clamp, TileMode::Clamp), nearest(), None)
        .ok_or_else(|| RenderError::Skia("sdf shader".into()))?;
    // A solid paint is bound exactly as a uniform. Any other paint is a child shader placed by the
    // device matrix, so stage 0 reads it at each device pixel.
    let (u_paint, mut prev, mut from_prev) = match &fill.paint {
        IrPaint::Solid(c) => ([c.r, c.g, c.b, c.a], shaders::color(skia_safe::Color::TRANSPARENT), 0.0_f32),
        other => {
            let base = content_paint(other, 0.0, None)?
                .shader()
                .ok_or_else(|| RenderError::Skia("base paint shader".into()))?;
            ([0.0; 4], base.with_local_matrix(&ltd), 1.0)
        }
    };
    let bounds = geometry.compute_tight_bounds();
    let last = fill.marks.len() - 1;
    for (i, mark) in fill.marks.iter().enumerate() {
        let effect = artscript_brush::effect(&mark.stage.program).map_err(|e| internal(origin, e))?;
        let mut u = Uniforms::new(&effect);
        u.set(origin, "u_dtl", &mat9(&dtl))?;
        u.set(origin, "u_paint", &u_paint)?;
        u.set(origin, "u_base", &[from_prev])?;
        u.set(origin, "u_last", &[if i == last { 1.0 } else { 0.0 }])?;
        let seeds = Seeds::from(mark.stage.seed);
        u.set(origin, "u_seed", &[seeds.seed])?;
        u.set(origin, "u_seedo", &seeds.offset)?;
        u.set(origin, "u_px", &[px])?;
        u.set(origin, "u_sdfo", &[field_rect.left as f32, field_rect.top as f32])?;
        u.set(origin, "u_frame", &frame(bounds, mark))?;
        u.set(origin, "u_dir", &[mark.dir_deg, mark.cos_sin[0], mark.cos_sin[1]])?;
        for (name, value) in &mark.stage.uniforms {
            u.set(origin, name, value.floats())?;
        }
        let children: Vec<ChildPtr> = effect
            .children()
            .iter()
            .map(|child| match child.name() {
                "u_prev" => Ok(prev.clone().into()),
                "u_sdf" => Ok(sdf.clone().into()),
                other => Err(internal(origin, format!("program declares an unknown child `{other}`"))),
            })
            .collect::<Result<_, _>>()?;
        prev = effect
            .make_shader(Data::new_copy(&u.bytes), &children, None)
            .ok_or_else(|| internal(origin, "stage shader instance".to_string()))?;
        from_prev = 1.0;
    }
    let mut paint = Paint::default();
    // The program dithers and snaps its own output, so Skia's dither must stay off.
    paint.set_dither(false);
    paint.set_shader(prev);
    canvas.save();
    canvas.reset_matrix();
    canvas.draw_irect(dev, &paint);
    canvas.restore();
    Ok(())
}

/// `u_frame`: the geometry's center and the size of its bounds rotated into the brush direction,
/// so `m.s`, `m.u`, `m.len` and `m.width` run along and across `dir`.
fn frame(bounds: skia_safe::Rect, mark: &AreaMark) -> [f32; 4] {
    let centered = bounds.with_offset((-bounds.center_x(), -bounds.center_y()));
    let rotated = Matrix::rotate_deg(-mark.dir_deg).map_rect(centered).0;
    [bounds.center_x(), bounds.center_y(), rotated.width(), rotated.height()]
}

fn internal(origin: &str, msg: impl Into<String>) -> RenderError {
    RenderError::AtNode { origin: origin.to_string(), message: msg.into() }
}

fn nearest() -> SamplingOptions {
    SamplingOptions::new(FilterMode::Nearest, MipmapMode::None)
}

/// Local units per device pixel, the geometric mean over both axes.
fn device_scale(m: &Matrix) -> f32 {
    let sx = (m.scale_x() * m.scale_x() + m.skew_y() * m.skew_y()).sqrt();
    let sy = (m.scale_y() * m.scale_y() + m.skew_x() * m.skew_x()).sqrt();
    (sx * sy).sqrt().max(1e-4)
}

/// Column-major 3x3, as SkSL lays out a `float3x3` uniform.
fn mat9(m: &Matrix) -> [f32; 9] {
    [
        m.scale_x(), m.skew_y(), m.persp_x(),
        m.skew_x(), m.scale_y(), m.persp_y(),
        m.translate_x(), m.translate_y(), m[8],
    ]
}

/// The uniform block of one effect, written by name. A name the program does not declare, or a
/// value of the wrong size, is an internal error: eval checked this same assembly.
struct Uniforms<'a> {
    effect: &'a RuntimeEffect,
    bytes: Vec<u8>,
}

impl<'a> Uniforms<'a> {
    fn new(effect: &'a RuntimeEffect) -> Self {
        Uniforms { effect, bytes: vec![0; effect.uniform_size()] }
    }

    fn set(&mut self, origin: &str, name: &str, v: &[f32]) -> Result<(), RenderError> {
        let u = self
            .effect
            .find_uniform(name)
            .ok_or_else(|| internal(origin, format!("program has no uniform `{name}`")))?;
        if u.size_in_bytes() != v.len() * 4 {
            return Err(internal(origin, format!("uniform `{name}` takes {} bytes, got {}", u.size_in_bytes(), v.len() * 4)));
        }
        let off = u.offset();
        for (i, x) in v.iter().enumerate() {
            self.bytes[off + i * 4..off + i * 4 + 4].copy_from_slice(&x.to_ne_bytes());
        }
        Ok(())
    }
}
