//! Draws brush marks: a slot's stages as one chain of runtime shaders over a CPU field. A fill
//! runs over the shape's signed distance field, a stroke over each contour's `(s, d)` ribbon
//! field. Stage 0 reads the base paint, each later stage reads the stage before it as `u_prev`,
//! and the last stage is drawn over the node's device bounds plus its reach. Every program here
//! was checked by eval, so a compile or binding failure is an internal error and names the node
//! it came from.

use artscript_brush::Seeds;
use artscript_ir::{AreaMark, Fill, Paint as IrPaint, Path, Ribbon, Stage, Stroke};
use skia_safe::{Canvas, Data, FilterMode, IRect, Matrix, MipmapMode, Paint, RoundOut, RuntimeEffect, SamplingOptions, Shader, TileMode, runtime_effect::ChildPtr, shaders};

use crate::RenderError;
use crate::draw::DrawCtx;
use crate::paint::content_paint;
use crate::ribbon::{STRIP_ROWS, Spec, Spine};

pub(crate) fn draw_area(
    canvas: &Canvas,
    geometry: &Path,
    fill: &Fill,
    ctx: &DrawCtx<'_>,
    origin: &str,
) -> Result<(), RenderError> {
    let Some(frame) = Frame::of(canvas) else { return Ok(()) };
    let reach = fill.reach();
    let local = geometry.compute_tight_bounds().with_outset((reach, reach));
    let rounded: IRect = frame.ltd.map_rect(local).0.round_out();
    // The field keeps a 2 px margin past the drawn area, so `edge` at its border is measured, not clamped.
    let field_rect = rounded.with_outset((2, 2));
    let Some(dev) = IRect::intersect(&frame.clip, &rounded) else { return Ok(()) };
    if dev.is_empty() {
        return Ok(());
    }
    let sdf = ctx
        .sdf
        .borrow_mut()
        .get(geometry, &frame.ltd, field_rect)?
        .to_raw_shader((TileMode::Clamp, TileMode::Clamp), nearest(), None)
        .ok_or_else(|| RenderError::Skia("sdf shader".into()))?;
    let bounds = geometry.compute_tight_bounds();
    let stages: Vec<(&Stage, &AreaMark)> = fill.marks.iter().map(|m| (&m.stage, m)).collect();
    let shader = frame.chain(
        &stages,
        &fill.paint,
        origin,
        &|u, mark| {
            u.set(origin, "u_sdfo", &[field_rect.left as f32, field_rect.top as f32])?;
            u.set(origin, "u_frame", &area_frame(bounds, mark))?;
            u.set(origin, "u_dir", &[mark.dir_deg, mark.cos_sin[0], mark.cos_sin[1]])
        },
        &|name| (name == "u_sdf").then(|| sdf.clone()),
    )?;
    frame.draw(canvas, dev, shader);
    Ok(())
}

/// Each contour of the geometry is its own ribbon, built and drawn in strips of [`STRIP_ROWS`]
/// device rows narrowed to the cells its spine reaches, so a long diagonal stroke costs its own
/// pixels and not its bounding box.
pub(crate) fn draw_ribbon(
    canvas: &Canvas,
    geometry: &Path,
    stroke: &Stroke,
    ribbon: &Ribbon,
    ctx: &DrawCtx<'_>,
    origin: &str,
) -> Result<(), RenderError> {
    let Some(frame) = Frame::of(canvas) else { return Ok(()) };
    let stages: Vec<(&Stage, &())> = ribbon.marks.iter().map(|m| (&m.stage, &())).collect();
    for (index, contour) in ribbon.contours.iter().enumerate() {
        let spec = Spec { index, contour, taper: ribbon.taper, reach: ribbon.reach() };
        let pad = spec.max_half_width() + spec.reach + 2.0 * frame.px;
        let local = geometry.compute_tight_bounds().with_outset((pad, pad));
        let rounded: IRect = frame.ltd.map_rect(local).0.round_out();
        let Some(dev) = IRect::intersect(&frame.clip, &rounded) else { continue };
        if dev.is_empty() {
            continue;
        }
        let Some(spine) = Spine::new(geometry, &frame.ltd, frame.px, &spec, dev) else { continue };
        let mut top = dev.top;
        while top < dev.bottom {
            let strip = IRect::from_ltrb(dev.left, top, dev.right, (top + STRIP_ROWS).min(dev.bottom));
            top += STRIP_ROWS;
            let Some(rect) = spine.live_rect(strip) else { continue };
            let field = ctx.ribbons.borrow_mut().get(&spine, rect, &spec)?;
            let texels = |img: &skia_safe::Image| {
                img.to_raw_shader((TileMode::Clamp, TileMode::Clamp), nearest(), None)
                    .ok_or_else(|| RenderError::Skia("ribbon field shader".into()))
            };
            let (field_shader, chan_shader) = (texels(&field.field)?, texels(&field.chan)?);
            let shader = frame.chain(
                &stages,
                &stroke.paint,
                origin,
                &|u, _| {
                    u.set(origin, "u_fieldo", &[rect.left as f32, rect.top as f32])?;
                    u.set(origin, "u_rib", &[contour.len, if contour.closed { 1.0 } else { 0.0 }])
                },
                &|name| match name {
                    "u_field" => Some(field_shader.clone()),
                    "u_chan" => Some(chan_shader.clone()),
                    _ => None,
                },
            )?;
            frame.draw(canvas, rect, shader);
        }
    }
    Ok(())
}

/// The canvas state a mark is drawn in: its device matrix, its clip, and local units per pixel.
struct Frame {
    ltd: Matrix,
    dtl: Matrix,
    px: f32,
    clip: IRect,
}

impl Frame {
    fn of(canvas: &Canvas) -> Option<Frame> {
        let ltd = canvas.local_to_device_as_3x3();
        let dtl = ltd.invert()?;
        let clip = canvas.device_clip_bounds()?;
        Some(Frame { ltd, dtl, px: 1.0 / device_scale(&ltd), clip })
    }

    /// The chain of `stages` over the base `paint`. `extra` sets the uniforms a kind owns, and
    /// `child` names the field shaders it binds; every other field child is transparent.
    fn chain<M>(
        &self,
        stages: &[(&Stage, &M)],
        paint: &IrPaint,
        origin: &str,
        extra: &dyn Fn(&mut Uniforms, &M) -> Result<(), RenderError>,
        child: &dyn Fn(&str) -> Option<Shader>,
    ) -> Result<Shader, RenderError> {
        // A solid paint is bound exactly as a uniform. Any other paint is a child shader placed by
        // the device matrix, so stage 0 reads it at each device pixel.
        let (u_paint, mut prev, mut from_prev) = match paint {
            IrPaint::Solid(c) => ([c.r, c.g, c.b, c.a], shaders::color(skia_safe::Color::TRANSPARENT), 0.0_f32),
            other => {
                let base = content_paint(other, 0.0, None)?
                    .shader()
                    .ok_or_else(|| RenderError::Skia("base paint shader".into()))?;
                ([0.0; 4], base.with_local_matrix(&self.ltd), 1.0)
            }
        };
        let last = stages.len() - 1;
        for (i, (stage, mark)) in stages.iter().enumerate() {
            let effect = artscript_brush::effect(&stage.program).map_err(|e| internal(origin, e))?;
            let mut u = Uniforms::new(&effect);
            u.set(origin, "u_dtl", &mat9(&self.dtl))?;
            u.set(origin, "u_paint", &u_paint)?;
            u.set(origin, "u_base", &[from_prev])?;
            u.set(origin, "u_last", &[if i == last { 1.0 } else { 0.0 }])?;
            let seeds = Seeds::from(stage.seed);
            u.set(origin, "u_seed", &[seeds.seed])?;
            u.set(origin, "u_seedo", &seeds.offset)?;
            u.set(origin, "u_px", &[self.px])?;
            extra(&mut u, mark)?;
            for (name, value) in &stage.uniforms {
                u.set(origin, name, value.floats())?;
            }
            let children: Vec<ChildPtr> = effect
                .children()
                .iter()
                .map(|c| match c.name() {
                    "u_prev" => Ok(prev.clone().into()),
                    name @ ("u_sdf" | "u_field" | "u_chan") => {
                        Ok(child(name).unwrap_or_else(|| shaders::color(skia_safe::Color::TRANSPARENT)).into())
                    }
                    other => Err(internal(origin, format!("program declares an unknown child `{other}`"))),
                })
                .collect::<Result<_, _>>()?;
            prev = effect
                .make_shader(Data::new_copy(&u.bytes), &children, None)
                .ok_or_else(|| internal(origin, "stage shader instance".to_string()))?;
            from_prev = 1.0;
        }
        Ok(prev)
    }

    fn draw(&self, canvas: &Canvas, dev: IRect, shader: Shader) {
        let mut paint = Paint::default();
        // The program dithers and snaps its own output, so Skia's dither must stay off.
        paint.set_dither(false);
        paint.set_shader(shader);
        canvas.save();
        canvas.reset_matrix();
        canvas.draw_irect(dev, &paint);
        canvas.restore();
    }
}

/// `u_frame`: the geometry's center and the size of its bounds rotated into the brush direction,
/// so `m.s`, `m.u`, `m.len` and `m.width` run along and across `dir`.
fn area_frame(bounds: skia_safe::Rect, mark: &AreaMark) -> [f32; 4] {
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
