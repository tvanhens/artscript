//! Brush declarations, overrides, the `brush:` key, seeds, std/brushes, and the diagnostics
//! each failure lands on.

use std::collections::BTreeMap;

use artscript_ir::{AreaMark, Ribbon, Scene, UniformValue};
use artscript_syntax::Diagnostic;

use crate::{Project, evaluate, plan};

use super::{diag, project, run_with, shape, show};

const DRY: &str = "\
brush dry strength:.5 scale:1 dir:0
  sksl \"\"\"
    float4 shade(Mark m) {
      float n = fbm(m.uv / (20 * scale) + seeded(m), 2);
      return float4(m.paint.rgb * (.8 + .4 * n), cover(m.edge, m) * step(strength * .5, n));
    }
  \"\"\"
";

const LIB: (&str, &str) = ("lib/brushes.art", DRY);

fn marks<'a>(s: &'a Scene, path: &str) -> &'a [AreaMark] {
    &shape(s, path).fill.as_ref().expect("fill").marks
}

fn seeds(s: &Scene, path: &str) -> Vec<u32> {
    marks(s, path).iter().map(|m| m.stage.seed).collect()
}

fn ribbon<'a>(s: &'a Scene, path: &str) -> &'a Ribbon {
    shape(s, path).stroke.as_ref().expect("stroke").ribbon.as_ref().expect("ribbon")
}

fn col(d: &Diagnostic) -> (u32, u32) {
    (d.span.line, d.span.col)
}

fn ok(src: &str, libs: &[(&str, &str)]) -> Scene {
    let ev = run_with(src, libs);
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
    ev.scene.unwrap()
}

fn errors(src: &str, libs: &[(&str, &str)]) -> Vec<Diagnostic> {
    run_with(src, libs).diagnostics
}

fn first(src: &str, libs: &[(&str, &str)], code: &str) -> Diagnostic {
    diag(&run_with(src, libs), code).clone()
}

#[test]
fn a_brush_declares_a_value_and_a_fill_gets_one_mark_per_stage() {
    let s = ok(
        "@target t.png 100x100\nuse brushes { dry }\nrect 40x40 at:30,50 fill:#ff0000 brush:dry name:a\nrect 40x40 at:70,50 fill:#00ff00 brush:[dry dry(strength:.9 dir:90 reach:3)] name:b\n",
        &[LIB],
    );
    let a = marks(&s, "a");
    assert_eq!(a.len(), 1);
    assert_eq!(a[0].stage.uniforms[0], ("strength".into(), UniformValue::Float(0.5)));
    assert_eq!(a[0].dir_deg, 0.0);
    assert_eq!(a[0].reach, 0.0);
    let b = marks(&s, "b");
    assert_eq!(b.len(), 2);
    assert_eq!(b[0].stage.uniforms[0].1, UniformValue::Float(0.5));
    assert_eq!(b[1].stage.uniforms[0].1, UniformValue::Float(0.9));
    assert_eq!(b[1].dir_deg, 90.0);
    assert!(b[1].cos_sin[0].abs() < 1e-6 && (b[1].cos_sin[1] - 1.0).abs() < 1e-6);
    assert_eq!(b[1].reach, 3.0);
    assert_eq!(shape(&s, "b").mark_reach(), 3.0);
    assert!(std::sync::Arc::ptr_eq(&b[0].stage.program, &b[1].stage.program), "one program per declaration");
    let sb = seeds(&s, "b");
    assert_ne!(sb[0], sb[1], "stages decorrelate");
    assert_ne!(seeds(&s, "a")[0], sb[0], "nodes decorrelate");
}

#[test]
fn seed_on_the_node_pins_its_brush_seeds_and_nothing_else() {
    let src = |seed_a: &str| {
        format!(
            "@target t.png 100x100\nuse brushes {{ dry }}\nrect 40x40 at:30,50 fill:#ff0000 brush:dry {seed_a} name:a\nrect 40x40 at:70,50 fill:#00ff00 brush:dry name:b\n"
        )
    };
    let base = ok(&src(""), &[LIB]);
    let pinned = ok(&src("seed:7"), &[LIB]);
    assert_ne!(seeds(&base, "a"), seeds(&pinned, "a"));
    assert_eq!(seeds(&base, "b"), seeds(&pinned, "b"), "the other node keeps its seed");
    let twice = ok(&src("seed:7"), &[LIB]);
    assert_eq!(seeds(&pinned, "a"), seeds(&twice, "a"));
}

#[test]
fn brush_call_errors_are_e0361_at_the_call_line() {
    let cases = [
        ("brush:dry(strenth:.9)", "has no parameter `strenth`", Some("did you mean `strength:`?")),
        ("brush:dry(strength:#ff0000)", "expects a number, got a color", None),
        ("brush:dry(.9)", "takes named arguments only", None),
        ("brush:dry(on:left)", "takes `fill` or `stroke`", None),
        ("brush:dry(reach:-1)", "must be 0 or more", None),
    ];
    for (key, msg, fix) in cases {
        let ds = errors(&format!("@target t.png 100x100\nuse brushes {{ dry }}\nrect 40x40 at:50,50 fill:#ff0000 {key} name:a\n"), &[LIB]);
        let d = ds.iter().find(|d| d.code == "E0361").unwrap_or_else(|| panic!("{key}: {}", show(&ds)));
        assert!(d.message.contains(msg), "{key}: {d}");
        assert_eq!(d.span.line, 3, "{key}: {d}");
        assert!(d.notes.iter().any(|n| n.starts_with("parameters: strength, scale")), "{d}");
        if let Some(fix) = fix {
            assert_eq!(d.fix.as_deref(), Some(fix), "{d}");
        }
        assert_eq!(ds.len(), 1, "{key}: {}", show(&ds));
    }
}

#[test]
fn a_word_that_is_not_a_brush_is_e0364_at_the_key() {
    let ev = run_with("@target t.png 100x100\nuse brushes { dry }\nrect 40x40 at:50,50 fill:#ff0000 brush:drty name:a\n", &[LIB]);
    let d = diag(&ev, "E0364");
    assert!(d.message.contains("`drty` is not a brush"), "{d}");
    assert_eq!(d.fix.as_deref(), Some("did you mean `brush:dry`?"), "{d}");
    assert_eq!((d.span.line, d.span.col), (3, 34), "{d}");
    assert!(d.notes.iter().any(|n| n == "brushes in scope: dry"), "{d}");
    assert_eq!(ev.diagnostics.len(), 1, "{}", show(&ev.diagnostics));
    let d = first("@target t.png 100x100\nrect 40x40 at:50,50 fill:#ff0000 brush:3 name:a\n", &[], "E0364");
    assert!(d.message.contains("got a number"), "{d}");
    let d = first("@target t.png 100x100\nrect 40x40 at:50,50 fill:#ff0000 brush:[] name:a\n", &[], "E0364");
    assert!(d.message.contains("no brushes"), "{d}");
}

#[test]
fn a_brush_on_a_stroke_makes_a_ribbon_and_on_fill_narrows_it() {
    let s = ok("@target t.png 100x100\nuse brushes { dry }\ncurve 0,0 50,20 90,0 stroke:#000000,4 brush:dry name:a\n", &[LIB]);
    let sh = shape(&s, "a");
    assert!(sh.fill.is_none());
    let r = ribbon(&s, "a");
    assert_eq!(r.marks.len(), 1);
    assert_eq!(r.contours.len(), 1);
    let c = &r.contours[0];
    assert!(!c.closed);
    assert_eq!(c.start, 0.0);
    assert!((c.len - r.len).abs() < 1e-4 && r.len > 90.0, "len {}", r.len);
    assert_eq!(c.knots.first().unwrap().s, 0.0);
    assert_eq!(c.knots.last().unwrap().s, c.len);
    assert!(c.knots.iter().all(|k| k.half_width == 2.0 && k.pressure == 1.0 && k.angle == 0.0 && k.tilt == 0.0));
    assert_eq!(sh.stroke.as_ref().unwrap().width, 4.0);
    assert!(!sh.geometry.is_last_contour_closed(), "the geometry stays the open spine");
    let s = ok("@target t.png 100x100\nuse brushes { dry }\nrect 40x40 at:50,50 fill:#ff0000 stroke:#000000,2 brush:dry(on:fill) name:a\n", &[LIB]);
    assert_eq!(marks(&s, "a").len(), 1);
    assert!(shape(&s, "a").stroke.as_ref().unwrap().ribbon.is_none());
    let s = ok("@target t.png 100x100\nuse brushes { dry }\nrect 40x40 at:50,50 fill:#ff0000 stroke:#000000,2 brush:dry(on:stroke) name:a\n", &[LIB]);
    assert_eq!(marks(&s, "a").len(), 0);
    assert!(ribbon(&s, "a").contours[0].closed, "a rect outline wraps");
    let s = ok("@target t.png 100x100\nuse brushes { dry }\nlayer back brush:dry(on:fill)\n  curve 0,0 50,20 90,0 stroke:#000000,2 name:line\n", &[LIB]);
    assert!(shape(&s, "line").fill.is_none(), "an inherited brush with no matching slot does nothing");
    assert!(shape(&s, "line").stroke.as_ref().unwrap().ribbon.is_none());
}

#[test]
fn a_fill_and_a_stroke_on_one_node_get_two_marks_seeded_apart() {
    let s = ok("@target t.png 100x100\nuse brushes { dry }\nrect 40x40 at:50,50 fill:#ff0000 stroke:#000000,3 brush:dry name:a\nrect 40x40 at:50,50 fill:#ff0000 stroke:#000000,3 brush:dry seed:7 name:b\n", &[LIB]);
    let fill_seed = seeds(&s, "a")[0];
    let stroke_seed = ribbon(&s, "a").marks[0].stage.seed;
    assert_ne!(fill_seed, stroke_seed);
    assert_eq!(seeds(&s, "b")[0], ribbon(&s, "b").marks[0].stage.seed, "seed: pins both");
    assert_ne!(seeds(&s, "b")[0], fill_seed);
}

#[test]
fn a_brushed_tapered_stroke_is_a_ribbon_whose_half_width_follows_the_knots() {
    let s = ok("@target t.png 100x100\nuse brushes { dry }\ncurve 0,0 10,0 40,0 stroke:#000000,[2 6 3] taper:.3 brush:dry name:a\n", &[LIB]);
    let r = ribbon(&s, "a");
    assert_eq!(r.taper, [0.3, 0.3]);
    let c = &r.contours[0];
    assert!((c.len - 40.0).abs() < 1e-3, "{}", c.len);
    let hw: Vec<(f32, f32)> = c.knots.iter().map(|k| ((k.s * 10.0).round() / 10.0, k.half_width)).collect();
    assert_eq!(hw, vec![(0.0, 1.0), (10.0, 3.0), (40.0, 1.5)], "widths sit on the knots by chord fraction");
    assert_eq!(shape(&s, "a").stroke.as_ref().unwrap().width, 6.0);
    assert_eq!(shape(&s, "a").geometry.count_points(), 7, "three knots, two cubics: not an outline");
    let s = ok("@target t.png 100x100\nuse brushes { dry }\ncurve 0,0 10,0 40,0 stroke:#000000,[2 6] brush:dry name:a\n", &[LIB]);
    let hw: Vec<(f32, f32)> = ribbon(&s, "a").contours[0].knots.iter().map(|k| ((k.s * 10.0).round() / 10.0, k.half_width)).collect();
    assert_eq!(hw, vec![(0.0, 1.0), (10.0, 1.5), (40.0, 3.0)], "two widths on three knots spread evenly; the middle knot holds the lerp");
}

#[test]
fn channels_sit_on_the_knots_or_spread_evenly_and_velocity_follows_knot_spacing() {
    let s = ok(
        "@target t.png 100x100\nuse brushes { dry }\ncurve 0,0 10,0 40,0 stroke:#000000,4 brush:dry pressure:[.2 1 .5] angle:45 tilt:[0 1] name:a\n",
        &[LIB],
    );
    let c = &ribbon(&s, "a").contours[0];
    let at = |s: f32| c.at(s);
    assert_eq!(at(0.0).pressure, 0.2);
    assert!((at(10.0).pressure - 1.0).abs() < 1e-3, "{:?}", at(10.0));
    assert!((at(5.0).pressure - 0.6).abs() < 1e-3, "linear between knots: {:?}", at(5.0));
    assert_eq!(at(40.0).pressure, 0.5);
    assert!(c.knots.iter().all(|k| k.angle == 45.0), "a number is constant along the path");
    assert!((at(20.0).tilt - 0.5).abs() < 1e-3, "two values spread over the whole length");
    assert!((at(0.0).velocity - 0.5).abs() < 1e-3, "chords 10 and 30, mean 20");
    assert!((at(10.0).velocity - 1.0).abs() < 1e-3);
    assert!((at(40.0).velocity - 1.5).abs() < 1e-3);
    let s = ok("@target t.png 100x100\nuse brushes { dry }\nrect 40x40 at:50,50 stroke:#000000,4 brush:dry pressure:[.2 1 .5] name:a\n", &[LIB]);
    let c = &ribbon(&s, "a").contours[0];
    assert!((c.at(c.len / 2.0).pressure - 1.0).abs() < 1e-3, "no knots: the list spreads by arc length");
    assert!(c.knots.iter().all(|k| k.velocity == 1.0));
}

#[test]
fn a_channel_key_without_a_brushed_stroke_is_e0362_at_the_key_and_an_inherited_one_does_nothing() {
    let d = first("@target t.png 100x100\nrect 40x40 at:50,50 fill:#ff0000 pressure:[.2 1] name:a\n", &[], "E0362");
    assert!(d.message.contains("`pressure:` has no effect without a brushed stroke"), "{d}");
    assert_eq!(col(&d), (2, 34), "{d}");
    let d = first("@target t.png 100x100\ncurve 0,0 50,20 90,0 stroke:#000000,2 tilt:.5 name:a\n", &[], "E0362");
    assert!(d.message.contains("`tilt:`"), "{d}");
    assert!(d.notes.iter().any(|n| n.contains("no brush")), "{d}");
    ok("@target t.png 100x100\nuse brushes { dry }\nlayer back angle:30\n  rect 40x40 at:50,50 fill:#ff0000 brush:dry name:a\n", &[LIB]);
    let s = ok(
        "@target t.png 100x100\nuse brushes { dry }\nlayer ink brush:dry pressure:[.3 1 .5]\n  curve 0,0 50,20 90,0 stroke:#000000,2 name:line\n  circle 20 at:50,50 fill:#ff0000 name:dot\nstyle inky stroke:#000000,4 pressure:.5\ncurve 0,90 50,80 90,90 style:inky name:plain\n",
        &[LIB],
    );
    assert_eq!(ribbon(&s, "line").contours[0].knots[0].pressure, 0.3, "an inherited channel reaches a brushed stroke");
    assert!(shape(&s, "plain").stroke.as_ref().unwrap().ribbon.is_none(), "and does nothing where there is none");
    let d = first("@target t.png 100x100\nuse brushes { dry }\ncurve 0,0 50,20 90,0 stroke:#000000,2 brush:dry pressure:#ff0000 name:a\n", &[LIB], "E0320");
    assert!(d.message.contains("`pressure:` takes a number or a list of numbers, got a color"), "{d}");
    assert_eq!(col(&d), (3, 49), "{d}");
}

#[test]
fn a_dash_on_a_brushed_stroke_is_e0362_at_the_dash() {
    let d = first("@target t.png 100x100\nuse brushes { dry }\ncurve 0,0 50,20 90,0 stroke:#000000,2 brush:dry dash:4,2 name:a\n", &[LIB], "E0362");
    assert!(d.message.contains("`dash:` on a brushed stroke"), "{d}");
    assert_eq!(col(&d), (3, 49), "{d}");
    let s = ok("@target t.png 100x100\nuse brushes { dry }\nrect 40x40 at:50,50 fill:#ff0000 stroke:#000000,2 brush:dry(on:fill) dash:4,2 name:a\n", &[LIB]);
    assert_eq!(shape(&s, "a").stroke.as_ref().unwrap().dash, vec![4.0, 2.0], "the plain stroke keeps its dash");
}

#[test]
fn cap_and_join_on_a_brushed_stroke_are_e0362_at_the_key_and_inherited_ones_do_nothing() {
    let d = first("@target t.png 100x100\nuse brushes { dry }\ncurve 0,0 50,20 90,0 stroke:#000000,2 brush:dry cap:butt name:a\n", &[LIB], "E0362");
    assert!(d.message.contains("`cap:` on a brushed stroke"), "{d}");
    assert_eq!(col(&d), (3, 49), "{d}");
    let d = first("@target t.png 100x100\nuse brushes { dry }\ncurve 0,0 50,20 90,0 stroke:#000000,2 join:bevel brush:dry name:a\n", &[LIB], "E0362");
    assert!(d.message.contains("`join:` on a brushed stroke"), "{d}");
    let s = ok("@target t.png 100x100\nuse brushes { dry }\nlayer back cap:butt join:bevel brush:dry\n  curve 0,0 50,20 90,0 stroke:#000000,2 name:a\n", &[LIB]);
    assert_eq!(ribbon(&s, "a").marks.len(), 1, "inherited cap and join do nothing");
    let s = ok("@target t.png 100x100\nuse brushes { dry }\nrect 40x40 at:50,50 fill:#ff0000 stroke:#000000,2 brush:dry(on:fill) cap:butt name:a\n", &[LIB]);
    assert!(shape(&s, "a").stroke.is_some(), "a plain stroke keeps cap");
    let s = ok("@target t.png 100x100\ncurve 0,0 50,20 90,0 stroke:#000000,2 cap:butt join:bevel name:a\n", &[]);
    assert!(shape(&s, "a").stroke.is_some());
}

#[test]
fn an_inherited_brush_reaches_a_stroke_only_node_as_a_ribbon() {
    let s = ok("@target t.png 100x100\nuse brushes { dry }\nlayer back brush:dry\n  curve 0,0 50,20 90,0 stroke:#000000,2 name:line\n", &[LIB]);
    assert_eq!(ribbon(&s, "line").marks.len(), 1);
    assert!(shape(&s, "line").fill.is_none());
}

#[test]
fn a_brush_with_no_paint_to_brush_and_blur_with_a_brush_are_e0362() {
    let d = first("@target t.png 100x100\nuse brushes { dry }\nrect 40x40 at:50,50 fill:none brush:dry name:a\n", &[LIB], "E0362");
    assert!(d.message.contains("no paint to brush"), "{d}");
    let d = first("@target t.png 100x100\nuse brushes { dry }\ncurve 0,0 50,20 90,0 stroke:#000000,2 blur:3 brush:dry name:a\n", &[LIB], "E0362");
    assert!(d.message.contains("`blur:`"), "{d}");
    let d = first("@target t.png 100x100\nuse brushes { dry }\nrect 40x40 at:50,50 fill:#ff0000 blur:4 brush:dry name:a\n", &[LIB], "E0362");
    assert!(d.message.contains("`blur:`"), "{d}");
    let s = ok("@target t.png 100x100\nuse brushes { dry }\nlayer back brush:dry\n  rect 40x40 at:50,50 fill:none name:a\n", &[LIB]);
    assert!(shape(&s, "a").fill.is_none());
}

#[test]
fn each_stage_on_the_nodes_own_line_that_reaches_no_slot_is_e0362() {
    let ds = errors("@target t.png 100x100\nuse brushes { dry }\nrect 40x40 at:50,50 fill:#ff0000 brush:[dry dry(on:stroke) dry(on:stroke)] name:a\n", &[LIB]);
    let stages: Vec<&Diagnostic> = ds.iter().filter(|d| d.code == "E0362").collect();
    assert_eq!(stages.len(), 2, "{}", show(&ds));
    assert!(stages[0].message.contains("stage 2 of `brush:` (`dry`)"), "{}", stages[0]);
    assert!(stages[1].message.contains("stage 3"), "{}", stages[1]);
    assert!(stages.iter().all(|d| d.span.line == 3), "{}", show(&ds));
    assert_eq!(ds.len(), 2, "{}", show(&ds));
}

#[test]
fn brush_none_clears_an_inherited_brush_and_a_style_passes_one() {
    let s = ok(
        "@target t.png 100x100\nuse brushes { dry }\nstyle rough brush:dry\nlayer back brush:dry\n  rect 20x20 at:20,50 fill:#ff0000 name:a\n  rect 20x20 at:50,50 fill:#ff0000 brush:none name:b\n  rect 20x20 at:80,50 fill:#ff0000 style:rough name:c\n",
        &[LIB],
    );
    assert_eq!(marks(&s, "a").len(), 1);
    assert_eq!(marks(&s, "b").len(), 0);
    assert_eq!(marks(&s, "c").len(), 1);
}

#[test]
fn a_def_takes_a_brush_parameter_and_recalls_it_with_overrides() {
    let stone = (
        "lib/stone.art",
        "use brushes { dry }\ndef stone size:20 tone:#ff0000 brush:dry\n  rect size fill:tone brush:brush name:base\n    rect size*.4 fill:#00ff00 brush:brush(scale:.5) name:core\n",
    );
    let s = ok(
        "@target t.png 100x100\nuse brushes { dry }\nuse stone\nstone at:30,50 name:a\nstone at:70,50 brush:dry(strength:.9) name:b\nlayer back brush:dry(dir:45)\n  stone at:50,80 brush:none name:c\n",
        &[LIB, stone],
    );
    assert_eq!(marks(&s, "a.base")[0].stage.uniforms[0].1, UniformValue::Float(0.5));
    assert_eq!(marks(&s, "a.core")[0].stage.uniforms[1].1, UniformValue::Float(0.5));
    assert_eq!(marks(&s, "b.base")[0].stage.uniforms[0].1, UniformValue::Float(0.9));
    assert_eq!(marks(&s, "b.core")[0].stage.uniforms[0].1, UniformValue::Float(0.9));
    assert_eq!(marks(&s, "c.base").len(), 0, "brush:none passed as the param clears it");
    let d = first("@target t.png 100x100\nuse stone\nstone at:30,50 tool:3 name:a\n", &[LIB, stone], "E0321");
    assert!(d.message.contains("no parameter `tool`"), "{d}");
}

#[test]
fn a_call_on_a_brush_parameter_is_checked_against_its_default_for_every_instance() {
    let stone = (
        "lib/stone.art",
        "use brushes { dry }\ndef stone size:20 brush:dry\n  rect size fill:#ff0000 brush:brush(scal:.5) name:base\n",
    );
    for instances in ["stone at:30,50 brush:none name:a\n", "stone at:30,50 brush:none name:a\nstone at:70,50 name:b\n"] {
        let ds = errors(&format!("@target t.png 100x100\nuse stone\n{instances}"), &[LIB, stone]);
        let d = ds.iter().find(|d| d.code == "E0361").unwrap_or_else(|| panic!("{instances}: {}", show(&ds)));
        assert!(d.message.contains("no parameter `scal`"), "{d}");
        assert_eq!((d.file.as_str(), d.span.line), ("lib/stone.art", 3), "{d}");
    }
    let lib = ("lib/stone.art", "use brushes { dry }\ndef stone size:20 brush:dry\n  rect size fill:#ff0000 brush:brush(strength:nope) name:base\n");
    let ds = errors("@target t.png 100x100\nuse stone\nstone at:30,50 brush:none name:a\n", &[LIB, lib]);
    assert!(ds.iter().any(|d| d.code == "E0361" && d.message.contains("expects a number, got a word")), "the argument is evaluated and checked: {}", show(&ds));
    let lib = ("lib/stone.art", "def stone size:20 tone:none\n  rect size fill:tone(a:1) name:base\n");
    let ds = errors("@target t.png 100x100\nuse stone\nstone at:30,50 name:a\n", &[lib]);
    assert!(ds.iter().any(|d| d.message.contains("unknown function `tone`")), "{}", show(&ds));
}

#[test]
fn a_layer_brush_reaches_a_def_body_through_the_inherited_style() {
    let box_lib = ("lib/boxy.art", "def boxy size:20\n  rect size fill:#ff0000 name:face\n");
    let s = ok("@target t.png 100x100\nuse brushes { dry }\nuse boxy\nlayer back brush:dry\n  boxy at:50,50 name:a\nboxy at:20,20 name:b\n", &[LIB, box_lib]);
    assert_eq!(marks(&s, "a.face").len(), 1);
    assert_eq!(marks(&s, "b.face").len(), 0);
}

#[test]
fn brush_on_a_boolean_operand_is_e0321() {
    let d = first("@target t.png 100x100\nuse brushes { dry }\nsubtract name:s\n  rect 40x40 at:50,50 brush:dry\n  circle 10 at:50,50\n", &[LIB], "E0321");
    assert!(d.message.contains("brush"), "{d}");
}

#[test]
fn sksl_errors_land_on_the_body_line_and_column_in_the_library() {
    let bad = (
        "lib/bad.art",
        "brush bad strength:.5\n  sksl \"\"\"\n    float4 shade(Mark m) {\n      float n = fbmx(m.uv, 3);\n      return float4(m.paint.rgb, n);\n    }\n  \"\"\"\n",
    );
    let ds = errors("@target t.png 100x100\nuse bad { bad }\nrect 40x40 at:50,50 fill:#ff0000 brush:bad name:a\n", &[bad]);
    let sksl: Vec<&Diagnostic> = ds.iter().filter(|d| d.code == "E0360").collect();
    assert_eq!(sksl.len(), 2, "{}", show(&ds));
    assert_eq!(sksl[0].file, "lib/bad.art");
    assert_eq!((sksl[0].span.line, sksl[0].span.col), (4, 17), "{}", sksl[0]);
    assert!(sksl[0].message.contains("unknown identifier 'fbmx'"), "{}", sksl[0]);
    assert_eq!((sksl[1].span.line, sksl[1].span.col), (5, 34), "{}", sksl[1]);
    assert!(ds.iter().all(|d| d.code == "E0360"), "the use site stays quiet: {}", show(&ds));
}

#[test]
fn calling_a_lost_brush_stays_quiet_at_every_use_site() {
    let bad = (
        "lib/house.art",
        "brush bad a:1\n  sksl \"\"\"\n    float4 shade(Mark m) { return float4(m.paint.rgb, zz); }\n  \"\"\"\nlet wall = bad(a:2)\n",
    );
    let ds = errors(
        "@target t.png 100x100\nuse house { bad wall }\nrect 20x20 at:20,20 fill:#ff0000 brush:bad name:a\nrect 20x20 at:50,20 fill:#ff0000 brush:bad(a:3) name:b\nrect 20x20 at:80,20 fill:#ff0000 brush:wall name:c\nlet w = bad(a:2)\n",
        &[bad],
    );
    assert_eq!(ds.len(), 1, "only the SkSL error: {}", show(&ds));
    assert_eq!((ds[0].code.as_str(), ds[0].file.as_str()), ("E0360", "lib/house.art"), "{}", ds[0]);
}

#[test]
fn a_frame_error_lands_on_the_sksl_keyword_and_an_unused_brush_is_still_checked() {
    let bad = ("lib/bad.art", "brush bad strength:.5\n  sksl \"\"\"\n    float f(float x) { return x; }\n  \"\"\"\n");
    let ds = errors("@target t.png 100x100\nuse bad\nrect 40x40 at:50,50 fill:#ff0000 name:a\n", &[bad]);
    assert_eq!(ds.len(), 1, "{}", show(&ds));
    let d = &ds[0];
    assert_eq!((d.code.as_str(), d.file.as_str(), d.span.line, d.span.col), ("E0360", "lib/bad.art", 2, 3), "{d}");
    assert!(d.message.contains("shade"), "{d}");
}

#[test]
fn malformed_declarations_are_e0363_at_their_key() {
    let cases = [
        ("brush b strength:.5\n", 1, "no `sksl` block"),
        ("brush b edge:.5\n  sksl \"\"\"\n    float4 shade(Mark m) { return m.paint; }\n  \"\"\"\n", 1, "cannot be named `edge`"),
        ("brush b u_x:.5\n  sksl \"\"\"\n    float4 shade(Mark m) { return m.paint; }\n  \"\"\"\n", 1, "cannot be named `u_x`"),
        ("brush b word:left\n  sksl \"\"\"\n    float4 shade(Mark m) { return m.paint; }\n  \"\"\"\n", 1, "must default to a number, color or vector"),
        ("brush b strength:.5\n  sksl \"\"\"\n    float4 shade(Mark m) { float u_k = 1; return m.paint; }\n  \"\"\"\n", 3, "`u_k` is reserved"),
    ];
    for (lib, line, msg) in cases {
        let ds = errors("@target t.png 100x100\nuse b\nrect 40x40 at:50,50 fill:#ff0000 brush:b name:a\n", &[("lib/b.art", lib)]);
        let d = ds.iter().find(|d| d.code == "E0363").unwrap_or_else(|| panic!("{lib}: {}", show(&ds)));
        assert!(d.message.contains(msg), "{lib}: {d}");
        assert_eq!((d.file.as_str(), d.span.line), ("lib/b.art", line), "{lib}: {d}");
        assert!(ds.iter().all(|d| d.code == "E0363"), "{lib}: {}", show(&ds));
    }
}

#[test]
fn a_reserved_name_lands_on_its_own_line_and_column_and_the_compile_errors_still_show() {
    let lib = (
        "lib/b.art",
        "brush b strength:.5\n  sksl \"\"\"\n    // u_prev is the base\n    float4 shade(Mark m) {\n      float n = fbmx(m.uv, 3);\n      float q = u_seed;\n      return float4(m.paint.rgb, n);\n    }\n  \"\"\"\n",
    );
    let ds = errors("@target t.png 100x100\nuse b\nrect 40x40 at:50,50 fill:#ff0000 brush:b name:a\n", &[lib]);
    let reserved: Vec<&Diagnostic> = ds.iter().filter(|d| d.code == "E0363").collect();
    assert_eq!(reserved.len(), 1, "a comment is not a use: {}", show(&ds));
    assert!(reserved[0].message.contains("`u_seed`"), "{}", reserved[0]);
    assert_eq!((reserved[0].span.line, reserved[0].span.col), (6, 17), "{}", reserved[0]);
    assert!(ds.iter().any(|d| d.code == "E0360" && d.message.contains("fbmx") && d.span.line == 5), "{}", show(&ds));
}

#[test]
fn a_param_default_that_breaks_the_uniform_line_lands_on_the_param() {
    let lib = ("lib/b.art", "brush b strength:.5 float:1\n  sksl \"\"\"\n    float4 shade(Mark m) { return m.paint; }\n  \"\"\"\n");
    let ds = errors("@target t.png 100x100\nuse b\nrect 40x40 at:50,50 fill:#ff0000 brush:b name:a\n", &[lib]);
    let d = ds.iter().find(|d| d.code == "E0360").unwrap_or_else(|| panic!("{}", show(&ds)));
    assert_eq!((d.span.line, d.span.col), (1, 21), "{d}");
}

#[test]
fn std_brushes_import_like_any_library_and_a_shadowing_file_is_refused() {
    let s = ok(
        "@target t.png 100x100\nuse std/brushes { dry chip }\nlet wall = dry(strength:.55 dir:-8)\nrect 40x40 at:30,50 fill:#ff0000 brush:wall name:a\nrect 40x40 at:70,50 fill:#00ff00 brush:[chip dry] name:b\n",
        &[],
    );
    assert_eq!(marks(&s, "a")[0].dir_deg, -8.0);
    assert_eq!(marks(&s, "b").len(), 2);
    let s = ok("@target t.png 100x100\nuse std/brushes as b\nrect 40x40 at:30,50 fill:#ff0000 brush:b.chip name:a\n", &[]);
    assert_eq!(marks(&s, "a").len(), 1);
    let d = first("@target t.png 100x100\nuse std/nope { x }\n", &[], "E0210");
    assert!(d.notes.iter().any(|n| n.contains("std/brushes")), "{d}");
    assert!(d.fix.as_deref().unwrap().contains("artscript std"), "{d}");
    for file in ["lib/std/brushes.art", "std/brushes.art", "lib/std/other.art"] {
        let files: BTreeMap<String, String> = [("art/t.art".to_string(), String::new()), (file.to_string(), String::new())].into();
        let err = Project::from_sources(super::MANIFEST, files).err().unwrap_or_else(|| panic!("{file} loaded"));
        assert_eq!((err.code.as_str(), err.file.as_str()), ("E0213", file), "{err}");
    }
    assert!(project(&[("art/t.art", "")]).library_names().contains(&"std/brushes".to_string()));
}

#[test]
fn std_files_are_formatted_lint_clean_and_their_brushes_compile() {
    let linted = "[project]\nout = \"out\"\nlib = [\"lib\"]\n";
    for (path, src) in crate::STD {
        let formatted = artscript_syntax::format_source(src).unwrap_or_else(|e| panic!("{path}: {e:?}"));
        assert_eq!(&formatted, src, "{path} is not fmt-clean");
        let names: Vec<String> = artscript_syntax::parse(src)
            .unwrap()
            .decls()
            .filter_map(|d| match &d.kind {
                artscript_syntax::ast::DeclKind::Brush { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
        assert!(!names.is_empty(), "{path} declares no brush");
        let mut art = format!("@target t.png 100x100\nuse {path} {{ {} }}\npalette pal\n  color red #ff0000\n", names.join(" "));
        for (i, n) in names.iter().enumerate() {
            art.push_str(&format!("rect 20x20 at:{},50 fill:pal.red brush:{n} name:n{i}\n", 20 + i * 30));
        }
        let files: BTreeMap<String, String> = [("art/t.art".to_string(), art)].into();
        let p = Project::from_sources(linted, files).unwrap();
        let plan = plan(&p);
        assert!(plan.diagnostics.is_empty(), "{path}: {}", show(&plan.diagnostics));
        let ev = evaluate(&p, &plan.jobs[0]);
        assert!(ev.diagnostics.is_empty(), "{path}: {}", show(&ev.diagnostics));
        assert!(crate::dependencies(&p, "art/t.art").contains(&format!("{path}.art")));
    }
}

#[test]
fn an_unnamed_brushed_node_gets_the_positional_seed_lint() {
    let linted = "[project]\nout = \"out\"\nlib = [\"lib\"]\n\n[lint]\nraw-color = \"off\"\n";
    let files: BTreeMap<String, String> = [(
        "art/t.art".to_string(),
        "@target t.png 100x100\nuse std/brushes { dry }\nrect 40x40 at:50,50 fill:#ff0000 brush:dry\n".to_string(),
    )]
    .into();
    let p = Project::from_sources(linted, files).unwrap();
    let ev = evaluate(&p, &plan(&p).jobs[0]);
    assert!(ev.diagnostics.iter().any(|d| d.code == "L0104"), "{}", show(&ev.diagnostics));
}

#[test]
fn a_same_line_sksl_body_maps_later_lines_to_their_own_column() {
    let scene = "@target t.png 100x100\nuse b\nrect 40x40 at:50,50 fill:#ff0000 brush:b name:a\n";
    let at = |lib: &str, code: &str, needle: &str| {
        let ds = errors(scene, &[("lib/b.art", lib)]);
        let d = ds.iter().find(|d| d.code == code && d.message.contains(needle)).unwrap_or_else(|| panic!("{lib}: {}", show(&ds)));
        (d.span.line, d.span.col)
    };
    let same = "brush b\n  sksl \"\"\"float4 shade(Mark m) {\n    return float4(m.paint.rgb, bogus);\n  }\"\"\"\n";
    assert_eq!(at(same, "E0360", "bogus"), (3, 32));
    let same = "brush b\n  sksl \"\"\"float4 shade(Mark m) {\n    float u_k = 1;\n    return m.paint;\n  }\"\"\"\n";
    assert_eq!(at(same, "E0363", "u_k"), (3, 11));
    let first = "brush b\n  sksl \"\"\"float4 shade(Mark m) { float u_k = 1; return m.paint; }\"\"\"\n";
    assert_eq!(at(first, "E0363", "u_k"), (2, 40));
    let newline = "brush b\n  sksl \"\"\"\n    float4 shade(Mark m) {\n      return float4(m.paint.rgb, bogus);\n    }\n  \"\"\"\n";
    assert_eq!(at(newline, "E0360", "bogus"), (4, 34));
}

#[test]
fn the_frame_owns_only_u_names_so_a_body_may_define_finish() {
    let lib = (
        "lib/b.art",
        "brush b\n  sksl \"\"\"\n  half4 finish(Mark m, float2 c) { return half4(1); }\n  float4 shade(Mark m) { return float4(m.paint.rgb, cover(m.edge, m)); }\n  \"\"\"\n",
    );
    ok("@target t.png 100x100\nuse b\nrect 40x40 at:50,50 fill:#ff0000 brush:b name:a\n", &[lib]);
    let lib = (
        "lib/b.art",
        "brush b\n  sksl \"\"\"\n  half4 u_finish(Mark m, float2 c) { return half4(1); }\n  float4 shade(Mark m) { return float4(m.paint.rgb, cover(m.edge, m)); }\n  \"\"\"\n",
    );
    let d = first("@target t.png 100x100\nuse b\nrect 40x40 at:50,50 fill:#ff0000 brush:b name:a\n", &[lib], "E0363");
    assert!(d.message.contains("`u_finish`"), "{d}");
    assert_eq!(col(&d), (3, 9), "{d}");
}
