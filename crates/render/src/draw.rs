//! Canvas walk. A node that needs isolation, a mask, a soft clip, an effect, or a non-default
//! composite is drawn into a layer. Effects see the clipped and masked content.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use artscript_ir::{Blend, Clip, Mask, MaskSource, Matrix, Node, NodeKind, Scene, Shape};
use skia_safe::canvas::SaveLayerRec;
use skia_safe::{
    BlendMode, Canvas, ClipOp, ColorFilter, ColorMatrix, EncodedImageFormat, Image, ImageFilter,
    Paint, Rect, TileMode, image_filters,
};

use crate::gpu::{self, Backend};
use crate::RenderError;
use crate::effects::{self, FxEnv, white_paint};
use crate::field::{FieldUse, field_shader};
use crate::paint::{blend_mode, color4, content_paint};

pub struct SceneIndex<'a> {
    nodes: HashMap<&'a str, &'a Node>,
    parents: HashMap<&'a str, Matrix>,
}

impl<'a> SceneIndex<'a> {
    pub fn build(scene: &'a Scene) -> Self {
        let mut index = Self {
            nodes: HashMap::new(),
            parents: HashMap::new(),
        };
        index.walk(&scene.children, Matrix::new_identity());
        index
    }

    fn walk(&mut self, nodes: &'a [Node], parent_world: Matrix) {
        for node in nodes {
            self.nodes.insert(node.path.as_str(), node);
            self.parents.insert(node.path.as_str(), parent_world);
            let world = parent_world * node.transform;
            if let Some(mask) = &node.mask
                && let MaskSource::Nodes(inner) = &mask.source
            {
                self.walk(inner, world);
            }
            self.walk(node.children(), world);
        }
    }

    pub fn node(&self, path: &str) -> Option<&'a Node> {
        self.nodes.get(path).copied()
    }

    pub fn parent_world(&self, path: &str) -> Matrix {
        self.parents
            .get(path)
            .copied()
            .unwrap_or_else(Matrix::new_identity)
    }
}

pub struct DrawCtx<'a> {
    pub index: SceneIndex<'a>,
    pub backend: Backend,
    visiting: RefCell<HashSet<String>>,
}

impl<'a> DrawCtx<'a> {
    pub fn new(scene: &'a Scene, backend: Backend) -> Self {
        Self {
            index: SceneIndex::build(scene),
            backend,
            visiting: RefCell::new(HashSet::new()),
        }
    }
}

struct Visit<'a> {
    set: &'a RefCell<HashSet<String>>,
    path: String,
}

impl Drop for Visit<'_> {
    fn drop(&mut self) {
        self.set.borrow_mut().remove(&self.path);
    }
}

pub fn render_png(scene: &Scene, scale: f32, backend: Backend) -> Result<Vec<u8>, RenderError> {
    if !(scale.is_finite() && scale > 0.0) {
        return Err(RenderError::Skia("scale must be positive".into()));
    }
    let w = (scene.width * scale).ceil().max(1.0) as i32;
    let h = (scene.height * scale).ceil().max(1.0) as i32;
    let mut surface =
        gpu::surface(backend, (w, h)).ok_or_else(|| RenderError::Skia("surface".into()))?;
    let canvas = surface.canvas();
    canvas.scale((scale, scale));
    if let Some(bg) = scene.background {
        canvas.clear(color4(bg));
    }
    let ctx = DrawCtx::new(scene, backend);
    for child in &scene.children {
        draw_node(canvas, child, &ctx, Matrix::new_identity())?;
    }
    let image = gpu::finish(backend, &mut surface)?;
    png_bytes(&image)
}

pub fn render_rgba(scene: &Scene, scale: f32, backend: Backend) -> Result<(u32, u32, Vec<u8>), RenderError> {
    if !(scale.is_finite() && scale > 0.0) {
        return Err(RenderError::Skia("scale must be positive".into()));
    }
    let w = (scene.width * scale).ceil().max(1.0) as i32;
    let h = (scene.height * scale).ceil().max(1.0) as i32;
    let mut surface =
        gpu::surface(backend, (w, h)).ok_or_else(|| RenderError::Skia("surface".into()))?;
    let canvas = surface.canvas();
    canvas.scale((scale, scale));
    if let Some(bg) = scene.background {
        canvas.clear(color4(bg));
    }
    let ctx = DrawCtx::new(scene, backend);
    for child in &scene.children {
        draw_node(canvas, child, &ctx, Matrix::new_identity())?;
    }
    let image = gpu::finish(backend, &mut surface)?;
    let info = skia_safe::ImageInfo::new(
        (w, h),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Unpremul,
        None,
    );
    let row = w as usize * 4;
    let mut buf = vec![0u8; row * h as usize];
    if !image.read_pixels(
        &info,
        &mut buf,
        row,
        (0, 0),
        skia_safe::image::CachingHint::Disallow,
    ) {
        return Err(RenderError::Skia("read pixels".into()));
    }
    Ok((w as u32, h as u32, buf))
}

pub struct Baked {
    pub png: Vec<u8>,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Renders `node` on transparent, in its parent's coordinates, including its transform.
/// Blend is left to the SVG element so the tile is the node's own pixels.
pub fn bake_node(
    scene: &Scene,
    node: &Node,
    bake_scale: f32,
    backend: Backend,
) -> Result<Baked, RenderError> {
    if !(bake_scale.is_finite() && bake_scale > 0.0) {
        return Err(RenderError::Skia("bake scale must be positive".into()));
    }
    let ctx = DrawCtx::new(scene, backend);
    let mut bounds = node
        .bounds()
        .unwrap_or_else(|| Rect::from_xywh(0.0, 0.0, 1.0, 1.0));
    let pad = visual_pad(node) * transform_scale(&node.transform);
    bounds.outset((pad, pad));
    if bounds.width() < 1.0 {
        bounds.right = bounds.left + 1.0;
    }
    if bounds.height() < 1.0 {
        bounds.bottom = bounds.top + 1.0;
    }
    let pw = (bounds.width() * bake_scale).ceil().max(1.0) as i32;
    let ph = (bounds.height() * bake_scale).ceil().max(1.0) as i32;
    let mut surface = gpu::surface(backend, (pw, ph))
        .ok_or_else(|| RenderError::Skia("bake surface".into()))?;
    let canvas = surface.canvas();
    canvas.scale((bake_scale, bake_scale));
    canvas.translate((-bounds.left, -bounds.top));
    let mut drawn = node.clone();
    drawn.blend = Blend::Normal;
    let parent = ctx.index.parent_world(&node.path);
    draw_node(canvas, &drawn, &ctx, parent)?;
    let image = gpu::finish(backend, &mut surface)?;
    Ok(Baked {
        png: png_bytes(&image)?,
        x: bounds.left,
        y: bounds.top,
        w: bounds.width(),
        h: bounds.height(),
    })
}

pub fn draw_node(
    canvas: &Canvas,
    node: &Node,
    ctx: &DrawCtx<'_>,
    parent_world: Matrix,
) -> Result<(), RenderError> {
    if !ctx.visiting.borrow_mut().insert(node.path.clone()) {
        return Ok(());
    }
    let _visit = Visit {
        set: &ctx.visiting,
        path: node.path.clone(),
    };

    canvas.save();
    canvas.concat(&node.transform);
    let world = parent_world * node.transform;

    if needs_layer(node) {
        if let Some(steps) = effects::layer_steps(&node.effects, &FxEnv { ctx, world })? {
            paint_layer_steps(canvas, node, ctx, world, steps)?;
        } else {
            let filter = effects::chain(&node.effects, &FxEnv { ctx, world })?;
            let mut paint = Paint::default();
            paint.set_anti_alias(true);
            paint.set_dither(true);
            paint.set_alpha_f(node.opacity.clamp(0.0, 1.0));
            paint.set_blend_mode(blend_mode(node.blend));
            if let Some(filter) = filter {
                paint.set_image_filter(filter);
            }
            let rec = SaveLayerRec::default().paint(&paint);
            canvas.save_layer(&rec);
            draw_clipped(canvas, node, ctx, world)?;
            canvas.restore();
        }
    } else {
        draw_clipped(canvas, node, ctx, world)?;
    }

    canvas.restore();
    Ok(())
}

fn paint_layer_steps(
    canvas: &Canvas,
    node: &Node,
    ctx: &DrawCtx<'_>,
    world: Matrix,
    mut steps: Vec<effects::LayerStep>,
) -> Result<(), RenderError> {
    let Some(bounds) = canvas.device_clip_bounds() else {
        return Ok(());
    };
    if bounds.is_empty() {
        return Ok(());
    }
    let mut local_to_px = canvas.local_to_device_as_3x3();
    local_to_px.post_translate((-(bounds.left as f32), -(bounds.top as f32)));
    let lead = match steps.first() {
        Some(effects::LayerStep::Filter(_)) => match steps.remove(0) {
            effects::LayerStep::Filter(filter) => Some(filter),
            _ => None,
        },
        _ => None,
    };
    let mut image = rasterize_layer(ctx.backend, bounds, &local_to_px, lead, |off| {
        draw_clipped(off, node, ctx, world)
    })?;
    for step in steps {
        image = match step {
            effects::LayerStep::Filter(filter) => {
                effects::apply_image_filter(ctx.backend, image, &local_to_px, filter)?
            }
            effects::LayerStep::Outline { width, color } => {
                effects::outline_geometry(ctx.backend, image, &local_to_px, node, width, color)?
            }
        };
    }
    canvas.save();
    canvas.reset_matrix();
    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    paint.set_dither(true);
    paint.set_alpha_f(node.opacity.clamp(0.0, 1.0));
    paint.set_blend_mode(blend_mode(node.blend));
    canvas.draw_image(
        &image,
        (bounds.left as f32, bounds.top as f32),
        Some(&paint),
    );
    canvas.restore();
    Ok(())
}

fn rasterize_layer(
    backend: Backend,
    bounds: skia_safe::IRect,
    local_to_px: &Matrix,
    filter: Option<ImageFilter>,
    draw: impl FnOnce(&Canvas) -> Result<(), RenderError>,
) -> Result<Image, RenderError> {
    let mut surface = gpu::surface(backend, (bounds.width(), bounds.height()))
        .ok_or_else(|| RenderError::Skia("layer surface".into()))?;
    let canvas = surface.canvas();
    canvas.concat(local_to_px);
    if let Some(filter) = filter {
        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        paint.set_image_filter(filter);
        canvas.save_layer(&SaveLayerRec::default().paint(&paint));
        draw(canvas)?;
        canvas.restore();
    } else {
        draw(canvas)?;
    }
    Ok(surface.image_snapshot())
}

fn needs_layer(node: &Node) -> bool {
    node.opacity < 1.0
        || node.blend != Blend::Normal
        || !node.effects.is_empty()
        || node.mask.is_some()
        || node.clip.as_ref().is_some_and(|c| c.feather > 0.0)
        || matches!(node.kind, NodeKind::Group { isolate: true, .. })
}

fn draw_clipped(
    canvas: &Canvas,
    node: &Node,
    ctx: &DrawCtx<'_>,
    world: Matrix,
) -> Result<(), RenderError> {
    let hard = node.clip.as_ref().filter(|c| c.feather <= 0.0);
    let soft = node.clip.as_ref().filter(|c| c.feather > 0.0);
    if let Some(clip) = hard {
        canvas.save();
        let op = if clip.invert {
            ClipOp::Difference
        } else {
            ClipOp::Intersect
        };
        canvas.clip_path(&clip.path, op, true);
    }
    let wrapped = soft.is_some() || node.mask.is_some();
    if wrapped {
        canvas.save_layer(&SaveLayerRec::default());
    }
    draw_kind(canvas, node, ctx, world)?;
    if let Some(clip) = soft {
        apply_clip_mask(canvas, clip)?;
    }
    if let Some(mask) = &node.mask {
        apply_mask(canvas, mask, ctx, world)?;
    }
    if wrapped {
        canvas.restore();
    }
    if hard.is_some() {
        canvas.restore();
    }
    Ok(())
}

fn draw_kind(
    canvas: &Canvas,
    node: &Node,
    ctx: &DrawCtx<'_>,
    world: Matrix,
) -> Result<(), RenderError> {
    match &node.kind {
        NodeKind::Group { children, .. } => {
            for child in children {
                draw_node(canvas, child, ctx, world)?;
            }
            Ok(())
        }
        NodeKind::Shape(shape) => draw_shape(canvas, shape, ctx, world),
    }
}

fn draw_shape(
    canvas: &Canvas,
    shape: &Shape,
    ctx: &DrawCtx<'_>,
    world: Matrix,
) -> Result<(), RenderError> {
    let paint_shape = |canvas: &Canvas| -> Result<(), RenderError> {
        if let Some(fill) = &shape.fill {
            let paint = content_paint(fill, shape.blur, None)?;
            canvas.draw_path(&shape.geometry, &paint);
        }
        if let Some(stroke) = &shape.stroke {
            let paint = content_paint(&stroke.paint, shape.blur, Some(stroke))?;
            canvas.draw_path(&shape.geometry, &paint);
        }
        Ok(())
    };

    if shape.children.is_empty() {
        return paint_shape(canvas);
    }

    // The layer keeps children's blend modes off the pixels below the shape. The clip is the
    // geometry, not the painted alpha, so a fill:none or translucent shape still shows them.
    canvas.save_layer(&SaveLayerRec::default());
    paint_shape(canvas)?;
    canvas.clip_path(&shape.geometry, ClipOp::Intersect, true);
    for child in &shape.children {
        draw_node(canvas, child, ctx, world)?;
    }
    canvas.restore();
    Ok(())
}

fn apply_clip_mask(canvas: &Canvas, clip: &Clip) -> Result<(), RenderError> {
    let paint = coverage_paint(false, false, clip.feather)?;
    let rec = SaveLayerRec::default().paint(&paint);
    canvas.save_layer(&rec);
    let white = white_paint();
    if clip.invert {
        let mut bounds = canvas.local_clip_bounds().unwrap_or_else(huge_rect);
        bounds.outset((clip.feather * 3.0, clip.feather * 3.0));
        canvas.draw_rect(bounds, &white);
        let mut punch = white_paint();
        punch.set_blend_mode(BlendMode::DstOut);
        canvas.draw_path(&clip.path, &punch);
    } else {
        canvas.draw_path(&clip.path, &white);
    }
    canvas.restore();
    Ok(())
}

fn apply_mask(
    canvas: &Canvas,
    mask: &Mask,
    ctx: &DrawCtx<'_>,
    world: Matrix,
) -> Result<(), RenderError> {
    let paint = coverage_paint(mask.luminance, mask.invert, mask.feather)?;
    let rec = SaveLayerRec::default().paint(&paint);
    canvas.save_layer(&rec);
    match &mask.source {
        MaskSource::Nodes(nodes) => {
            for node in nodes {
                draw_node(canvas, node, ctx, world)?;
            }
        }
        MaskSource::Field { field, threshold } => {
            let shader = field_shader(
                field,
                FieldUse::Coverage {
                    threshold: *threshold,
                    as_luma: mask.luminance,
                },
            )?;
            let mut paint = Paint::default();
            paint.set_anti_alias(true);
            paint.set_shader(shader);
            let mut bounds = canvas.local_clip_bounds().unwrap_or_else(huge_rect);
            bounds.outset((mask.feather.max(1.0), mask.feather.max(1.0)));
            canvas.draw_rect(bounds, &paint);
        }
    }
    canvas.restore();
    Ok(())
}

fn coverage_paint(luminance: bool, invert: bool, feather: f32) -> Result<Paint, RenderError> {
    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    paint.set_blend_mode(BlendMode::DstIn);
    let mut filter: Option<ColorFilter> = None;
    if luminance {
        filter = Some(ColorFilter::luma());
    }
    if invert {
        let inv = invert_alpha();
        filter = Some(match filter {
            Some(inner) => skia_safe::color_filters::compose(inv.clone(), inner).unwrap_or(inv),
            None => inv,
        });
    }
    if let Some(filter) = filter {
        paint.set_color_filter(filter);
    }
    if feather > 0.0 {
        let blur = image_filters::blur(
            (feather, feather),
            TileMode::Decal,
            None::<ImageFilter>,
            None,
        )
        .ok_or_else(|| RenderError::Skia("feather".into()))?;
        paint.set_image_filter(blur);
    }
    Ok(paint)
}

fn invert_alpha() -> ColorFilter {
    let matrix = ColorMatrix::new(
        1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0,
        -1.0, 1.0,
    );
    skia_safe::color_filters::matrix(&matrix, None)
}

fn huge_rect() -> Rect {
    Rect::from_ltrb(-100000.0, -100000.0, 100000.0, 100000.0)
}

fn visual_pad(node: &Node) -> f32 {
    let mut pad = 0.0_f32;
    for effect in &node.effects {
        let extra = match &effect.op {
            artscript_ir::EffectOp::Blur { sigma } => *sigma * 3.0,
            artscript_ir::EffectOp::Shadow { dx, dy, blur, .. } => dx.abs() + dy.abs() + blur * 3.0,
            artscript_ir::EffectOp::Glow { radius, .. } => *radius * 3.0,
            artscript_ir::EffectOp::Outline { width, .. } => *width + 2.0,
            artscript_ir::EffectOp::Inner { dx, dy, blur, .. } => dx.abs() + dy.abs() + blur,
            artscript_ir::EffectOp::Rim { width, .. } => *width + 2.0,
            _ => 0.0,
        };
        pad = pad.max(extra);
    }
    if let NodeKind::Shape(shape) = &node.kind {
        pad = pad.max(shape.blur * 3.0);
    }
    if let Some(clip) = &node.clip {
        pad = pad.max(clip.feather * 3.0);
    }
    if let Some(mask) = &node.mask {
        pad = pad.max(mask.feather * 3.0);
        if let MaskSource::Nodes(nodes) = &mask.source {
            for n in nodes {
                pad = pad.max(visual_pad(n));
            }
        }
    }
    for child in node.children() {
        pad = pad.max(visual_pad(child));
    }
    pad
}

#[allow(deprecated)]
fn png_bytes(image: &skia_safe::Image) -> Result<Vec<u8>, RenderError> {
    let data = image
        .encode_to_data(EncodedImageFormat::PNG)
        .ok_or_else(|| RenderError::Skia("png encode".into()))?;
    Ok(data.as_bytes().to_vec())
}

fn transform_scale(m: &Matrix) -> f32 {
    let sx = (m.scale_x() * m.scale_x() + m.skew_y() * m.skew_y()).sqrt();
    let sy = (m.scale_y() * m.scale_y() + m.skew_x() * m.skew_x()).sqrt();
    sx.max(sy).max(1.0)
}
