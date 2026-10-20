//! A warped node draws its content flat into an offscreen at device density, and that image is
//! drawn through the warp. Brushes, fields, children and masks keep an affine canvas. The one
//! decision made here is [`Warp::placer`]: a stack that is one 3x3 matrix draws the image once
//! under it, and any other stack draws it as a grid of projective cells whose corners the warp
//! maps. Everything else is the warp's own geometry.

use ramose_ir::{Matrix, Node, Placer, Rect, Warp, transform_scale};
use skia_safe::canvas::SrcRectConstraint;
use skia_safe::{Canvas, FilterMode, IRect, Image, MipmapMode, Paint, RoundOut, SamplingOptions};

use crate::RenderError;
use crate::draw::{DrawCtx, draw_clipped, path_extent, rasterize_layer};
use crate::gpu;

/// Transparent flat pixels around the content, so the grid's aliased border lands on clear texels.
const MARGIN: f32 = 2.0;
/// Device pixels per grid cell, measured in the warped output.
const CELL: f32 = 4.0;
/// The most the flat offscreen oversamples the device under a magnifying warp.
const MAX_SUPERSAMPLE: f32 = 4.0;

/// [`draw_clipped`] through the node's warp. The canvas is under `node.transform`, so the image
/// lands in the frame the transform places, and inside the enclosing layer, so effects see it.
pub(crate) fn draw_warped(
    canvas: &Canvas,
    node: &Node,
    ctx: &DrawCtx<'_>,
    world: Matrix,
) -> Result<(), RenderError> {
    if node.warp.is_none() {
        return draw_clipped(canvas, node, ctx, world);
    }
    let index = &ctx.index;
    let reach = node.content_reach(&world, &|p| Some((index.node(p)?, index.parent_world(p))));
    let Some(content) = join(reach, path_extent(node)) else {
        return Ok(());
    };
    let ltd = canvas.local_to_device_as_3x3();
    let stretch = node.warp.max_stretch(content).min(MAX_SUPERSAMPLE);
    let density = transform_scale(&ltd) * stretch;
    let margin = MARGIN / density;
    let flat = node.warp.visible(content.with_outset((margin, margin)));
    let warped_px: IRect = ltd.map_rect(node.warp.map_rect(flat)).0.round_out();
    if canvas
        .device_clip_bounds()
        .and_then(|clip| IRect::intersect(&clip, &warped_px))
        .is_none()
    {
        return Ok(());
    }
    let flat_ltd = Matrix::scale((density, density)) * Matrix::translate((-flat.left, -flat.top));
    let size = IRect::from_wh(px(flat.width() * density), px(flat.height() * density));
    let image = rasterize_layer(ctx.backend, size, &flat_ltd, None, |flat_canvas| {
        draw_clipped(flat_canvas, node, ctx, world)
    })?;
    let image = gpu::mipmapped(ctx.backend, image)?;
    match node.warp.placer() {
        Placer::Matrix(m) => draw_projective(canvas, &image, &flat_ltd, &m),
        Placer::Grid => {
            let cells = (cells(warped_px.width()), cells(warped_px.height()));
            draw_grid(canvas, &image, flat, &flat_ltd, cells, &node.warp);
        }
    }
    Ok(())
}

fn px(extent: f32) -> i32 {
    extent.ceil().max(1.0) as i32
}

fn cells(device_extent: i32) -> usize {
    (device_extent as f32 / CELL).ceil().max(1.0) as usize
}

fn join(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
    match (a, b) {
        (Some(mut a), Some(b)) => {
            a.join(b);
            Some(a)
        }
        (a, b) => a.or(b),
    }
}

fn sampling() -> SamplingOptions {
    SamplingOptions::new(FilterMode::Linear, MipmapMode::Linear)
}

/// The whole image once under the stack's matrix. `flat` was cut to the warp's visible box, so
/// every pixel is in front of the horizon.
fn draw_projective(canvas: &Canvas, image: &Image, flat_ltd: &Matrix, m: &Matrix) {
    let Some(flat_px_to_content) = flat_ltd.invert() else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    canvas.save();
    canvas.concat(&(*m * flat_px_to_content));
    canvas.draw_image_rect_with_sampling_options(
        image,
        None,
        Rect::from_wh(image.width() as f32, image.height() as f32),
        sampling(),
        &paint,
    );
    canvas.restore();
}

/// `nx` by `ny` cells over `flat`, each drawn under the projective matrix that takes its four
/// flat corners to where the warp maps them. A cell whose quad collapses (a cylinder's tangent
/// edge) covers no pixels and is skipped. Antialiasing stays off so cells share edges without
/// seams; the image's transparent margin keeps the outer border on clear texels.
fn draw_grid(
    canvas: &Canvas,
    image: &Image,
    flat: Rect,
    flat_ltd: &Matrix,
    (nx, ny): (usize, usize),
    warp: &Warp,
) {
    let edge = |i: usize, n: usize, lo: f32, hi: f32| {
        if i == n {
            hi
        } else {
            lo + (hi - lo) * i as f32 / n as f32
        }
    };
    let mut paint = Paint::default();
    paint.set_anti_alias(false);
    for j in 0..ny {
        for i in 0..nx {
            let cell = Rect::new(
                edge(i, nx, flat.left, flat.right),
                edge(j, ny, flat.top, flat.bottom),
                edge(i + 1, nx, flat.left, flat.right),
                edge(j + 1, ny, flat.top, flat.bottom),
            );
            let Some(dst) = cell
                .to_quad(None)
                .iter()
                .map(|&p| warp.map_point(p))
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            let src = flat_ltd.map_rect(cell).0;
            let Some(m) = Matrix::from_poly_to_poly(&src.to_quad(None), &dst) else {
                continue;
            };
            canvas.save();
            canvas.concat(&m);
            canvas.draw_image_rect_with_sampling_options(
                image,
                Some((&src, SrcRectConstraint::Fast)),
                src,
                sampling(),
                &paint,
            );
            canvas.restore();
        }
    }
}
