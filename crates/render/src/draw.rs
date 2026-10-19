//! Canvas walk. A node that needs isolation, a mask, an effect, or a non-default composite is
//! drawn into a layer. A soft clip draws into layers that start from the canvas below, so its
//! children still blend with it. Effects see the clipped and masked content.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use artscript_ir::{Blend, Clip, Fill, Mask, MaskSource, Matrix, Node, NodeKind, Path, Scene, Shape};
use skia_safe::canvas::{SaveLayerFlags, SaveLayerRec};
use skia_safe::{
    BlendMode, Blender, Canvas, ClipOp, ColorFilter, ColorMatrix, Data, EncodedImageFormat, IRect,
    Image, ImageFilter, Paint, Rect, RoundOut, RuntimeEffect, TileMode, image_filters,
};

use crate::gpu::{self, Backend};
use crate::RenderError;
use crate::effects::{self, FxEnv, white_paint};
use crate::field::{FieldUse, field_shader};
use crate::paint::{blend_mode, color4, content_paint, dithered};

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
            self.visit(node, parent_world);
        }
    }

    fn visit(&mut self, node: &'a Node, parent_world: Matrix) {
        self.nodes.insert(node.path.as_str(), node);
        self.parents.insert(node.path.as_str(), parent_world);
        let world = parent_world * node.transform;
        if let Some(mask) = &node.mask
            && let MaskSource::Nodes(inner) = &mask.source
        {
            self.walk(inner, world);
        }
        for child in node.descendants() {
            self.visit(child, world);
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
    /// Signed distance fields built for brushed fills in this render.
    pub(crate) sdf: RefCell<crate::sdf::SdfMemo>,
    /// Ribbon fields built for brushed strokes in this render.
    pub(crate) ribbons: RefCell<crate::ribbon::FieldMemo>,
}

impl<'a> DrawCtx<'a> {
    pub fn new(scene: &'a Scene, backend: Backend) -> Self {
        Self {
            index: SceneIndex::build(scene),
            backend,
            visiting: RefCell::new(HashSet::new()),
            sdf: RefCell::new(crate::sdf::SdfMemo::default()),
            ribbons: RefCell::new(crate::ribbon::FieldMemo::default()),
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
    let (w, h) = artscript_ir::pixel_size(scene.width, scene.height, scale);
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
    let (w, h) = artscript_ir::pixel_size(scene.width, scene.height, scale);
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
    let mut drawn = node.clone();
    drawn.blend = Blend::Normal;
    let parent = ctx.index.parent_world(&node.path);
    bake(&ctx, &drawn, parent, bake_scale)
}

/// Renders a shape's own fill and stroke on transparent, in the shape's local coordinates, with
/// none of its stamps, children, effects, mask, or opacity: the SVG writer draws those as elements.
pub fn bake_paint(scene: &Scene, node: &Node, bake_scale: f32, backend: Backend) -> Result<Baked, RenderError> {
    if !(bake_scale.is_finite() && bake_scale > 0.0) {
        return Err(RenderError::Skia("bake scale must be positive".into()));
    }
    let ctx = DrawCtx::new(scene, backend);
    let mut drawn = node.clone();
    drawn.transform = Matrix::new_identity();
    drawn.opacity = 1.0;
    drawn.blend = Blend::Normal;
    drawn.clip = None;
    drawn.mask = None;
    drawn.effects.clear();
    if let NodeKind::Shape(shape) = &mut drawn.kind {
        shape.stamps.clear();
        shape.children.clear();
    }
    let parent = ctx.index.parent_world(&node.path) * node.transform;
    // The same pixel density as a tile of the whole node, which is drawn in the parent's units.
    bake(&ctx, &drawn, parent, bake_scale * transform_scale(&node.transform))
}

/// `node` drawn on transparent in its parent's coordinates, where `parent` is the canvas from them.
fn bake(ctx: &DrawCtx<'_>, node: &Node, parent: Matrix, bake_scale: f32) -> Result<Baked, RenderError> {
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
    let mut surface = gpu::surface(ctx.backend, (pw, ph))
        .ok_or_else(|| RenderError::Skia("bake surface".into()))?;
    let canvas = surface.canvas();
    canvas.scale((bake_scale, bake_scale));
    canvas.translate((-bounds.left, -bounds.top));
    draw_node(canvas, node, ctx, parent)?;
    let image = gpu::finish(ctx.backend, &mut surface)?;
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

    let parent_to_px = canvas.local_to_device_as_3x3();
    canvas.save();
    canvas.concat(&node.transform);
    let world = parent_world * node.transform;

    if needs_layer(node) {
        if let Some(steps) = effects::layer_steps(&node.effects, &FxEnv { ctx, world })? {
            let reach = reach_px(node, ctx, &parent_world, &parent_to_px, &steps);
            paint_layer_steps(canvas, node, ctx, world, reach, steps)?;
        } else {
            let filter = effects::chain(&node.effects, &FxEnv { ctx, world })?;
            let mut paint = Paint::default();
            paint.set_anti_alias(true);
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
    reach: Option<IRect>,
    mut steps: Vec<effects::LayerStep>,
) -> Result<(), RenderError> {
    let Some(bounds) = canvas.device_clip_bounds().and_then(|clip| match reach {
        Some(reach) => IRect::intersect(&clip, &reach),
        None => Some(clip),
    }) else {
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
            effects::LayerStep::Outline(outline) => {
                effects::outline_geometry(ctx.backend, image, &local_to_px, node, outline)?
            }
        };
    }
    canvas.save();
    canvas.reset_matrix();
    let mut paint = Paint::default();
    paint.set_anti_alias(true);
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

/// Skia antialiases a path that reaches past the device differently, so the layer also covers
/// [`path_extent`]: every path stays inside it and draws as it would on a layer over the whole clip.
fn reach_px(
    node: &Node,
    ctx: &DrawCtx<'_>,
    parent_world: &Matrix,
    parent_to_px: &Matrix,
    steps: &[effects::LayerStep],
) -> Option<IRect> {
    let index = &ctx.index;
    let mut reach =
        node.outer_bounds(parent_world, &|p| Some((index.node(p)?, index.parent_world(p))))?;
    let ring = steps
        .iter()
        .map(|step| match step {
            effects::LayerStep::Outline(outline) => outline.width,
            effects::LayerStep::Filter(_) => 0.0,
        })
        .fold(0.0_f32, f32::max);
    if let Some(paths) = path_extent(node) {
        reach.join(node.transform.map_rect(paths.with_outset((ring, ring))).0);
    }
    let px: IRect = parent_to_px.map_rect(reach).0.round_out();
    Some(px.with_outset((2, 2)))
}

/// The bounds Skia culls the paths under `node` by, in `node`'s space: control points, not the
/// curve, and strokes grown by half their width times the default miter limit of 4.
fn path_extent(node: &Node) -> Option<Rect> {
    let own = match &node.kind {
        NodeKind::Shape(shape) => {
            let stroke = shape.stroke.as_ref().map_or(0.0, |s| s.width * 2.0);
            let pad = stroke + shape.blur * 3.0 + shape.mark_reach();
            let mut r = shape.geometry.bounds().with_outset((pad, pad));
            if let Some(ink) = shape.stroke.as_ref().and_then(|s| s.ink.as_ref()) {
                r.join(ink.bounds().with_outset((pad, pad)));
            }
            Some(r)
        }
        NodeKind::Group { .. } => None,
    };
    let masks = match node.mask.as_ref().map(|m| &m.source) {
        Some(MaskSource::Nodes(nodes)) => &nodes[..],
        _ => &[],
    };
    node.descendants()
        .chain(masks)
        .filter_map(|child| Some(child.transform.map_rect(path_extent(child)?).0))
        .chain(own)
        .reduce(|mut a, b| {
            a.join(b);
            a
        })
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
    if soft.is_some() {
        let backdrop = || SaveLayerRec::default().flags(SaveLayerFlags::INIT_WITH_PREVIOUS);
        let mut paint = Paint::default();
        paint.set_blender(uncomposite()?);
        canvas.save_layer(&backdrop());
        canvas.save_layer(&backdrop().paint(&paint));
        draw_kind(canvas, node, ctx, world)?;
        canvas.restore();
    } else {
        if wrapped {
            canvas.save_layer(&SaveLayerRec::default());
        }
        draw_kind(canvas, node, ctx, world)?;
    }
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

/// Replaces the backdrop `dst` with `S`, such that `S` drawn over `dst` gives `src`: the
/// children composited over the backdrop. The clip coverage `m` then scales `S`, and `m * S`
/// over the backdrop is `lerp(dst, src, m)` in every color channel. Over a translucent
/// backdrop the alpha of `S` solves `src.a = S.a + dst.a * (1 - S.a)`. Each color channel also
/// bounds `S.a` from below, so `S` stays a valid premultiplied color when 8-bit rounding or an
/// opaque backdrop leaves the alpha unconstrained.
fn uncomposite() -> Result<Blender, RenderError> {
    const SKSL: &str = "half4 main(half4 src, half4 dst) {
        float4 s = float4(src);
        float4 d = float4(dst);
        float a = 0;
        if (d.a < 0.999) { a = (s.a - d.a) / (1 - d.a); }
        for (int i = 0; i < 3; i++) {
            if (d[i] > 0.001) { a = max(a, 1 - s[i] / d[i]); }
            if (d[i] < 0.999) { a = max(a, (s[i] - d[i]) / (1 - d[i])); }
        }
        a = saturate(a);
        return half4(clamp(s.rgb - d.rgb * (1 - a), 0, a), a);
    }";
    RuntimeEffect::make_for_blender(SKSL, None)
        .map_err(RenderError::Skia)?
        .make_blender(Data::new_empty(), None)
        .ok_or_else(|| RenderError::Skia("uncomposite".into()))
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
        NodeKind::Shape(shape) => draw_shape(canvas, shape, ctx, world, &node.origin),
    }
}

fn draw_shape(
    canvas: &Canvas,
    shape: &Shape,
    ctx: &DrawCtx<'_>,
    world: Matrix,
    origin: &str,
) -> Result<(), RenderError> {
    let paint_shape = |canvas: &Canvas| -> Result<(), RenderError> {
        if let Some(fill) = &shape.fill {
            if fill.marks.is_empty() {
                let paint =
                    dithered(content_paint(&fill.paint, shape.blur, None)?, &canvas.local_to_device_as_3x3())?;
                canvas.draw_path(&shape.geometry, &paint);
            } else {
                crate::mark::draw_area(canvas, &shape.geometry, fill, ctx, origin)?;
            }
        }
        if let Some(stroke) = &shape.stroke {
            match (&stroke.ribbon, &stroke.ink) {
                (None, Some(ink)) => {
                    let paint =
                        dithered(content_paint(&stroke.paint, shape.blur, None)?, &canvas.local_to_device_as_3x3())?;
                    canvas.draw_path(ink, &paint);
                }
                (None, None) => {
                    let paint = dithered(
                        content_paint(&stroke.paint, shape.blur, Some(stroke))?,
                        &canvas.local_to_device_as_3x3(),
                    )?;
                    canvas.draw_path(&shape.geometry, &paint);
                }
                (Some(ribbon), _) => crate::mark::draw_ribbon(canvas, &shape.geometry, stroke, ribbon, ctx, origin)?,
            }
        }
        Ok(())
    };

    if shape.children.is_empty() && shape.stamps.is_empty() {
        return paint_shape(canvas);
    }

    // The layer keeps the stamps' and children's blend modes off the pixels below the shape. The
    // clip is never the painted alpha, so a fill:none or translucent shape still shows the
    // children. Stamps are not clipped: a leaf spills past its stem.
    canvas.save_layer(&SaveLayerRec::default());
    paint_shape(canvas)?;
    for stamp in &shape.stamps {
        draw_node(canvas, stamp, ctx, world)?;
    }
    let draw_children = || {
        for child in &shape.children {
            draw_node(canvas, child, ctx, world)?;
        }
        Ok(())
    };
    match child_clip(shape) {
        ChildClip::Path(path) => {
            canvas.clip_path(&path, ClipOp::Intersect, true);
            draw_children()?;
        }
        ChildClip::Coverage(fill) => {
            crate::mark::with_coverage(canvas, &shape.geometry, fill, ctx, origin, draw_children)?;
        }
    }
    canvas.restore();
    Ok(())
}

enum ChildClip<'a> {
    Path(Cow<'a, Path>),
    /// The coverage a brushed fill leaves: the stage coverages without the paint's alpha, so a
    /// translucent shape keeps its children opaque.
    Coverage(&'a Fill),
}

fn child_clip(shape: &Shape) -> ChildClip<'_> {
    match shape.coverage_clip() {
        Some(fill) => ChildClip::Coverage(fill),
        None => ChildClip::Path(shape.clip()),
    }
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

pub(crate) fn visual_pad(node: &Node) -> f32 {
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
        pad = pad.max(shape.blur * 3.0).max(shape.mark_reach());
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
    for child in node.descendants() {
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
