//! Brushed fills: the stage chain over the signed distance field, the base paint reaching
//! `m.paint`, reach growing the drawn area, determinism, and GPU closeness.

mod common;

use std::sync::Arc;

use artscript_brush::{MainKind, Program, UniformKind, UniformValue};
use artscript_ir::{AreaMark, Fill, NodeKind, Paint, Path, Point, Stage, Stop};
use artscript_render::{Backend, gpu_available, raster_only_features, render_png, render_svg};
use common::{color, pixel, red, scene, solid, stamp, with_stamps};

fn stage(body: &str, params: &[(&str, UniformKind)], uniforms: &[(&str, UniformValue)], seed: u32) -> Stage {
    let params: Vec<(String, UniformKind)> = params.iter().map(|(n, k)| (n.to_string(), *k)).collect();
    let program = Program::assemble(MainKind::Area, &params, body);
    artscript_brush::check(&program).unwrap_or_else(|e| panic!("{e:?}"));
    Stage {
        program: Arc::new(program),
        uniforms: uniforms.iter().map(|(n, v)| (n.to_string(), *v)).collect(),
        seed,
    }
}

fn mark(stage: Stage, reach: f32) -> AreaMark {
    AreaMark { stage, dir_deg: 0.0, cos_sin: [1.0, 0.0], reach }
}

fn brushed(path: &str, geometry: Path, paint: Paint, marks: Vec<AreaMark>) -> artscript_ir::Node {
    let mut node = solid(path, geometry, red());
    if let NodeKind::Shape(shape) = &mut node.kind {
        shape.fill = Some(Fill { paint, marks });
    }
    node
}

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
            stamp("line.dots.0", artscript_ir::Matrix::translate((20.0, 40.0)), vec![plain]),
            stamp("line.dots.1", artscript_ir::Matrix::translate((60.0, 40.0)), vec![textured]),
        ],
        vec![],
    );
    let scene = scene(80.0, 80.0, vec![host]);
    let census: Vec<String> = raster_only_features(&scene).iter().map(|u| format!("{} {}", u.node, u.feature)).collect();
    assert_eq!(census, ["line.dots.1.c brush fill"]);
    let svg = render_svg(&scene, artscript_ir::Profile::Svg, 1.0, Backend::Cpu).unwrap();
    assert_eq!(svg.matches("<image").count(), 1, "{svg}");
    assert!(svg.contains("<g id=\"line.dots.0.c\""), "the plain stamp stays a path: {svg}");
    assert!(svg.contains("<g id=\"line.dots.1.c\">"), "the brushed stamp's shape is the baked node: {svg}");
}

#[test]
fn a_brushed_host_bakes_only_its_own_paint_in_svg_and_its_stamps_stay_vector_over_it() {
    let dry = stage("float4 shade(Mark m) { return float4(m.paint.rgb, cover(m.edge, m)); }", &[], &[], 3);
    let mut host = brushed("line", Path::rect(skia_safe::Rect::from_xywh(0.0, 38.0, 80.0, 4.0), None), Paint::Solid(color(0, 0, 0, 255)), vec![mark(dry, 0.0)]);
    let plain = solid("line.dots.0.c", Path::rect(skia_safe::Rect::from_xywh(-2.0, -2.0, 4.0, 4.0), None), red());
    with_stamps(&mut host, vec![stamp("line.dots.0", artscript_ir::Matrix::translate((20.0, 40.0)), vec![plain])], vec![]);
    let scene = scene(80.0, 80.0, vec![host]);
    let census: Vec<String> = raster_only_features(&scene).iter().map(|u| format!("{} {}", u.node, u.feature)).collect();
    assert_eq!(census, ["line brush fill"], "profile:vector still reports the host");
    let svg = render_svg(&scene, artscript_ir::Profile::Svg, 1.0, Backend::Cpu).unwrap();
    assert_eq!(svg.matches("<image").count(), 1, "{svg}");
    let (host_at, image_at, stamp_at) = (svg.find("<g id=\"line\"").unwrap(), svg.find("<image").unwrap(), svg.find("<g id=\"line.dots.0.c\"").unwrap());
    assert!(host_at < image_at && image_at < stamp_at, "the tile sits in the host and the stamp draws over it: {svg}");
}

/// A brush that leaves a gap at x 38..42 and 40% coverage at x 50..54.
const GAPPED: &str = "float4 shade(Mark m) {\n  float k = (m.p.x >= 38 && m.p.x < 42) ? 0 : ((m.p.x >= 50 && m.p.x < 54) ? .4 : 1);\n  return float4(m.paint.rgb, cover(m.edge, m) * k);\n}\n";

fn with_child(mut parent: artscript_ir::Node, child: artscript_ir::Node) -> artscript_ir::Node {
    if let NodeKind::Shape(shape) = &mut parent.kind {
        shape.children.push(child);
    }
    parent
}

fn band() -> artscript_ir::Node {
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
    assert!((100..=104).contains(&partial[3]) && partial[2] > 250, "40% coverage shows 40% of the child: {partial:?}");
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
