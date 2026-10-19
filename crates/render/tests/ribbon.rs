//! Brushed strokes: the stage chain over the ribbon field, the channels and the half width
//! reaching the body, caps and closed contours, reach, determinism, and GPU closeness.

mod common;

use std::sync::Arc;

use artscript_brush::{MainKind, Program, UniformKind, UniformValue};
use artscript_ir::{Cap, Fill, Join, Knot, NodeKind, Paint, Path, Ribbon, RibbonContour, RibbonMark, Stage, Stroke};
use artscript_render::{Backend, gpu_available, render_png};
use common::{color, pixel, red, scene, solid};

fn stage(body: &str, params: &[(&str, UniformKind)], uniforms: &[(&str, UniformValue)], seed: u32) -> Stage {
    let params: Vec<(String, UniformKind)> = params.iter().map(|(n, k)| (n.to_string(), *k)).collect();
    let program = Program::assemble(MainKind::Ribbon, &params, body);
    artscript_brush::check(&program).unwrap_or_else(|e| panic!("{e:?}"));
    Stage {
        program: Arc::new(program),
        uniforms: uniforms.iter().map(|(n, v)| (n.to_string(), *v)).collect(),
        seed,
    }
}

fn knot(s: f32, half_width: f32, pressure: f32, angle: f32, tilt: f32) -> Knot {
    Knot { s, half_width, pressure, angle, tilt, velocity: 1.0 }
}

/// A stroke-only node along `path`, one open contour of length `len` with the given knots.
fn ribbon(path: Path, len: f32, closed: bool, knots: Vec<Knot>, paint: Paint, stages: Vec<Stage>, reach: f32) -> artscript_ir::Node {
    let mut node = solid("a", path, red());
    if let NodeKind::Shape(shape) = &mut node.kind {
        shape.fill = None;
        shape.stroke = Some(Stroke {
            paint,
            width: knots.iter().map(|k| k.half_width * 2.0).fold(0.0, f32::max),
            cap: Cap::Round,
            join: Join::Round,
            dash: vec![],
            ink: None,
            ribbon: Some(Ribbon {
                marks: stages.into_iter().map(|stage| RibbonMark { stage, reach }).collect(),
                contours: vec![RibbonContour { len, closed, knots }],
                taper: [0.0, 0.0],
            }),
        });
    }
    node
}

fn line(x0: f32, y: f32, x1: f32) -> Path {
    let mut b = skia_safe::PathBuilder::new();
    b.move_to((x0, y));
    b.line_to((x1, y));
    b.detach()
}

const IDENTITY: &str = "float4 shade(Mark m) { return float4(m.paint.rgb, cover(m.edge, m)); }\n";
const INK: [u8; 4] = [20, 40, 200, 255];

fn ink() -> Paint {
    Paint::Solid(color(INK[0], INK[1], INK[2], INK[3]))
}

#[test]
fn a_ribbon_covers_its_half_width_with_round_caps_and_renders_the_same_bytes_twice() {
    let node = ribbon(line(10.0, 40.0, 70.0), 60.0, false, vec![knot(0.0, 4.0, 1.0, 0.0, 0.0), knot(60.0, 4.0, 1.0, 0.0, 0.0)], ink(), vec![stage(IDENTITY, &[], &[], 3)], 0.0);
    let s = scene(80.0, 80.0, vec![node]);
    let png = render_png(&s, 1.0, Backend::Cpu).unwrap();
    assert_eq!(png, render_png(&s, 1.0, Backend::Cpu).unwrap());
    assert_eq!(pixel(&png, 40, 40), INK, "on the spine");
    assert_eq!(pixel(&png, 40, 36), INK, "3.5 px above, inside the half width of 4");
    assert_eq!(pixel(&png, 40, 35)[3], 0, "4.5 px above");
    assert_eq!(pixel(&png, 40, 44)[3], 0, "4.5 px below");
    assert_eq!(pixel(&png, 7, 40), INK, "2.5 px before the start, inside the round cap");
    assert_eq!(pixel(&png, 5, 40)[3], 0, "4.5 px before the start");
    assert_eq!(pixel(&png, 7, 35)[3], 0, "2.5 before and 4.5 up is 5.1 from the cap center");
    assert!(pixel(&png, 7, 36)[3] < 80, "2.5 before and 3.5 up is 4.3 from it: the antialiased rim: {:?}", pixel(&png, 7, 36));
    assert!(pixel(&png, 7, 37)[3] > 200, "2.5 before and 2.5 up is 3.5 from it: {:?}", pixel(&png, 7, 37));
    assert_eq!(pixel(&png, 74, 40)[3], 0, "4.5 px past the end");
    assert_eq!(pixel(&png, 72, 40), INK, "2.5 px past the end");
    assert!(pixel(&png, 73, 37)[3] < 80, "3.5 past and 2.5 up is 4.3 from the end: a square cap would be solid here: {:?}", pixel(&png, 73, 37));
}

#[test]
fn the_channels_s_and_half_width_reach_the_body_from_the_knots() {
    let body = "float4 shade(Mark m) { return float4(m.pressure, m.angle / 360, m.u, cover(m.edge, m)); }\n";
    let knots = vec![knot(0.0, 1.0, 0.0, 90.0, 0.0), knot(60.0, 4.0, 1.0, 90.0, 0.0)];
    let node = ribbon(line(10.0, 40.0, 70.0), 60.0, false, knots, ink(), vec![stage(body, &[], &[], 1)], 0.0);
    let png = render_png(&scene(80.0, 80.0, vec![node]), 1.0, Backend::Cpu).unwrap();
    let quarter = pixel(&png, 25, 40);
    let three_quarters = pixel(&png, 55, 40);
    assert!((quarter[0] as i32 - 64).abs() <= 2, "pressure .25 at s 15: {quarter:?}");
    assert!((three_quarters[0] as i32 - 191).abs() <= 2, "pressure .75 at s 45: {three_quarters:?}");
    assert!((quarter[1] as i32 - 64).abs() <= 2, "angle 90 everywhere: {quarter:?}");
    assert!((quarter[2] as i32 - 64).abs() <= 2 && (three_quarters[2] as i32 - 191).abs() <= 2, "u is s over len: {quarter:?} {three_quarters:?}");
    assert_eq!(pixel(&png, 25, 42)[3], 0, "half width 1.75 at s 15: 2.5 px off is outside");
    assert_ne!(pixel(&png, 55, 42)[3], 0, "half width 3.25 at s 45: 2.5 px off is inside");
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
    let knots = vec![knot(0.0, 4.0, 1.0, 0.0, 0.0), knot(60.0, 4.0, 1.0, 0.0, 0.0)];
    let node = ribbon(line(10.0, 40.0, 70.0), 60.0, false, knots, ink(), vec![first, second], 0.0);
    let png = render_png(&scene(80.0, 80.0, vec![node]), 1.0, Backend::Cpu).unwrap();
    let p = pixel(&png, 40, 40);
    assert!(p[0] <= 2 && p[1] <= 2, "{p:?}");
    assert!((p[2] as i32 - 128).abs() <= 2, "half of stage 1's blue: {p:?}");
    assert!((p[3] as i32 - 128).abs() <= 2, "stage 1's .5 coverage times stage 2's 1: {p:?}");
}

#[test]
fn reach_lets_the_body_paint_past_the_edge_and_the_cut_follows_it() {
    let body = "float4 shade(Mark m) { return float4(m.paint.rgb, cover(m.edge + 3, m)); }\n";
    let knots = vec![knot(0.0, 2.0, 1.0, 0.0, 0.0), knot(60.0, 2.0, 1.0, 0.0, 0.0)];
    let with = ribbon(line(10.0, 40.0, 70.0), 60.0, false, knots.clone(), ink(), vec![stage(body, &[], &[], 1)], 3.0);
    let without = ribbon(line(10.0, 40.0, 70.0), 60.0, false, knots, ink(), vec![stage(body, &[], &[], 1)], 0.0);
    let with = render_png(&scene(80.0, 80.0, vec![with]), 1.0, Backend::Cpu).unwrap();
    let without = render_png(&scene(80.0, 80.0, vec![without]), 1.0, Backend::Cpu).unwrap();
    assert_eq!(pixel(&with, 40, 36), INK, "3.5 px off a 2 px half width is within reach 3");
    assert_eq!(pixel(&with, 40, 34)[3], 0, "5.5 px off is past reach");
    assert_eq!(pixel(&without, 40, 36)[3], 0, "without reach the field is cut at the half width plus a pixel");
}

#[test]
fn a_closed_contour_outlines_a_rect_and_leaves_its_middle_empty() {
    let rect = skia_safe::Rect::from_xywh(20.0, 20.0, 40.0, 40.0);
    let knots = vec![knot(0.0, 3.0, 1.0, 0.0, 0.0), knot(160.0, 3.0, 1.0, 0.0, 0.0)];
    let node = ribbon(Path::rect(rect, None), 160.0, true, knots, ink(), vec![stage(IDENTITY, &[], &[], 1)], 0.0);
    let png = render_png(&scene(80.0, 80.0, vec![node]), 1.0, Backend::Cpu).unwrap();
    assert_eq!(pixel(&png, 40, 20), INK, "on the top edge");
    assert_eq!(pixel(&png, 20, 40), INK, "on the left edge");
    assert_eq!(pixel(&png, 20, 20), INK, "the start corner is closed, not capped");
    assert_eq!(pixel(&png, 18, 18), INK, "2.1 px outside the corner, inside the round join");
    assert_eq!(pixel(&png, 17, 17)[3], 0, "3.5 px outside the corner");
    assert_eq!(pixel(&png, 40, 40)[3], 0, "the middle");
    assert_eq!(pixel(&png, 40, 25)[3], 0, "4.5 px inside the top edge");
}

#[test]
fn a_brushed_fill_and_a_brushed_stroke_share_one_node() {
    let area = {
        let program = Program::assemble(MainKind::Area, &[], IDENTITY);
        artscript_brush::check(&program).unwrap();
        Stage { program: Arc::new(program), uniforms: vec![], seed: 5 }
    };
    let rect = skia_safe::Rect::from_xywh(20.0, 20.0, 40.0, 40.0);
    let knots = vec![knot(0.0, 3.0, 1.0, 0.0, 0.0), knot(160.0, 3.0, 1.0, 0.0, 0.0)];
    let mut node = ribbon(Path::rect(rect, None), 160.0, true, knots, ink(), vec![stage(IDENTITY, &[], &[], 1)], 0.0);
    if let NodeKind::Shape(shape) = &mut node.kind {
        shape.fill = Some(Fill { paint: Paint::Solid(red()), marks: vec![artscript_ir::AreaMark { stage: area, dir_deg: 0.0, cos_sin: [1.0, 0.0], reach: 0.0 }] });
    }
    let png = render_png(&scene(80.0, 80.0, vec![node]), 1.0, Backend::Cpu).unwrap();
    assert_eq!(pixel(&png, 40, 40), [255, 0, 0, 255], "the brushed fill in the middle");
    assert_eq!(pixel(&png, 40, 20), INK, "the brushed stroke on the edge, over the fill");
}

#[test]
fn gpu_output_is_within_a_few_levels_of_cpu_and_repeatable() {
    if !gpu_available() {
        return;
    }
    let body = "float4 shade(Mark m) {\n  float n = fbm(float2(m.s / 9, m.v * 3) + seeded(m), 3);\n  return float4(m.paint.rgb * (.7 + .6 * n), cover(m.edge - n * 2, m) * (.5 + .5 * m.pressure));\n}\n";
    let mut b = skia_safe::PathBuilder::new();
    b.move_to((10.0, 20.0));
    b.cubic_to((40.0, 70.0), (50.0, 10.0), (70.0, 60.0));
    let path = b.detach();
    let len = skia_safe::ContourMeasureIter::new(&path, false, None).next().unwrap().length();
    let knots = vec![knot(0.0, 2.0, 0.3, 0.0, 0.0), knot(len * 0.5, 6.0, 1.0, 0.0, 0.0), knot(len, 3.0, 0.6, 0.0, 0.0)];
    let node = ribbon(path, len, false, knots, ink(), vec![stage(body, &[], &[], 9)], 2.0);
    let s = scene(80.0, 80.0, vec![node]);
    let cpu = render_png(&s, 1.0, Backend::Cpu).unwrap();
    let gpu = render_png(&s, 1.0, Backend::Gpu).unwrap();
    assert_eq!(gpu, render_png(&s, 1.0, Backend::Gpu).unwrap(), "two GPU builds match");
    let mut worst = 0u8;
    let mut inked = 0;
    for y in 0..80 {
        for x in 0..80 {
            let (c, g) = (pixel(&cpu, x, y), pixel(&gpu, x, y));
            inked += (c[3] > 0) as i32;
            for i in 0..4 {
                worst = worst.max(c[i].abs_diff(g[i]));
            }
        }
    }
    assert!(inked > 400, "the ribbon drew: {inked} px");
    assert!(worst <= 16, "worst channel difference {worst}");
}

const NORMAL_AS_COLOR: &str = "float4 shade(Mark m) { return float4(m.normal * .5 + .5, 0, cover(m.edge, m)); }\n";

fn assert_normal(png: &[u8], at: (i32, i32), want: [f32; 2]) {
    let p = pixel(png, at.0, at.1);
    assert_eq!(p[3], 255, "({}, {}) is inside the stroke: {p:?}", at.0, at.1);
    let got = [p[0], p[1]].map(|c| c as f32 / 255.0 * 2.0 - 1.0);
    assert!((got[0] - want[0]).abs() < 0.1 && (got[1] - want[1]).abs() < 0.1, "normal at {at:?}: {got:?}, want {want:?}");
}

#[test]
fn the_normal_points_away_from_the_spine_whatever_the_taper() {
    for hw_end in [6.0, 10.0] {
        let knots = vec![knot(0.0, 6.0, 1.0, 0.0, 0.0), knot(60.0, hw_end, 1.0, 0.0, 0.0)];
        let node = ribbon(line(10.0, 40.5, 70.0), 60.0, false, knots, ink(), vec![stage(NORMAL_AS_COLOR, &[], &[], 1)], 0.0);
        let png = render_png(&scene(80.0, 80.0, vec![node]), 1.0, Backend::Cpu).unwrap();
        assert_normal(&png, (40, 36), [0.0, -1.0]);
        assert_normal(&png, (40, 45), [0.0, 1.0]);
        assert_normal(&png, (72, 40), [1.0, 0.0]);
        assert_normal(&png, (7, 40), [-1.0, 0.0]);
        assert_normal(&png, (40, 40), [0.0, 0.0]);
    }
}

#[test]
fn the_normal_is_in_the_local_frame_of_a_rotated_stroke() {
    let knots = vec![knot(0.0, 6.0, 1.0, 0.0, 0.0), knot(60.0, 6.0, 1.0, 0.0, 0.0)];
    let mut node = ribbon(line(10.0, 40.5, 70.0), 60.0, false, knots, ink(), vec![stage(NORMAL_AS_COLOR, &[], &[], 1)], 0.0);
    node.transform = skia_safe::Matrix::rotate_deg_pivot(90.0, (40.0, 40.0));
    let png = render_png(&scene(80.0, 80.0, vec![node]), 1.0, Backend::Cpu).unwrap();
    assert_normal(&png, (43, 40), [0.0, -1.0]);
    assert_normal(&png, (36, 40), [0.0, 1.0]);
    assert_normal(&png, (39, 73), [1.0, 0.0]);
}

#[test]
fn a_thin_leg_of_the_spine_does_not_carve_a_wide_leg_it_passes_near() {
    let mut b = skia_safe::PathBuilder::new();
    b.move_to((10.0, 30.0));
    b.line_to((190.0, 30.0));
    b.line_to((190.0, 36.0));
    b.line_to((10.0, 36.0));
    let knots = vec![knot(0.0, 12.0, 1.0, 0.0, 0.0), knot(180.0, 12.0, 1.0, 0.0, 0.0), knot(186.0, 1.0, 1.0, 0.0, 0.0), knot(366.0, 1.0, 1.0, 0.0, 0.0)];
    let node = ribbon(b.detach(), 366.0, false, knots, ink(), vec![stage(IDENTITY, &[], &[], 1)], 0.0);
    let png = render_png(&scene(200.0, 60.0, vec![node]), 1.0, Backend::Cpu).unwrap();
    for y in 19..42 {
        assert_eq!(pixel(&png, 100, y), INK, "y {y}: inside the wide leg's half width of 12, whichever leg is nearer");
    }
    assert_eq!(pixel(&png, 100, 17)[3], 0, "12.5 px above the wide leg");
    assert_eq!(pixel(&png, 100, 43)[3], 0, "12.5 px below the wide leg");
}

#[test]
fn s_stays_exact_80000_px_along_a_stroke() {
    let body = "float4 shade(Mark m) { return float4(fract(m.s / 128), 0, 0, cover(m.edge, m)); }\n";
    let knots = vec![knot(0.0, 3.0, 1.0, 0.0, 0.0), knot(80128.0, 3.0, 1.0, 0.0, 0.0)];
    let node = ribbon(line(-80000.0, 20.0, 128.0), 80128.0, false, knots, ink(), vec![stage(body, &[], &[], 1)], 0.0);
    let png = render_png(&scene(128.0, 40.0, vec![node]), 1.0, Backend::Cpu).unwrap();
    for x in 0..127 {
        let want = (x as f32 + 0.5) / 128.0 * 255.0;
        let got = pixel(&png, x, 20)[0];
        assert!((got as f32 - want).abs() <= 1.5, "x {x}: s is 80000 + {x}.5, so red is {want}: got {got}");
    }
}

#[test]
fn a_strip_that_holds_more_length_than_the_field_encodes_is_an_error() {
    let mut b = skia_safe::PathBuilder::new();
    b.move_to((4.0, 20.0));
    for i in 0..600 {
        b.line_to((if i % 2 == 0 { 124.0 } else { 4.0 }, 20.0 + (i % 7) as f32));
    }
    let path = b.detach();
    let len = skia_safe::ContourMeasureIter::new(&path, false, None).next().unwrap().length();
    let knots = vec![knot(0.0, 1.0, 1.0, 0.0, 0.0), knot(len, 1.0, 1.0, 0.0, 0.0)];
    let node = ribbon(path, len, false, knots, ink(), vec![stage(IDENTITY, &[], &[], 1)], 0.0);
    let err = render_png(&scene(128.0, 40.0, vec![node]), 1.0, Backend::Cpu).unwrap_err();
    let message = format!("{err:?}");
    assert!(message.contains("more than the ribbon field encodes"), "{message}");
}
