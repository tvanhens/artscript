//! Brushed fills: the stage chain over the signed distance field, the base paint reaching
//! `m.paint`, reach growing the drawn area, determinism, and GPU closeness.

mod common;

use std::sync::Arc;

use ramose_brush::{MainKind, Program, UniformKind, UniformValue};
use ramose_ir::{EffectOp, Fill, NodeKind, Paint, Path, Point, RibbonMark, Sides, Stage, Stop};
use ramose_render::{Backend, gpu_available, raster_only_features, render_png, render_svg};
use common::{brushed, color, effect, mark, pixel, red, scene, solid, stage, stamp, with_stamps};

const IDENTITY: &str = "float4 shade(Mark m) { return float4(m.paint.rgb, cover(m.edge, m)); }\n";
const SQUARE: skia_safe::Rect = skia_safe::Rect { left: 20.0, top: 20.0, right: 60.0, bottom: 60.0 };

#[test]
fn a_solid_base_reaches_the_body_exactly_and_renders_the_same_bytes_twice() {
    let node = brushed("a", Path::rect(SQUARE, None), Paint::Solid(color(200, 40, 10, 255)), vec![mark(stage(IDENTITY, &[], &[], 3), 0.0)]);
    let s = scene(80.0, 80.0, vec![node]);
    let png = render_png(&s, 1.0, Backend::Cpu).unwrap();
    assert_eq!(png, render_png(&s, 1.0, Backend::Cpu).unwrap());
    assert_eq!(pixel(&png, 40, 40), [200, 40, 10, 255]);
    assert_eq!(pixel(&png, 21, 21), [200, 40, 10, 255]);
    assert_eq!(pixel(&png, 10, 40)[3], 0, "nothing past the edge");
    assert_eq!(pixel(&png, 61, 40)[3], 0);
}

#[test]
fn a_gradient_base_is_read_at_each_pixel_in_local_space() {
    let gradient = Paint::Linear {
        from: Point::new(20.0, 0.0),
        to: Point::new(60.0, 0.0),
        stops: vec![Stop { pos: 0.0, color: color(0, 0, 0, 255) }, Stop { pos: 1.0, color: color(255, 255, 255, 255) }],
    };
    let plain = solid("p", Path::rect(SQUARE, None), red());
    let mut plain = plain;
    if let NodeKind::Shape(shape) = &mut plain.kind {
        shape.fill = Some(Fill::plain(gradient.clone()));
    }
    let brushed = brushed("b", Path::rect(SQUARE, None), gradient, vec![mark(stage(IDENTITY, &[], &[], 3), 0.0)]);
    let a = render_png(&scene(80.0, 80.0, vec![plain]), 1.0, Backend::Cpu).unwrap();
    let b = render_png(&scene(80.0, 80.0, vec![brushed]), 1.0, Backend::Cpu).unwrap();
    for x in [22, 30, 40, 50, 58] {
        let (pa, pb) = (pixel(&a, x, 40), pixel(&b, x, 40));
        for c in 0..3 {
            assert!(pa[c].abs_diff(pb[c]) <= 2, "x={x}: plain {pa:?} brushed {pb:?}");
        }
    }
    assert!(pixel(&b, 22, 40)[0] < 40 && pixel(&b, 58, 40)[0] > 215, "the ramp runs left to right");
}

#[test]
fn a_later_stage_reads_the_previous_stage_and_coverages_multiply() {
    let first = stage("float4 shade(Mark m) { return float4(0, 0, 1, cover(m.edge, m) * .5); }\n", &[], &[], 1);
    let second = stage(
        "float4 shade(Mark m) { return float4(m.paint.rgb * k, 1); }\n",
        &[("k", UniformKind::Float)],
        &[("k", UniformValue::Float(0.5))],
        2,
    );
    let node = brushed("a", Path::rect(SQUARE, None), Paint::Solid(red()), vec![mark(first, 0.0), mark(second, 0.0)]);
    let png = render_png(&scene(80.0, 80.0, vec![node]), 1.0, Backend::Cpu).unwrap();
    let p = pixel(&png, 40, 40);
    assert!(p[3] == 127 || p[3] == 128, "coverage .5 * 1: {p:?}");
    assert_eq!(p[0], 0, "{p:?}");
    assert!(p[2] >= 126 && p[2] <= 129, "half of the first stage's blue: {p:?}");
}

#[test]
fn reach_lets_a_brush_paint_past_the_geometry_inside_a_layer_too() {
    let body = "float4 shade(Mark m) { return float4(m.paint.rgb, cover(m.edge + 6, m)); }\n";
    let far = brushed("far", Path::rect(SQUARE, None), Paint::Solid(red()), vec![mark(stage(body, &[], &[], 1), 8.0)]);
    let mut layered = far.clone();
    layered.path = "layered".into();
    layered.opacity = 0.5;
    let tight = brushed("tight", Path::rect(SQUARE, None), Paint::Solid(red()), vec![mark(stage(body, &[], &[], 1), 0.0)]);
    let a = render_png(&scene(80.0, 80.0, vec![far]), 1.0, Backend::Cpu).unwrap();
    let b = render_png(&scene(80.0, 80.0, vec![layered]), 1.0, Backend::Cpu).unwrap();
    let c = render_png(&scene(80.0, 80.0, vec![tight]), 1.0, Backend::Cpu).unwrap();
    assert_eq!(pixel(&a, 16, 40)[3], 255, "4 px outside the square is painted with reach");
    assert_eq!(pixel(&a, 11, 40)[3], 0, "but not 9 px outside");
    assert!(pixel(&b, 16, 40)[3] > 100, "a layer does not crop the reach: {:?}", pixel(&b, 16, 40));
    assert_eq!(pixel(&c, 17, 40)[3], 0, "without reach the draw stops at the geometry");
}

#[test]
fn ink_never_lands_past_the_bounds_plus_reach() {
    let body = "float4 shade(Mark m) { return float4(m.paint.rgb, cover(m.edge + 6, m)); }\n";
    for reach in [0.0_f32, 3.0] {
        let node = brushed("a", Path::rect(SQUARE, None), Paint::Solid(red()), vec![mark(stage(body, &[], &[], 1), reach)]);
        let png = render_png(&scene(80.0, 80.0, vec![node]), 1.0, Backend::Cpu).unwrap();
        let (lo, hi) = (20 - reach as i32, 60 + reach as i32);
        for y in 0..80 {
            for x in 0..80 {
                let inside = (lo..hi).contains(&x) && (lo..hi).contains(&y);
                let a = pixel(&png, x, y)[3];
                assert!(inside || a == 0, "reach {reach}: ink at {x},{y} outside {lo}..{hi}");
                if inside {
                    assert_eq!(a, 255, "reach {reach}: {x},{y} inside the bounds is painted");
                }
            }
        }
    }
}

const NORMAL_AS_COLOR: &str = "float4 shade(Mark m) { return float4(m.normal * .5 + .5, 0, cover(m.edge, m)); }\n";

/// The normal `m.normal` encoded at pixel `(x, y)`, each component in -1..1.
fn normal_at(png: &[u8], x: i32, y: i32) -> [f32; 2] {
    let p = pixel(png, x, y);
    assert_eq!(p[3], 255, "({x}, {y}) is inside the shape: {p:?}");
    [p[0], p[1]].map(|c| c as f32 / 255.0 * 2.0 - 1.0)
}

fn assert_normal(png: &[u8], at: (i32, i32), want: [f32; 2]) {
    let got = normal_at(png, at.0, at.1);
    assert!((got[0] - want[0]).abs() < 0.1 && (got[1] - want[1]).abs() < 0.1, "normal at {at:?}: {got:?}, want {want:?}");
}

#[test]
fn the_normal_points_out_of_a_fill_and_is_zero_on_its_medial_axis() {
    let node = brushed("a", Path::circle((40.5, 40.5), 28.0, None), Paint::Solid(red()), vec![mark(stage(NORMAL_AS_COLOR, &[], &[], 1), 0.0)]);
    let png = render_png(&scene(80.0, 80.0, vec![node]), 1.0, Backend::Cpu).unwrap();
    assert_normal(&png, (15, 40), [-1.0, 0.0]);
    assert_normal(&png, (65, 40), [1.0, 0.0]);
    assert_normal(&png, (40, 15), [0.0, -1.0]);
    assert_normal(&png, (40, 65), [0.0, 1.0]);
    let r = std::f32::consts::FRAC_1_SQRT_2;
    assert_normal(&png, (22, 22), [-r, -r]);
    assert_normal(&png, (40, 40), [0.0, 0.0]);
}

#[test]
fn the_normal_is_in_the_local_frame_of_a_rotated_node() {
    let rect = skia_safe::Rect::from_xywh(20.0, 30.0, 40.0, 20.0);
    let mut node = brushed("a", Path::rect(rect, None), Paint::Solid(red()), vec![mark(stage(NORMAL_AS_COLOR, &[], &[], 1), 0.0)]);
    node.transform = skia_safe::Matrix::rotate_deg_pivot(90.0, (40.0, 40.0));
    let png = render_png(&scene(80.0, 80.0, vec![node]), 1.0, Backend::Cpu).unwrap();
    assert_normal(&png, (40, 22), [-1.0, 0.0]);
    assert_normal(&png, (48, 40), [0.0, -1.0]);
    assert_normal(&png, (40, 58), [1.0, 0.0]);
    assert_normal(&png, (32, 40), [0.0, 1.0]);
}

#[test]
fn gpu_brush_render_is_close_to_cpu_and_repeatable() {
    if !gpu_available() {
        return;
    }
    let noisy = "float4 shade(Mark m) {\n  float n = fbm(m.uv / 9 + seeded(m), 3);\n  return float4(m.paint.rgb * (.7 + .5 * n), cover(m.edge, m) * step(.35, n));\n}\n";
    let node = brushed("a", Path::circle((40.0, 40.0), 28.0, None), Paint::Solid(color(90, 140, 200, 255)), vec![mark(stage(noisy, &[], &[], 77), 0.0)]);
    let s = scene(80.0, 80.0, vec![node]);
    let cpu = render_png(&s, 1.0, Backend::Cpu).unwrap();
    let gpu = render_png(&s, 1.0, Backend::Gpu).unwrap();
    let mut worst = 0u8;
    let mut differing = 0usize;
    for y in 0..80 {
        for x in 0..80 {
            let (a, b) = (pixel(&cpu, x, y), pixel(&gpu, x, y));
            let d = (0..4).map(|c| a[c].abs_diff(b[c])).max().unwrap();
            worst = worst.max(d);
            differing += usize::from(d > 0);
        }
    }
    assert!(worst <= 16, "max channel difference {worst}, {differing} pixels differ");
    assert_eq!(gpu, render_png(&s, 1.0, Backend::Gpu).unwrap());
}

#[test]
fn only_a_brushed_stamp_bakes_in_svg() {
    let plain = solid("line.dots.0.c", Path::rect(skia_safe::Rect::from_xywh(-2.0, -2.0, 4.0, 4.0), None), red());
    let dry = stage("float4 shade(Mark m) { return float4(m.paint.rgb, cover(m.edge, m)); }", &[], &[], 3);
    let textured = brushed("line.dots.1.c", Path::rect(skia_safe::Rect::from_xywh(-2.0, -2.0, 4.0, 4.0), None), Paint::Solid(red()), vec![mark(dry, 0.0)]);
    let mut host = solid("line", Path::rect(skia_safe::Rect::from_xywh(0.0, 38.0, 80.0, 4.0), None), color(0, 0, 0, 255));
    with_stamps(
        &mut host,
        vec![
            stamp("line.dots.0", ramose_ir::Matrix::translate((20.0, 40.0)), vec![plain]),
            stamp("line.dots.1", ramose_ir::Matrix::translate((60.0, 40.0)), vec![textured]),
        ],
        vec![],
    );
    let scene = scene(80.0, 80.0, vec![host]);
    let census: Vec<String> = raster_only_features(&scene).iter().map(|u| format!("{} {}", u.node, u.feature)).collect();
    assert_eq!(census, ["line.dots.1.c brush fill"]);
    let svg = render_svg(&scene, ramose_ir::Profile::Svg, 1.0, Backend::Cpu).unwrap();
    assert_eq!(svg.matches("<image").count(), 1, "{svg}");
    assert!(svg.contains("<g id=\"line.dots.0.c\""), "the plain stamp stays a path: {svg}");
    assert!(svg.contains("<g id=\"line.dots.1.c\">"), "the brushed stamp's shape is the baked node: {svg}");
}

#[test]
fn a_brushed_host_bakes_only_its_own_paint_in_svg_and_its_stamps_stay_vector_over_it() {
    let dry = stage("float4 shade(Mark m) { return float4(m.paint.rgb, cover(m.edge, m)); }", &[], &[], 3);
    let mut host = brushed("line", Path::rect(skia_safe::Rect::from_xywh(0.0, 38.0, 80.0, 4.0), None), Paint::Solid(color(0, 0, 0, 255)), vec![mark(dry, 0.0)]);
    let plain = solid("line.dots.0.c", Path::rect(skia_safe::Rect::from_xywh(-2.0, -2.0, 4.0, 4.0), None), red());
    with_stamps(&mut host, vec![stamp("line.dots.0", ramose_ir::Matrix::translate((20.0, 40.0)), vec![plain])], vec![]);
    let scene = scene(80.0, 80.0, vec![host]);
    let census: Vec<String> = raster_only_features(&scene).iter().map(|u| format!("{} {}", u.node, u.feature)).collect();
    assert_eq!(census, ["line brush fill"], "profile:vector still reports the host");
    let svg = render_svg(&scene, ramose_ir::Profile::Svg, 1.0, Backend::Cpu).unwrap();
    assert_eq!(svg.matches("<image").count(), 1, "{svg}");
    let (host_at, image_at, stamp_at) = (svg.find("<g id=\"line\"").unwrap(), svg.find("<image").unwrap(), svg.find("<g id=\"line.dots.0.c\"").unwrap());
    assert!(host_at < image_at && image_at < stamp_at, "the tile sits in the host and the stamp draws over it: {svg}");
}

/// A brush that leaves a gap at x 38..42 and 40% coverage at x 50..54.
const GAPPED: &str = "float4 shade(Mark m) {\n  float k = (m.p.x >= 38 && m.p.x < 42) ? 0 : ((m.p.x >= 50 && m.p.x < 54) ? .4 : 1);\n  return float4(m.paint.rgb, cover(m.edge, m) * k);\n}\n";

fn with_child(mut parent: ramose_ir::Node, child: ramose_ir::Node) -> ramose_ir::Node {
    if let NodeKind::Shape(shape) = &mut parent.kind {
        shape.children.push(child);
    }
    parent
}

fn band() -> ramose_ir::Node {
    solid("band", Path::rect(skia_safe::Rect::from_ltrb(10.0, 30.0, 70.0, 50.0), None), color(0, 0, 255, 255))
}

#[test]
fn children_of_a_brushed_fill_clip_to_its_coverage_not_its_geometry() {
    let parent = brushed("a", Path::rect(SQUARE, None), Paint::Solid(red()), vec![mark(stage(GAPPED, &[], &[], 3), 0.0)]);
    let png = render_png(&scene(80.0, 80.0, vec![with_child(parent, band())]), 1.0, Backend::Cpu).unwrap();
    assert_eq!(pixel(&png, 30, 40), [0, 0, 255, 255], "the child shows over covered fill");
    assert_eq!(pixel(&png, 40, 40)[3], 0, "the child does not fill the gap the brush leaves");
    assert_eq!(pixel(&png, 15, 40)[3], 0, "the child does not spill past the geometry");
    let partial = pixel(&png, 52, 40);
    assert!(
        partial[3].abs_diff(163) <= 2 && partial[0].abs_diff(95) <= 2 && partial[2].abs_diff(160) <= 2,
        "40% blue child over 40% red fill is 64% alpha, 60% of it the fill's red lost to the child: {partial:?}"
    );
}

#[test]
fn a_translucent_brushed_fill_keeps_its_children_opaque() {
    let faint = Paint::Solid(color(255, 0, 0, 51));
    let parent = brushed("a", Path::rect(SQUARE, None), faint, vec![mark(stage(GAPPED, &[], &[], 3), 0.0)]);
    let png = render_png(&scene(80.0, 80.0, vec![with_child(parent, band())]), 1.0, Backend::Cpu).unwrap();
    assert_eq!(pixel(&png, 30, 40), [0, 0, 255, 255], "paint alpha does not dim the child");
    assert_eq!(pixel(&png, 40, 40)[3], 0);
}

#[test]
fn children_of_an_unbrushed_fill_still_clip_to_its_geometry() {
    let parent = with_child(solid("a", Path::rect(SQUARE, None), red()), band());
    let png = render_png(&scene(80.0, 80.0, vec![parent]), 1.0, Backend::Cpu).unwrap();
    assert_eq!(pixel(&png, 40, 40), [0, 0, 255, 255]);
    assert_eq!(pixel(&png, 15, 40)[3], 0);
}

#[test]
fn children_of_a_brushed_fill_still_blend_with_it() {
    let parent = brushed("a", Path::rect(SQUARE, None), Paint::Solid(red()), vec![mark(stage(GAPPED, &[], &[], 3), 0.0)]);
    let mut gray = solid("band", Path::rect(skia_safe::Rect::from_ltrb(10.0, 30.0, 70.0, 50.0), None), color(128, 128, 128, 255));
    gray.blend = ramose_ir::Blend::Multiply;
    let png = render_png(&scene(80.0, 80.0, vec![with_child(parent, gray)]), 1.0, Backend::Cpu).unwrap();
    let px = pixel(&png, 30, 40);
    assert!(px[0].abs_diff(128) <= 2 && px[1] <= 2 && px[2] <= 2 && px[3] == 255, "gray multiplied over red is dark red: {px:?}");
}

#[test]
fn a_stamped_brushed_host_with_children_bakes_its_children_in_svg() {
    let mut host = brushed("a", Path::rect(SQUARE, None), Paint::Solid(red()), vec![mark(stage(GAPPED, &[], &[], 3), 0.0)]);
    let plain = solid("a.dots.0.c", Path::rect(skia_safe::Rect::from_xywh(-2.0, -2.0, 4.0, 4.0), None), red());
    with_stamps(&mut host, vec![stamp("a.dots.0", ramose_ir::Matrix::translate((30.0, 25.0)), vec![plain])], vec![band()]);
    let svg = render_svg(&scene(80.0, 80.0, vec![host]), ramose_ir::Profile::Svg, 1.0, Backend::Cpu).unwrap();
    assert_eq!(svg.matches("<image").count(), 1, "{svg}");
    assert!(!svg.contains("id=\"band\""), "the child clips to the brushed coverage inside the tile, not as an element: {svg}");
}

fn outlined(mut node: ramose_ir::Node, width: f32) -> ramose_ir::Node {
    node.effects.push(effect(EffectOp::Outline { width, color: color(0, 0, 0, 255), sides: Sides::ALL, mask: None, brush: vec![] }));
    node
}

#[test]
fn fx_outline_follows_the_coverage_a_brushed_fill_leaves() {
    let node = outlined(brushed("a", Path::rect(SQUARE, None), Paint::Solid(red()), vec![mark(stage(GAPPED, &[], &[], 3), 0.0)]), 1.0);
    let png = render_png(&scene(80.0, 80.0, vec![node]), 1.0, Backend::Cpu).unwrap();
    assert_eq!(pixel(&png, 40, 40)[3], 0, "the middle of the 4 px gap shows nothing, not the ring's ink");
    assert_eq!(pixel(&png, 38, 40), [0, 0, 0, 255], "the ring runs along the gap's edge");
    assert_eq!(pixel(&png, 30, 40), [255, 0, 0, 255], "covered fill stays the fill");
    assert_eq!(pixel(&png, 19, 40), [0, 0, 0, 255], "the ring still runs around the geometry");
    let faint = pixel(&png, 52, 40);
    assert!(faint[3].abs_diff(102) <= 2, "40% coverage is below one half, so the ring leaves it: {faint:?}");
}

#[test]
fn fx_outline_follows_a_brushed_fill_past_its_geometry_by_its_reach() {
    let body = "float4 shade(Mark m) { return float4(m.paint.rgb, cover(m.edge + 6, m)); }\n";
    let node = outlined(brushed("a", Path::rect(SQUARE, None), Paint::Solid(red()), vec![mark(stage(body, &[], &[], 1), 8.0)]), 2.0);
    let png = render_png(&scene(80.0, 80.0, vec![node]), 1.0, Backend::Cpu).unwrap();
    assert_eq!(pixel(&png, 15, 40), [255, 0, 0, 255], "ink 5 px past the geometry is the fill");
    assert_eq!(pixel(&png, 12, 40), [0, 0, 0, 255], "the ring sits past the reach, not at the geometry");
    assert_eq!(pixel(&png, 11, 40)[3], 0);
}

#[test]
fn fx_outline_on_an_unbrushed_fill_traces_its_geometry() {
    let png = render_png(&scene(80.0, 80.0, vec![outlined(solid("a", Path::rect(SQUARE, None), red()), 3.0)]), 1.0, Backend::Cpu).unwrap();
    assert_eq!(pixel(&png, 40, 40), [255, 0, 0, 255]);
    assert_eq!(pixel(&png, 17, 40), [0, 0, 0, 255]);
    assert_eq!(pixel(&png, 15, 40)[3], 0);
}

#[test]
fn fx_outline_over_a_brushed_fill_is_raster_only() {
    let node = outlined(brushed("a", Path::rect(SQUARE, None), Paint::Solid(red()), vec![mark(stage(GAPPED, &[], &[], 3), 0.0)]), 3.0);
    let mut host = outlined(solid("h", Path::rect(SQUARE, None), red()), 3.0);
    with_stamps(&mut host, vec![stamp("h.dots.0", ramose_ir::Matrix::translate((30.0, 25.0)), vec![node.clone()])], vec![]);
    let scene = scene(80.0, 80.0, vec![node, host]);
    let census: Vec<String> = raster_only_features(&scene).iter().map(|u| format!("{} {}", u.node, u.feature)).collect();
    assert_eq!(census, ["a fx outline brush", "a brush fill", "h fx outline brush", "a fx outline brush", "a brush fill"]);
    assert!(matches!(render_svg(&scene, ramose_ir::Profile::Vector, 1.0, Backend::Cpu), Err(ramose_render::RenderError::NotVector(_))));
    let svg = render_svg(&scene, ramose_ir::Profile::Svg, 1.0, Backend::Cpu).unwrap();
    assert_eq!(svg.matches("<image").count(), 2, "{svg}");
    assert!(!svg.contains("-outline"), "the ring is in the tile, not a path: {svg}");
}

fn ring_stage(body: &str, seed: u32) -> Stage {
    let program = Program::assemble(MainKind::Ribbon, &[], body);
    ramose_brush::check(&program).unwrap_or_else(|e| panic!("{e:?}"));
    Stage { program: Arc::new(program), uniforms: vec![], seed }
}

fn ring_outlined(mut node: ramose_ir::Node, width: f32, body: &str, reach: f32) -> ramose_ir::Node {
    let brush = vec![RibbonMark { stage: ring_stage(body, 5), reach }];
    node.effects.push(effect(EffectOp::Outline { width, color: color(0, 0, 0, 255), sides: Sides::ALL, mask: None, brush }));
    node
}

const INNER_HALF: &str = "float4 shade(Mark m) { return float4(m.paint.rgb, cover(m.edge, m) * step(0, m.d)); }\n";
const OUTER_HALF: &str = "float4 shade(Mark m) { return float4(m.paint.rgb, cover(m.edge, m) * step(0, -m.d)); }\n";

/// A 60-unit square with a 20-unit hole, so the ring runs around the outside and inside the hole.
fn holed() -> Path {
    let mut path = skia_safe::PathBuilder::new();
    path.add_rect(skia_safe::Rect::from_ltrb(10.0, 10.0, 70.0, 70.0), None, None);
    path.add_rect(skia_safe::Rect::from_ltrb(30.0, 30.0, 50.0, 50.0), None, None);
    path.set_fill_type(skia_safe::PathFillType::EvenOdd);
    path.snapshot()
}

#[test]
fn a_brushed_outline_runs_with_the_traced_area_on_its_right_around_the_outside_and_each_hole() {
    let inner = render_png(&scene(80.0, 80.0, vec![ring_outlined(solid("a", holed(), red()), 4.0, INNER_HALF, 0.0)]), 1.0, Backend::Cpu).unwrap();
    let outer = render_png(&scene(80.0, 80.0, vec![ring_outlined(solid("a", holed(), red()), 4.0, OUTER_HALF, 0.0)]), 1.0, Backend::Cpu).unwrap();
    for (x, where_) in [(8, "outside the square"), (32, "inside the hole")] {
        assert_eq!(pixel(&inner, x, 40)[3], 0, "the inner half (m.d > 0) lies under the fill, so nothing shows {where_}");
        assert_eq!(pixel(&outer, x, 40), [0, 0, 0, 255], "the outer half (m.d < 0) shows {where_}");
    }
    assert_eq!(pixel(&outer, 20, 40), [255, 0, 0, 255], "the fill stays the fill");
    assert_eq!(pixel(&outer, 40, 40)[3], 0, "the middle of the hole is open");
}

#[test]
fn a_brushed_outline_renders_the_same_bytes_twice_and_is_raster_only() {
    let body = "float4 shade(Mark m) { return float4(m.paint.rgb, cover(m.edge + 3 * hash12(floor(m.uv / 4)), m)); }\n";
    let s = scene(80.0, 80.0, vec![ring_outlined(solid("a", Path::rect(SQUARE, None), red()), 3.0, body, 3.0)]);
    let png = render_png(&s, 1.0, Backend::Cpu).unwrap();
    assert_eq!(png, render_png(&s, 1.0, Backend::Cpu).unwrap());
    let census: Vec<String> = raster_only_features(&s).iter().map(|u| format!("{} {}", u.node, u.feature)).collect();
    assert_eq!(census, ["a brush outline"]);
    assert!(matches!(render_svg(&s, ramose_ir::Profile::Vector, 1.0, Backend::Cpu), Err(ramose_render::RenderError::NotVector(_))));
    let svg = render_svg(&s, ramose_ir::Profile::Svg, 1.0, Backend::Cpu).unwrap();
    assert_eq!(svg.matches("<image").count(), 1, "{svg}");
}
