//! `m.below`: the straight rgba already composited under the pixel in the node's group. The
//! node's paint is computed over the real backdrop and lands in one layer with its stamps and
//! children, so `a:`, `blend:`, a mask, and a clip apply to the paint alone as they do today.

mod common;

use std::sync::Arc;

use artscript_brush::{MainKind, Program, UniformKind};
use artscript_ir::{AreaMark, Clip, EffectOp, Fill, Mask, MaskSource, NodeKind, Paint, Path, Ribbon, RibbonContour, RibbonMark, Sides, Stage, Stroke};
use artscript_render::{Backend, RenderError, gpu_available, render_png};
use common::{color, effect, pixel, red, scene, solid, stamp};

/// Mixes the paint halfway toward the backdrop, weighted by the backdrop's alpha, so a pixel
/// over nothing stays the paint and a pixel over an opaque color is the mean of the two.
const HALF_BELOW: &str = "float4 shade(Mark m) { return float4(mix(m.paint.rgb, m.below.rgb, m.below.a * .5), cover(m.edge, m)); }\n";
/// Returns the backdrop itself, so the draw changes no pixel over an opaque backdrop.
const BELOW: &str = "float4 shade(Mark m) { return float4(m.below.rgb, m.below.a * cover(m.edge, m)); }\n";
/// Reads nothing beneath it.
const PLAIN: &str = "float4 shade(Mark m) { return float4(m.paint.rgb, cover(m.edge, m)); }\n";

fn stage(kind: MainKind, body: &str, seed: u32) -> Stage {
    let program = Program::assemble(kind, &[] as &[(String, UniformKind)], body);
    artscript_brush::check(&program).unwrap_or_else(|e| panic!("{e:?}"));
    Stage { program: Arc::new(program), uniforms: vec![], seed }
}

fn area(body: &str, seed: u32) -> AreaMark {
    AreaMark { stage: stage(MainKind::Area, body, seed), dir_deg: 0.0, cos_sin: [1.0, 0.0], reach: 0.0 }
}

const SQUARE: skia_safe::Rect = skia_safe::Rect { left: 20.0, top: 20.0, right: 60.0, bottom: 60.0 };

fn gold() -> artscript_ir::Color {
    color(232, 179, 58, 255)
}

fn blue() -> artscript_ir::Color {
    color(79, 127, 224, 255)
}

/// The gold backdrop every scene here draws first: a rect past the square on every side.
fn backdrop() -> artscript_ir::Node {
    solid("back", Path::rect(skia_safe::Rect::from_ltrb(10.0, 10.0, 70.0, 70.0), None), gold())
}

fn brushed(path: &str, paint: artscript_ir::Color, marks: Vec<AreaMark>) -> artscript_ir::Node {
    let mut node = solid(path, Path::rect(SQUARE, None), red());
    if let NodeKind::Shape(shape) = &mut node.kind {
        shape.fill = Some(Fill { paint: Paint::Solid(paint), marks });
    }
    node
}

/// A ribbon of constant half width along the square's one contour.
fn ribbon(body: &str, half_width: f32) -> Ribbon {
    let len = 160.0;
    let knot = |s: f32| artscript_ir::Knot { s, half_width, pressure: 1.0, angle: 0.0, tilt: 0.0, velocity: 1.0 };
    Ribbon {
        marks: vec![RibbonMark { stage: stage(MainKind::Ribbon, body, 9), reach: 0.0 }],
        contours: vec![RibbonContour { len, closed: true, knots: vec![knot(0.0), knot(len)] }],
        taper: [0.0, 0.0],
    }
}

fn with_stroke(mut node: artscript_ir::Node, paint: artscript_ir::Color, ribbon: Option<Ribbon>) -> artscript_ir::Node {
    if let NodeKind::Shape(shape) = &mut node.kind {
        shape.stroke = Some(Stroke {
            paint: Paint::Solid(paint),
            width: 8.0,
            cap: artscript_ir::Cap::Round,
            join: artscript_ir::Join::Round,
            dash: vec![],
            ink: None,
            ribbon,
        });
    }
    node
}

fn with_child(mut parent: artscript_ir::Node, child: artscript_ir::Node) -> artscript_ir::Node {
    if let NodeKind::Shape(shape) = &mut parent.kind {
        shape.children.push(child);
    }
    parent
}

fn band() -> artscript_ir::Node {
    solid("band", Path::rect(skia_safe::Rect::from_ltrb(10.0, 30.0, 70.0, 50.0), None), color(43, 36, 64, 255))
}

fn mix(a: artscript_ir::Color, b: artscript_ir::Color, t: f32) -> [u8; 3] {
    [a.r + (b.r - a.r) * t, a.g + (b.g - a.g) * t, a.b + (b.b - a.b) * t].map(|v| (v * 255.0).round() as u8)
}

fn assert_rgb(got: [u8; 4], want: [u8; 3], what: &str) {
    assert_eq!(got[3], 255, "{what}: {got:?}");
    for c in 0..3 {
        assert!(got[c].abs_diff(want[c]) <= 1, "{what}: {got:?}, want {want:?}");
    }
}

fn render(nodes: Vec<artscript_ir::Node>) -> Vec<u8> {
    render_png(&scene(80.0, 80.0, nodes), 1.0, Backend::Cpu).unwrap()
}

#[test]
fn a_brush_reads_the_pixels_already_drawn_under_the_node_and_renders_the_same_bytes_twice() {
    let s = scene(80.0, 80.0, vec![backdrop(), brushed("a", blue(), vec![area(BELOW, 1)])]);
    let same = render_png(&s, 1.0, Backend::Cpu).unwrap();
    assert_eq!(same, render_png(&s, 1.0, Backend::Cpu).unwrap());
    assert_eq!(pixel(&same, 40, 40), [232, 179, 58, 255], "returning m.below draws the backdrop back");
    assert_eq!(pixel(&same, 21, 21), [232, 179, 58, 255]);
    let half = render(vec![backdrop(), brushed("a", blue(), vec![area(HALF_BELOW, 1)])]);
    assert_rgb(pixel(&half, 40, 40), mix(blue(), gold(), 0.5), "halfway between the paint and the gold under it");
    assert_eq!(pixel(&half, 15, 40), [232, 179, 58, 255], "nothing past the geometry");
    let alone = render(vec![brushed("a", blue(), vec![area(HALF_BELOW, 1)])]);
    assert_eq!(pixel(&alone, 40, 40), [79, 127, 224, 255], "over nothing, m.below is transparent and the paint stays");
}

#[test]
fn inside_an_isolated_group_the_backdrop_is_only_what_the_group_drew() {
    let group = stamp("g", artscript_ir::Matrix::new_identity(), vec![brushed("g.a", blue(), vec![area(HALF_BELOW, 1)])]);
    let png = render(vec![backdrop(), group]);
    assert_eq!(pixel(&png, 40, 40), [79, 127, 224, 255], "the gold is outside the group, so m.below is transparent");
    let inner = solid("g.back", Path::rect(skia_safe::Rect::from_ltrb(10.0, 10.0, 70.0, 70.0), None), gold());
    let group = stamp("g", artscript_ir::Matrix::new_identity(), vec![inner, brushed("g.a", blue(), vec![area(HALF_BELOW, 1)])]);
    let png = render_png(&scene(80.0, 80.0, vec![group]), 1.0, Backend::Cpu).unwrap();
    assert_rgb(pixel(&png, 40, 40), mix(blue(), gold(), 0.5), "gold drawn inside the group is seen");
}

#[test]
fn a_stroke_sees_its_own_fill_and_a_plain_fill_stays_under_it() {
    let node = with_stroke(solid("a", Path::rect(SQUARE, None), gold()), blue(), Some(ribbon(HALF_BELOW, 4.0)));
    let png = render(vec![node]);
    assert_rgb(pixel(&png, 22, 40), mix(blue(), gold(), 0.5), "the inner half of the stroke sits over the fill");
    assert_eq!(pixel(&png, 18, 40), [79, 127, 224, 255], "the outer half sits over nothing");
    assert_eq!(pixel(&png, 40, 40), [232, 179, 58, 255], "the fill is still there in the middle");
    assert_eq!(pixel(&png, 14, 40)[3], 0, "nothing past the stroke");
}

#[test]
fn a_fill_and_a_stroke_that_both_read_below_see_the_backdrop_then_the_fill() {
    let fill = brushed("a", red(), vec![area(HALF_BELOW, 1)]);
    let node = with_child(with_stroke(fill, blue(), Some(ribbon(HALF_BELOW, 4.0))), band());
    let png = render(vec![backdrop(), node]);
    let fill_out = mix(red(), gold(), 0.5);
    assert_rgb(pixel(&png, 40, 25), fill_out, "the fill mixes with the gold");
    let fill_color = color(fill_out[0], fill_out[1], fill_out[2], 255);
    assert_rgb(pixel(&png, 22, 25), mix(blue(), fill_color, 0.5), "the inner stroke mixes with the fill's output");
    assert_rgb(pixel(&png, 18, 25), mix(blue(), gold(), 0.5), "the outer stroke mixes with the gold");
    assert_eq!(pixel(&png, 40, 40), [43, 36, 64, 255], "the child draws over the fill");
    assert_eq!(pixel(&png, 15, 40), [232, 179, 58, 255], "the child clips to the geometry");
}

#[test]
fn children_draw_over_a_fill_that_read_below_and_clip_to_it() {
    let node = with_child(brushed("a", blue(), vec![area(HALF_BELOW, 1)]), band());
    let png = render(vec![backdrop(), node]);
    assert_rgb(pixel(&png, 40, 25), mix(blue(), gold(), 0.5), "the fill mixes with the gold");
    assert_eq!(pixel(&png, 40, 40), [43, 36, 64, 255], "the child draws over the fill");
    assert_eq!(pixel(&png, 15, 40), [232, 179, 58, 255], "the child clips to the geometry");
}

#[test]
fn opacity_applies_to_the_paint_alone_after_it_read_the_backdrop() {
    let mut node = brushed("a", blue(), vec![area(HALF_BELOW, 1)]);
    node.opacity = 0.5;
    let png = render(vec![backdrop(), node]);
    let out = mix(blue(), gold(), 0.5);
    assert_rgb(pixel(&png, 40, 40), mix(color(out[0], out[1], out[2], 255), gold(), 0.5), "half of the mixed paint over the gold");
}

#[test]
fn a_mask_and_a_clip_keep_the_paint_only_where_they_keep_it() {
    let left = Path::rect(skia_safe::Rect::from_ltrb(0.0, 0.0, 40.0, 80.0), None);
    let mut masked = brushed("a", blue(), vec![area(HALF_BELOW, 1)]);
    masked.mask = Some(Box::new(Mask { source: MaskSource::Nodes(vec![solid("a.m", left.clone(), red())]), luminance: false, feather: 0.0, invert: false }));
    let png = render(vec![backdrop(), masked]);
    assert_rgb(pixel(&png, 30, 40), mix(blue(), gold(), 0.5), "kept by the mask");
    assert_eq!(pixel(&png, 50, 40), [232, 179, 58, 255], "masked away: the gold shows");
    for feather in [0.0, 2.0] {
        let mut clipped = brushed("a", blue(), vec![area(HALF_BELOW, 1)]);
        clipped.clip = Some(Clip { path: left.clone(), feather, invert: false });
        let png = render(vec![backdrop(), clipped]);
        assert_rgb(pixel(&png, 30, 40), mix(blue(), gold(), 0.5), "inside the clip");
        assert_eq!(pixel(&png, 50, 40), [232, 179, 58, 255], "outside the clip, feather {feather}");
    }
}

#[test]
fn every_stage_of_a_stack_sees_the_same_backdrop() {
    let both = brushed("a", blue(), vec![area(HALF_BELOW, 1), area("float4 shade(Mark m) { return float4(mix(m.paint.rgb, m.below.rgb, .5), 1); }\n", 2)]);
    let png = render(vec![backdrop(), both]);
    let first = mix(blue(), gold(), 0.5);
    assert_rgb(pixel(&png, 40, 40), mix(color(first[0], first[1], first[2], 255), gold(), 0.5), "stage 2 mixes stage 1's color with the gold, not with stage 1 over the gold");
}

#[test]
fn a_brushed_sibling_that_does_not_read_below_still_draws_as_before() {
    let before = render(vec![backdrop(), brushed("a", blue(), vec![area(PLAIN, 1)])]);
    assert_eq!(pixel(&before, 40, 40), [79, 127, 224, 255]);
}

#[test]
fn the_gpu_agrees_with_the_cpu_and_repeats_itself() {
    if !gpu_available() {
        return;
    }
    let fill = brushed("a", red(), vec![area(HALF_BELOW, 1)]);
    let node = with_child(with_stroke(fill, blue(), Some(ribbon(HALF_BELOW, 4.0))), band());
    let mut faint = brushed("b", blue(), vec![area(HALF_BELOW, 3)]);
    faint.opacity = 0.5;
    faint.transform = artscript_ir::Matrix::translate((5.0, 5.0));
    let s = scene(80.0, 80.0, vec![backdrop(), node, faint]);
    let cpu = render_png(&s, 1.0, Backend::Cpu).unwrap();
    let gpu = render_png(&s, 1.0, Backend::Gpu).unwrap();
    let mut worst = 0u8;
    for y in 0..80 {
        for x in 0..80 {
            let (a, b) = (pixel(&cpu, x, y), pixel(&gpu, x, y));
            worst = worst.max((0..4).map(|c| a[c].abs_diff(b[c])).max().unwrap());
        }
    }
    assert!(worst <= 4, "max channel difference {worst}");
    assert_eq!(gpu, render_png(&s, 1.0, Backend::Gpu).unwrap());
}

#[test]
fn an_outline_on_a_node_whose_brush_reads_below_is_a_render_error_not_a_wrong_backdrop() {
    let mut node = brushed("a", blue(), vec![area(HALF_BELOW, 1)]);
    node.effects.push(effect(EffectOp::Outline { width: 2.0, color: red(), sides: Sides::ALL, mask: None, brush: vec![] }));
    let err = render_png(&scene(80.0, 80.0, vec![backdrop(), node]), 1.0, Backend::Cpu).unwrap_err();
    assert!(matches!(err, RenderError::AtNode { .. }), "{err:?}");
}
