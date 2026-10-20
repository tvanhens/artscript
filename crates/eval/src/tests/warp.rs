use ramose_ir::Scene;

use crate::{Evaluation, Project, evaluate, plan};

use super::{at, diag, node, run_with, scene, shape, show};

fn close(got: (f32, f32), want: (f64, f64), tol: f64) {
    let (dx, dy) = (f64::from(got.0) - want.0, f64::from(got.1) - want.1);
    assert!(dx.abs() < tol && dy.abs() < tol, "{got:?} != {want:?}");
}

fn probe(s: &Scene, name: &str) -> (f32, f32) {
    at(node(s, name))
}

fn sin(x: f64) -> f64 {
    x.sin()
}

const QUAD: &str = "corners(84,22 140,22 150,54 74,54)";

#[test]
fn corners_pin_the_ink_box_where_the_points_are_written_whatever_the_node_s_own_keys() {
    let s = scene(&format!(
        "@target t.png 200x100\n\
         rect 40x24 stroke:#000000,2 warp:{QUAD} name:side\n\
         circle 1 at:@side.left,@side.top name:tl\n\
         circle 1 at:@side.right,@side.bottom name:br\n\
         rect 40x24 at:60,0 rot:30 scale:2 stroke:#000000,2 warp:{QUAD} name:moved\n\
         circle 1 at:@moved.left,@moved.top name:mtl\n\
         circle 1 at:@moved.right,@moved.bottom name:mbr\n\
         group at:50,0 name:box\n  rect 40x24 at:0,0 warp:{QUAD} name:inner\n\
         circle 1 at:@box.left,@box.top name:gtl\n"
    ));
    close(probe(&s, "tl"), (74.0, 22.0), 1e-3);
    close(probe(&s, "br"), (150.0, 54.0), 1e-3);
    close(probe(&s, "mtl"), (74.0, 22.0), 1e-3);
    close(probe(&s, "mbr"), (150.0, 54.0), 1e-3);
    close(probe(&s, "gtl"), (124.0, 22.0), 1e-3);
}

#[test]
fn a_cylinder_compresses_the_box_along_the_bending_axis_only() {
    let s = scene(
        "@target t.png 200x100\n\
         group at:100,50 warp:cylinder(40) name:label\n  rect 96x40 stroke:#000000,2\n\
         group at:100,50 warp:cylinder(40 axis:x) name:flag\n  rect 96x40 stroke:#000000,2\n\
         circle 1 at:@label.w,@label.h name:lp\n\
         circle 1 at:@flag.w,@flag.h name:fp\n",
    );
    close(probe(&s, "lp"), (80.0 * sin(49.0 / 40.0), 42.0), 1e-3);
    close(probe(&s, "fp"), (98.0, 80.0 * sin(21.0 / 40.0)), 1e-3);
}

#[test]
fn a_stack_applies_left_to_right_and_each_stage_maps_the_box_before_it() {
    let s = scene(
        "@target t.png 400x400\n\
         rect 60x220 at:0,0 warp:[cylinder(40) corners(300,100 360,108 358,330 298,322)] name:spine\n\
         circle 1 at:@spine.left,@spine.top name:sl\n\
         circle 1 at:@spine.right,@spine.bottom name:sb\n\
         rect 40x24 at:0,0 warp:[corners(-40,-50 40,-50 40,50 -40,50) cylinder(40)] name:bent\n\
         circle 1 at:@bent.w,@bent.h name:bp\n",
    );
    close(probe(&s, "sl"), (298.0, 100.0), 1e-3);
    close(probe(&s, "sb"), (360.0, 330.0), 1e-3);
    close(probe(&s, "bp"), (80.0 * sin(1.0), 100.0), 1e-3);
}

#[test]
fn parts_read_flat_inside_the_body_and_warped_from_outside_after_it_closes() {
    let ev = run_with(
        "@target t.png 200x100\n\
         rect 10x10 at:90,50 name:ref\n\
         group at:100,50 warp:cylinder(40) name:label\n\
         \x20 rect 96x40\n\
         \x20 rect 10x10 at:40,0 name:title\n\
         \x20 circle 1 at:@title.cx,@title.cy name:inner\n\
         \x20 circle 1 at:@ref.cx,0 name:outside\n\
         \x20 anchor tip 40,0\n\
         circle 1 at:@title.cx,@title.cy name:probe\n\
         circle 1 at:@tip name:on_tip\n",
        &[],
    );
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
    let s = ev.scene.as_ref().unwrap();
    close(probe(s, "inner"), (40.0, 0.0), 1e-4);
    close(probe(s, "outside"), (-10.0, 0.0), 1e-4);
    let warped_cx = 100.0 + 20.0 * (sin(35.0 / 40.0) + sin(45.0 / 40.0));
    close(probe(s, "probe"), (warped_cx, 50.0), 1e-3);
    let tip = (100.0 + 40.0 * sin(1.0), 50.0);
    close(probe(s, "on_tip"), tip, 1e-3);
    let rig = ev
        .rig
        .anchors
        .get("tip")
        .unwrap_or_else(|| panic!("{:?}", ev.rig.anchors));
    assert!(
        (rig.0 - tip.0).abs() < 1e-3 && (rig.1 - tip.1).abs() < 1e-3,
        "{rig:?}"
    );
}

#[test]
fn inside_a_warped_node_clips_to_the_warped_silhouette() {
    let s = scene(&format!(
        "@target t.png 200x100\n\
         rect 40x24 at:0,0 warp:{QUAD} name:pinned\n\
         inside @pinned name:clip\n  rect 400x400 at:100,50\n\
         circle 1 at:@clip.w,@clip.h name:cp\n"
    ));
    let clip = node(&s, "clip").clip.as_ref().expect("inside sets a clip");
    let b = clip.path.compute_tight_bounds();
    let want = [74.0, 22.0, 150.0, 54.0];
    for (got, want) in [b.left, b.top, b.right, b.bottom].iter().zip(want) {
        assert!((got - want).abs() < 1e-2, "{b:?}");
    }
    close(probe(&s, "cp"), (76.0, 32.0), 1e-2);
}

#[test]
fn a_grid_inside_a_warped_group_and_a_warped_grid_both_shift_their_copies_under_the_warp() {
    let s = scene(
        "@target t.png 200x100\n\
         group at:100,50 warp:cylinder(40) name:g\n  grid 2x1 gap:4\n    rect 10x10 name:cell\n\
         grid 2x1 gap:4 at:100,50 warp:cylinder(40) name:h\n  rect 10x10 name:hcell\n\
         circle 1 at:@cell.1.cx,@hcell.1.cx name:p\n",
    );
    let cx = 100.0 + 20.0 * (sin(2.0 / 40.0) + sin(12.0 / 40.0));
    close(probe(&s, "p"), (cx, cx), 1e-3);
}

#[test]
fn none_clears_a_warp_and_a_let_a_def_default_a_patch_and_an_anim_hold_one() {
    let s = scene(
        "@target t.png 100x100\n\
         let bend = cylinder(60)\n\
         def card bend:cylinder(60)\n  rect 40x24 at:0,0 warp:bend name:face\n\
         def tag\n  rect 40x24 at:0,0 warp:cylinder(20) name:label\n\
         rect 40x24 at:0,0 warp:none name:plain\n\
         rect 40x24 at:0,0 warp:bend name:bound\n\
         card name:c\n\
         card bend:none name:d\n\
         tag name:t\n  .label warp:none\n\
         tag name:u\n\
         circle 1 at:@plain.w,@bound.w name:p1\n\
         circle 1 at:@c.face.w,@d.face.w name:p2\n\
         circle 1 at:@t.label.w,@u.label.w name:p3\n",
    );
    let bent = 120.0 * sin(20.0 / 60.0);
    close(probe(&s, "p1"), (40.0, bent), 1e-3);
    close(probe(&s, "p2"), (bent, 40.0), 1e-3);
    close(probe(&s, "p3"), (40.0, 40.0 * sin(1.0)), 1e-3);
    let s = scene(
        "@target t.png 100x100 t:.5s\n\
         anim bendy dur:1s\n  .a warp:cylinder(60)\n\
         rect 40x24 at:0,0 name:a\n\
         play bendy\n\
         circle 1 at:@a.w,0 name:p\n",
    );
    close(probe(&s, "p"), (bent, 0.0), 1e-3);
}

#[test]
fn a_bad_quad_radius_or_arity_is_reported_at_the_line() {
    let cases = [
        (
            "warp:corners(0,0 10,0 0,10 10,10)",
            "E0320",
            "the corners cross",
        ),
        (
            "warp:corners(0,0 5,0 10,0 0,10)",
            "E0320",
            "three lie on one line",
        ),
        ("warp:corners(0,0 10,0 5,3 0,10)", "E0320", "fold inward"),
        ("warp:cylinder(0)", "E0320", "must be positive"),
        ("warp:cylinder(-1)", "E0320", "must be positive"),
        ("warp:corners(0,0 10,0 10,10)", "E0370", "takes four points"),
        (
            "warp:corners(0,0 10,0 10,10 0,10 axis:x)",
            "E0370",
            "takes no keys",
        ),
        (
            "warp:corners(0,0 10,0 10,10 50%,10)",
            "E0370",
            "in canvas units",
        ),
        ("warp:cylinder(40 axis:z)", "E0370", "is x or y"),
        ("warp:cylinder(40 r:2)", "E0370", "takes `axis:` only"),
        ("warp:cylinder()", "E0370", "takes one radius"),
        ("warp:cylinder(red)", "E0370", "must be a number"),
        ("warp:5", "E0371", "got a number"),
        ("warp:[]", "E0371", "has no stages"),
        ("warp:[cylinder(4) 3]", "E0371", "got a number"),
        ("warp:nope", "E0371", "`nope` is not a warp"),
    ];
    for (key, code, msg) in cases {
        let ev = run_with(
            &format!("@target t.png 100x100\nrect 40x24 at:50,50 {key} name:a\n"),
            &[],
        );
        let d = diag(&ev, code);
        assert!(d.message.contains(msg), "{key}: {d}");
        assert_eq!(d.span.line, 2, "{key}: {d}");
    }
}

#[test]
fn a_warp_on_a_boolean_operand_is_ignored_and_reported() {
    let ev = run_with(
        "@target t.png 100x100\nsubtract name:s\n  rect 20x20 at:50,50 warp:cylinder(40)\n  circle 4 at:50,50\n",
        &[],
    );
    let d = diag(&ev, "E0321");
    assert!(d.message.contains("`warp:` on an operand"), "{d}");
}

#[test]
fn a_warp_with_nothing_to_map_is_an_error() {
    let ev = run_with(
        "@target t.png 100x100\ngroup warp:cylinder(40) name:empty\n",
        &[],
    );
    assert!(
        diag(&ev, "E0320").message.contains("nothing to warp"),
        "{}",
        show(&ev.diagnostics)
    );
    let ev = run_with(
        "@target t.png 100x100\ngroup at:50,50 warp:[cylinder(10) corners(0,0 10,0 10,10 0,10)] name:gone\n  rect 20x10 at:100,0\n",
        &[],
    );
    let d = diag(&ev, "E0320");
    assert!(d.message.contains("nothing left to map"), "{d}");
}

#[test]
fn adding_a_warp_does_not_reseed_the_content() {
    let radius = |key: &str| {
        let s = scene(&format!(
            "@target t.png 100x100\nrect 40x24 at:50,50 {key} name:a\n  circle rand(2 6) name:dot\n"
        ));
        shape(&s, "a.dot").geometry.compute_tight_bounds().width()
    };
    assert_eq!(radius(""), radius("warp:cylinder(60)"));
}

#[test]
fn explain_prints_the_stack_as_written() {
    let ev = run_with(
        "@target t.png 100x100\nrect 40x24 at:50,50 warp:[cylinder(60) corners(10,10 90,10 90,90 10,90)] name:a\nrect 4x4 at:10,10 warp:cylinder(8 axis:x) name:b\n",
        &[],
    );
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
    let text = crate::explain(ev.scene.as_ref().unwrap(), &ev.seeded);
    assert!(
        text.contains("warp:[cylinder(60) corners(10,10 90,10 90,90 10,90)]"),
        "{text}"
    );
    assert!(text.contains("warp:cylinder(8 axis:x)"), "{text}");
}

#[test]
fn key_interpolates_warps_of_one_shape_and_rejects_a_mix() {
    let s = scene(
        "@target t.png 100x100 t:.5s\n\
         rect 40x24 at:0,0 warp:key(corners(0,0 40,0 40,24 0,24) corners(0,0 80,0 80,24 0,24) ease:linear dur:1s) name:k\n\
         rect 40x24 at:0,0 warp:key(cylinder(20) cylinder(60) ease:linear dur:1s) name:r\n\
         rect 40x24 at:0,0 warp:key([cylinder(20) corners(-20,-12 20,-12 20,12 -20,12)] [cylinder(60) corners(-40,-12 40,-12 40,12 -40,12)] dur:1s) name:st\n\
         circle 1 at:@k.w,@r.w name:p\n\
         circle 1 at:@st.w,@st.h name:sp\n",
    );
    close(probe(&s, "p"), (60.0, 80.0 * sin(20.0 / 40.0)), 1e-3);
    close(probe(&s, "sp"), (60.0, 24.0), 1e-3);
    for t in ["0s", ".5s", "1s"] {
        let ev = run_with(
            &format!(
                "@target t.png 100x100 t:{t}\nrect 40x24 at:0,0 warp:key(cylinder(4) corners(0,0 10,0 10,10 0,10) dur:1s) name:k\n"
            ),
            &[],
        );
        let d = diag(&ev, "E0320");
        assert!(d.message.contains("mixes"), "{t}: {d}");
        let ev = run_with(
            &format!(
                "@target t.png 100x100 t:{t}\nrect 40x24 at:0,0 warp:key(cylinder(4) none dur:1s) name:k\n"
            ),
            &[],
        );
        assert!(diag(&ev, "E0320").message.contains("mixes"), "{t}");
    }
}

fn with_manifest(manifest: &str, src: &str) -> (Project, crate::targets::Plan) {
    let p = Project::from_sources(
        manifest,
        [("art/t.ramo".to_string(), src.to_string())].into(),
    )
    .unwrap();
    let plan = plan(&p);
    assert!(plan.diagnostics.is_empty(), "{:?}", plan.diagnostics);
    (p, plan)
}

fn lint_manifest(level: &str) -> String {
    format!(
        "[project]\nout = \"out\"\nlib = [\"lib\"]\n\n[lint]\nraw-color = \"off\"\nunused = \"off\"\nwarp-cut = \"{level}\"\n"
    )
}

#[test]
fn content_past_the_visible_half_is_linted_once_with_a_radius_that_fits() {
    let src = "@target t.png 300x100\nrect 200x24 at:150,50 warp:cylinder(40) name:wide\n";
    let (p, plan) = with_manifest(&lint_manifest("warn"), src);
    let ev: Evaluation = evaluate(&p, &plan.jobs[0]);
    let cuts: Vec<_> = ev
        .diagnostics
        .iter()
        .filter(|d| d.code == "L0108")
        .collect();
    assert_eq!(cuts.len(), 1, "{}", show(&ev.diagnostics));
    let d = cuts[0];
    assert!(
        d.message
            .contains("`cylinder(40)` hides 74 units of `wide`"),
        "{d}"
    );
    assert!(
        d.fix.as_deref().is_some_and(|f| f.contains("cylinder(64)")),
        "{d}"
    );
    assert_eq!(d.span.line, 2, "{d}");
    let (p, plan) = with_manifest(&lint_manifest("off"), src);
    let ev = evaluate(&p, &plan.jobs[0]);
    assert!(
        ev.diagnostics.iter().all(|d| d.code != "L0108"),
        "{}",
        show(&ev.diagnostics)
    );
    let anim = "@target t.gif 300x100 bg:#ffffff dur:1s fps:4\nanim slide dur:1s\n  .wide at:key(0 10 ease:linear),0\nrect 200x24 at:150,50 warp:cylinder(40) name:wide\nplay slide\n";
    let (p, plan) = with_manifest(&lint_manifest("warn"), anim);
    let sampled = crate::sample(&p, &plan.jobs[0]);
    assert!(sampled.frames.len() > 1);
    let cuts = sampled
        .diagnostics
        .iter()
        .filter(|d| d.code == "L0108")
        .count();
    assert_eq!(cuts, 1, "{}", show(&sampled.diagnostics));
}
