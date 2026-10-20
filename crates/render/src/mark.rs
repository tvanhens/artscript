//! Draws brush marks: a slot's stages as one chain of runtime effects over a CPU field. A fill
//! runs over the shape's signed distance field, a stroke over each contour's `(s, d)` ribbon
//! field. Stage 0 reads the base paint, each later stage reads the stage before it as `u_prev`,
//! and the last stage is drawn over the node's device bounds plus its reach. A chain is a
//! shader, or a blender when it is collected for [`initialize`]: then every stage runs under
//! the brush crate's blender frame with the backdrop as `dst`, the paint shader is `ENCODE`, and
//! the chain returns the slot's own output. Every program here was checked by eval, so a
//! compile or binding failure is an internal error and names the node it came from.

use std::collections::HashMap;

use ramose_brush::Seeds;
use ramose_ir::{AreaMark, Fill, Paint as IrPaint, Path, Ribbon, Stage};
use skia_safe::{
    BlendMode, Blender, Canvas, ClipOp, ColorMatrix, Data, FilterMode, IRect, Matrix, MipmapMode, Paint, RoundOut,
    RuntimeEffect, SamplingOptions, Shader, TileMode, color_filters, runtime_effect::ChildPtr, shaders,
};

use crate::RenderError;
use crate::draw::DrawCtx;
use crate::paint::{content_paint, dither_origin, mat9};
use crate::ribbon::{STRIP_ROWS, Spec, Spine};

/// What a chain's alpha means. `Coverage` is the product of the stage coverages alone, as if the
/// base paint were opaque.
#[derive(Clone, Copy)]
pub(crate) enum Ink {
    Paint,
    Coverage,
}

/// Where a slot's chain lands.
pub(crate) enum Mode<'a> {
    /// Drawn over what is there, as a shader that reads no backdrop. A paint stage that reads
    /// `m.below` is an internal error here: eval keeps such a stage off `fx outline`, and
    /// [`crate::draw`] collects it instead.
    Over,
    /// Drawn under what is there (`DstOver`): a plain slot put back beneath the own output of
    /// a stroke that read it.
    Under,
    /// Collected for [`initialize`]: a blender per draw rect that returns the slot's own
    /// premultiplied output computed over the backdrop in `dst`.
    Collect(&'a mut Vec<Draw>),
}

pub(crate) struct Draw {
    pub rect: IRect,
    pub blender: Blender,
}

enum Chain {
    Shader(Shader),
    Blender(Blender),
}

impl Chain {
    fn child(&self) -> ChildPtr {
        match self {
            Chain::Shader(s) => s.clone().into(),
            Chain::Blender(b) => b.clone().into(),
        }
    }
}

pub(crate) fn draw_area(
    canvas: &Canvas,
    geometry: &Path,
    fill: &Fill,
    ctx: &DrawCtx<'_>,
    origin: &str,
    ink: Ink,
    mode: &mut Mode<'_>,
) -> Result<(), RenderError> {
    let Some(frame) = Frame::of(canvas, origin) else { return Ok(()) };
    if let Some((dev, chain)) = area_chain(&frame, geometry, fill, ctx, ink, mode)? {
        frame.land(canvas, dev, chain, mode);
    }
    Ok(())
}

/// Runs `children` clipped to the coverage the fill's brush leaves. The clip is a clip shader,
/// not a layer, so the children still blend with the fill and with what is below it. Coverage
/// is a shader, so a stage that reads `m.below` sees a transparent backdrop here.
pub(crate) fn with_coverage(
    canvas: &Canvas,
    geometry: &Path,
    fill: &Fill,
    ctx: &DrawCtx<'_>,
    origin: &str,
    children: impl FnOnce() -> Result<(), RenderError>,
) -> Result<(), RenderError> {
    let Some(frame) = Frame::of(canvas, origin) else { return Ok(()) };
    let Some((dev, Chain::Shader(shader))) = area_chain(&frame, geometry, fill, ctx, Ink::Coverage, &Mode::Over)? else {
        return Ok(());
    };
    let ltd = canvas.local_to_device();
    canvas.save();
    canvas.reset_matrix();
    canvas.clip_irect(dev, ClipOp::Intersect);
    canvas.clip_shader(shader, ClipOp::Intersect);
    canvas.set_matrix(&ltd);
    let drawn = children();
    canvas.restore();
    drawn
}

/// The fill's chain and the device rect it covers, or `None` when that rect is clipped away.
fn area_chain(
    frame: &Frame<'_>,
    geometry: &Path,
    fill: &Fill,
    ctx: &DrawCtx<'_>,
    ink: Ink,
    mode: &Mode<'_>,
) -> Result<Option<(IRect, Chain)>, RenderError> {
    let reach = fill.reach();
    let local = geometry.compute_tight_bounds().with_outset((reach, reach));
    let rounded: IRect = frame.ltd.map_rect(local).0.round_out();
    // The field keeps a 2 px margin past the drawn area, so `edge` at its border is measured, not clamped.
    let field_rect = rounded.with_outset((2, 2));
    let Some(dev) = IRect::intersect(&frame.clip, &rounded) else { return Ok(None) };
    if dev.is_empty() {
        return Ok(None);
    }
    let sdf = ctx
        .sdf
        .borrow_mut()
        .get(geometry, &frame.ltd, field_rect)?
        .to_raw_shader((TileMode::Clamp, TileMode::Clamp), nearest(), None)
        .ok_or_else(|| RenderError::Skia("sdf shader".into()))?;
    let bounds = geometry.compute_tight_bounds();
    let stages: Vec<(&Stage, &AreaMark)> = fill.marks.iter().map(|m| (&m.stage, m)).collect();
    let origin = frame.origin;
    let chain = frame.chain(
        &stages,
        &fill.paint,
        ink,
        mode,
        &|u, mark| {
            u.set(origin, "u_sdfo", &[field_rect.left as f32, field_rect.top as f32])?;
            u.set(origin, "u_frame", &area_frame(bounds, mark))?;
            u.set(origin, "u_dir", &[mark.dir_deg, mark.cos_sin[0], mark.cos_sin[1]])
        },
        &|name| (name == "u_sdf").then(|| sdf.clone()),
    )?;
    Ok(Some((dev, chain)))
}

/// Each contour of the geometry is its own ribbon over `paint`, built and drawn in strips of
/// [`STRIP_ROWS`] device rows narrowed to the cells its spine reaches, so a long diagonal stroke
/// costs its own pixels and not its bounding box.
pub(crate) fn draw_ribbon(
    canvas: &Canvas,
    geometry: &Path,
    paint: &IrPaint,
    ribbon: &Ribbon,
    ctx: &DrawCtx<'_>,
    origin: &str,
    mode: &mut Mode<'_>,
) -> Result<(), RenderError> {
    let Some(frame) = Frame::of(canvas, origin) else { return Ok(()) };
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
            let field = ctx.ribbons.borrow_mut().get(&spine, rect, &spec, origin)?;
            let texels = |img: &skia_safe::Image| {
                img.to_raw_shader((TileMode::Clamp, TileMode::Clamp), nearest(), None)
                    .ok_or_else(|| RenderError::Skia("ribbon field shader".into()))
            };
            let (field_shader, chan_shader, norm_shader) = (texels(&field.field)?, texels(&field.chan)?, texels(&field.norm)?);
            let chain = frame.chain(
                &stages,
                paint,
                Ink::Paint,
                mode,
                &|u, _| {
                    u.set(origin, "u_fieldo", &[rect.left as f32, rect.top as f32])?;
                    u.set(origin, "u_rib", &[contour.len, if contour.closed { 1.0 } else { 0.0 }])?;
                    u.set(origin, "u_s0", &[field.s0])
                },
                &|name| match name {
                    "u_field" => Some(field_shader.clone()),
                    "u_chan" => Some(chan_shader.clone()),
                    "u_norm" => Some(norm_shader.clone()),
                    _ => None,
                },
            )?;
            frame.land(canvas, rect, chain, mode);
        }
    }
    Ok(())
}

/// Puts the own output of `draws` into the current layer, which holds a copy of the backdrop
/// (`INIT_WITH_PREVIOUS`). Each pixel is drawn once, by the fold of every draw whose rect holds
/// it, in draw order, so a later draw sees the earlier ones over the backdrop and the result is
/// their composite alone. The rest of the copy is cleared. A clip of an ancestor does not reach
/// into a layer, so no part of the copy survives at an antialiased edge.
pub(crate) fn initialize(canvas: &Canvas, draws: &[Draw], origin: &str) -> Result<(), RenderError> {
    canvas.save();
    canvas.reset_matrix();
    let mut xs: Vec<i32> = draws.iter().flat_map(|d| [d.rect.left, d.rect.right]).collect();
    let mut ys: Vec<i32> = draws.iter().flat_map(|d| [d.rect.top, d.rect.bottom]).collect();
    xs.sort_unstable();
    xs.dedup();
    ys.sort_unstable();
    ys.dedup();
    let covering = |cell: IRect| -> Vec<usize> {
        draws
            .iter()
            .enumerate()
            .filter(|(_, d)| d.rect.left <= cell.left && cell.right <= d.rect.right && d.rect.top <= cell.top && cell.bottom <= d.rect.bottom)
            .map(|(i, _)| i)
            .collect()
    };
    let mut folds: HashMap<Vec<usize>, Blender> = HashMap::new();
    let mut paint = Paint::default();
    paint.set_shader(encoder());
    for row in ys.windows(2) {
        let mut col = 0;
        while col + 1 < xs.len() {
            let over = covering(IRect::from_ltrb(xs[col], row[0], xs[col + 1], row[1]));
            let mut end = col + 1;
            while end + 1 < xs.len() && covering(IRect::from_ltrb(xs[end], row[0], xs[end + 1], row[1])) == over {
                end += 1;
            }
            if !over.is_empty() {
                paint.set_blender(fold(draws, &over, &mut folds, origin)?);
                canvas.draw_irect(IRect::from_ltrb(xs[col], row[0], xs[end], row[1]), &paint);
            }
            col = end;
        }
    }
    canvas.save();
    for d in draws {
        canvas.clip_irect(d.rect, ClipOp::Difference);
    }
    let mut clear = Paint::default();
    clear.set_blend_mode(BlendMode::Clear);
    canvas.draw_paint(&clear);
    canvas.restore();
    canvas.restore();
    Ok(())
}

/// The blender of the draws `over` in order: each later one sees the earlier ones over the
/// backdrop and lands over their output.
fn fold(draws: &[Draw], over: &[usize], memo: &mut HashMap<Vec<usize>, Blender>, origin: &str) -> Result<Blender, RenderError> {
    if let Some(b) = memo.get(over) {
        return Ok(b.clone());
    }
    let (last, under) = over.split_last().expect("a covered cell has a draw");
    let top = draws[*last].blender.clone();
    let blender = if under.is_empty() {
        top
    } else {
        let under = fold(draws, under, memo, origin)?;
        COMBINE
            .with(|e| e.make_blender(Data::new_empty(), &[ChildPtr::from(under), ChildPtr::from(top)][..]))
            .ok_or_else(|| internal(origin, "combined blender instance"))?
    };
    memo.insert(over.to_vec(), blender.clone());
    Ok(blender)
}

thread_local! {
    /// The paint shader of every blender draw. See `ramose_brush::ENCODE`.
    static ENCODER: Shader = RuntimeEffect::make_for_shader(ramose_brush::ENCODE, None)
        .expect("encoder compiles")
        .make_shader(Data::new_empty(), &[], None)
        .expect("encoder shader");
    /// Stage 0's `u_prev` over a non-solid base paint, read at the decoded device pixel.
    static ADAPTER: RuntimeEffect = RuntimeEffect::make_for_blender(
        format!("{}uniform shader p;\nhalf4 main(half4 src, half4 dst) {{ return p.eval(u_decode(src)); }}\n", ramose_brush::DECODE),
        None,
    )
    .expect("base adapter compiles");
    /// `u_over` drawn over `u_under`, both own outputs over the same backdrop `dst`. What
    /// `u_over` composites with is written to 8-bit levels first, as the canvas would have.
    static COMBINE: RuntimeEffect = RuntimeEffect::make_for_blender(
        "uniform blender u_under; uniform blender u_over;
half4 main(half4 src, half4 dst) {
    float4 u = float4(u_under.eval(src, dst));
    float4 d = floor((u + float4(dst) * (1 - u.a)) * 255 + 0.5) / 255;
    float4 o = float4(u_over.eval(src, half4(d)));
    return half4(o + u * (1 - o.a));
}
",
        None,
    )
    .expect("combine compiles");
}

fn encoder() -> Shader {
    ENCODER.with(Shader::clone)
}

/// The canvas state a mark is drawn in: its device matrix, its clip, and local units per pixel,
/// and the node an internal error names.
struct Frame<'a> {
    ltd: Matrix,
    dtl: Matrix,
    px: f32,
    clip: IRect,
    origin: &'a str,
}

impl<'a> Frame<'a> {
    fn of(canvas: &Canvas, origin: &'a str) -> Option<Frame<'a>> {
        let ltd = canvas.local_to_device_as_3x3();
        let dtl = ltd.invert()?;
        let clip = canvas.device_clip_bounds()?;
        Some(Frame { ltd, dtl, px: 1.0 / device_scale(&ltd), clip, origin })
    }

    /// The chain of `stages` over the base `paint`. `extra` sets the uniforms a kind owns, and
    /// `child` names the field shaders it binds; every other field child is transparent.
    fn chain<M>(
        &self,
        stages: &[(&Stage, &M)],
        paint: &IrPaint,
        ink: Ink,
        mode: &Mode<'_>,
        extra: &dyn Fn(&mut Uniforms, &M) -> Result<(), RenderError>,
        child: &dyn Fn(&str) -> Option<Shader>,
    ) -> Result<Chain, RenderError> {
        let origin = self.origin;
        let blend = match mode {
            Mode::Collect(_) => true,
            Mode::Over | Mode::Under => {
                if matches!(ink, Ink::Paint) && stages.iter().any(|(s, _)| s.program.reads_below()) {
                    return Err(internal(origin, "a brush stage reads `m.below` where the node has no backdrop"));
                }
                false
            }
        };
        // A solid paint is bound exactly as a uniform. Any other paint is a child shader placed by
        // the device matrix, so stage 0 reads it at each device pixel.
        let (u_paint, mut prev, mut from_prev) = match paint {
            IrPaint::Solid(c) => {
                let a = if matches!(ink, Ink::Coverage) { 1.0 } else { c.a };
                let unread = if blend {
                    Chain::Blender(Blender::mode(BlendMode::SrcOver))
                } else {
                    Chain::Shader(shaders::color(skia_safe::Color::TRANSPARENT))
                };
                ([c.r, c.g, c.b, a], unread, 0.0_f32)
            }
            other => {
                let mut base = content_paint(other, 0.0, None)?
                    .shader()
                    .ok_or_else(|| RenderError::Skia("base paint shader".into()))?;
                if matches!(ink, Ink::Coverage) {
                    base = base.with_color_filter(opaque());
                }
                let base = base.with_local_matrix(&self.ltd);
                let read = if blend {
                    let adapted = ADAPTER
                        .with(|e| e.make_blender(Data::new_empty(), &[ChildPtr::from(base)][..]))
                        .ok_or_else(|| internal(origin, "base paint adapter instance"))?;
                    Chain::Blender(adapted)
                } else {
                    Chain::Shader(base)
                };
                ([0.0; 4], read, 1.0)
            }
        };
        let last = stages.len() - 1;
        for (i, (stage, mark)) in stages.iter().enumerate() {
            let effect = if blend { ramose_brush::blender(&stage.program) } else { ramose_brush::effect(&stage.program) };
            let effect = effect.map_err(|e| internal(origin, e))?;
            let mut u = Uniforms::new(&effect);
            u.set(origin, "u_dtl", &mat9(&self.dtl))?;
            u.set(origin, "u_paint", &u_paint)?;
            u.set(origin, "u_base", &[from_prev])?;
            u.set(origin, "u_last", &[if i == last { 1.0 } else { 0.0 }])?;
            let seeds = Seeds::from(stage.seed);
            u.set(origin, "u_seed", &[seeds.seed])?;
            u.set(origin, "u_seedo", &seeds.offset)?;
            u.set(origin, "u_px", &[self.px])?;
            u.set(origin, "u_dito", &dither_origin(&self.ltd))?;
            extra(&mut u, mark)?;
            for (name, value) in &stage.uniforms {
                u.set(origin, name, value.floats())?;
            }
            let children: Vec<ChildPtr> = effect
                .children()
                .iter()
                .map(|c| match c.name() {
                    "u_prev" => Ok(prev.child()),
                    name @ ("u_sdf" | "u_field" | "u_chan" | "u_norm") => {
                        Ok(child(name).unwrap_or_else(|| shaders::color(skia_safe::Color::TRANSPARENT)).into())
                    }
                    other => Err(internal(origin, format!("program declares an unknown child `{other}`"))),
                })
                .collect::<Result<_, _>>()?;
            let data = Data::new_copy(&u.bytes);
            prev = if blend {
                Chain::Blender(effect.make_blender(data, &children[..]).ok_or_else(|| internal(origin, "stage blender instance".to_string()))?)
            } else {
                Chain::Shader(effect.make_shader(data, &children, None).ok_or_else(|| internal(origin, "stage shader instance".to_string()))?)
            };
            from_prev = 1.0;
        }
        Ok(prev)
    }

    /// Draws a shader chain over `dev`, or collects a blender chain for [`initialize`].
    fn land(&self, canvas: &Canvas, dev: IRect, chain: Chain, mode: &mut Mode<'_>) {
        match (chain, mode) {
            (Chain::Shader(shader), Mode::Over) => self.draw(canvas, dev, shader, BlendMode::SrcOver),
            (Chain::Shader(shader), Mode::Under) => self.draw(canvas, dev, shader, BlendMode::DstOver),
            (Chain::Blender(blender), Mode::Collect(draws)) => draws.push(Draw { rect: dev, blender }),
            _ => unreachable!("`chain` builds the kind its mode lands"),
        }
    }

    fn draw(&self, canvas: &Canvas, dev: IRect, shader: Shader, blend: BlendMode) {
        let mut paint = Paint::default();
        paint.set_shader(shader);
        if blend != BlendMode::SrcOver {
            paint.set_blend_mode(blend);
        }
        canvas.save();
        canvas.reset_matrix();
        canvas.draw_irect(dev, &paint);
        canvas.restore();
    }
}

fn opaque() -> skia_safe::ColorFilter {
    let matrix = ColorMatrix::new(
        1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    );
    color_filters::matrix(&matrix, None)
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

/// Device pixels per local unit, the geometric mean over both axes.
pub(crate) fn device_scale(m: &Matrix) -> f32 {
    let sx = (m.scale_x() * m.scale_x() + m.skew_y() * m.skew_y()).sqrt();
    let sy = (m.scale_y() * m.scale_y() + m.skew_x() * m.skew_x()).sqrt();
    (sx * sy).sqrt().max(1e-4)
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
