//! Evaluator behaviour tests over in-memory projects.

use artscript_ir::{Blend, EffectOp, Node, NodeKind, Paint, Scene};
use artscript_syntax::Diagnostic;

use crate::{Evaluation, Project, evaluate, plan};

const MANIFEST: &str = "[project]\nout = \"out\"\nlib = [\"lib\"]\n\n[lint]\nraw-color = \"off\"\nunused = \"off\"\noff-canvas = \"off\"\npositional-seed = \"off\"\n";

fn project(files: &[(&str, &str)]) -> Project {
    let files = files
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    Project::from_sources(MANIFEST, files).unwrap()
}

/// Evaluates the first target of `art/t.art` alongside `libs`.
fn run_with(src: &str, libs: &[(&str, &str)]) -> Evaluation {
    let mut files = vec![("art/t.art", src)];
    files.extend_from_slice(libs);
    let p = project(&files);
    let plan = plan(&p);
    assert!(plan.diagnostics.is_empty(), "{:?}", plan.diagnostics);
    evaluate(&p, &plan.jobs[0])
}

fn scene(src: &str) -> Scene {
    let ev = run_with(src, &[]);
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
    ev.scene.unwrap()
}

fn show(ds: &[Diagnostic]) -> String {
    ds.iter()
        .map(|d| d.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn walk<'a>(nodes: &'a [Node], out: &mut Vec<&'a Node>) {
    for n in nodes {
        out.push(n);
        walk(n.children(), out);
    }
}

fn paths(s: &Scene) -> Vec<String> {
    let mut all = Vec::new();
    walk(&s.children, &mut all);
    all.into_iter().map(|n| n.path.clone()).collect()
}

fn node<'a>(s: &'a Scene, path: &str) -> &'a Node {
    let mut all = Vec::new();
    walk(&s.children, &mut all);
    all.into_iter()
        .find(|n| n.path == path)
        .unwrap_or_else(|| panic!("no node {path} in {:?}", paths(s)))
}

fn at(n: &Node) -> (f32, f32) {
    (n.transform.translate_x(), n.transform.translate_y())
}

fn fill_hex(n: &Node) -> String {
    match &n.kind {
        NodeKind::Shape(s) => match &s.fill {
            Some(Paint::Solid(c)) => c.hex(),
            other => panic!("{} has fill {other:?}", n.path),
        },
        _ => panic!("{} is not a shape", n.path),
    }
}

const EYE_LIB: &str = "\
def eye size:10 iris:#3366cc
  circle size fill:#ffffff name:white
    circle size*.5 fill:iris name:pupil
  slot lashes at:0,-size

def lash needs:@white
  curve 0,0 4,-4 stroke:#000000,1
";

#[test]
fn unnamed_siblings_get_count_suffixes_and_loops_append_the_index() {
    let s = scene(
        "@target t.png 100x100\nlayer bg\n  circle 10\n  circle 10\n  rect 4x4 name:box\n  each i in 0..3\n    circle 2 at:i*10,0\n",
    );
    assert_eq!(
        paths(&s),
        [
            "bg",
            "bg.circle~1",
            "bg.circle~2",
            "box",
            "bg.circle~3.0",
            "bg.circle~3.1",
            "bg.circle~3.2"
        ]
    );
    assert_eq!(at(node(&s, "bg.circle~3.2")), (20.0, 0.0));
}

#[test]
fn shape_name_is_registered_before_its_children_so_copy_can_reach_it() {
    let s = scene(
        "@target t.png 100x100\ncircle 20 at:50,50 name:white\n  subtract\n    copy @white\n    copy @white at:10,0\n",
    );
    let sub = node(&s, "white.subtract~1");
    let b = sub.bounds().unwrap();
    assert_eq!((b.left.round(), b.right.round()), (-20.0, 5.0));
}

#[test]
fn patches_rotate_hide_and_reach_nested_params() {
    let lib = (
        "lib/face.art",
        "use eye\ndef face\n  eye 8 at:-10,0 name:a\n  eye 8 at:10,0 name:c\n  rect 40x2 at:0,10 name:mouth\n",
    );
    let ev = run_with(
        "@target t.png 100x100\nuse face\nface at:50,50 name:f\n  .a rot:-20\n  .mouth off\n  .c.pupil fill:#ff0000\n  .a iris:#00ff00\n",
        &[lib, ("lib/eye.art", EYE_LIB)],
    );
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
    let s = ev.scene.unwrap();
    let a = node(&s, "f.a");
    let (sin, cos) = (a.transform.skew_y(), a.transform.scale_x());
    assert!((sin.atan2(cos).to_degrees() + 20.0).abs() < 1e-3);
    assert!(!paths(&s).iter().any(|p| p == "f.mouth"));
    assert_eq!(fill_hex(node(&s, "f.c.pupil")), "#ff0000");
    assert_eq!(fill_hex(node(&s, "f.a.pupil")), "#00ff00");
    assert_eq!(fill_hex(node(&s, "f.c.white")), "#ffffff");

    let head = ("lib/head.art", "use face\ndef head\n  face name:f\n");
    let ev = run_with(
        "@target t.png 100x100\nuse head\nhead name:head\n  .f.c iris:#0000ff\n",
        &[head, lib, ("lib/eye.art", EYE_LIB)],
    );
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
    let s = ev.scene.unwrap();
    assert_eq!(fill_hex(node(&s, "head.f.c.pupil")), "#0000ff");
    assert_eq!(fill_hex(node(&s, "head.f.a.pupil")), "#3366cc");
}

#[test]
fn unmatched_patch_is_e0340_listing_parts() {
    let ev = run_with(
        "@target t.png 100x100\nuse eye\neye\n  .iris off\n",
        &[("lib/eye.art", EYE_LIB)],
    );
    let d = ev
        .diagnostics
        .iter()
        .find(|d| d.code == "E0340")
        .unwrap_or_else(|| panic!("{}", show(&ev.diagnostics)));
    assert!(d.notes.iter().any(|n| n.contains("white")), "{d}");
}

#[test]
fn z_behind_moves_a_node_first() {
    let s = scene(
        "@target t.png 100x100\nlayer l\n  circle 5 name:a\n  circle 5 name:b\n  circle 5 name:c z:behind\n",
    );
    assert_eq!(paths(&s), ["l", "c", "a", "b"]);
}

#[test]
fn slot_content_resolves_host_parts_and_needs_checks_them() {
    let ok = run_with(
        "@target t.png 100x100\nuse eye\neye 10 at:50,50 name:eye\n  lashes:\n    lash\n",
        &[("lib/eye.art", EYE_LIB)],
    );
    assert!(ok.diagnostics.is_empty(), "{}", show(&ok.diagnostics));
    assert!(
        paths(ok.scene.as_ref().unwrap())
            .iter()
            .any(|p| p == "eye.lashes.lash~1"),
        "{:?}",
        paths(ok.scene.as_ref().unwrap())
    );

    let lib =
        "def blank\n  circle 4 name:ball\n  slot top\n\ndef lash needs:@white\n  curve 0,0 4,-4\n";
    let ev = run_with(
        "@target t.png 100x100\nuse b\nblank\n  top:\n    lash\n",
        &[("lib/b.art", lib)],
    );
    let d = ev
        .diagnostics
        .iter()
        .find(|d| d.code == "E0311")
        .unwrap_or_else(|| panic!("{}", show(&ev.diagnostics)));
    assert_eq!(
        d.message,
        "lash needs @white, but the host `blank` has no part named white"
    );
    assert_eq!(d.notes, ["parts available: ball, top"]);
}

#[test]
fn mirror_draws_a_flipped_copy_with_m_suffix() {
    let s = scene(
        "@target t.png 100x100\ngroup g at:50,50\n  mirror x\n    circle 4 at:10,0 name:ear\n",
    );
    let all = paths(&s);
    assert!(
        all.contains(&"ear".to_string()) && all.contains(&"ear~m".to_string()),
        "{all:?}"
    );
    let m = node(&s, "ear~m");
    let b = m
        .transform
        .map_rect(
            m.bounds()
                .map(|r| m.transform.invert().unwrap().map_rect(r).0)
                .unwrap(),
        )
        .0;
    assert_eq!(b.center_x().round(), -10.0);
}

#[test]
fn repeat_and_grid_make_indexed_copies() {
    let s = scene(
        "@target t.png 100x100\ngroup g at:50,50\n  repeat 4 rot:90\n    rect 2x10 at:0,-20 name:petal\n",
    );
    let petals: Vec<String> = paths(&s)
        .into_iter()
        .filter(|p| p.contains("petal"))
        .collect();
    assert_eq!(
        petals,
        [
            "petal.0",
            "petal.1",
            "petal.2",
            "petal.3"
        ]
    );

    let s = scene("@target t.png 100x100\ngroup g\n  grid 3x2 gap:2\n    rect 10x10 name:tile\n");
    let tiles: Vec<&Node> = {
        let mut all = Vec::new();
        walk(&s.children, &mut all);
        all.into_iter()
            .filter(|n| n.path.contains("tile"))
            .collect()
    };
    assert_eq!(tiles.len(), 6);
    let xs: Vec<f32> = tiles.iter().map(|n| at(n).0).collect();
    assert_eq!(xs, [38.0, 50.0, 62.0, 38.0, 50.0, 62.0]);
    let ys: Vec<f32> = tiles.iter().map(|n| at(n).1).collect();
    assert_eq!(ys, [44.0, 44.0, 44.0, 56.0, 56.0, 56.0]);
}

#[test]
fn a_tight_comma_is_one_argument_and_a_real_tuple_stays_a_tuple() {
    let tight = run_with(
        "@target t.png 40x40\ncircle 10 fill:shade(#cc6644,-2) name:tight\n",
        &[],
    );
    let d = tight
        .diagnostics
        .iter()
        .find(|d| d.code == "E0320")
        .unwrap_or_else(|| {
            panic!(
                "shade(c,-2) should be E0320, got:\n{}",
                show(&tight.diagnostics)
            )
        });
    assert!(d.message.contains("color"), "{d}");

    let level = run_with(
        "@target t.png 40x40\ncircle 5 at:30,40 name:eye_l\ncircle 5 at:70,43 name:eye_r\nassert level(@eye_l,@eye_r) tol:.5\n",
        &[],
    );
    assert!(
        level.diagnostics.iter().any(|d| d.code == "E0320"),
        "{}",
        show(&level.diagnostics)
    );
    assert!(
        level.diagnostics.iter().all(|d| d.code != "C0101"),
        "tight comma must not compare two nodes\n{}",
        show(&level.diagnostics)
    );

    let s = scene(
        "@target t.png 40x40\ncircle 10 fill:shade(#cc6644 -2) name:spaced\ncircle 8 at:14,0 stroke:#112233,2 name:point\ncircle 4 at:min(1 2 0),0 name:low\ncircle 4 at:min([1 2 0]),16 name:list\n",
    );
    assert_ne!(fill_hex(node(&s, "spaced")), "#cc6644");
    let point = node(&s, "point");
    assert_eq!(at(point), (14.0, 0.0));
    let NodeKind::Shape(shape) = &point.kind else {
        panic!("point is not a shape")
    };
    let stroke = shape.stroke.as_ref().expect("stroke tuple");
    assert_eq!(stroke.width, 2.0);
    let Paint::Solid(color) = &stroke.paint else {
        panic!("stroke paint {:?}", shape.stroke)
    };
    assert_eq!(color.hex(), "#112233");
    assert_eq!(at(node(&s, "low")).0, 0.0);
    assert_eq!(at(node(&s, "list")).0, 0.0);

    let min_comma = run_with(
        "@target t.png 40x40\ncircle 4 at:min(1,2,0),0 name:comma\n",
        &[],
    );
    assert!(
        min_comma.diagnostics.iter().any(|d| d.code == "E0320"),
        "min(1,2,0) should not reduce the tuple\n{}",
        show(&min_comma.diagnostics)
    );
}

#[test]
fn level_failure_is_c0101_with_measurements_and_a_fix() {
    let ev = run_with(
        "@target t.png 100x100\ncircle 5 at:30,40 name:eye_l\ncircle 5 at:70,43 name:eye_r\nassert level(@eye_l @eye_r) tol:.5\n",
        &[],
    );
    assert_eq!(ev.diagnostics.len(), 1, "{}", show(&ev.diagnostics));
    let d = &ev.diagnostics[0];
    assert_eq!(
        d.to_string(),
        "error[C0101] art/t.art:4:1\n  assert level(@eye_l @eye_r) tol:.5\n  eye_l.cy = 40   eye_r.cy = 43   difference 3 > tol 0.5\n  fix: set .eye_r at:70,40"
    );
}

#[test]
fn expect_failure_is_warning_c0201() {
    let ev = run_with(
        "@target t.png 100x100\nrect 10x10 name:head\nrect 40x10 name:body\nexpect @head.w / @body.w in .9..1.4 \"head as wide as the body\"\n",
        &[],
    );
    let d = &ev.diagnostics[0];
    assert_eq!(
        (d.code.as_str(), d.severity),
        ("C0201", artscript_syntax::Severity::Warning)
    );
    assert_eq!(
        d.notes,
        [
            "head as wide as the body",
            "head.w = 10",
            "body.w = 40",
            "0.25 is outside 0.9..1.4"
        ]
    );

    let inside = run_with(
        "@target t.png 100x100\nrect 12x10 name:head\nrect 10x10 name:body\nexpect @head.w / @body.w in .9..1.4 \"head as wide as the body\"\n",
        &[],
    );
    assert!(
        inside.diagnostics.is_empty(),
        "{}",
        show(&inside.diagnostics)
    );

    let end = run_with(
        "@target t.png 100x100\nrect 14x10 name:head\nrect 10x10 name:body\nexpect @head.w / @body.w in .9..1.4 \"head as wide as the body\"\n",
        &[],
    );
    let note = end
        .diagnostics
        .iter()
        .find(|d| d.code == "C0201")
        .unwrap_or_else(|| panic!("{}", show(&end.diagnostics)));
    assert!(
        note.notes
            .iter()
            .any(|n| n == "1.4 is outside 0.9..1.4 (the end is excluded)"),
        "{:?}",
        note.notes
    );
}

#[test]
fn importing_an_entry_file_is_e0201() {
    let p = project(&[
        ("art/a.art", "@target a.png 10x10\nuse art/b\nrect 1x1\n"),
        ("art/b.art", "@target b.png 10x10\nrect 1x1\n"),
    ]);
    let plan = plan(&p);
    let job = plan.jobs.iter().find(|j| j.source == "art/a.art").unwrap();
    let ev = evaluate(&p, job);
    assert_eq!(
        ev.diagnostics
            .iter()
            .map(|d| d.code.as_str())
            .collect::<Vec<_>>(),
        ["E0201"],
        "{}",
        show(&ev.diagnostics)
    );
}

#[test]
fn drawing_in_a_library_is_e0204() {
    let ev = run_with(
        "@target t.png 10x10\nuse bad\n",
        &[("lib/bad.art", "circle 4\n")],
    );
    let d = ev
        .diagnostics
        .iter()
        .find(|d| d.code == "E0204")
        .unwrap_or_else(|| panic!("{}", show(&ev.diagnostics)));
    assert_eq!(d.file, "lib/bad.art");
}

#[test]
fn param_kind_mismatch_is_e0320() {
    let ev = run_with(
        "@target t.png 10x10\nuse eye\neye size:#ff0000\n",
        &[("lib/eye.art", EYE_LIB)],
    );
    let d = ev
        .diagnostics
        .iter()
        .find(|d| d.code == "E0320")
        .unwrap_or_else(|| panic!("{}", show(&ev.diagnostics)));
    assert!(
        d.fix.as_deref().unwrap_or_default().contains("number"),
        "{d}"
    );
}

fn shape<'a>(s: &'a Scene, path: &str) -> &'a artscript_ir::Shape {
    match &node(s, path).kind {
        NodeKind::Shape(sh) => sh,
        _ => panic!("{path} is not a shape"),
    }
}

fn near(got: (f32, f32), expect: (f64, f64)) {
    let (dx, dy) = (f64::from(got.0) - expect.0, f64::from(got.1) - expect.1);
    assert!(dx.abs() < 1e-4 && dy.abs() < 1e-4, "{got:?} != {expect:?}");
}

#[test]
fn each_expr_builds_curve_points_and_stroke_widths() {
    let s = scene(
        "@target t.png 80x80\n\
         let cx = 0\n\
         let cy = 0\n\
         let r = 10\n\
         curve [each k in 0..4 -> cx+r*cos(k*90),cy+r*sin(k*90)] tension:0 name:ring\n\
         curve 0,0 50,0 100,0 stroke:#111,[each k in 0..3 -> 4+k*4] taper:0 name:ink\n\
         curve 0,0 50,0 100,0 stroke:#111,[4 8 12] taper:0 name:lit\n",
    );
    let b = shape(&s, "ring").geometry.compute_tight_bounds();
    assert!((f64::from(b.left) + 10.0).abs() < 1e-4, "left {}", b.left);
    assert!(
        (f64::from(b.right) - 10.0).abs() < 1e-4,
        "right {}",
        b.right
    );
    assert!((f64::from(b.top) + 10.0).abs() < 1e-4, "top {}", b.top);
    assert!(
        (f64::from(b.bottom) - 10.0).abs() < 1e-4,
        "bottom {}",
        b.bottom
    );
    let ink = shape(&s, "ink").geometry.to_svg();
    let lit = shape(&s, "lit").geometry.to_svg();
    assert_eq!(
        ink, lit,
        "a width list built with each differs from the same literal list"
    );
    assert!(
        shape(&s, "ink")
            .geometry
            .contains(artscript_ir::Point::new(50.0, 3.5))
    );
    assert!(
        !shape(&s, "ink")
            .geometry
            .contains(artscript_ir::Point::new(50.0, 4.6))
    );
}

#[test]
fn each_walks_a_list_of_points() {
    let s =
        scene("@target t.png 80x80\nlet pts = [0,0 10,4 20,-3]\neach p in pts\n  circle 1 at:p\n");
    assert_eq!(at(node(&s, "circle~1.0")), (0.0, 0.0));
    assert_eq!(at(node(&s, "circle~1.1")), (10.0, 4.0));
    assert_eq!(at(node(&s, "circle~1.2")), (20.0, -3.0));
}

#[test]
fn range_end_is_excluded_for_in_and_each() {
    let s = scene(
        "@target t.png 40x40\nexpect 0 in 0..3\nexpect 2.999 in 0..3\nexpect 5..1 == 3..3\nexpect max(0..5) == 4\nexpect [each k in 1.2..2.2 -> k] == [1.2]\nexpect [each k in .5..3.2 -> k] == [.5 1.5 2.5]\nexpect [each k in (1/3)..(1/3+2) -> k] == [1/3 1/3+1]\nexpect (2..4) * 2 == 4..8\neach i in 5..5\n  circle 1 name:skip\neach i in 0..3\n  circle 1 at:i*10,0 name:n\n",
    );
    assert!(
        !paths(&s).iter().any(|p| p.contains("skip")),
        "{:?}",
        paths(&s)
    );
    assert_eq!(at(node(&s, "n.0")), (0.0, 0.0));
    assert_eq!(at(node(&s, "n.1")), (10.0, 0.0));
    assert_eq!(at(node(&s, "n.2")), (20.0, 0.0));

    let outside = |expr: &str| {
        let ev = run_with(
            &format!("@target t.png 10x10\nexpect {expr}\ncircle 1\n"),
            &[],
        );
        let d = ev
            .diagnostics
            .iter()
            .find(|d| d.code == "C0201")
            .unwrap_or_else(|| panic!("{}", show(&ev.diagnostics)));
        d.notes.clone()
    };
    assert_eq!(
        outside("3 in 0..3"),
        ["3 is outside 0..3 (the end is excluded)"]
    );
    assert_eq!(
        outside("5 in 5..5"),
        ["5 is outside 5..5 (the end is excluded)"]
    );
    assert_eq!(
        outside("2 in 6..2"),
        ["2 is outside 6..2 (the end is excluded)"]
    );

    let unequal = run_with(
        "@target t.png 10x10\nexpect .5..3.2 == .5..3\nexpect 0..3 == [0 1 2]\ncircle 1\n",
        &[],
    );
    let misses: Vec<_> = unequal
        .diagnostics
        .iter()
        .filter(|d| d.code == "C0201")
        .collect();
    assert_eq!(misses.len(), 2, "{}", show(&unequal.diagnostics));
    assert!(
        unequal.diagnostics.iter().all(|d| d.code == "C0201"),
        "{}",
        show(&unequal.diagnostics)
    );
}

#[test]
fn stroke_range_is_e0320() {
    let ev = run_with(
        "@target t.png 40x40\npalette p\n  color ink #112233\ncurve 0,0 50,0 100,0 stroke:#111,2..6\ncurve 0,0 50,0 100,0 stroke:2..6\ncurve 0,0 50,0 100,0 stroke:p.ink,2.004..5.996\n",
        &[],
    );
    let fixes: Vec<_> = ev
        .diagnostics
        .iter()
        .filter(|d| d.code == "E0320")
        .map(|d| d.fix.clone().unwrap_or_default())
        .collect();
    assert_eq!(
        fixes,
        vec![
            "write `stroke:#111,[2 6]`".to_string(),
            "write `stroke:[2 6]`".to_string(),
            "write `stroke:p.ink,[2.004 5.996]`".to_string(),
        ],
        "{}",
        show(&ev.diagnostics)
    );

    let patched = run_with(
        "@target t.png 40x40\ndef ink\n  curve 0,0 30,0 name:line stroke:#111,1..3\nink\n  .line stroke:#222,4..6\n",
        &[],
    );
    let patch_fixes: Vec<_> = patched
        .diagnostics
        .iter()
        .filter(|d| d.code == "E0320")
        .map(|d| d.fix.clone().unwrap_or_default())
        .collect();
    assert_eq!(
        patch_fixes,
        vec!["write `stroke:paint,[2 6]`".to_string()],
        "{}",
        show(&patched.diagnostics)
    );

    let restyle = run_with(
        "@target t.png 40x40\nstyle old stroke:#111,1..3\nstyle new stroke:#222,4..6\ndef ink\n  curve 0,0 30,0 name:line style:old\nink\n  .line style:new\n",
        &[],
    );
    let restyle_fixes: Vec<_> = restyle
        .diagnostics
        .iter()
        .filter(|d| d.code == "E0320")
        .map(|d| d.fix.clone().unwrap_or_default())
        .collect();
    assert_eq!(
        restyle_fixes,
        vec!["write `stroke:#222,[4 6]`".to_string()],
        "{}",
        show(&restyle.diagnostics)
    );
}

#[test]
fn each_expr_respects_e0306() {
    let bad = run_with(
        "@target t.png 10x10\nlet xs = [each k in 4 -> k]\ncircle 1\n",
        &[],
    );
    let d = bad
        .diagnostics
        .iter()
        .find(|d| d.code == "E0306")
        .unwrap_or_else(|| panic!("{}", show(&bad.diagnostics)));
    assert!(d.message.contains("number"), "{d}");
    let huge = run_with(
        "@target t.png 10x10\nlet xs = [each k in 0..100001 -> k]\ncircle 1\n",
        &[],
    );
    assert!(
        huge.diagnostics
            .iter()
            .any(|d| d.code == "E0306" && d.message.contains("100000")),
        "{}",
        show(&huge.diagnostics)
    );
    let mixed = run_with("@target t.png 10x10\ncurve [0,0 10,0] 20,0\n", &[]);
    assert!(
        mixed.diagnostics.iter().any(|d| d.code == "E0320"),
        "{}",
        show(&mixed.diagnostics)
    );
}

#[test]
fn vectors_scale_add_subtract_and_negate() {
    let s = scene(
        "@target t.png 80x80\n\
         let crown = 10,20\n\
         let d = 4,-2\n\
         circle 1 at:crown+d*.25 name:scaled\n\
         circle 1 at:crown+d name:added\n\
         circle 1 at:crown-d name:sub\n\
         circle 1 at:-d name:neg\n\
         circle 1 at:3*d name:left\n\
         circle 1 at:d/2 name:div\n",
    );
    near(at(node(&s, "scaled")), (11.0, 19.5));
    near(at(node(&s, "added")), (14.0, 18.0));
    near(at(node(&s, "sub")), (6.0, 22.0));
    near(at(node(&s, "neg")), (-4.0, 2.0));
    near(at(node(&s, "left")), (12.0, -6.0));
    near(at(node(&s, "div")), (2.0, -1.0));
}

#[test]
fn vector_plus_color_is_e0320() {
    let ev = run_with(
        "@target t.png 20x20\nlet p = (#ff0000)+(1,0)\ncircle 1 at:p\n",
        &[],
    );
    let d = ev
        .diagnostics
        .iter()
        .find(|d| d.code == "E0320")
        .unwrap_or_else(|| panic!("{}", show(&ev.diagnostics)));
    assert!(
        d.message.contains("color") && d.message.contains("vector"),
        "{d}"
    );
}

#[test]
fn atan2_is_degrees_and_mod_is_floored() {
    let s = scene(
        "@target t.png 400x40\n\
         circle 1 at:atan2(1 1),mod(-1 3) name:q\n\
         circle 1 at:atan2(0 -1),mod(-7 4) name:h\n\
         circle 1 at:mod(7 4),mod(5.5 2) name:p\n",
    );
    near(at(node(&s, "q")), (45.0, 2.0));
    near(at(node(&s, "h")), (180.0, 1.0));
    near(at(node(&s, "p")), (3.0, 1.5));
}

#[test]
fn grad_points_are_local_endpoints_and_direction_still_spans_bounds() {
    let s = scene(
        "@target t.png 200x80\n\
         rect 80x40 fill:grad(10,5 70,-15 #ff0000 #00ff00,.4 #0000ff) name:band\n\
         rect 80x40 fill:grad(down #ff0000 #0000ff) name:down\n\
         rect 80x40 fill:grad(17 #ff0000 #00ff00) name:angle\n",
    );
    let NodeKind::Shape(band) = &node(&s, "band").kind else {
        panic!("band")
    };
    let Some(Paint::Linear { from, to, stops }) = &band.fill else {
        panic!("{:?}", band.fill)
    };
    assert_eq!((from.x, from.y, to.x, to.y), (10.0, 5.0, 70.0, -15.0));
    assert!(
        stops.iter().any(|st| (st.pos - 0.4).abs() < 1e-4),
        "stop at .4 missing: {stops:?}"
    );
    let NodeKind::Shape(down) = &node(&s, "down").kind else {
        panic!("down")
    };
    let Some(Paint::Linear { from, to, .. }) = &down.fill else {
        panic!("{:?}", down.fill)
    };
    assert!(
        (f64::from(from.x)).abs() < 1e-4 && (f64::from(from.y) + 20.0).abs() < 1e-4,
        "{from:?}"
    );
    assert!(
        (f64::from(to.x)).abs() < 1e-4 && (f64::from(to.y) - 20.0).abs() < 1e-4,
        "{to:?}"
    );
    let NodeKind::Shape(angle) = &node(&s, "angle").kind else {
        panic!("angle")
    };
    let Some(Paint::Linear { from, .. }) = &angle.fill else {
        panic!("{:?}", angle.fill)
    };
    assert_ne!((from.x, from.y), (17.0, 17.0));
}

#[test]
fn round_cuts_corners_of_a_flat_curve_and_a_poly() {
    let s = scene(
        "@target t.png 80x80\n\
         curve -10,-10 10,-10 10,10 -10,10 closed tension:0 fill:#fff name:sharp\n\
         curve -10,-10 10,-10 10,10 -10,10 closed tension:0 round:4 fill:#fff name:soft\n\
         poly 4 r:10 fill:#fff name:diamond\n\
         poly 4 r:10 round:4 fill:#fff name:gem\n",
    );
    let has = |path: &str, x: f32, y: f32| {
        shape(&s, path)
            .geometry
            .contains(artscript_ir::Point::new(x, y))
    };
    assert!(has("sharp", -9.2, -9.2));
    assert!(!has("soft", -9.2, -9.2));
    assert!(has("soft", 0.0, 0.0));
    assert!(has("diamond", 0.0, -9.2));
    assert!(!has("gem", 0.0, -9.2));
    assert!(has("gem", 0.0, 0.0));
}

#[test]
fn jitter_is_amplitude_and_wavelength() {
    let a = scene("@target t.png 80x80\ncircle 20 name:c jitter:3,30\n");
    let b = scene("@target t.png 80x80\ncircle 20 name:c jitter:3,90\n");
    assert_ne!(
        shape(&a, "c").geometry.to_svg(),
        shape(&b, "c").geometry.to_svg()
    );
    let bare = run_with("@target t.png 40x40\ncircle 20 jitter:2\n", &[]);
    let d = bare
        .diagnostics
        .iter()
        .find(|d| d.code == "E0320")
        .unwrap_or_else(|| panic!("{}", show(&bare.diagnostics)));
    assert_eq!(d.fix.as_deref(), Some("write `jitter:2,60`"));
}

#[test]
fn scaled_taper_matches_the_same_stroke_in_pixel_space() {
    let unit = png(
        "@target t.png 200x120\ngroup scale:50 at:100,60\n  curve -1,0 0,-.5 1,0 stroke:#000,[.05 .1 .05] taper:.3\n",
    );
    let pixels = png(
        "@target t.png 200x120\ngroup at:100,60\n  curve -50,0 0,-25 50,0 stroke:#000,[2.5 5 2.5] taper:.3\n",
    );
    let fine = png(
        "@target t.png 200x120 scale:1\ngroup at:100,60\n  curve -40,0 0,-20 40,0 stroke:#000,[4 8 4] taper:.3\n",
    );
    let doubled = png(
        "@target t.png 100x60 scale:2\ngroup scale:.5 at:50,30\n  curve -40,0 0,-20 40,0 stroke:#000,[4 8 4] taper:.3\n",
    );
    assert_pixels_close(&unit, &pixels, 0.15);
    assert_pixels_close(&fine, &doubled, 0.15);
}

fn png(src: &str) -> Vec<u8> {
    let ev = run_with(src, &[]);
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
    artscript_render::render_png(
        &ev.scene.unwrap(),
        ev_scale(src),
        artscript_render::Backend::Cpu,
    ).unwrap()
}

fn ev_scale(src: &str) -> f32 {
    if src.contains("scale:2") { 2.0 } else { 1.0 }
}

fn rgba(png: &[u8]) -> (i32, i32, Vec<u8>) {
    let image = skia_safe::Image::from_encoded(skia_safe::Data::new_copy(png)).expect("png");
    let (w, h) = (image.width(), image.height());
    let info = skia_safe::ImageInfo::new(
        (w, h),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Unpremul,
        None,
    );
    let row = w as usize * 4;
    let mut buf = vec![0u8; row * h as usize];
    assert!(image.read_pixels(
        &info,
        &mut buf,
        row,
        (0, 0),
        skia_safe::image::CachingHint::Disallow
    ));
    (w, h, buf)
}

fn assert_pixels_close(a: &[u8], b: &[u8], mean_limit: f64) {
    let (aw, ah, ab) = rgba(a);
    let (bw, bh, bb) = rgba(b);
    assert_eq!((aw, ah), (bw, bh));
    let mut sum = 0.0f64;
    let mut ink = 0u32;
    for (x, y) in ab.iter().zip(&bb) {
        sum += f64::from(x.abs_diff(*y));
        if *x > 0 || *y > 0 {
            ink += 1;
        }
    }
    let mean = sum / ab.len() as f64;
    assert!(ink > 100, "both images are empty");
    assert!(
        mean < mean_limit,
        "mean channel delta {mean} over {ink} ink bytes"
    );
}

#[test]
fn raw_color_lint_skips_palettes() {
    let p = Project::from_sources(
        "[project]\nout = \"out\"\nlib = [\"lib\"]\n",
        [("art/t.art".to_string(), "@target t.png 10x10\npalette x\n  color a #ff0000\ncircle 3 fill:x.a\nrect 2x2 fill:#00ff00\n".to_string())].into(),
    )
    .unwrap();
    let plan = plan(&p);
    let ev = evaluate(&p, &plan.jobs[0]);
    let lints: Vec<(&str, u32)> = ev
        .diagnostics
        .iter()
        .map(|d| (d.code.as_str(), d.span.line))
        .collect();
    assert_eq!(lints, [("L0101", 5)]);
}

#[test]
fn palette_rows_evaluate_to_colors_and_ramps() {
    let s = scene(
        "@target t.png 10x10\npalette x\n  ramp sky #102030 #f0e0d0 3\n  ramp leaf #7db956 5 hue:12\n  color ink #3d2632\n\
         circle 1 fill:x.ink name:ink\ncircle 1 fill:x.leaf.2 name:leaf\ncircle 1 fill:x.sky.0 name:dark\ncircle 1 fill:x.sky.2 name:bright\n",
    );
    assert_eq!(fill_hex(node(&s, "ink")), "#3d2632");
    assert_eq!(fill_hex(node(&s, "leaf")), "#7db956");
    assert_eq!(fill_hex(node(&s, "dark")), "#102030");
    assert_eq!(fill_hex(node(&s, "bright")), "#f0e0d0");

    let ev = run_with(
        "@target t.png 10x10\npalette x\n  color ink x2\ncircle 1\n",
        &[],
    );
    let d = ev
        .diagnostics
        .iter()
        .find(|d| d.code == "E0305")
        .unwrap_or_else(|| panic!("{}", show(&ev.diagnostics)));
    assert_eq!(d.message, "`color` takes a color, got a word");
}

#[test]
fn palette_rows_may_take_reserved_names_and_read_back_qualified() {
    let s = scene(
        "@target t.png 10x10 bg:shroom.backdrop\npalette shroom\n  ramp cap #e0453a 5\n  color backdrop #1d2b53\n  color if #ff00ff\n\
         palette night from shroom\n  ramp cap #102030 3\n\
         circle 1 fill:night.backdrop name:inherited\ncircle 1 fill:night.if name:keyword\n",
    );
    assert_eq!(s.background.map(|c| c.hex()).as_deref(), Some("#1d2b53"));
    assert_eq!(fill_hex(node(&s, "inherited")), "#1d2b53");
    assert_eq!(fill_hex(node(&s, "keyword")), "#ff00ff");

    let p = Project::from_sources(
        &format!("{MANIFEST}\n[defaults]\npalette = \"palette/house\"\n"),
        [
            (
                "art/t.art".to_string(),
                "@target t.png 10x10\ncircle 1 fill:p.backdrop name:c\n".to_string(),
            ),
            (
                "lib/palette/house.art".to_string(),
                "palette house\n  color backdrop #1d2b53\n".to_string(),
            ),
        ]
        .into(),
    )
    .unwrap();
    let plan = plan(&p);
    let ev = evaluate(&p, &plan.jobs[0]);
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
    assert_eq!(fill_hex(node(&ev.scene.unwrap(), "c")), "#1d2b53");
}


#[test]
fn mask_value_and_mask_block_both_mask_the_draw() {
    let s = scene(
        "@target t.png 100x100\ncircle 10 name:body\ngroup g\n  rect 4x4\n  mask @body\ngroup f\n  mask simplex(30) > .5\n  rect 4x4\ngroup b\n  mask\n    circle 3\n",
    );
    let source = |p: &str| {
        node(&s, p)
            .mask
            .as_ref()
            .map(|m| &m.source)
            .unwrap_or_else(|| panic!("{p} has no mask"))
    };
    assert!(matches!(source("g"), artscript_ir::MaskSource::Nodes(n) if n.len() == 1));
    assert!(
        matches!(source("f"), artscript_ir::MaskSource::Field { threshold: Some(t), .. } if (*t - 0.5).abs() < 1e-6)
    );
    assert!(
        matches!(source("b"), artscript_ir::MaskSource::Nodes(n) if n[0].path == "b.mask~1.circle~1")
    );
}

#[test]
fn def_fx_run_before_call_site_fx_and_the_mask_is_the_call_sites() {
    let lib = (
        "lib/cup.art",
        "def cup\n  circle 4\n  fx blur 1\n  slot mask\n",
    );
    let s = scene_with(
        "@target t.png 100x100\nuse cup\ncup name:cup\n  fx glow 6\n  mask\n    circle 2\n",
        &[lib],
    );
    let cup = node(&s, "cup");
    let ops: Vec<&artscript_ir::EffectOp> = cup.effects.iter().map(|e| &e.op).collect();
    assert!(
        matches!(
            ops.as_slice(),
            [
                artscript_ir::EffectOp::Blur { .. },
                artscript_ir::EffectOp::Glow { .. }
            ]
        ),
        "{ops:?}"
    );
    assert!(cup.mask.is_some());

    let s = scene_with(
        "@target t.png 100x100\nuse cup\ncup name:cup\n  mask:\n    circle 2\n",
        &[lib],
    );
    assert!(
        node(&s, "cup").mask.is_none(),
        "`mask:` fills the slot, it does not mask"
    );
    node(&s, "cup.mask.circle~1");

    let s = scene_with(
        "@target t.png 100x100\nuse cup\ncup name:cup\n  mask:\n    circle 2\n  .mask off\n",
        &[lib],
    );
    assert!(!paths(&s).iter().any(|p| p.starts_with("cup.mask")));
}

#[test]
fn a_fill_line_is_a_draw_with_an_unknown_verb() {
    let ev = run_with("@target t.png 10x10\nfill #fff\n", &[]);
    assert_eq!(
        ev.diagnostics
            .iter()
            .map(|d| d.code.as_str())
            .collect::<Vec<_>>(),
        ["E0301"],
        "{}",
        show(&ev.diagnostics)
    );
}

#[test]
fn slot_fill_under_a_shape_is_still_e0331() {
    let ev = run_with(
        "@target t.png 10x10\ncircle 4\n  head:\n    circle 1\n",
        &[],
    );
    assert_eq!(
        ev.diagnostics
            .iter()
            .map(|d| d.code.as_str())
            .collect::<Vec<_>>(),
        ["E0331"],
        "{}",
        show(&ev.diagnostics)
    );
}

fn scene_with(src: &str, libs: &[(&str, &str)]) -> Scene {
    let ev = run_with(src, libs);
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
    ev.scene.unwrap()
}

fn assert_box(s: &Scene, path: &str, w: f64, h: f64) {
    let b = shape(s, path).geometry.compute_tight_bounds();
    let (bw, bh) = (f64::from(b.width()), f64::from(b.height()));
    assert!(
        (bw - w).abs() < 1e-3 && (bh - h).abs() < 1e-3,
        "{path} is {bw}x{bh}, expected {w}x{h}"
    );
}

fn lib_scene(src: &str, lib: &str) -> Scene {
    let ev = run_with(src, &[("lib/d.art", lib)]);
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
    ev.scene.unwrap()
}

#[test]
fn a_def_instance_keeps_its_blend_modes_inside_it() {
    let lib = "def hole r:20\n  circle r fill:#000 blend:erase\ndef tint\n  rect 60x60 fill:#3366ff blend:multiply\n";
    let src = "@target t.png 300x100 bg:#ffffff\nuse d { hole tint }\nrect 100%x100% fill:#e0a020\nhole 30 at:50,50\nlayer iso at:150,50\n  tint\ntint at:250,50\n";
    let ev = run_with(src, &[("lib/d.art", lib)]);
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
    let png = artscript_render::render_png(&ev.scene.unwrap(), 1.0, artscript_render::Backend::Cpu).unwrap();
    let (w, _, px) = rgba(&png);
    let at = |x: i32, y: i32| {
        let i = (y * w + x) as usize * 4;
        [px[i], px[i + 1], px[i + 2], px[i + 3]]
    };
    assert_eq!(at(50, 50), at(5, 5), "hole erased the caller's art");
    assert_eq!(at(250, 50), at(150, 50), "tint blended with the caller's art");
}

#[test]
fn def_size_box_measures_percents_from_its_own_center() {
    let lib = "def d size:40x20\n  rect 100%x100% name:r\n  circle 50% name:c\n  circle 2 at:0%,0% name:mark\n";
    let s = lib_scene("@target t.png 200x100\nuse d\nd name:d\n", lib);
    assert_box(&s, "d.r", 40.0, 20.0);
    assert_box(&s, "d.c", 20.0, 20.0);
    near(at(node(&s, "d.mark")), (-20.0, -10.0));
}

#[test]
fn caller_percent_size_resolves_against_the_caller_frame() {
    let lib = "def d size:40x20\n  rect size name:r\n  circle size.w*.1 name:c\n";
    let pair = lib_scene("@target t.png 200x100\nuse d\nd 50%x50% name:d\n", lib);
    assert_box(&pair, "d.r", 100.0, 50.0);
    assert_box(&pair, "d.c", 20.0, 20.0);
    let bare = lib_scene(
        "@target t.png 200x100\nuse d\nd 50% name:d\n",
        "def d size:40x20\n  rect size name:r\n",
    );
    assert_box(&bare, "d.r", 50.0, 50.0);
    let scaled = lib_scene(
        "@target t.png 200x100\nuse d\nd 100%x100% scale:2 name:d\n",
        "def d size:10x10\n  rect 100%x100% name:r\n",
    );
    assert_box(&scaled, "d.r", 100.0, 50.0);
    let square = lib_scene(
        "@target t.png 80x40\nuse d\nd 30 name:d\n",
        "def d size:72x32\n  rect size.w,size.h name:r\n",
    );
    assert_box(&square, "d.r", 30.0, 30.0);
}

#[test]
fn a_square_size_rejects_a_pair() {
    let ev = run_with(
        "@target t.png 80x40\nuse d\nd 30x16\n",
        &[("lib/d.art", "def d size:10\n  circle size*.5\n")],
    );
    assert!(
        ev.diagnostics.iter().any(|d| d.code == "E0320"),
        "{}",
        show(&ev.diagnostics)
    );
}

#[test]
fn child_without_at_stays_at_the_instance_origin() {
    let s = lib_scene(
        "@target t.png 200x100\nuse d\nd at:30,16 name:d\n",
        "def d size:80x40\n  circle 5 name:c\n",
    );
    near(at(node(&s, "d")), (30.0, 16.0));
    near(at(node(&s, "d.c")), (0.0, 0.0));
    let placed = lib_scene(
        "@target t.png 200x100\nuse d\nd at:100%,100% name:d\n",
        "def d size:40x20\n  circle 4 name:c\n",
    );
    near(at(node(&placed, "d")), (200.0, 100.0));
    near(at(node(&placed, "d.c")), (0.0, 0.0));
}

#[test]
fn percent_in_a_def_without_size_is_e0322() {
    let ev = run_with(
        "@target t.png 80x40\nuse d\nblush\n",
        &[("lib/d.art", "def blush\n  circle 50%\n")],
    );
    let d = ev
        .diagnostics
        .iter()
        .find(|d| d.code == "E0322")
        .unwrap_or_else(|| panic!("{}", show(&ev.diagnostics)));
    assert_eq!(
        d.message,
        "`blush` has no size, so 50% has nothing to measure"
    );
    assert_eq!(
        d.fix.as_deref(),
        Some("declare the box with `size:`, as in `def blush size:72x32`")
    );

    let outside = run_with(
        "@target t.png 80x40\nlayer x\ninside @x\n  circle 50%\n",
        &[],
    );
    let d = outside
        .diagnostics
        .iter()
        .find(|d| d.code == "E0322")
        .unwrap_or_else(|| panic!("{}", show(&outside.diagnostics)));
    assert_eq!(d.message, "50% has nothing to measure");
}

#[test]
fn slot_fill_percents_use_the_host_box() {
    let lib = "def host size:40x20\n  slot hole\n";
    let s = lib_scene(
        "@target t.png 200x100\nuse d\nhost name:host\n  hole:\n    rect 100%x100% name:r\n    circle 2 at:0%,0% name:mark\n",
        lib,
    );
    assert_box(&s, "host.hole.r", 40.0, 20.0);
    near(at(node(&s, "host.hole.mark")), (-20.0, -10.0));
    let shifted = lib_scene(
        "@target t.png 200x100\nuse d\nhost name:host\n",
        "def host size:40x20\n  slot hole at:10,0\n    circle 2 at:0%,0% name:mark\n",
    );
    near(at(node(&shifted, "host.hole.mark")), (-20.0, -10.0));
}

#[test]
fn unplaced_slot_content_stays_at_the_slot() {
    let lib = "def host size:40x20\n  slot hole at:10,0\n    circle 2 name:own\n";
    let own = lib_scene("@target t.png 200x100\nuse d\nhost name:host\n", lib);
    near(at(node(&own, "host.hole.own")), (0.0, 0.0));
    let filled = lib_scene(
        "@target t.png 200x100\nuse d\nhost name:host\n  hole:\n    circle 2 name:fill\n",
        lib,
    );
    near(at(node(&filled, "host.hole.fill")), (0.0, 0.0));
}

#[test]
fn size_is_not_unused_when_only_percents_use_the_box() {
    let manifest = "[project]\nout = \"out\"\nlib = [\"lib\"]\n\n[lint]\nraw-color = \"off\"\nunused = \"warn\"\noff-canvas = \"off\"\n";
    let files = [
        (
            "art/t.art".to_string(),
            "@target t.png 80x40\nuse d\nblush\n".to_string(),
        ),
        (
            "lib/d.art".to_string(),
            "def blush size:40x20 extra:1\n  circle 50%\n".to_string(),
        ),
    ]
    .into();
    let p = Project::from_sources(manifest, files).unwrap();
    let plan = plan(&p);
    assert!(plan.diagnostics.is_empty(), "{:?}", plan.diagnostics);
    let ev = evaluate(&p, &plan.jobs[0]);
    let unused: Vec<&str> = ev
        .diagnostics
        .iter()
        .filter(|d| d.code == "L0103")
        .map(|d| d.message.as_str())
        .collect();
    assert!(unused.iter().any(|m| m.contains("`extra`")), "{unused:?}");
    assert!(unused.iter().all(|m| !m.contains("`size`")), "{unused:?}");
}

#[test]
fn absolute_size_still_boxes_a_child_of_a_sizeless_def() {
    let s = lib_scene(
        "@target t.png 80x40\nuse d\nouter name:outer\n",
        "def outer\n  inner 20 name:inner\ndef inner size:10\n  rect 100%x100% name:r\n",
    );
    assert_box(&s, "outer.inner.r", 20.0, 20.0);
    let ev = run_with(
        "@target t.png 80x40\nuse d\nouter\n",
        &[(
            "lib/d.art",
            "def outer\n  inner 50%\ndef inner size:10\n  circle 4\n",
        )],
    );
    let d = ev
        .diagnostics
        .iter()
        .find(|d| d.code == "E0322")
        .unwrap_or_else(|| panic!("{}", show(&ev.diagnostics)));
    assert_eq!(
        d.message,
        "`outer` has no size, so 50% has nothing to measure"
    );
}

fn fx_ops(n: &Node) -> Vec<String> {
    n.effects
        .iter()
        .map(|e| {
            format!("{:?}", e.op)
                .split([' ', '{'])
                .next()
                .unwrap_or_default()
                .to_string()
        })
        .collect()
}

fn fx_debug(s: &Scene, path: &str) -> String {
    format!("{:?}", node(s, path).effects)
}

fn grain_seed(n: &Node) -> u32 {
    n.effects
        .iter()
        .find_map(|e| match e.op {
            EffectOp::Grain { seed, .. } => Some(seed),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{} has no grain", n.path))
}

fn diag<'a>(ev: &'a Evaluation, code: &str) -> &'a Diagnostic {
    ev.diagnostics
        .iter()
        .find(|d| d.code == code)
        .unwrap_or_else(|| panic!("no {code} in\n{}", show(&ev.diagnostics)))
}

const SURFACE: &str =
    "finish surface width:5 a:.55\n  fx rim width dir:1,1 a:a\n  fx grain .035 scale:2\n";

#[test]
fn finish_splices_its_lines_at_the_fx_line_between_local_lines() {
    let s = scene(&format!(
        "@target t.png 100x100\n{SURFACE}circle 10 name:c\n  fx shadow 0,4\n  fx surface width:6\n  fx outline 3\n"
    ));
    assert_eq!(fx_ops(node(&s, "c")), ["Shadow", "Rim", "Grain", "Outline"]);
    let written = scene(
        "@target t.png 100x100\ncircle 10 name:c\n  fx shadow 0,4\n  fx rim 6 dir:1,1 a:.55\n  fx grain .035 scale:2\n  fx outline 3\n",
    );
    assert_eq!(fx_debug(&s, "c"), fx_debug(&written, "c"));
}

#[test]
fn finish_reaches_only_the_node_its_fx_line_is_under() {
    let s = scene(&format!(
        "@target t.png 100x100\n{SURFACE}layer l\n  circle 10 name:c\n    circle 3 name:dot\n    fx surface\n"
    ));
    assert_eq!(fx_ops(node(&s, "c")), ["Rim", "Grain"]);
    assert!(node(&s, "l").effects.is_empty());
    assert!(node(&s, "dot").effects.is_empty());
}

#[test]
fn shared_finish_seeds_grain_per_node_like_written_lines() {
    let shared = scene(&format!(
        "@target t.png 100x100\n{SURFACE}circle 10 name:cap\n  fx surface\ncircle 10 name:shimaki\n  fx surface\n"
    ));
    let written = scene(
        "@target t.png 100x100\n\
         circle 10 name:cap\n  fx rim 5 dir:1,1 a:.55\n  fx grain .035 scale:2\n\
         circle 10 name:shimaki\n  fx rim 5 dir:1,1 a:.55\n  fx grain .035 scale:2\n",
    );
    assert_ne!(
        grain_seed(node(&shared, "cap")),
        grain_seed(node(&shared, "shimaki"))
    );
    for path in ["cap", "shimaki"] {
        assert_eq!(fx_debug(&shared, path), fx_debug(&written, path));
    }
}

#[test]
fn library_finish_resolves_names_in_the_library_and_seeds_from_the_use_site() {
    let lib = (
        "lib/finish/surface.art",
        "let shine = #ff0000\nfinish surface width:4\n  fx rim width color:shine\n  fx grain .04 scale:2.5\n",
    );
    let ev = run_with(
        "@target t.png 100x100\nuse finish/surface { surface }\nlet shine = #0000ff\nlet w = 7\ncircle 10 name:cap\n  fx surface width:w\n",
        &[lib],
    );
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
    let used = ev.scene.unwrap();
    let written = scene(
        "@target t.png 100x100\ncircle 10 name:cap\n  fx rim 7 color:#ff0000\n  fx grain .04 scale:2.5\n",
    );
    assert_eq!(fx_debug(&used, "cap"), fx_debug(&written, "cap"));
}

#[test]
fn def_body_finish_comes_before_call_block_fx() {
    let lib = (
        "lib/leaf.art",
        "finish _edge tone:#00aa00\n  fx rim 3\n  fx grain .05\n  fx outline 4 color:tone\ndef leaf tone:#00ff00\n  circle 10 fill:tone\n  fx _edge tone:tone\n",
    );
    let written = (
        "lib/leaf.art",
        "def leaf tone:#00ff00\n  circle 10 fill:tone\n  fx rim 3\n  fx grain .05\n  fx outline 4 color:tone\n",
    );
    let src = "@target t.png 100x100\nuse leaf\nleaf tone:#008800\n  fx adjust light:-.06\n";
    let ev = run_with(src, &[lib]);
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
    let s = ev.scene.unwrap();
    assert_eq!(
        fx_ops(node(&s, "leaf~1")),
        ["Rim", "Grain", "Outline", "Adjust"]
    );
    let w = run_with(src, &[written]);
    assert!(w.diagnostics.is_empty(), "{}", show(&w.diagnostics));
    assert_eq!(fx_debug(&s, "leaf~1"), fx_debug(&w.scene.unwrap(), "leaf~1"));
}

#[test]
fn finish_as_a_node_key_is_e0339_pointing_at_an_fx_line() {
    let ev = run_with(
        &format!("@target t.png 100x100\n{SURFACE}circle 10 finish:surface\n"),
        &[],
    );
    let d = diag(&ev, "E0339");
    assert_eq!(d.span.line, 5);
    assert_eq!(d.fix.as_deref(), Some("write `fx surface` under the node"));

    let ev = run_with(
        &format!("@target t.png 100x100\n{SURFACE}circle 10 style:surface\n"),
        &[],
    );
    assert_eq!(
        diag(&ev, "E0307").fix.as_deref(),
        Some("write `fx surface` under the node")
    );
}

#[test]
fn finish_body_is_plain_built_in_effects() {
    let ev = run_with(
        "@target t.png 100x100\nfinish a\n  fx blur 2 as:soft\n  fx merge soft self of:self\ncircle 4\n",
        &[],
    );
    let lines: Vec<(&str, u32)> = ev
        .diagnostics
        .iter()
        .map(|d| (d.code.as_str(), d.span.line))
        .collect();
    assert_eq!(
        lines,
        [("E0336", 3), ("E0336", 4)],
        "{}",
        show(&ev.diagnostics)
    );

    let ev = run_with(
        &format!(
            "@target t.png 100x100\n{SURFACE}finish outer\n  fx rim 2\n  fx surface\ncircle 4\n"
        ),
        &[],
    );
    let d = diag(&ev, "E0338");
    assert_eq!(d.span.line, 7);
    assert!(
        d.notes
            .iter()
            .any(|n| n == "`surface` is a finish, and finishes do not nest"),
        "{d}"
    );

    let ev = run_with(
        "@target t.png 100x100\nfinish rim\n  fx grain .1\ncircle 4\n",
        &[],
    );
    assert_eq!(diag(&ev, "E0337").span.line, 2);

    let ev = run_with(
        "@target t.png 100x100\nuse bad\ncircle 4\n",
        &[("lib/bad.art", "finish f\n  fx rim 3\n  circle 4\n")],
    );
    let d = diag(&ev, "E0101");
    assert_eq!(
        (d.file.as_str(), d.span.line, d.message.as_str()),
        ("lib/bad.art", 3, "a finish holds only `fx` lines")
    );
}

#[test]
fn a_private_name_is_not_an_export() {
    let lib = (
        "lib/finish/surface.art",
        "finish _surface width:4\n  fx rim width\nfinish surface width:4\n  fx rim width\nstyle _ink fill:#112233\nstyle ink fill:#112233\n",
    );
    let explicit = run_with(
        "@target t.png 100x100\nuse finish/surface { _surface }\ncircle 10\n  fx _surface\n",
        &[lib],
    );
    assert_eq!(
        diag(&explicit, "E0212").message,
        "`finish/surface` does not export `_surface`"
    );

    let style = run_with(
        "@target t.png 100x100\nuse finish/surface { _ink }\ncircle 10 style:_ink\n",
        &[lib],
    );
    assert_eq!(
        diag(&style, "E0212").message,
        "`finish/surface` does not export `_ink`"
    );

    let wild = run_with(
        "@target t.png 100x100\nuse finish/surface\ncircle 10 name:cap\n  fx _surface\n",
        &[lib],
    );
    assert_eq!(diag(&wild, "E0391").message, "unknown effect `_surface`");

    let public = run_with(
        "@target t.png 100x100\nuse finish/surface { surface ink }\ncircle 10 name:cap style:ink\n  fx surface\n",
        &[lib],
    );
    assert!(
        public.diagnostics.is_empty(),
        "{}",
        show(&public.diagnostics)
    );
    let s = public.scene.unwrap();
    assert_eq!(fx_ops(node(&s, "cap")), ["Rim"]);
    let Some(Paint::Solid(color)) = &shape(&s, "cap").fill else {
        panic!("fill")
    };
    assert_eq!(color.hex(), "#112233");
}

#[test]
fn explicit_use_binds_a_style_and_a_finish_that_share_a_name() {
    let lib = (
        "lib/finish/surface.art",
        "style surface fill:#112233\nfinish surface width:4\n  fx rim width\n",
    );
    let ev = run_with(
        "@target t.png 100x100\nuse finish/surface { surface }\ncircle 10 name:cap style:surface\n  fx surface\n",
        &[lib],
    );
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
    let s = ev.scene.unwrap();
    assert_eq!(fx_ops(node(&s, "cap")), ["Rim"]);
    let Some(Paint::Solid(color)) = &shape(&s, "cap").fill else {
        panic!("fill")
    };
    assert_eq!(color.hex(), "#112233");
}

#[test]
fn finish_parameters_are_named_and_typed_by_their_defaults() {
    let ev = run_with(
        &format!("@target t.png 100x100\n{SURFACE}circle 10\n  fx surface 6\n"),
        &[],
    );
    assert!(
        diag(&ev, "E0321")
            .notes
            .iter()
            .any(|n| n == "parameters: width, a")
    );
    let ev = run_with(
        &format!("@target t.png 100x100\n{SURFACE}circle 10\n  fx surface depth:6\n"),
        &[],
    );
    assert_eq!(
        diag(&ev, "E0321").message,
        "finish `surface` has no parameter `depth`"
    );
    let ev = run_with(
        &format!("@target t.png 100x100\n{SURFACE}circle 10\n  fx surface width:#ff0000\n"),
        &[],
    );
    assert_eq!(diag(&ev, "E0320").span.line, 6);
}

#[test]
fn non_positive_size_is_e0320() {
    let ev = run_with(
        "@target t.png 40x40\nuse d\nd 0\n",
        &[("lib/d.art", "def d size:40\n  circle size\n")],
    );
    let d = ev
        .diagnostics
        .iter()
        .find(|d| d.code == "E0320")
        .unwrap_or_else(|| panic!("{}", show(&ev.diagnostics)));
    assert!(d.message.contains("must be positive"), "{d}");
    assert_eq!(d.fix.as_deref(), Some("write `size:40` or `size:72x32`"));
}

#[test]
fn toggling_an_if_keeps_later_sibling_paths_and_seeds() {
    let src = |show: bool| {
        format!(
            "@target t.png 100x100\nlet show = {show}\ncircle 4 at:rand(0 40),0\nif show\n  circle 4\ncircle 4 at:rand(0 40),10\n"
        )
    };
    let hidden = scene(&src(false));
    let shown = scene(&src(true));
    assert_eq!(paths(&hidden), ["circle~1", "circle~3"]);
    assert_eq!(paths(&shown), ["circle~1", "circle~2", "circle~3"]);
    assert_eq!(at(node(&hidden, "circle~3")), at(node(&shown, "circle~3")));
    assert_eq!(at(node(&hidden, "circle~1")), at(node(&shown, "circle~1")));
}

#[test]
fn key_samples_smooth_easing_and_steps() {
    let s = scene("@target t.png 100x100 t:.25s\nrect 4 at:key(0 10 0 dur:1s),0 name:box\n");
    assert!(
        (at(node(&s, "box")).0 - 5.625).abs() < 1e-3,
        "{}",
        at(node(&s, "box")).0
    );
    let linear =
        scene("@target t.png 100x100 t:.5s\nrect 4 at:key(0 10 ease:linear dur:1s),0 name:box\n");
    assert!((at(node(&linear, "box")).0 - 5.0).abs() < 1e-3);
    let inn = scene("@target t.png 100x100 t:.5s\nrect 4 at:key(0 10 ease:in dur:1s),0 name:box\n");
    assert!((at(node(&inn, "box")).0 - 2.5).abs() < 1e-3);
    let out =
        scene("@target t.png 100x100 t:.5s\nrect 4 at:key(0 10 ease:out dur:1s),0 name:box\n");
    assert!((at(node(&out, "box")).0 - 7.5).abs() < 1e-3);
    let hold =
        scene("@target t.png 100x100 t:.9s\nrect 4 at:key(1 9 ease:hold dur:1s),0 name:box\n");
    assert!((at(node(&hold, "box")).0 - 1.0).abs() < 1e-3);
    let ev = run_with(
        "@target t.png 100x100 t:.9s\nlet w = key(red blue dur:1s)\nassert w == red\nrect 4\n",
        &[],
    );
    assert!(
        ev.diagnostics.iter().all(|d| !d.code.starts_with('C')),
        "{}",
        show(&ev.diagnostics)
    );
    let bad = run_with(
        "@target t.png 100x100\nlet w = key([1] [2] dur:1s)\nrect 4\n",
        &[],
    );
    assert!(
        bad.diagnostics
            .iter()
            .any(|d| d.code == "E0320" && d.message.contains("cannot animate a list")),
        "{}",
        show(&bad.diagnostics)
    );
}

#[test]
fn play_offsets_add_and_abs_replaces_and_later_sets_win() {
    let src = "\
@target t.png 100x100 t:.5s clip:go
anim nudge dur:1s
  .box at:10,0
  .box rot:5
anim shove dur:1s
  .box at:0,4
  .box rot:abs(1)
anim paint dur:1s
  .box fill:#ff0000
anim paint2 dur:1s
  .box fill:#00ff00
if clip == go
  play nudge
  play shove
  play paint
  play paint2
rect 10 at:1,2 fill:#0000ff name:box
";
    let s = scene(src);
    let n = node(&s, "box");
    assert!(
        (at(n).0 - 11.0).abs() < 1e-3 && (at(n).1 - 6.0).abs() < 1e-3,
        "{:?}",
        at(n)
    );
    let (sin, cos) = (n.transform.skew_y(), n.transform.scale_x());
    assert!(
        (sin.atan2(cos).to_degrees() - 1.0).abs() < 1e-2,
        "{}",
        sin.atan2(cos).to_degrees()
    );
    assert_eq!(fill_hex(n), "#00ff00");
}

#[test]
fn an_anim_without_time_branching_keeps_one_path_set() {
    let src = "\
@target t.gif 40x40 bg:#ffffff dur:1s fps:4 clip:go
anim bob dur:1s
  .box at:0,key(0 8)
if clip == go
  play bob
rect 8 at:10,10 name:box
";
    let p = project(&[("art/t.art", src)]);
    let plan = plan(&p);
    assert!(plan.diagnostics.is_empty(), "{:?}", plan.diagnostics);
    let sampled = crate::sample(&p, &plan.jobs[0]);
    assert!(
        sampled.diagnostics.iter().all(|d| !d.code.starts_with('E')),
        "{}",
        show(&sampled.diagnostics)
    );
    let sets: Vec<Vec<String>> = sampled
        .frames
        .iter()
        .map(|f| {
            let mut all = Vec::new();
            walk(&f.scene.children, &mut all);
            all.into_iter().map(|n| n.path.clone()).collect()
        })
        .collect();
    assert!(sets.len() > 1);
    assert!(sets.iter().all(|s| s == &sets[0]), "{sets:?}");
}

#[test]
fn per_frame_checks_report_first_and_worst() {
    let src = "\
@target t.gif 20x20 bg:#ffffff dur:1s fps:4
assert t < .5
circle 2
";
    let p = project(&[("art/t.art", src)]);
    let plan = plan(&p);
    let sampled = crate::sample(&p, &plan.jobs[0]);
    let fail = sampled
        .diagnostics
        .iter()
        .find(|d| d.code == "C0101")
        .unwrap_or_else(|| panic!("{}", show(&sampled.diagnostics)));
    assert_eq!(
        sampled
            .diagnostics
            .iter()
            .filter(|d| d.code == "C0101")
            .count(),
        1
    );
    assert!(
        fail.notes.iter().any(|n| n.contains("first at 0.5s")),
        "{:?}",
        fail.notes
    );
    assert!(
        fail.notes.iter().any(|n| n.contains("worst at 0.75s")),
        "{:?}",
        fail.notes
    );
}

#[test]
fn a_let_inside_if_does_not_escape_and_a_later_let_is_not_early() {
    let leaked = run_with(
        "@target t.png 40x40\nif true\n  let r = 9\ncircle r name:c\n",
        &[],
    );
    assert!(
        !leaked.diagnostics.is_empty(),
        "a let inside if reached the circle"
    );
    if let Some(scene) = &leaked.scene {
        let mut all = Vec::new();
        walk(&scene.children, &mut all);
        assert!(
            all.iter().all(|n| n.path != "c" || {
                let b = n.bounds().unwrap();
                (f64::from(b.width()) - 18.0).abs() > 1.0
            }),
            "{}",
            show(&leaked.diagnostics)
        );
    }
    let early = run_with("@target t.png 40x40\ncircle r name:c\nlet r = 9\n", &[]);
    assert!(
        !early.diagnostics.is_empty(),
        "{}",
        show(&early.diagnostics)
    );
    let s = scene("@target t.png 40x40\nlet r = 9\ncircle r name:c\n");
    assert_box(&s, "c", 18.0, 18.0);
    let moved = scene(
        "@target t.png 40x40\nlet n = 7\nanim idle dur:1s\n  .c at:n,0\ncircle 4 name:c\nplay idle\n",
    );
    near(at(node(&moved, "c")), (27.0, 20.0));
}

#[test]
fn the_same_check_on_two_instances_stays_two_failures() {
    let ev = run_with(
        "@target t.png 40x20\nuse d\ndot name:a\ndot name:b\n",
        &[("lib/d.art", "def dot\n  circle 4\n  assert false\n")],
    );
    let mut nodes: Vec<_> = ev
        .diagnostics
        .iter()
        .filter(|d| d.code == "C0101")
        .map(|d| d.node.as_str())
        .collect();
    nodes.sort();
    assert_eq!(nodes, ["a", "b"], "{}", show(&ev.diagnostics));
}

#[test]
fn the_worst_frame_keeps_its_notes_and_fix() {
    use artscript_syntax::ast::Span;
    let span = Span { line: 2, col: 1 };
    let mild = Diagnostic::error("C0101", span, "gap")
        .in_file("art/t.art")
        .at_node("a")
        .with_note("gap is 1")
        .with_fix("nudge 1");
    let worse = Diagnostic::error("C0101", span, "gap")
        .in_file("art/t.art")
        .at_node("a")
        .with_note("gap is 9")
        .with_fix("nudge 9");
    let other = Diagnostic::error("C0101", span, "gap")
        .in_file("art/t.art")
        .at_node("b")
        .with_note("gap is 2")
        .with_fix("nudge 2");
    let out = super::aggregate_frames(&[(0.0, vec![mild, other.clone()]), (0.5, vec![worse])]);
    assert_eq!(out.len(), 2);
    let a = out.iter().find(|d| d.node == "a").unwrap();
    assert_eq!(a.fix.as_deref(), Some("nudge 9"));
    assert!(a.notes.iter().any(|n| n == "gap is 9"), "{:?}", a.notes);
    assert!(
        a.notes.iter().any(|n| n.contains("first at 0s")),
        "{:?}",
        a.notes
    );
    assert!(
        a.notes.iter().any(|n| n.contains("worst at 0.5s")),
        "{:?}",
        a.notes
    );
    assert_eq!(out.iter().filter(|d| d.node == "b").count(), 1);
}

#[test]
fn play_marks_an_imported_anim_as_used() {
    let manifest = "[project]\nout = \"out\"\nlib = [\"lib\"]\n\n[lint]\nraw-color = \"off\"\nunused = \"warn\"\noff-canvas = \"off\"\n";
    let files = [
        (
            "art/t.art".to_string(),
            "@target t.png 40x40\nuse motion { idle spare }\ncircle 4 name:c\nplay idle\n"
                .to_string(),
        ),
        (
            "lib/motion.art".to_string(),
            "anim idle dur:1s\n  .c at:1,0\nstyle spare ink:#000000\n".to_string(),
        ),
    ]
    .into();
    let p = Project::from_sources(manifest, files).unwrap();
    let plan = plan(&p);
    assert!(plan.diagnostics.is_empty(), "{:?}", plan.diagnostics);
    let ev = evaluate(&p, &plan.jobs[0]);
    let unused: Vec<&str> = ev
        .diagnostics
        .iter()
        .filter(|d| d.code == "L0103")
        .map(|d| d.message.as_str())
        .collect();
    assert!(unused.iter().any(|m| m.contains("`spare`")), "{unused:?}");
    assert!(unused.iter().all(|m| !m.contains("`idle`")), "{unused:?}");
    near(at(node(ev.scene.as_ref().unwrap(), "c")), (21.0, 20.0));
}

#[test]
fn an_alias_counts_under_the_word_written() {
    let s = lib_scene(
        "@target t.png 40x40\nuse d\nlet alias = eye\nalias\neye\n",
        "def eye\n  circle 4\n",
    );
    assert_eq!(
        paths(&s),
        ["alias~1", "alias~1.circle~1", "eye~1", "eye~1.circle~1"]
    );
}

#[test]
fn a_missing_reference_in_if_is_reported_once() {
    let ev = run_with(
        "@target t.png 20x20\nif @missing\n  circle 1\ncircle 2\n",
        &[],
    );
    let n = ev
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("missing"))
        .count();
    assert_eq!(n, 1, "{}", show(&ev.diagnostics));
}

#[test]
fn off_canvas_on_a_later_frame_is_kept() {
    let manifest = "[project]\nout = \"out\"\nlib = [\"lib\"]\n\n[lint]\nraw-color = \"off\"\nunused = \"off\"\noff-canvas = \"error\"\n";
    let still_files = [(
        "art/t.art".to_string(),
        "@target t.png 40x40\ncircle 2 name:c\n".to_string(),
    )]
    .into();
    let still = Project::from_sources(manifest, still_files).unwrap();
    let still_plan = plan(&still);
    let still_ev = evaluate(&still, &still_plan.jobs[0]);
    assert!(
        still_ev.diagnostics.iter().all(|d| d.code != "L0102"),
        "{}",
        show(&still_ev.diagnostics)
    );
    let files = [(
        "art/t.art".to_string(),
        "@target t.gif 40x40 bg:#ffffff dur:1s fps:4\nanim slide dur:1s\n  .c at:key(0 80),0\ncircle 2 name:c\nplay slide\n".to_string(),
    )]
    .into();
    let p = Project::from_sources(manifest, files).unwrap();
    let plan = plan(&p);
    assert!(plan.diagnostics.is_empty(), "{:?}", plan.diagnostics);
    let sampled = crate::sample(&p, &plan.jobs[0]);
    let offs: Vec<_> = sampled
        .diagnostics
        .iter()
        .filter(|d| d.code == "L0102")
        .collect();
    assert_eq!(offs.len(), 1, "{}", show(&sampled.diagnostics));
    assert_eq!(offs[0].node, "c");
}

#[test]
fn an_off_canvas_node_stays_one_failure_when_the_wording_changes() {
    let manifest = "[project]\nout = \"out\"\nlib = [\"lib\"]\n\n[lint]\nraw-color = \"off\"\nunused = \"off\"\noff-canvas = \"error\"\n";
    let files = [(
        "art/t.art".to_string(),
        "@target t.gif 40x40 bg:#ffffff dur:1s fps:2\nanim slide dur:1s\n  .c at:key(22 80 ease:linear),0\ncircle 2 name:c\nplay slide\n".to_string(),
    )]
    .into();
    let p = Project::from_sources(manifest, files).unwrap();
    let plan = plan(&p);
    assert!(plan.diagnostics.is_empty(), "{:?}", plan.diagnostics);
    let sampled = crate::sample(&p, &plan.jobs[0]);
    let offs: Vec<_> = sampled
        .diagnostics
        .iter()
        .filter(|d| d.code == "L0102")
        .collect();
    assert_eq!(offs.len(), 1, "{}", show(&sampled.diagnostics));
    assert!(
        offs[0].message.contains("entirely outside"),
        "{}",
        offs[0].message
    );
    assert!(
        offs[0].notes.iter().any(|n| n.contains("first at")),
        "{:?}",
        offs[0].notes
    );
}

#[test]
fn sample_reports_a_gif_that_build_would_refuse_to_encode() {
    let fast = project(&[(
        "art/t.art",
        "@target t.gif 8x8 bg:#ffffff fps:120 dur:1s\ncircle 2\n",
    )]);
    let fast_plan = plan(&fast);
    let fast_sample = crate::sample(&fast, &fast_plan.jobs[0]);
    let zero = fast_sample
        .diagnostics
        .iter()
        .find(|d| d.code == "E0206")
        .unwrap_or_else(|| panic!("{}", show(&fast_sample.diagnostics)));
    assert!(
        zero.message.contains("more frames than centiseconds"),
        "{zero}"
    );
    assert_eq!(
        zero.fix.as_deref(),
        Some("lower `fps` or lengthen `dur` so each frame lasts at least 1cs")
    );
    let bare = project(&[("art/t.art", "@target t.gif 8x8 bg:#ffffff\ncircle 2\n")]);
    let bare_plan = plan(&bare);
    let bare_sample = crate::sample(&bare, &bare_plan.jobs[0]);
    assert!(
        bare_sample
            .diagnostics
            .iter()
            .any(|d| d.code == "E0206" && d.message.contains("clip has no duration")),
        "{}",
        show(&bare_sample.diagnostics)
    );
}

#[test]
fn a_clip_past_the_frame_cap_is_e0206() {
    let p = project(&[(
        "art/t.art",
        "@target t.gif 8x8 bg:#ffffff dur:5000s fps:24\ncircle 2\n",
    )]);
    let plan = plan(&p);
    assert!(plan.diagnostics.is_empty(), "{:?}", plan.diagnostics);
    let sampled = crate::sample(&p, &plan.jobs[0]);
    assert!(sampled.frames.is_empty(), "{}", sampled.frames.len());
    assert!(
        sampled
            .diagnostics
            .iter()
            .any(|d| d.code == "E0206" && d.message.contains("100000")),
        "{}",
        show(&sampled.diagnostics)
    );
}

#[test]
fn a_play_inside_if_sees_an_earlier_name() {
    let s = scene(
        "@target t.png 40x40\nanim bob dur:1s\n  .marker at:10,0\ncircle 4 name:marker\nif @marker\n  play bob\n",
    );
    near(at(node(&s, "marker")), (30.0, 20.0));
}

#[test]
fn a_hidden_parent_matches_a_channel_under_it() {
    let s = scene(
        "@target t.png 40x40\nanim idle dur:1s\n  .hero off\n  .hero.eye at:10,0\ndef hero\n  circle 4 name:eye\nhero name:hero\nplay idle\n",
    );
    assert!(
        paths(&s).iter().all(|p| !p.contains("eye")),
        "{:?}",
        paths(&s)
    );
}

#[test]
fn an_each_binding_shares_the_components_occurrence_base() {
    let s = lib_scene(
        "@target t.png 40x40\nuse d\neach part in [eye]\n  part\neach item in [1]\n  eye\n",
        "def eye\n  circle 4\n",
    );
    assert_eq!(
        paths(&s),
        [
            "part~1.0",
            "part~1.0.circle~1",
            "eye~1.0",
            "eye~1.0.circle~1"
        ]
    );
}

#[test]
fn a_patch_expression_is_reported_on_the_anim() {
    let ev = run_with(
        "@target t.png 40x40\nuse motion { idle }\ncircle 4 name:c\nplay idle\n",
        &[("lib/motion.art", "anim idle dur:1s\n  .c at:@missing\n")],
    );
    let d = ev
        .diagnostics
        .iter()
        .find(|d| d.file == "lib/motion.art")
        .unwrap_or_else(|| panic!("{}", show(&ev.diagnostics)));
    assert_eq!(d.span.line, 2, "{d}");
    assert!(d.notes.iter().any(|n| n.contains("in anim `idle`")), "{d}");
    assert!(
        ev.diagnostics.iter().all(|d| d.code != "E0340"),
        "{}",
        show(&ev.diagnostics)
    );
    let blend = run_with(
        "@target t.png 40x40\nuse motion { idle }\ncircle 4 name:c\nplay idle\n",
        &[("lib/motion.art", "anim idle dur:1s\n  .c at:#ffffff\n")],
    );
    let b = blend
        .diagnostics
        .iter()
        .find(|d| d.code == "E0320")
        .unwrap_or_else(|| panic!("{}", show(&blend.diagnostics)));
    assert_eq!(b.file, "lib/motion.art");
    assert_eq!(b.span.line, 2, "{b}");
    let unmatched = run_with(
        "@target t.png 40x40\nuse motion { idle }\ncircle 4 name:c\nplay idle\n",
        &[("lib/motion.art", "anim idle dur:1s\n  .missing at:1,0\n")],
    );
    let u = unmatched
        .diagnostics
        .iter()
        .find(|d| d.code == "E0340")
        .unwrap_or_else(|| panic!("{}", show(&unmatched.diagnostics)));
    assert_eq!(u.file, "art/t.art");
    assert_eq!(u.span.line, 4, "{u}");
}

#[test]
fn two_calls_to_one_rule_stay_two_failures() {
    let p = project(&[(
        "art/t.art",
        "@target t.gif 8x8 bg:#ffffff dur:1s fps:2\nrule bad\n  assert false\nassert bad()\nassert bad()\ncircle 1\n",
    )]);
    let plan = plan(&p);
    assert!(plan.diagnostics.is_empty(), "{:?}", plan.diagnostics);
    let sampled = crate::sample(&p, &plan.jobs[0]);
    let fails: Vec<_> = sampled
        .diagnostics
        .iter()
        .filter(|d| d.code == "C0101")
        .collect();
    assert_eq!(fails.len(), 2, "{}", show(&sampled.diagnostics));
    let mut sites: Vec<_> = fails
        .iter()
        .filter_map(|d| d.notes.iter().find(|n| n.contains("applied at")))
        .collect();
    sites.sort();
    assert_eq!(sites.len(), 2, "{sites:?}");
    assert_ne!(sites[0], sites[1]);
}

#[test]
fn an_anim_style_replaces_the_rest_style() {
    let s = scene(
        "@target t.png 40x40\nstyle red fill:#ff0000\nstyle blue fill:#0000ff\nanim mood dur:1s\n  .c style:blue\ncircle 4 style:red name:c\nplay mood\n",
    );
    assert_eq!(fill_hex(node(&s, "c")), "#0000ff");
}

#[test]
fn a_later_file_play_wins_over_a_play_under_the_instance() {
    let later = scene(
        "@target t.png 40x40\nanim inner dur:1s\n  .eye a:.2\nanim outer dur:1s\n  .hero.eye a:.8\ndef hero\n  circle 4 name:eye\nhero name:hero\n  play inner\nplay outer\n",
    );
    let over = node(&later, "hero.eye").opacity;
    assert!((over - 0.8).abs() < 1e-4, "{over}");
    let earlier = scene(
        "@target t.png 40x40\nanim inner dur:1s\n  .eye a:.2\nanim outer dur:1s\n  .hero.eye a:.8\ndef hero\n  circle 4 name:eye\nplay outer\nhero name:hero\n  play inner\n",
    );
    let under = node(&earlier, "hero.eye").opacity;
    assert!((under - 0.2).abs() < 1e-4, "{under}");
}

#[test]
fn a_list_clip_stays_a_target_option_after_format() {
    let src = "@target t-{clip}.gif 8x8 clip:[idle blink] each x:[one]\ncircle 1\n";
    let formatted = artscript_syntax::format(&artscript_syntax::parse(src).unwrap());
    let before = plan(&project(&[("art/t.art", src)]));
    let after = plan(&project(&[("art/t.art", &formatted)]));
    assert!(
        before.jobs.is_empty() && after.jobs.is_empty(),
        "{:?}",
        after.jobs
    );
    let codes = |p: &crate::targets::Plan| -> Vec<String> {
        p.diagnostics.iter().map(|d| d.code.clone()).collect()
    };
    assert_eq!(codes(&before), codes(&after));
    assert!(
        after.diagnostics.iter().any(|d| d.code == "E0207"),
        "{:?}",
        after.diagnostics
    );
}

#[test]
fn a_loop_reached_after_the_first_frame_loops_the_clip() {
    let late = project(&[(
        "art/t.art",
        "@target t.gif 8x8 bg:#ffffff dur:2s fps:2\nanim pulse dur:1s loop\n  .c a:.5\ncircle 4 name:c\nif t > 1\n  play pulse\n",
    )]);
    let late_plan = plan(&late);
    assert!(
        late_plan.diagnostics.is_empty(),
        "{:?}",
        late_plan.diagnostics
    );
    let late_sample = crate::sample(&late, &late_plan.jobs[0]);
    assert!(late_sample.loops, "{}", show(&late_sample.diagnostics));
    let once = project(&[(
        "art/t.art",
        "@target t.gif 8x8 bg:#ffffff dur:2s fps:2\nanim pulse dur:1s\n  .c a:.5\ncircle 4 name:c\nif t > 1\n  play pulse\n",
    )]);
    let once_plan = plan(&once);
    assert!(!crate::sample(&once, &once_plan.jobs[0]).loops);
}

#[test]
fn a_gif_without_an_opaque_background_is_e0206() {
    let bare = project(&[("art/t.art", "@target t.gif 8x8 dur:1s fps:2\ncircle 1\n")]);
    let bare_plan = plan(&bare);
    let bare_sample = crate::sample(&bare, &bare_plan.jobs[0]);
    assert!(
        bare_sample
            .diagnostics
            .iter()
            .any(|d| d.code == "E0206" && d.message.contains("opaque")),
        "{}",
        show(&bare_sample.diagnostics)
    );
    let painted = project(&[(
        "art/t.art",
        "@target t.gif 8x8 bg:#ffffff dur:1s fps:2\ncircle 1\n",
    )]);
    let painted_plan = plan(&painted);
    let painted_sample = crate::sample(&painted, &painted_plan.jobs[0]);
    assert!(
        painted_sample.diagnostics.iter().all(|d| d.code != "E0206"),
        "{}",
        show(&painted_sample.diagnostics)
    );
}

#[test]
fn added_times_do_not_grow_a_boundary_frame() {
    let p = project(&[(
        "art/t.art",
        "@target t.gif 8x8 bg:#ffffff fps:10\nanim idle dur:200ms\n  .c at:1,0\ncircle 4 name:c\nplay idle start:100ms\n",
    )]);
    let plan = plan(&p);
    assert!(plan.diagnostics.is_empty(), "{:?}", plan.diagnostics);
    let sampled = crate::sample(&p, &plan.jobs[0]);
    assert!(
        sampled.diagnostics.iter().all(|d| d.code != "E0206"),
        "{}",
        show(&sampled.diagnostics)
    );
    assert_eq!(sampled.frames.len(), 3, "{}", sampled.dur);
}

#[test]
fn a_boolean_is_not_a_time() {
    let cases = [
        "@target t.png 40x40\nanim idle dur:true\n  .c a:.2\ncircle 4 name:c\nplay idle\n",
        "@target t.png 40x40\nanim idle dur:1s\n  .c a:.2\ncircle 4 name:c\nplay idle start:false\n",
        "@target t.png 40x40\nanim idle dur:1s\n  .c a:key(0 1 dur:true)\ncircle 4 name:c\nplay idle\n",
    ];
    for src in cases {
        let ev = run_with(src, &[]);
        let d = ev
            .diagnostics
            .iter()
            .find(|d| d.code == "E0320")
            .unwrap_or_else(|| panic!("{}", show(&ev.diagnostics)));
        assert!(
            d.message.contains("boolean"),
            "{}\n{}",
            d.message,
            show(&ev.diagnostics)
        );
    }
}

#[test]
fn a_percent_rest_accepts_a_numeric_offset() {
    let held = scene(
        "@target t.png 40x40\nanim nudge dur:1s\n  .c at:key(0,0 10,0)\ncircle 4 at:50%,50% name:c\nplay nudge\n",
    );
    near(at(node(&held, "c")), (20.0, 20.0));
    let moved = scene(
        "@target t.png 40x40\nanim nudge dur:1s\n  .c at:10,0\ncircle 4 at:50%,50% name:c\nplay nudge\n",
    );
    near(at(node(&moved, "c")), (30.0, 20.0));
    let percents = scene(
        "@target t.png 40x40\nanim nudge dur:1s\n  .c at:10%,0%\ncircle 4 at:10%,10% name:c\nplay nudge\n",
    );
    near(at(node(&percents, "c")), (8.0, 4.0));
}

#[test]
fn a_let_inside_if_under_an_instance_stays_in_the_branch() {
    let leaked = run_with(
        "@target t.png 40x40\nanim idle dur:1s\n  .eye a:.5\ndef hero\n  circle 4 name:eye\nhero name:hero\n  if true\n    let delay = 0\n  play idle start:delay\n",
        &[],
    );
    let d = leaked
        .diagnostics
        .iter()
        .find(|d| d.code == "E0320")
        .unwrap_or_else(|| panic!("{}", show(&leaked.diagnostics)));
    assert!(d.message.contains("a word"), "{d}");
    let eye = node(leaked.scene.as_ref().unwrap(), "hero.eye");
    assert!((eye.opacity - 1.0).abs() < 1e-4, "{}", eye.opacity);
    let direct = scene(
        "@target t.png 40x40\nanim idle dur:1s\n  .eye a:.5\ndef hero\n  circle 4 name:eye\nhero name:hero\n  let delay = 0\n  play idle start:delay\n",
    );
    assert!((node(&direct, "hero.eye").opacity - 0.5).abs() < 1e-4);
    let inside = scene(
        "@target t.png 40x40\nanim idle dur:1s\n  .eye a:.5\ndef hero\n  circle 4 name:eye\nhero name:hero\n  if true\n    let delay = 0\n    play idle start:delay\n",
    );
    assert!((node(&inside, "hero.eye").opacity - 0.5).abs() < 1e-4);
}

#[test]
fn an_imported_anim_style_resolves_in_its_module() {
    let lib = "style blue fill:#0000ff\nanim idle dur:1s\n  .c style:blue\n";
    let s = scene_with(
        "@target t.png 40x40\nstyle blue fill:#ff0000\nuse motion { idle }\ncircle 4 name:c\nplay idle\n",
        &[("lib/motion.art", lib)],
    );
    assert_eq!(fill_hex(node(&s, "c")), "#0000ff");
    let kept = scene_with(
        "@target t.png 40x40\nuse motion { idle }\ncircle 4 fill:#00ff00 name:c\nplay idle\n",
        &[("lib/motion.art", lib)],
    );
    assert_eq!(fill_hex(node(&kept, "c")), "#00ff00");
    let missing = run_with(
        "@target t.png 40x40\nstyle blue fill:#ff0000\nuse motion { idle }\ncircle 4 name:c\nplay idle\n",
        &[("lib/motion.art", "anim idle dur:1s\n  .c style:blue\n")],
    );
    assert!(
        missing.diagnostics.iter().any(|d| d.code == "E0307"),
        "{}",
        show(&missing.diagnostics)
    );
}

#[test]
fn a_gif_that_would_retain_scenes_past_the_budget_is_e0206_from_sample() {
    let heavy =
        "@target t.gif 8x8 bg:#ffffff dur:10s fps:100\neach i in 0..1500\n  circle 1 at:i,0\n";
    let over = project(&[("art/t.art", heavy)]);
    let over_plan = plan(&over);
    assert!(
        over_plan.diagnostics.is_empty(),
        "{:?}",
        over_plan.diagnostics
    );
    let over_sample = crate::sample(&over, &over_plan.jobs[0]);
    assert!(
        over_sample
            .diagnostics
            .iter()
            .any(|d| { d.code == "E0206" && d.message.contains("a gif retains more than 256MiB") }),
        "{}",
        show(&over_sample.diagnostics)
    );
    assert!(
        over_sample.frames.is_empty(),
        "{}",
        over_sample.frames.len()
    );
    let one = project(&[(
        "art/t.art",
        "@target t.gif 8x8 bg:#ffffff dur:1s fps:1\neach i in 0..1500\n  circle 1 at:i,0\n",
    )]);
    let one_plan = plan(&one);
    let one_sample = crate::sample(&one, &one_plan.jobs[0]);
    assert!(
        one_sample
            .diagnostics
            .iter()
            .all(|d| !d.message.contains("retains")),
        "{}",
        show(&one_sample.diagnostics)
    );
    assert_eq!(one_sample.frames.len(), 1, "{}", one_sample.dur);
}

#[test]
fn a_gif_raster_over_the_budget_is_e0206_from_sample() {
    let huge = project(&[(
        "art/t.art",
        "@target t.gif 4096x4096 bg:#ffffff dur:1s fps:5\ncircle 1\n",
    )]);
    let huge_plan = plan(&huge);
    assert!(
        huge_plan.diagnostics.is_empty(),
        "{:?}",
        huge_plan.diagnostics
    );
    let huge_sample = crate::sample(&huge, &huge_plan.jobs[0]);
    assert!(
        huge_sample.diagnostics.iter().any(|d| {
            d.code == "E0206" && d.message.contains("a gif raster is larger than 256MiB")
        }),
        "{}",
        show(&huge_sample.diagnostics)
    );
    let small = project(&[(
        "art/t.art",
        "@target t.gif 8x8 bg:#ffffff dur:1s fps:2\ncircle 1\n",
    )]);
    let small_plan = plan(&small);
    let small_sample = crate::sample(&small, &small_plan.jobs[0]);
    assert!(
        small_sample
            .diagnostics
            .iter()
            .all(|d| !d.message.contains("raster")),
        "{}",
        show(&small_sample.diagnostics)
    );
}

#[test]
fn an_anim_cannot_rename_a_node() {
    let ev = run_with(
        "@target t.png 40x40\nanim idle dur:1s\n  .c name:d\n  .c a:.5\ncircle 4 name:c\nplay idle\n",
        &[],
    );
    let d = ev
        .diagnostics
        .iter()
        .find(|d| d.code == "E0320")
        .unwrap_or_else(|| panic!("{}", show(&ev.diagnostics)));
    assert!(d.message.contains("name"), "{d}");
    assert_eq!(d.file, "art/t.art");
    let scene = ev.scene.as_ref().unwrap();
    assert_eq!(paths(scene), ["c"]);
    assert!((node(scene, "c").opacity - 0.5).abs() < 1e-4);
    let patched = run_with(
        "@target t.png 40x40\nuse d\nd name:d\n  .eye name:other\n",
        &[("lib/d.art", "def d\n  circle 4 name:eye\n")],
    );
    let p = patched
        .diagnostics
        .iter()
        .find(|d| d.code == "E0320")
        .unwrap_or_else(|| panic!("{}", show(&patched.diagnostics)));
    assert!(p.message.contains("name"), "{p}");
    assert_eq!(paths(patched.scene.as_ref().unwrap()), ["d", "d.eye"]);
}

#[test]
fn a_gif_side_over_65535px_is_e0206_from_sample() {
    let wide = project(&[(
        "art/t.art",
        "@target t.gif 70000x1 bg:#ffffff dur:1s fps:1\ncircle 1\n",
    )]);
    let wide_plan = plan(&wide);
    assert!(
        wide_plan.diagnostics.is_empty(),
        "{:?}",
        wide_plan.diagnostics
    );
    let wide_sample = crate::sample(&wide, &wide_plan.jobs[0]);
    assert!(
        wide_sample
            .diagnostics
            .iter()
            .any(|d| d.code == "E0206" && d.message.contains("65535px")),
        "{}",
        show(&wide_sample.diagnostics)
    );
}

#[test]
fn a_rule_line_number_does_not_choose_the_worst_frame() {
    use artscript_syntax::ast::Span;
    let span = Span { line: 2, col: 1 };
    let note = "via rule `bad` (lib/d.art:400) applied at art/t.art:3";
    let mild = Diagnostic::error("C0101", span, "gap")
        .in_file("art/t.art")
        .at_node("a")
        .with_note("gap is 1")
        .with_note(note)
        .with_fix("nudge 1");
    let worse = Diagnostic::error("C0101", span, "gap")
        .in_file("art/t.art")
        .at_node("a")
        .with_note("gap is 9")
        .with_note(note)
        .with_fix("nudge 9");
    let out = super::aggregate_frames(&[(0.0, vec![mild]), (0.5, vec![worse])]);
    let a = out.iter().find(|d| d.node == "a").unwrap();
    assert_eq!(a.fix.as_deref(), Some("nudge 9"));
    assert!(
        a.notes.iter().any(|n| n.contains("worst at 0.5s")),
        "{:?}",
        a.notes
    );
}

#[test]
fn a_geometry_guard_keeps_an_instance_play_ahead_of_a_later_play() {
    let s = scene(
        "@target t.png 40x40\nanim inner dur:1s\n  .eye a:.8 at:10,0\nanim outer dur:1s\n  .hero.eye a:.2\ndef hero size:40x40\n  circle 4 name:eye\ncircle 4 name:marker\nhero name:hero\n  if @marker.w > 0\n    play inner\nplay outer\n",
    );
    let eye = node(&s, "hero.eye");
    assert!((eye.opacity - 0.2).abs() < 1e-4, "{}", eye.opacity);
    near(at(eye), (10.0, 0.0));
}

#[test]
fn an_anim_rand_ignores_a_toggling_let() {
    let p = project(&[(
        "art/t.art",
        "@target t.gif 40x40 bg:#ffffff dur:1s fps:2\nanim idle dur:1s\n  .c at:rand(0 10),0\ncircle 4 name:c\nif t > 0\n  let n = 1\nplay idle\n",
    )]);
    let plan = plan(&p);
    assert!(plan.diagnostics.is_empty(), "{:?}", plan.diagnostics);
    let sampled = crate::sample(&p, &plan.jobs[0]);
    assert!(
        sampled.diagnostics.iter().all(|d| d.code != "E0320"),
        "{}",
        show(&sampled.diagnostics)
    );
    assert_eq!(sampled.frames.len(), 2, "{}", sampled.dur);
    let xs: Vec<f32> = sampled
        .frames
        .iter()
        .map(|f| {
            f.scene
                .children
                .iter()
                .find(|n| n.path == "c")
                .unwrap()
                .transform
                .translate_x()
        })
        .collect();
    assert!((xs[0] - xs[1]).abs() < 1e-4, "{xs:?}");
    let lib = [("lib/motion.art", "anim idle dur:1s\n  .c at:rand(0 10),0\n")];
    let bare = scene_with(
        "@target t.png 40x40\nuse motion { idle }\ncircle 4 name:c\nplay idle\n",
        &lib,
    );
    let noisy = scene_with(
        "@target t.png 40x40\nuse motion { idle }\nlet n = 1\nif true\n  let m = 2\ncircle 4 name:c\nplay idle\n",
        &lib,
    );
    assert_eq!(at(node(&bare, "c")), at(node(&noisy, "c")));
}

#[test]
fn a_picked_component_keeps_one_path() {
    let s = scene(
        "@target t.png 40x40\ndef a\n  circle 4 name:dot\ndef b\n  rect 4 name:dot\ndef c\n  circle 4 name:dot\nlet which = pick([a b c])\nwhich name:which\na name:a\nb name:b\nc name:c\n",
    );
    let ps = paths(&s);
    let mut uniq = ps.clone();
    uniq.sort();
    uniq.dedup();
    assert_eq!(ps.len(), uniq.len(), "{ps:?}");
    let each = scene(
        "@target t.png 40x40\ndef a\n  circle 4 name:dot\ndef b\n  rect 4 name:dot\ndef c\n  circle 4 name:dot\neach part in [pick([a b c])]\n  part name:part\na name:a\nb name:b\nc name:c\n",
    );
    let ep = paths(&each);
    let mut eu = ep.clone();
    eu.sort();
    eu.dedup();
    assert_eq!(ep.len(), eu.len(), "{ep:?}");
}

#[test]
fn an_unreadable_each_keeps_an_instance_play_ahead_of_a_later_play() {
    let s = scene(
        "@target t.png 40x40\nanim inner dur:1s\n  .eye a:.8\nanim outer dur:1s\n  .hero.eye a:.2\ndef hero size:40x40\n  circle 4 name:eye\ncircle 4 name:marker\nhero name:hero\n  each x in [@marker.w]\n    play inner\nplay outer\n",
    );
    let eye = node(&s, "hero.eye");
    assert!((eye.opacity - 0.2).abs() < 1e-4, "{}", eye.opacity);
}

#[test]
fn a_play_start_rand_ignores_a_toggling_let() {
    let body = |guard: &str| {
        format!(
            "@target t.gif 40x40 bg:#ffffff dur:1s fps:2\nanim idle dur:1s\n  .c at:t*10,0\ncircle 4 name:c\nif {guard}\n  let n = rand(0 1)\nplay idle start:rand(0 1)\n"
        )
    };
    let quiet_src = body("false");
    let noisy_src = body("t > 0");
    let quiet = project(&[("art/t.art", &quiet_src)]);
    let noisy = project(&[("art/t.art", &noisy_src)]);
    let xs = |p: &Project| {
        let plan = plan(p);
        assert!(plan.diagnostics.is_empty(), "{:?}", plan.diagnostics);
        let sampled = crate::sample(p, &plan.jobs[0]);
        assert!(
            sampled.diagnostics.iter().all(|d| !d.code.starts_with('E')),
            "{}",
            show(&sampled.diagnostics)
        );
        sampled
            .frames
            .iter()
            .map(|f| {
                f.scene
                    .children
                    .iter()
                    .find(|n| n.path == "c")
                    .unwrap()
                    .transform
                    .translate_x()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(xs(&quiet), xs(&noisy));
}

#[test]
fn a_gif_scale_list_does_not_refine_a_tapered_stroke() {
    let one = scene(
        "@target t.gif 40x40 scale:1 bg:#ffffff\ncurve 0,0 20,0 40,0 stroke:#111,4 taper:1 name:ink\n",
    );
    let listed = scene(
        "@target t.gif 40x40 scale:1,2 bg:#ffffff\ncurve 0,0 20,0 40,0 stroke:#111,4 taper:1 name:ink\n",
    );
    let n = |s: &Scene| match &node(s, "ink").kind {
        NodeKind::Shape(shape) => shape.geometry.count_points(),
        NodeKind::Group { .. } => panic!("ink is a group"),
    };
    assert_eq!(n(&one), n(&listed));
}

#[test]
fn project_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Project>();
}

#[test]
fn a_duration_error_is_reported_on_the_anim() {
    let ev = run_with(
        "@target t.png 40x40\nuse motion { idle }\ncircle 4 name:c\nplay idle\n",
        &[("lib/motion.art", "anim idle dur:missing\n  .c a:.5\n")],
    );
    let d = ev
        .diagnostics
        .iter()
        .find(|d| d.code.starts_with('E'))
        .unwrap_or_else(|| panic!("no error in {}", show(&ev.diagnostics)));
    assert_eq!(d.file, "lib/motion.art", "{d}");
    assert_eq!(d.span.line, 1, "{d}");
    assert!(
        d.notes.iter().any(|n| n.contains("played at art/t.art:4")),
        "{d}"
    );
}

#[test]
fn an_anim_duration_rand_ignores_a_toggling_let() {
    let body = |guard: &str| {
        format!(
            "@target t.gif 40x40 bg:#ffffff dur:1s fps:4\nanim idle dur:rand(1 2)\n  .c at:key(0,0 10,0)\ncircle 4 name:c\nif {guard}\n  let n = rand(0 1)\nplay idle\n"
        )
    };
    let xs = |src: &str| {
        let p = project(&[("art/t.art", src)]);
        let plan = plan(&p);
        assert!(plan.diagnostics.is_empty(), "{:?}", plan.diagnostics);
        let sampled = crate::sample(&p, &plan.jobs[0]);
        assert!(
            sampled.diagnostics.iter().all(|d| !d.code.starts_with('E')),
            "{}",
            show(&sampled.diagnostics)
        );
        sampled
            .frames
            .iter()
            .map(|f| at(node(&f.scene, "c")).0)
            .collect::<Vec<_>>()
    };
    assert_eq!(xs(&body("false")), xs(&body("t > 0")));
}

#[test]
fn a_path_only_anim_patch_that_matches_nothing_is_e0340() {
    let ev = run_with(
        "@target t.png 40x40\nanim idle dur:1s\n  .missing\ncircle 4 name:c\nplay idle\n",
        &[],
    );
    assert!(
        ev.diagnostics.iter().any(|d| d.code == "E0340"),
        "{}",
        show(&ev.diagnostics)
    );
    let hit = run_with(
        "@target t.png 40x40\nanim idle dur:1s\n  .c\ncircle 4 name:c\nplay idle\n",
        &[],
    );
    assert!(hit.diagnostics.is_empty(), "{}", show(&hit.diagnostics));
}

#[test]
fn two_plays_of_an_unmatched_patch_are_two_e0340s() {
    let ev = run_with(
        "@target t.png 40x40\nanim idle dur:1s\n  .missing a:.5 x:2\ncircle 4 name:c\nplay idle\nplay idle start:1s\n",
        &[],
    );
    let lines: Vec<_> = ev
        .diagnostics
        .iter()
        .filter(|d| d.code == "E0340")
        .map(|d| d.span.line)
        .collect();
    assert_eq!(lines, [5, 6], "{}", show(&ev.diagnostics));
}

#[test]
fn an_animated_style_error_is_reported_on_the_anim() {
    let ev = run_with(
        "@target t.png 40x40\nuse motion { idle }\ncircle 4 name:c\nplay idle\n",
        &[("lib/motion.art", "anim idle dur:1s\n  .c style:blue\n")],
    );
    let d = ev
        .diagnostics
        .iter()
        .find(|d| d.code.starts_with('E'))
        .unwrap_or_else(|| panic!("no error in {}", show(&ev.diagnostics)));
    assert_eq!(d.file, "lib/motion.art", "{d}");
    assert_eq!(d.span.line, 2, "{d}");
}

#[test]
fn a_boolean_key_position_is_e0320() {
    let ev = run_with(
        "@target t.png 40x40\ncircle 4 at:key(0,0 10,0 dur:1s at:[false true]) name:c\n",
        &[],
    );
    assert!(
        ev.diagnostics.iter().any(|d| d.code == "E0320"),
        "{}",
        show(&ev.diagnostics)
    );
}

#[test]
fn an_instance_play_inside_a_group_keeps_its_source_order() {
    let s = scene(
        "@target t.png 40x40\nanim inner dur:1s\n  .eye a:.8\nanim outer dur:1s\n  .hero.eye a:.2\ndef hero size:40x40\n  circle 4 name:eye\ngroup root\n  hero name:hero\n    play inner\nplay outer\n",
    );
    let eye = node(&s, "hero.eye");
    assert!((eye.opacity - 0.2).abs() < 1e-4, "{}", eye.opacity);
}

#[test]
fn two_plays_of_a_bad_duration_stay_two_failures_across_frames() {
    let src = "@target t.gif 40x40 bg:#ffffff dur:1s fps:2\nanim idle dur:missing\n  .c a:.5\ncircle 4 name:c\nplay idle\nplay idle\n";
    let p = project(&[("art/t.art", src)]);
    let plan = plan(&p);
    assert!(plan.diagnostics.is_empty(), "{:?}", plan.diagnostics);
    let sampled = crate::sample(&p, &plan.jobs[0]);
    let sites: Vec<_> = sampled
        .diagnostics
        .iter()
        .filter(|d| d.code == "E0320")
        .flat_map(|d| d.notes.iter().filter(|n| n.starts_with("played at ")))
        .cloned()
        .collect();
    assert_eq!(
        sites,
        ["played at art/t.art:5", "played at art/t.art:6"],
        "{}",
        show(&sampled.diagnostics)
    );
}

#[test]
fn a_direct_instance_let_stays_in_the_call() {
    let ev = run_with(
        "@target t.png 40x40\ndef hero size:40x40\n  circle 4 name:eye\nhero name:hero\n  let delay = 1\ncircle 4 at:delay,0 name:x\n",
        &[],
    );
    assert!(
        ev.diagnostics
            .iter()
            .any(|d| d.code.starts_with('E') && d.span.line == 6),
        "{}",
        show(&ev.diagnostics)
    );
}

fn eval_lint(src: &str, lint: &str) -> Evaluation {
    let manifest = format!(
        "[project]\nout = \"out\"\nlib = [\"lib\"]\n\n[lint]\nraw-color = \"off\"\nunused = \"off\"\noff-canvas = \"off\"\n{lint}"
    );
    let files = [("art/t.art".to_string(), src.to_string())].into();
    let p = Project::from_sources(&manifest, files).unwrap();
    let plan = plan(&p);
    assert!(plan.diagnostics.is_empty(), "{:?}", plan.diagnostics);
    evaluate(&p, &plan.jobs[0])
}

fn pixel(scene: &Scene, x: i32, y: i32) -> [u8; 4] {
    let (w, h, buf) = artscript_render::render_rgba(scene, 1.0, artscript_render::Backend::Cpu).unwrap();
    assert!(x >= 0 && y >= 0 && x < w as i32 && y < h as i32, "{w}x{h} has no {x},{y}");
    let i = ((y as u32 * w) + x as u32) as usize * 4;
    [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
}

#[test]
fn a_nose_above_the_eyes_keeps_the_patched_eye_red() {
    let face = |nose: bool| {
        format!(
            "@target t.png 80x40\ndef face\n{}  circle 10 at:-16,0 fill:#ffffff name:eye_l\n  circle 10 at:16,0 fill:#ffffff name:eye_r\nface name:face at:40,20\n  .eye_r fill:#ff0000\n",
            if nose {
                "  circle 6 at:0,10 fill:#c47a4a name:nose\n"
            } else {
                ""
            }
        )
    };
    let before = scene(&face(false));
    let after = scene(&face(true));
    let eye = pixel(&before, 56, 20);
    assert_eq!(pixel(&after, 56, 20), eye, "right eye pixel changed");
    assert!(eye[0] > 200 && eye[1] < 40 && eye[2] < 40, "{eye:?}");
    assert_ne!(
        pixel(&before, 40, 30),
        pixel(&after, 40, 30),
        "the nose did not change a pixel"
    );

    let missed = run_with(
        "@target t.png 80x40\ndef face\n  circle 10 at:-16,0 fill:#ffffff\n  circle 10 at:16,0 fill:#ffffff\nface name:face at:40,20\n  .circle2 fill:#ff0000\n",
        &[],
    );
    let d = diag(&missed, "E0340");
    assert_eq!(d.file, "art/t.art");
    assert_eq!(d.span.line, 6);
    assert!(d.fix.as_deref().unwrap().contains("add name:"), "{d}");
}

#[test]
fn e0333_is_reported_at_the_second_name() {
    let ev = run_with(
        "@target t.png 40x40\ngroup beam\n  circle 4 name:beam\n",
        &[],
    );
    let d = diag(&ev, "E0333");
    assert_eq!(d.file, "art/t.art");
    assert_eq!(d.span.line, 3);
    assert_eq!(d.notes, ["first named at line 2"]);
    assert_eq!(d.fix.as_deref(), Some("rename one of them"));

    let shared = run_with(
        "@target t.png 40x40\ndef a\n  circle 4 name:lips\ndef b\n  rect 4 name:lips\nif true\n  circle 4 name:lid\nelse\n  rect 4 name:lid\n",
        &[],
    );
    assert!(
        shared.diagnostics.iter().all(|d| d.code != "E0333"),
        "{}",
        show(&shared.diagnostics)
    );
}

#[test]
fn e0312_rejects_a_bad_name_and_an_unnamed_call_that_holds_one() {
    let bad = run_with(
        "@target t.png 40x40\ncircle 4 name:{x}\nlayer hat name:cap\n",
        &[],
    );
    assert!(
        bad.diagnostics
            .iter()
            .any(|d| d.code == "E0312" && d.span.line == 2),
        "{}",
        show(&bad.diagnostics)
    );
    let hat = bad
        .diagnostics
        .iter()
        .find(|d| d.code == "E0312" && d.span.line == 3)
        .unwrap_or_else(|| panic!("{}", show(&bad.diagnostics)));
    assert!(hat.message.contains("hat"), "{hat}");

    let bare = run_with(
        "@target t.png 40x40\ndef _pillar\n  rect 2x12 name:shaft\n_pillar\n",
        &[],
    );
    let call = diag(&bare, "E0312");
    assert_eq!(call.span.line, 4);
    assert_eq!(call.fix.as_deref(), Some("add name:"));

    let named = scene(
        "@target t.png 40x40\ndef _pillar\n  rect 2x12 name:shaft\n_pillar name:pillar_l\n",
    );
    node(&named, "pillar_l.shaft");
    let shifted = run_with(
        "@target t.png 40x40\ndef _pillar\n  rect 2x12 name:shaft\n_pillar\n_pillar name:pillar_l\n",
        &[],
    );
    assert!(
        shifted
            .diagnostics
            .iter()
            .any(|d| d.code == "E0312" && d.span.line == 4),
        "{}",
        show(&shifted.diagnostics)
    );
    assert!(
        shifted
            .diagnostics
            .iter()
            .all(|d| d.span.line != 5 || d.code != "E0312"),
        "{}",
        show(&shifted.diagnostics)
    );
    node(shifted.scene.as_ref().unwrap(), "pillar_l.shaft");
}

#[test]
fn e0340_for_a_hop_through_a_group_names_the_patch_line() {
    let ev = run_with(
        "@target t.png 40x40\ndef prop\n  group face\n    circle 4 name:leaf\nprop name:prop\n  .face.leaf fill:#ff0000\n",
        &[],
    );
    let d = diag(&ev, "E0340");
    assert_eq!(d.file, "art/t.art");
    assert_eq!(d.span.line, 6);
    assert_eq!(d.fix.as_deref(), Some("write .leaf"));
}

#[test]
fn e0343_for_a_family_and_for_a_name_in_two_scopes() {
    let family = run_with(
        "@target t.png 40x40\ndef eye\n  each i in 0..2\n    circle 3 name:pupil\n  copy @pupil\neye name:eye\n",
        &[],
    );
    let d = diag(&family, "E0343");
    assert_eq!(d.span.line, 5);
    assert!(d.notes.iter().any(|n| n.contains(".pupil.0")), "{d}");
    assert!(d.notes.iter().any(|n| n.contains(".pupil.1")), "{d}");

    let both = run_with(
        "@target t.png 40x40\ndef host\n  circle 4 name:x\n  slot hole\ncircle 4 name:x\nhost name:host\n  hole:\n    copy @x\n",
        &[],
    );
    let d = diag(&both, "E0343");
    assert_eq!(d.span.line, 8);
    assert!(d.notes.iter().any(|n| n.contains("`host`")), "{d}");
    assert!(d.notes.iter().any(|n| n.contains("this file")), "{d}");
}

#[test]
fn l0104_warns_once_when_an_unnamed_circle_draws_a_seed() {
    let src = "@target t.png 40x40\ncircle 4 at:rand(0 10),0\n";
    let ev = eval_lint(src, "");
    let hits: Vec<_> = ev.diagnostics.iter().filter(|d| d.code == "L0104").collect();
    assert_eq!(hits.len(), 1, "{}", show(&ev.diagnostics));
    assert_eq!(hits[0].file, "art/t.art");
    assert_eq!(hits[0].span.line, 2);
    assert_eq!(hits[0].fix.as_deref(), Some("add name:"));
    let quiet = eval_lint(src, "positional-seed = \"off\"\n");
    assert!(
        quiet.diagnostics.iter().all(|d| d.code != "L0104"),
        "{}",
        show(&quiet.diagnostics)
    );
}

#[test]
fn wrapping_a_named_node_in_a_group_keeps_its_key_and_grain_seed() {
    let bare = scene("@target t.png 40x40\ncircle 8 name:head\n  fx grain .2\n");
    let wrapped = scene(
        "@target t.png 40x40\ngroup name:face\n  circle 8 name:head\n    fx grain .2\n",
    );
    assert_eq!(
        grain_seed(node(&bare, "head")),
        grain_seed(node(&wrapped, "head"))
    );
    assert!(paths(&wrapped).contains(&"head".to_string()));
}

#[test]
fn a_let_above_each_over_pick_does_not_rename_a_node() {
    let lib = "def a\n  circle 4 name:dot\ndef b\n  rect 4 name:dot\n";
    let direct = lib_scene(
        "@target t.png 40x40\nuse d\nlet which = a\neach part in [which]\n  part name:piece\n",
        lib,
    );
    let picked = lib_scene(
        "@target t.png 40x40\nuse d\nlet which = pick([a b])\neach part in [which]\n  part name:piece\n",
        lib,
    );
    assert_eq!(paths(&direct), paths(&picked));
    assert_eq!(paths(&picked), ["piece.0", "piece.0.dot"]);
}

fn walk_deep<'a>(nodes: &'a [Node], out: &mut Vec<&'a Node>) {
    for n in nodes {
        out.push(n);
        if let Some(mask) = &n.mask
            && let artscript_ir::MaskSource::Nodes(inner) = &mask.source
        {
            walk_deep(inner, out);
        }
        walk_deep(n.children(), out);
    }
}

fn node_print(n: &Node) -> String {
    let geom = match &n.kind {
        NodeKind::Shape(s) => s.geometry.to_svg(),
        NodeKind::Group { .. } => String::new(),
    };
    format!("{:?} a:{} {geom} {:?}", n.transform, n.opacity, n.effects)
}

fn stable_print(scene: &Scene) -> std::collections::BTreeMap<String, Vec<String>> {
    let mut all = Vec::new();
    walk_deep(&scene.children, &mut all);
    let mut map: std::collections::BTreeMap<String, Vec<String>> = std::collections::BTreeMap::new();
    for n in all {
        if n.path.contains('~') {
            continue;
        }
        map.entry(n.path.clone()).or_default().push(node_print(n));
    }
    for prints in map.values_mut() {
        prints.sort();
    }
    map
}

fn insert_probe(src: &str) -> String {
    let mut out = src.to_string();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("circle 4 at:-80,-80\nrect 4x4 at:-80,-80 name:__probe\n");
    out
}

fn chibi_jobs() -> (Project, Vec<crate::TargetJob>) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/chibi");
    let project = crate::load(&root).expect("examples/chibi");
    let planned = plan(&project);
    assert!(planned.diagnostics.is_empty(), "{:?}", planned.diagnostics);
    let mut jobs = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for job in planned.jobs {
        if job.clip.is_some() || job.frame_t != 0.0 {
            continue;
        }
        if !job.outputs.iter().any(|o| o.format == crate::Format::Png) {
            continue;
        }
        if seen.insert(job.source.clone()) {
            jobs.push(job);
        }
    }
    assert_eq!(jobs.len(), 6, "{seen:?}");
    (project, jobs)
}

fn key_of(path: &str) -> crate::identity::Key {
    let mut owner = crate::identity::Key::root();
    let mut name: Option<crate::identity::Name> = None;
    let mut copies = Vec::new();
    let flush =
        |owner: &mut crate::identity::Key,
         name: &mut Option<crate::identity::Name>,
         copies: &mut Vec<u32>| {
            if let Some(n) = name.take() {
                *owner = crate::identity::Key::named(owner, &n, copies);
                copies.clear();
            }
        };
    for seg in path.split('.') {
        if !seg.is_empty() && seg.bytes().all(|b| b.is_ascii_digit()) {
            copies.push(seg.parse().unwrap());
        } else {
            flush(&mut owner, &mut name, &mut copies);
            name = crate::identity::Name::parse(seg);
        }
    }
    flush(&mut owner, &mut name, &mut copies);
    owner
}

fn patch_segments(address: &str) -> Vec<String> {
    let src = format!("@target t.png 10x10\ndef h\n  circle 1\nh name:h\n  {address} a:1\n");
    let file = artscript_syntax::parse(&src).unwrap_or_else(|e| panic!("{address}: {e:?}"));
    for scene in file.scenes() {
        let artscript_syntax::ast::SceneKind::Draw(draw) = &scene.kind else {
            continue;
        };
        for item in draw.body.items() {
            if let artscript_syntax::ast::DrawItem::Patch(patch) = item {
                return patch.path.clone();
            }
        }
    }
    panic!("{address} did not parse as a patch");
}

#[test]
fn stable_chibi_nodes_keep_geometry_and_seeds_when_a_draw_is_inserted() {
    let (project, jobs) = chibi_jobs();
    for job in &jobs {
        let before = evaluate(&project, job);
        assert!(
            before.scene.is_some(),
            "{} did not evaluate\n{}",
            job.source,
            show(&before.diagnostics)
        );
        let base = stable_print(before.scene.as_ref().unwrap());
        assert!(!base.is_empty(), "{}", job.source);
        let mut files = project.files.clone();
        let src = files.get(&job.source).unwrap();
        files.insert(job.source.clone(), insert_probe(src));
        let shifted = Project {
            files,
            ..project.clone()
        };
        let planned = plan(&shifted);
        assert!(planned.diagnostics.is_empty(), "{:?}", planned.diagnostics);
        let job2 = planned
            .jobs
            .iter()
            .find(|j| {
                j.source == job.source
                    && j.clip.is_none()
                    && j.outputs.iter().any(|o| o.format == crate::Format::Png)
            })
            .unwrap();
        let after = evaluate(&shifted, job2);
        let got = stable_print(after.scene.as_ref().unwrap());
        for (path, print) in &base {
            assert_eq!(got.get(path), Some(print), "{} moved {path}", job.source);
        }
    }
}

#[test]
fn stable_chibi_keys_round_trip_through_a_patch() {
    let (project, jobs) = chibi_jobs();
    for job in &jobs {
        let ev = evaluate(&project, job);
        let scene = ev.scene.expect(&job.source);
        let mut all = Vec::new();
        walk_deep(&scene.children, &mut all);
        let mut stable: Vec<String> = all
            .iter()
            .map(|n| n.path.clone())
            .filter(|p| !p.contains('~'))
            .collect();
        stable.sort();
        stable.dedup();
        let mut lines = String::new();
        let mut expect = std::collections::BTreeMap::new();
        for path in &stable {
            let key = key_of(path);
            assert_eq!(key.to_string(), *path, "{}", job.source);
            let address = key
                .address_from(&crate::identity::Key::root())
                .unwrap_or_else(|| panic!("{path} has no address"));
            assert_eq!(
                patch_segments(&address),
                address
                    .trim_start_matches('.')
                    .split('.')
                    .collect::<Vec<_>>()
            );
            let old = all.iter().find(|n| n.path == *path).unwrap().blend;
            let (word, blend) = if old == Blend::Multiply {
                ("screen", Blend::Screen)
            } else {
                ("multiply", Blend::Multiply)
            };
            lines.push_str(&format!("  {address} blend:{word}\n"));
            expect.insert(path.clone(), blend);
        }
        let mut files = project.files.clone();
        let src = files.get(&job.source).unwrap().clone();
        files.insert(
            job.source.clone(),
            format!("{src}\nanim __probe dur:1s\n{lines}play __probe\n"),
        );
        let shifted = Project {
            files,
            ..project.clone()
        };
        let planned = plan(&shifted);
        let job2 = planned
            .jobs
            .iter()
            .find(|j| {
                j.source == job.source
                    && j.clip.is_none()
                    && j.outputs.iter().any(|o| o.format == crate::Format::Png)
            })
            .unwrap();
        let probed = evaluate(&shifted, job2);
        let scene = probed
            .scene
            .as_ref()
            .unwrap_or_else(|| panic!("{}", show(&probed.diagnostics)));
        let mut found = Vec::new();
        walk_deep(&scene.children, &mut found);
        for path in &stable {
            let hits: Vec<_> = found.iter().filter(|n| n.path == *path).collect();
            assert!(
                !hits.is_empty(),
                "{} lost {path}\n{}",
                job.source,
                show(&probed.diagnostics)
            );
            let blend = expect[path];
            for node in hits {
                assert_eq!(
                    node.blend, blend,
                    "{} {path} blend {:?} expected {blend:?}\n{}",
                    job.source, node.blend, show(&probed.diagnostics)
                );
            }
        }
    }
}

#[test]
fn a_copy_hop_stays_on_that_copy() {
    let s = scene(
        "@target t.png 80x40\neach i in 0..2\n  group name:row\n    each j in 0..2\n      circle 4 at:(i * 20 + j * 5),0 name:gem\nassert @row.1.gem.0.cx ~= 20\n",
    );
    assert!(paths(&s).contains(&"gem.1.0".to_string()));

    let marked = scene(
        "@target t.png 80x40\neach i in 0..2\n  group name:row\n    circle 4 at:i * 10,0 name:mark\nassert @row.1.mark.cx ~= 10\n",
    );
    assert!(paths(&marked).iter().any(|p| p == "mark.1"));

    let sized = scene(
        "@target t.png 80x40\neach i in 0..2\n  group name:row\n    circle (8 + i * 10) name:a\n    circle (8 + i * 10) name:b\nassert same_size(@row.0.*)\n",
    );
    assert!(paths(&sized).contains(&"a.0".to_string()));

    let missed = run_with(
        "@target t.png 40x40\neach i in 0..2\n  group name:row\n    if i == 0\n      circle 4 name:mark\ncopy @row.1.mark\n",
        &[],
    );
    let d = diag(&missed, "E0350");
    assert_eq!(d.file, "art/t.art");
    assert_eq!(d.span.line, 6);
    assert!(d.fix.is_some(), "{d}");
}

#[test]
fn an_index_that_does_not_fit_is_not_copy_zero() {
    let ev = run_with(
        "@target t.png 40x40\ndef face\n  circle 8 fill:#ffffff name:eye\nface name:face\n  .eye.99999999999 fill:#ff0000\n",
        &[],
    );
    let d = diag(&ev, "E0340");
    assert_eq!(d.file, "art/t.art");
    assert_eq!(d.span.line, 5);
    assert!(d.message.contains("not an address"), "{d}");
    assert!(
        d.fix.as_deref().unwrap().contains("whole-number"),
        "{d}"
    );
    assert_eq!(fill_hex(node(ev.scene.as_ref().unwrap(), "face.eye")), "#ffffff");

    let reference = run_with(
        "@target t.png 40x40\ncircle 4 name:eye\nlet x = @eye.99999999999\n",
        &[],
    );
    let d = diag(&reference, "E0350");
    assert_eq!(d.file, "art/t.art");
    assert_eq!(d.span.line, 3);
    assert!(d.message.contains("not a copy index"), "{d}");
    assert!(d.fix.as_deref().unwrap().contains("whole number"), "{d}");
    assert_eq!(paths(reference.scene.as_ref().unwrap()), ["eye"]);
}

#[test]
fn an_unnamed_patch_cites_the_statement_it_spells() {
    let verb = run_with(
        "@target t.png 40x40\ndef box\n  a 4\n  a1 4\nbox name:box\n  .a1 fill:#ff0000\n",
        &[],
    );
    let d = diag(&verb, "E0340");
    assert_eq!(d.file, "art/t.art");
    assert_eq!(d.span.line, 6);
    assert!(
        d.notes.iter().any(|n| n.contains("a1") && n.contains("art/t.art:4")),
        "{d}"
    );
    assert!(d.fix.as_deref().unwrap().contains("art/t.art:4"), "{d}");

    let second = run_with(
        "@target t.png 40x40\ndef face\n  circle 4\n  circle 4\nface name:face\n  .circle2 fill:#ff0000\n",
        &[],
    );
    let d = diag(&second, "E0340");
    assert_eq!(d.span.line, 6);
    assert!(
        d.notes.iter().any(|n| n.contains("circle") && n.contains("art/t.art:4")),
        "{d}"
    );
    assert_eq!(d.fix.as_deref(), Some("add name: at art/t.art:4"));
}

#[test]
fn a_missing_drawn_copy_is_e0340_and_an_undrawn_name_is_not() {
    let missing = run_with(
        "@target t.png 40x40\ndef row\n  each i in 0..2\n    circle 4 fill:#ffffff name:eye\nrow name:row\n  .eye.5 fill:#ff0000\n",
        &[],
    );
    let hits: Vec<_> = missing.diagnostics.iter().filter(|d| d.code == "E0340").collect();
    assert_eq!(hits.len(), 1, "{}", show(&missing.diagnostics));
    assert_eq!(hits[0].file, "art/t.art");
    assert_eq!(hits[0].span.line, 6);
    assert!(hits[0].message.contains("matches no drawn copy"), "{hits:?}");
    assert!(hits[0].fix.as_deref().unwrap().contains(".eye.0"), "{hits:?}");
    assert_eq!(fill_hex(node(missing.scene.as_ref().unwrap(), "row.eye.0")), "#ffffff");

    let arm = run_with(
        "@target t.png 40x40\ndef face\n  if false\n    each i in 0..2\n      circle 4 name:eye\n  else\n    circle 8 fill:#ffffff name:eye\nface name:face\n  .eye.0 fill:#ff0000\n",
        &[],
    );
    let hits: Vec<_> = arm.diagnostics.iter().filter(|d| d.code == "E0340").collect();
    assert_eq!(hits.len(), 1, "{}", show(&arm.diagnostics));
    assert_eq!(hits[0].file, "art/t.art");
    assert_eq!(hits[0].span.line, 9);
    assert!(hits[0].fix.as_deref().unwrap().contains(".eye"), "{hits:?}");
    assert_eq!(fill_hex(node(arm.scene.as_ref().unwrap(), "face.eye")), "#ffffff");

    let empty = run_with(
        "@target t.png 40x40\ndef face\n  each i in 0..0\n    circle 4 name:eye\nface name:face\n  .eye fill:#ff0000\n",
        &[],
    );
    assert!(
        empty.diagnostics.iter().all(|d| d.code != "E0340"),
        "{}",
        show(&empty.diagnostics)
    );

    let off = run_with(
        "@target t.png 40x40\ndef face\n  circle 4 name:eye\nface name:face\n  .eye off\n",
        &[],
    );
    assert!(
        off.diagnostics.iter().all(|d| d.code != "E0340"),
        "{}",
        show(&off.diagnostics)
    );
    assert!(!paths(off.scene.as_ref().unwrap()).iter().any(|p| p.contains("eye")));
}

#[test]
fn a_patch_on_an_anchor_is_e0340() {
    let ev = run_with(
        "@target t.png 40x40\ndef face\n  anchor neck 0,0\nface name:face\n  .neck fill:#ff0000\n",
        &[],
    );
    let d = diag(&ev, "E0340");
    assert_eq!(d.file, "art/t.art");
    assert_eq!(d.span.line, 5);
    assert_eq!(d.fix.as_deref(), Some("patch a shape or a group"));
}

#[test]
fn a_group_hop_fix_keeps_the_copy_index_on_the_child() {
    let indexed = run_with(
        "@target t.png 40x40\ndef prop\n  each i in 0..2\n    group name:face\n      circle 4 name:leaf\nprop name:prop\n  .face.0.leaf fill:#ff0000\n",
        &[],
    );
    let d = diag(&indexed, "E0340");
    assert_eq!(d.file, "art/t.art");
    assert_eq!(d.span.line, 7);
    assert_eq!(d.fix.as_deref(), Some("write .leaf.0"));

    let star = run_with(
        "@target t.png 40x40\ndef prop\n  group name:face\n    circle 4 name:leaf\nprop name:prop\n  .face.* fill:#ff0000\n",
        &[],
    );
    let d = diag(&star, "E0340");
    assert_eq!(d.span.line, 6);
    assert_eq!(d.fix.as_deref(), Some("write .leaf"));
}

#[test]
fn explain_includes_a_seeded_mask_node() {
    let ev = run_with(
        "@target t.png 40x40\ncircle 8 name:body\n  mask\n    circle 3 at:rand(0 1),0 name:hole\n",
        &[],
    );
    let text = crate::explain(ev.scene.as_ref().unwrap(), &ev.seeded);
    assert!(text.contains("hole"), "{text}");
    assert!(text.contains("seed"), "{text}");
}

#[test]
fn a_play_guard_sees_the_same_copies_as_the_draw() {
    let hit = scene(
        "@target t.png 40x40\nanim tint dur:1s\n  .eye.1 fill:#ff0000\nlet n = 2\nrepeat n\n  circle 8 fill:#ffffff name:eye\nif @eye.1\n  play tint\n",
    );
    assert_eq!(fill_hex(node(&hit, "eye.1")), "#ff0000");
    assert_eq!(fill_hex(node(&hit, "eye.0")), "#ffffff");

    let grid = scene(
        "@target t.png 80x80\nanim tint dur:1s\n  .gem.3 fill:#ff0000\ngrid 2x2\n  circle 4 fill:#ffffff name:gem\nif @gem.3\n  play tint\n",
    );
    assert_eq!(fill_hex(node(&grid, "gem.3")), "#ff0000");
    assert_eq!(fill_hex(node(&grid, "gem.0")), "#ffffff");

    let family = run_with(
        "@target t.png 40x40\nanim tint dur:1s\n  .eye.0 fill:#ff0000\nrepeat 2\n  circle 8 fill:#ffffff name:eye\nif @eye\n  play tint\n",
        &[],
    );
    let d = diag(&family, "E0343");
    assert_eq!(d.file, "art/t.art");
    assert_eq!(d.span.line, 6);
    assert!(d.fix.is_some(), "{d}");
    assert_eq!(fill_hex(node(family.scene.as_ref().unwrap(), "eye.0")), "#ffffff");
}

#[test]
fn a_final_star_lists_every_copy() {
    let even = scene(
        "@target t.png 80x40\neach i in 0..2\n  group name:row\n    circle 8 name:a\n    circle 8 name:b\nassert same_size(@row.*)\n",
    );
    assert!(paths(&even).contains(&"a.0".to_string()));
    assert!(paths(&even).contains(&"b.1".to_string()));

    let uneven = run_with(
        "@target t.png 80x40\neach i in 0..2\n  group name:row\n    circle (8 + i * 10) name:a\n    circle (8 + i * 10) name:b\nassert same_size(@row.*)\n",
        &[],
    );
    assert!(
        uneven.diagnostics.iter().all(|d| d.code != "E0343"),
        "{}",
        show(&uneven.diagnostics)
    );
    let d = diag(&uneven, "C0101");
    assert_eq!(d.file, "art/t.art");
    assert_eq!(d.span.line, 6);

    let indexed = scene(
        "@target t.png 80x40\neach i in 0..2\n  group name:row\n    circle (8 + i * 10) name:a\n    circle (8 + i * 10) name:b\nassert same_size(@row.0.*)\n",
    );
    assert!(paths(&indexed).contains(&"a.1".to_string()));
}

#[test]
fn a_def_that_reuses_a_shape_word_is_the_component() {
    let painted = scene(
        "@target t.png 40x40\ndef circle\n  rect 8 fill:#ffffff name:child\ncircle name:c\n  .child fill:#ff0000\n",
    );
    assert_eq!(fill_hex(node(&painted, "c.child")), "#ff0000");

    let hopped = scene(
        "@target t.png 40x40\ndef wrap\n  circle name:c\ndef circle\n  rect 8 fill:#ffffff name:child\nwrap name:wrap\n  .c.child fill:#ff0000\n",
    );
    assert_eq!(fill_hex(node(&hopped, "wrap.c.child")), "#ff0000");

    let slotted = scene(
        "@target t.png 40x40\ndef circle\n  slot hole\ncircle name:c\n  hole:\n    rect 6 fill:#00ff00 name:gem\n",
    );
    assert_eq!(fill_hex(node(&slotted, "c.hole.gem")), "#00ff00");

    let through = scene(
        "@target t.png 40x40\ndef wrap\n  circle name:c\ndef circle\n  slot hole\n    rect 4 fill:#ffffff name:gem\nwrap name:wrap\n  .c.hole.gem fill:#ff0000\n",
    );
    assert_eq!(fill_hex(node(&through, "wrap.c.hole.gem")), "#ff0000");
}

#[test]
fn an_unnamed_call_still_has_self() {
    let s = scene(
        "@target t.png 40x40\ndef dot\n  circle 4\n  assert centered(@self in:canvas) tol:1\ndot at:20,20\n",
    );
    assert!(
        paths(&s).iter().any(|p| p.starts_with("dot~")),
        "{:?}",
        paths(&s)
    );
}

#[test]
fn an_off_instance_still_checks_a_hop_through_it() {
    let anim = run_with(
        "@target t.png 40x40\nanim idle dur:1s\n  .hero off\n  .hero.typo at:1,0\ndef hero\n  circle 4 name:eye\nhero name:hero\nplay idle\n",
        &[],
    );
    let hits: Vec<_> = anim.diagnostics.iter().filter(|d| d.code == "E0340").collect();
    assert_eq!(hits.len(), 1, "{}", show(&anim.diagnostics));
    assert_eq!(hits[0].file, "art/t.art");
    assert_eq!(hits[0].span.line, 8);
    assert!(hits[0].message.contains("typo"), "{}", hits[0]);
    assert!(
        paths(anim.scene.as_ref().unwrap()).iter().all(|p| !p.contains("eye")),
        "{:?}",
        paths(anim.scene.as_ref().unwrap())
    );

    let static_off = run_with(
        "@target t.png 40x40\ndef wrap\n  hero name:hero\ndef hero\n  circle 4 name:eye\nwrap name:wrap\n  .hero off\n  .hero.typo at:1,0\n",
        &[],
    );
    let hits: Vec<_> = static_off
        .diagnostics
        .iter()
        .filter(|d| d.code == "E0340")
        .collect();
    assert_eq!(hits.len(), 1, "{}", show(&static_off.diagnostics));
    assert_eq!(hits[0].file, "art/t.art");
    assert_eq!(hits[0].span.line, 8);
    assert!(hits[0].message.contains("typo"), "{}", hits[0]);
    assert!(
        paths(static_off.scene.as_ref().unwrap())
            .iter()
            .all(|p| !p.contains("eye")),
        "{:?}",
        paths(static_off.scene.as_ref().unwrap())
    );
}

#[test]
fn a_hidden_path_checks_every_hop() {
    let nested = run_with(
        "@target t.png 40x40\nanim idle dur:1s\n  .hero off\n  .hero.head.typo at:1,0\ndef head\n  circle 4 name:eye\ndef hero\n  head name:head\nhero name:hero\nplay idle\n",
        &[],
    );
    let hits: Vec<_> = nested.diagnostics.iter().filter(|d| d.code == "E0340").collect();
    assert_eq!(hits.len(), 1, "{}", show(&nested.diagnostics));
    assert_eq!(hits[0].file, "art/t.art");
    assert_eq!(hits[0].span.line, 10);
    assert!(hits[0].message.contains("typo"), "{}", hits[0]);
    assert!(
        paths(nested.scene.as_ref().unwrap()).iter().all(|p| !p.contains("eye")),
        "{:?}",
        paths(nested.scene.as_ref().unwrap())
    );

    let kept = run_with(
        "@target t.png 40x40\nanim idle dur:1s\n  .hero off\n  .hero.head.eye at:10,0\ndef head\n  circle 4 name:eye\ndef hero\n  head name:head\nhero name:hero\nplay idle\n",
        &[],
    );
    assert!(
        kept.diagnostics.iter().all(|d| d.code != "E0340"),
        "{}",
        show(&kept.diagnostics)
    );
    assert!(
        paths(kept.scene.as_ref().unwrap()).iter().all(|p| !p.contains("eye")),
        "{:?}",
        paths(kept.scene.as_ref().unwrap())
    );

    let through_slot = run_with(
        "@target t.png 40x40\nanim idle dur:1s\n  .hero off\n  .hero.hole.typo at:1,0\ndef hero\n  slot hole\n    circle 4 name:gem\nhero name:hero\nplay idle\n",
        &[],
    );
    let hits: Vec<_> = through_slot.diagnostics.iter().filter(|d| d.code == "E0340").collect();
    assert_eq!(hits.len(), 1, "{}", show(&through_slot.diagnostics));
    assert_eq!(hits[0].span.line, 9);
    assert!(hits[0].message.contains("typo"), "{}", hits[0]);
}

#[test]
fn an_off_slot_still_checks_a_hop_through_it() {
    let static_off = run_with(
        "@target t.png 40x40\ndef hero\n  slot hole\n    circle 4 name:gem\nhero name:hero\n  .hole off\n  .hole.typo at:1,0\n",
        &[],
    );
    let hits: Vec<_> = static_off.diagnostics.iter().filter(|d| d.code == "E0340").collect();
    assert_eq!(hits.len(), 1, "{}", show(&static_off.diagnostics));
    assert_eq!(hits[0].file, "art/t.art");
    assert_eq!(hits[0].span.line, 7);
    assert!(hits[0].message.contains("typo"), "{}", hits[0]);
    assert!(
        paths(static_off.scene.as_ref().unwrap()).iter().all(|p| !p.contains("gem")),
        "{:?}",
        paths(static_off.scene.as_ref().unwrap())
    );

    let anim = run_with(
        "@target t.png 40x40\nanim idle dur:1s\n  .hero.hole off\n  .hero.hole.typo at:1,0\ndef hero\n  slot hole\n    circle 4 name:gem\nhero name:hero\nplay idle\n",
        &[],
    );
    let hits: Vec<_> = anim.diagnostics.iter().filter(|d| d.code == "E0340").collect();
    assert_eq!(hits.len(), 1, "{}", show(&anim.diagnostics));
    assert_eq!(hits[0].span.line, 9);
    assert!(hits[0].message.contains("typo"), "{}", hits[0]);
    assert!(
        paths(anim.scene.as_ref().unwrap()).iter().all(|p| !p.contains("gem")),
        "{:?}",
        paths(anim.scene.as_ref().unwrap())
    );
}

#[test]
fn an_empty_slot_does_not_borrow_a_file_name() {
    let ev = run_with(
        "@target t.png 40x40\ncircle 8 fill:#ffffff name:foo\ndef hero\n  slot hole\nhero name:hero\n  .hole.foo fill:#ff0000\n",
        &[],
    );
    let hits: Vec<_> = ev.diagnostics.iter().filter(|d| d.code == "E0340").collect();
    assert_eq!(hits.len(), 1, "{}", show(&ev.diagnostics));
    assert_eq!(hits[0].span.line, 6);
    assert!(hits[0].message.contains("foo"), "{}", hits[0]);
    assert_eq!(fill_hex(node(ev.scene.as_ref().unwrap(), "foo")), "#ffffff");

    let filled = scene(
        "@target t.png 40x40\ndef hero\n  slot hole\nhero name:hero\n  hole:\n    rect 6 fill:#ffffff name:gem\n  .hole.gem fill:#ff0000\n",
    );
    assert_eq!(fill_hex(node(&filled, "hero.hole.gem")), "#ff0000");
}

#[test]
fn a_stable_host_part_keeps_its_key_in_the_check_fix() {
    let ev = run_with(
        "@target t.png 80x80\ndef host\n  circle 4 at:0,0 name:eye\n  slot hole\nhost name:host at:40,40\n  hole:\n    circle 4 at:0,12 name:b\n    assert level(@b @eye) tol:.5\n",
        &[],
    );
    let d = diag(&ev, "C0101");
    assert_eq!(d.file, "art/t.art");
    assert_eq!(d.span.line, 8);
    let fix = d.fix.as_deref().unwrap();
    assert!(fix.contains("host.eye"), "{d}");
    assert!(!fix.contains("add name:"), "{d}");
}

#[test]
fn a_bad_call_name_is_one_e0312() {
    let ev = run_with(
        "@target t.png 40x40\ndef hero\n  circle 4 name:eye\nhero name:x+1\n",
        &[],
    );
    let hits: Vec<_> = ev.diagnostics.iter().filter(|d| d.code == "E0312").collect();
    assert_eq!(hits.len(), 1, "{}", show(&ev.diagnostics));
    assert_eq!(hits[0].span.line, 4);
    assert!(hits[0].message.contains("identifier"), "{}", hits[0]);
    assert!(
        paths(ev.scene.as_ref().unwrap()).iter().any(|p| p.contains("eye")),
        "{:?}",
        paths(ev.scene.as_ref().unwrap())
    );
}

#[test]
fn a_star_with_no_declared_part_is_e0340() {
    let ev = run_with(
        "@target t.png 40x40\ndef hero\n  circle 4\nhero name:hero\n  .* fill:#ff0000\n",
        &[],
    );
    let hits: Vec<_> = ev.diagnostics.iter().filter(|d| d.code == "E0340").collect();
    assert_eq!(hits.len(), 1, "{}", show(&ev.diagnostics));
    assert_eq!(hits[0].span.line, 5);
    assert!(hits[0].message.contains('*'), "{}", hits[0]);
    assert_eq!(hits[0].fix.as_deref(), Some("name a part in this scope"));

    let hit = scene(
        "@target t.png 40x40\ndef hero\n  circle 4 fill:#ffffff name:eye\nhero name:hero\n  .* fill:#ff0000\n",
    );
    assert_eq!(fill_hex(node(&hit, "hero.eye")), "#ff0000");
}

#[test]
fn an_anim_reports_each_path_that_misses() {
    let ev = run_with(
        "@target t.png 80x40\nanim idle dur:1s\n  .hero.left.typo at:1,0\n  .hero.right.typo at:1,0\ndef side\n  circle 4 name:eye\ndef hero\n  side name:left\n  side name:right\nhero name:hero\nplay idle\n",
        &[],
    );
    let hits: Vec<_> = ev.diagnostics.iter().filter(|d| d.code == "E0340").collect();
    assert_eq!(hits.len(), 2, "{}", show(&ev.diagnostics));
    assert!(hits.iter().all(|d| d.span.line == 11), "{hits:?}");
    assert!(
        hits.iter().any(|d| d.message.contains("hero.left.typo")),
        "{hits:?}"
    );
    assert!(
        hits.iter().any(|d| d.message.contains("hero.right.typo")),
        "{hits:?}"
    );
}

#[test]
fn self_is_hidden_until_checks() {
    let ev = run_with(
        "@target t.png 40x40\ndef dot\n  copy @self\n  circle 4\n  assert centered(@self in:canvas) tol:1\ndot at:20,20\n",
        &[],
    );
    let hits: Vec<_> = ev.diagnostics.iter().filter(|d| d.code == "E0350").collect();
    assert_eq!(hits.len(), 1, "{}", show(&ev.diagnostics));
    assert_eq!(hits[0].span.line, 3);
    assert!(hits[0].message.contains("checks"), "{}", hits[0]);
    assert!(
        ev.diagnostics.iter().all(|d| d.code != "C0101"),
        "{}",
        show(&ev.diagnostics)
    );
    assert!(
        paths(ev.scene.as_ref().unwrap()).iter().any(|p| p.contains("circle")),
        "{:?}",
        paths(ev.scene.as_ref().unwrap())
    );
}

#[test]
fn a_shared_outer_loop_keeps_the_current_copy() {
    let s = scene(
        "@target t.png 120x40\neach row in 0..2\n  each col in 0..2\n    circle (8 + row * 10) at:(row * 30 + col * 8),0 name:dot\n  assert @dot.0.cx ~= row * 30\n  each side in 0..1\n    assert same_size(@dot)\n    assert @dot.1.cx ~= row * 30 + 8\n",
    );
    assert!(paths(&s).contains(&"dot.0.0".to_string()));
    assert!(paths(&s).contains(&"dot.1.1".to_string()));
}

#[test]
fn a_slot_keeps_the_host_copy() {
    let filled = scene(
        "@target t.png 80x40\ndef host\n  each i in 0..2\n    circle 4 at:(i * 16),0 name:dot\n    slot hole\nhost name:host\n  hole:\n    circle 3 at:@dot.cx,12 name:mark\nassert @host.hole.0.mark.cx ~= 0\nassert @host.hole.1.mark.cx ~= 16\n",
    );
    assert!(paths(&filled).contains(&"host.hole.0.mark".to_string()));

    let fallback = scene(
        "@target t.png 80x40\ndef host\n  each i in 0..2\n    circle 4 at:(i * 16),0 name:dot\n    slot hole\n      circle 3 at:@dot.cx,12 name:mark\nhost name:host\nassert @host.hole.0.mark.cx ~= 0\nassert @host.hole.1.mark.cx ~= 16\n",
    );
    assert!(paths(&fallback).contains(&"host.hole.1.mark".to_string()));

    let imported = scene(
        "@target t.png 80x40\ndef gem needs:@dot\n  circle 3 at:@dot.cx,12 name:mark\ndef host\n  each i in 0..2\n    circle 4 at:(i * 16),0 name:dot\n    slot hole\nhost name:host\n  hole:\n    gem name:gem\nassert @host.hole.0.gem.mark.cx ~= 0\nassert @host.hole.1.gem.mark.cx ~= 16\n",
    );
    assert!(paths(&imported).contains(&"host.hole.1.gem.mark".to_string()));
}

#[test]
fn a_grid_copy_measures_where_it_was_drawn() {
    let s = scene(
        "@target t.png 80x40\ngrid 2x1 gap:0 cell:20x20\n  circle 4 fill:#ffffff name:tile\ncopy @tile.1 name:echo\nassert @tile.1.cx - @tile.0.cx ~= 20\nassert @echo.cx ~= @tile.1.cx\n",
    );
    assert!(paths(&s).contains(&"tile.0".to_string()));
    assert!(paths(&s).contains(&"tile.1".to_string()));

    let nested = scene(
        "@target t.png 80x40\ndef gem\n  circle 4 fill:#ffffff name:mark\ngrid 2x1 gap:0 cell:20x20\n  gem name:tile\nassert @tile.1.mark.cx - @tile.0.mark.cx ~= 20\n",
    );
    assert!(paths(&nested).contains(&"tile.1.mark".to_string()));
}

#[test]
fn one_inner_copy_keeps_the_outer_index() {
    let s = scene(
        "@target t.png 80x40\neach row in 0..2\n  each col in 0..1\n    circle 4 at:(row * 30),0 name:dot\n  assert @dot.0.cx ~= row * 30\n",
    );
    assert!(paths(&s).contains(&"dot.1.0".to_string()));
}

#[test]
fn manifest_backend_defaults_to_auto_and_rejects_unknown_values() {
    use crate::{BackendChoice, Manifest};

    assert_eq!(Manifest::parse("").unwrap().backend, BackendChoice::Auto);
    for (value, want) in [
        ("auto", BackendChoice::Auto),
        ("gpu", BackendChoice::Gpu),
        ("cpu", BackendChoice::Cpu),
    ] {
        let manifest = Manifest::parse(&format!("[build]\nbackend = \"{value}\"\n")).unwrap();
        assert_eq!(manifest.backend, want);
    }
    for bad in ["backend = \"metal\"", "backend = 1"] {
        let err = Manifest::parse(&format!("[project]\n[build]\n{bad}\n")).unwrap_err();
        assert_eq!(err.code, "E0200");
        assert!(err.message.contains("[build]"), "{}", err.message);
        assert_eq!(err.span.line, 3, "the error points at the backend line");
    }
}

#[test]
fn manifest_errors_point_at_the_bad_line() {
    use crate::{LintLevel, Manifest};

    let manifest = Manifest::parse("[lint]\nraw-color = \"off\"\nunused = \"error\"\n").unwrap();
    assert_eq!(manifest.lint_level("raw-color", LintLevel::Warn), LintLevel::Off);
    assert_eq!(manifest.lint_level("unused", LintLevel::Warn), LintLevel::Error);
    for (src, line, col) in [
        ("[project]\nname = \"x\"\n\n[lint]\nraw-color = \"loud\"\n", 5, 13),
        ("[project]\n\n[lint]\nunused = 1\n", 4, 10),
        ("name = \"x\"\nlint = 1\n", 2, 8),
        ("[project]\nname = \"x\"\n\n[lint\nraw-color = \"warn\"\n", 4, 6),
    ] {
        let err = Manifest::parse(src).unwrap_err();
        assert_eq!(err.code, "E0200");
        assert_eq!((err.span.line, err.span.col), (line, col), "{src:?}: {}", err.message);
    }
}

#[test]
fn comments_and_blank_lines_keep_statement_seeds() {
    let src = |pad: &str| {
        format!(
            "@target t.png 100x100 t:.5s
{pad}def blip
{pad}  let d = rand(0 30)
  circle 2 at:d,0 name:dot
{pad}anim bob dur:1s
{pad}  .box at:key(0 rand(1 9)),0
{pad}let y = rand(20 80)
{pad}if rand(0 1) > 2
  circle 1
{pad}each i in [rand(0 9) 1]
  circle 1 at:i,0
{pad}play bob start:rand(0 .4) * 1s
blip name:b
rect 4 at:0,y name:box
"
        )
    };
    let plain = scene(&src(""));
    let padded = scene(&src("\n# note\n"));
    for p in paths(&plain) {
        assert_eq!(at(node(&plain, &p)), at(node(&padded, &p)), "{p}");
    }
}

#[test]
fn every_play_of_an_instance_each_stays_ahead_of_a_later_play() {
    let s = scene(
        "@target t.png 40x40\nanim inner dur:1s\n  .eye a:.8\nanim outer dur:1s\n  .hero.eye a:.2\ndef hero size:40x40\n  circle 4 name:eye\ncircle 4 name:marker\nhero name:hero\n  each x in [@marker.w 1]\n    play inner\nplay outer\n",
    );
    let eye = node(&s, "hero.eye");
    assert!((eye.opacity - 0.2).abs() < 1e-4, "{}", eye.opacity);
}

#[test]
fn plays_in_a_file_each_apply_in_iteration_order() {
    let s = scene(
        "@target t.png 40x40\nanim inner dur:1s\n  .eye a:.8\nanim outer dur:1s\n  .hero.eye a:.2\ndef hero size:40x40\n  circle 4 name:eye\neach i in 0..2\n  hero name:hero\n    play inner\n  if i == 0\n    play outer\n",
    );
    let first = node(&s, "hero.0.eye").opacity;
    let second = node(&s, "hero.1.eye").opacity;
    assert!((first - 0.2).abs() < 1e-4, "{first}");
    assert!((second - 0.8).abs() < 1e-4, "{second}");
}

#[test]
fn an_entry_play_beats_a_play_in_a_def_body() {
    let s = scene_with(
        "@target t.png 40x40\nuse knight { knight }\nanim outer dur:1s\n  .knight.helmet.eye a:.2\nknight name:knight\nplay outer\n",
        &[(
            "lib/knight.art",
            "anim inner dur:1s\n  .eye a:.8\ndef helmet size:40x40\n  circle 4 name:eye\ndef knight size:40x40\n  helmet name:helmet\n    play inner\n",
        )],
    );
    let eye = node(&s, "knight.helmet.eye").opacity;
    assert!((eye - 0.2).abs() < 1e-4, "{eye}");
}

#[test]
fn a_file_play_guard_that_reads_the_drawn_scene_is_e0344() {
    let head = "@target t.png 40x40\nanim bob dur:1s\n  .marker at:10,0\ncircle 4 name:marker\n";
    for (guard, line) in [
        ("if @marker.w > 0\n  play bob\n", 5),
        ("each x in [@marker.w]\n  play bob\n", 5),
        ("let w = @marker.w\nif w > 0\n  play bob\n", 6),
        ("if true\n  each x in 0..@marker.w\n    play bob\n", 6),
    ] {
        let ev = run_with(&format!("{head}{guard}"), &[]);
        let errors: Vec<_> = ev
            .diagnostics
            .iter()
            .filter(|d| d.code.starts_with('E'))
            .collect();
        assert_eq!(errors.len(), 1, "{guard}\n{}", show(&ev.diagnostics));
        assert_eq!(errors[0].code, "E0344", "{guard}\n{}", errors[0]);
        assert_eq!(errors[0].span.line, line, "{guard}\n{}", errors[0]);
    }
    let s = scene(&format!("{head}group g\n  circle 4 name:dot\nif @dot\n  play bob\n"));
    near(at(node(&s, "marker")), (30.0, 20.0));
}

#[test]
fn a_file_play_guard_that_fails_in_both_passes_is_reported_once() {
    let ev = run_with(
        "@target t.png 40x40\nanim bob dur:1s\n  .marker at:10,0\ncircle 4 name:marker\nif @missing.w > 0\n  play bob\n",
        &[],
    );
    let codes: Vec<_> = ev.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert_eq!(codes, ["E0350"], "{}", show(&ev.diagnostics));
}

#[test]
fn a_geometry_guard_without_a_play_is_not_e0344() {
    let s = scene(
        "@target t.png 40x40\nanim bob dur:1s\n  .marker at:10,0\ncircle 4 name:marker\nif @marker.w > 0\n  circle 2 name:dot\nif true\n  play bob\n",
    );
    near(at(node(&s, "marker")), (30.0, 20.0));
    node(&s, "dot");
}

#[test]
fn a_play_none_under_a_geometry_guard_is_not_e0344() {
    let s = scene(
        "@target t.png 40x40\ncircle 4 name:marker\nif @marker.w > 0\n  play none\neach x in [@marker.w]\n  play none\n",
    );
    node(&s, "marker");
}

#[test]
fn a_top_level_play_input_that_reads_the_drawn_scene_is_e0344() {
    let head = "@target t.png 40x40\nanim bob dur:1s\n  .c a:.2\ncircle 8 name:marker\ncircle 4 name:c\n";
    for (tail, line) in [
        ("let w = @marker.w\nplay bob start:w\n", 7),
        ("let w = @marker.w\nplay bob start:(if w == none then 0 else w)\n", 7),
        ("play bob start:@marker.w\n", 6),
        ("play bob start:gap(@marker @c)\n", 6),
        ("let w = @marker.w\neach x in [if w == none then 0 else 5]\n  play bob start:x\n", 7),
        ("group g\n  circle 4 name:a\nlet n = [each k in @g.* -> 1]\nanim fade dur:1s\n  .c a:(if n == [] then .25 else .9)\nplay fade\n", 10),
        ("each i in 0..3\n  if @marker.w > 0\n    play bob\n", 7),
        ("rule r\n  check true\nlet floor = if @marker.w > 0 then r else r\nplay bob start:floor(1)\n", 9),
        ("def hero size:8x8\n  circle 4\nlet k = if @marker.w > 100 then hero else none\nk name:x\nplay bob start:(if @x then 0 else 1)\n", 10),
    ] {
        let ev = run_with(&format!("{head}{tail}"), &[]);
        let errors: Vec<_> = ev
            .diagnostics
            .iter()
            .filter(|d| d.code.starts_with('E'))
            .collect();
        assert_eq!(errors.len(), 1, "{tail}\n{}", show(&ev.diagnostics));
        assert_eq!(errors[0].code, "E0344", "{tail}\n{}", errors[0]);
        assert_eq!(errors[0].span.line, line, "{tail}\n{}", errors[0]);
    }
}

#[test]
fn a_misspelled_name_in_a_top_level_play_is_e0350_not_e0344() {
    let ev = run_with(
        "@target t.png 40x40\nanim bob dur:1s\n  .c a:.2\ncircle 4 name:c\nplay bob start:@tpyo.x\n",
        &[],
    );
    let errors: Vec<_> = ev
        .diagnostics
        .iter()
        .filter(|d| d.code.starts_with('E'))
        .collect();
    assert_eq!(errors.len(), 1, "{}", show(&ev.diagnostics));
    assert_eq!(errors[0].code, "E0350", "{}", errors[0]);
    assert_eq!(errors[0].span.line, 5, "{}", errors[0]);
    assert!(errors[0].notes.iter().any(|n| n.contains('c')), "{}", errors[0]);

    let later = run_with(
        "@target t.png 40x40\nanim bob dur:1s\n  .c a:.2\nplay bob start:@c.x\ncircle 4 name:c\n",
        &[],
    );
    let codes: Vec<_> = later.diagnostics.iter().map(|d| d.code.as_str()).collect();
    assert_eq!(codes, ["E0344"], "{}", show(&later.diagnostics));
}

#[test]
fn a_shadowed_geometry_let_does_not_block_a_top_level_play() {
    let head = "@target t.png 40x40\nanim bob dur:1s\n  .c a:.2\ncircle 8 name:marker\ncircle 4 name:c\n";
    for tail in [
        "let w = @marker.w\ncircle 2 at:w,0 name:d\nplay bob\n",
        "let w = @marker.w\nlet w = 0\nplay bob start:w\n",
        "let w = 0\nif true\n  let w = @marker.w\nplay bob start:w\n",
        "play bob start:-canvas.w/400\n",
    ] {
        let s = scene(&format!("{head}{tail}"));
        let c = node(&s, "c").opacity;
        assert!((c - 0.2).abs() < 1e-4, "{tail}: {c}");
    }
}

#[test]
fn every_read_of_an_unseen_let_is_e0344() {
    let head = "@target t.png 40x40\npalette p\n  ramp ink #102030 #f0e0d0 11\nanim bob dur:1s\n  .c a:.2\nrect 8x4 name:marker\ncircle 4 name:c fill:#000000\n";
    for (tail, at) in [
        ("let w = @marker.w\nplay bob start:canvas.{w}\n", "play bob"),
        ("anim tint dur:1s\n  .c fill:p.ink.{line}\nlet line = @marker.w\nplay tint\n", ".c fill"),
        ("anim fade dur:1s\n  .c a:(if n == 0 then .25 else .9)\nlet seg = @marker.w\nlet color = p.ink.{seg}\nlet n = if color == none then 0 else 1\nplay fade\n", ".c a:(if"),
        ("if @marker.w > 0\n  circle 4 name:dot\nlet r = @dot\nplay bob start:(if r == none then 0 else 1)\n", "play bob"),
    ] {
        let src = format!("{head}{tail}");
        let line = src.lines().position(|l| l.contains(at)).unwrap() + 1;
        let ev = run_with(&src, &[]);
        let errors: Vec<_> = ev
            .diagnostics
            .iter()
            .filter(|d| d.code.starts_with('E'))
            .collect();
        assert_eq!(errors.len(), 1, "{tail}\n{}", show(&ev.diagnostics));
        assert_eq!(errors[0].code, "E0344", "{tail}\n{}", errors[0]);
        assert_eq!(errors[0].span.line as usize, line, "{tail}\n{}", errors[0]);
    }
}

#[test]
fn a_new_binding_of_an_unseen_name_is_seen() {
    let head = "@target t.png 40x40\nrule width w:0\n  expect w >= 0\nanim bob dur:1s\n  .c a:.2\nrect 8x4 name:marker\ncircle 4 name:c\nlet w = @marker.w\n";
    for tail in [
        "each w in [0]\n  play bob start:w\n",
        "let xs = [each w in [0] -> w]\nlet n = xs.0\nplay bob start:n\n",
        "let n = if width(0) then 0 else 1\nplay bob start:n\n",
        "if width(0)\n  play bob\n",
    ] {
        let s = scene(&format!("{head}{tail}"));
        let c = node(&s, "c").opacity;
        assert!((c - 0.2).abs() < 1e-4, "{tail}: {c}");
    }
}

#[test]
fn a_rule_that_reads_the_drawn_scene_is_unseen_in_the_prepass() {
    let head = "@target t.png 40x40\nrule big\n  expect @marker.w > 10\nrule close\n  expect 3 ~= 5 tol:@marker.w\nanim bob dur:1s\n  .c a:.2\ncircle 8 name:marker\ncircle 4 name:c\n";
    for tail in [
        "let ok = big()\nplay bob start:(if ok then 0 else 5)\n",
        "play bob start:(if close() then 0 else 5)\n",
    ] {
        let src = format!("{head}{tail}");
        let line = src.lines().position(|l| l.starts_with("play bob")).unwrap() + 1;
        let ev = run_with(&src, &[]);
        let codes: Vec<_> = ev.diagnostics.iter().map(|d| (d.code.as_str(), d.span.line as usize)).collect();
        assert_eq!(codes, [("E0344", line)], "{tail}\n{}", show(&ev.diagnostics));
    }
}

#[test]
fn the_prepass_records_the_names_the_drawing_pass_registers() {
    let s = scene(
        "@target t.png 40x40\ndef fill size:10x10\n  circle 4 name:dot\nanim bob dur:1s\n  .c a:.2\ncircle 4 name:c\nfill name:fill\nif @fill\n  play bob\n",
    );
    let c = node(&s, "c").opacity;
    assert!((c - 0.2).abs() < 1e-4, "{c}");
    let ev = run_with(
        "@target t.png 40x40\nanim bob dur:1s\n  .c a:.2\ncircle 4 name:c\nlet k = none\nk name:x\nplay bob start:(if @x then 0 else 5)\n",
        &[],
    );
    let codes: Vec<_> = ev.diagnostics.iter().map(|d| (d.code.as_str(), d.span.line)).collect();
    assert_eq!(codes, [("E0344", 7)], "{}", show(&ev.diagnostics));
}

const BADGE_LIB: &str = "def badge r:20\n  circle r fill:#cc3333 name:disc\n  circle r*.4 fill:#ffffff name:_core\n  copy @_core at:0,0\n";

fn e0340s(ev: &Evaluation) -> Vec<&Diagnostic> {
    ev.diagnostics.iter().filter(|d| d.code == "E0340").collect()
}

#[test]
fn a_patch_on_a_private_part_is_e0340_naming_it_private() {
    let public = run_with(
        "@target t.png 100x100\nuse badge\nbadge 20 at:50,50 name:b\n  .disc off\n",
        &[("lib/badge.art", BADGE_LIB)],
    );
    assert!(public.diagnostics.is_empty(), "{}", show(&public.diagnostics));
    let ev = run_with(
        "@target t.png 100x100\nuse badge\nbadge 20 at:50,50 name:b\n  ._core off\n",
        &[("lib/badge.art", BADGE_LIB)],
    );
    let ds = e0340s(&ev);
    assert_eq!(ds.len(), 1, "{}", show(&ev.diagnostics));
    assert_eq!(ds[0].span.line, 4);
    assert!(ds[0].message.contains("`_core`, which is private to `badge`"), "{}", ds[0]);
    assert!(ds[0].notes.iter().any(|n| n == "names: disc"), "{}", ds[0]);
}

#[test]
fn an_anim_patch_on_a_private_part_is_e0340_at_the_play() {
    let ev = run_with(
        "@target t.png 100x100\nuse badge\nanim pulse dur:1s\n  ._core scale:2\nbadge 20 at:50,50 name:b\n  play pulse\n",
        &[("lib/badge.art", BADGE_LIB)],
    );
    let ds = e0340s(&ev);
    assert_eq!(ds.len(), 1, "{}", show(&ev.diagnostics));
    assert_eq!(ds[0].span.line, 6);
    assert!(ds[0].message.contains("`pulse` patch `._core`"), "{}", ds[0]);
    assert!(ds[0].message.contains("private to `badge`"), "{}", ds[0]);
}

#[test]
fn a_star_patch_skips_private_parts() {
    let s = run_with(
        "@target t.png 100x100\nuse badge\nbadge 20 at:50,50 name:b\n  .* fill:#0000ff\n",
        &[("lib/badge.art", BADGE_LIB)],
    );
    assert!(s.diagnostics.is_empty(), "{}", show(&s.diagnostics));
    let s = s.scene.unwrap();
    assert_eq!(fill_hex(node(&s, "b.disc")), "#0000ff");
    assert_eq!(fill_hex(node(&s, "b._core")), "#ffffff");
    let only_private = run_with(
        "@target t.png 100x100\ndef dot\n  circle 4 name:_c\ndot name:d\n  .* off\n",
        &[],
    );
    assert_eq!(e0340s(&only_private).len(), 1, "{}", show(&only_private.diagnostics));
}

#[test]
fn a_patch_into_a_slot_fill_cannot_name_a_private_part() {
    let ev = run_with(
        "@target t.png 100x100\nuse eye\neye 10 at:50,50 name:e\n  lashes:\n    circle 2 name:_tip\n  .lashes._tip off\n",
        &[("lib/eye.art", EYE_LIB)],
    );
    let ds = e0340s(&ev);
    assert_eq!(ds.len(), 1, "{}", show(&ev.diagnostics));
    assert!(ds[0].message.contains("private to the `lashes:` fill"), "{}", ds[0]);
}

#[test]
fn an_entry_file_patches_its_own_underscore_names() {
    let ev = run_with(
        "@target t.png 40x40\nanim idle dur:1s\n  ._c scale:2\ncircle 4 name:_c\nplay idle\n",
        &[],
    );
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
}

#[test]
fn a_fill_none_shape_clips_its_children_to_its_geometry() {
    let s = scene("@target t.png 64x64\nrect 40x40 at:32,32 fill:none name:a\n  circle 30 fill:#0000ff name:c\n");
    assert_eq!(pixel(&s, 32, 32), [0, 0, 255, 255]);
    assert_eq!(pixel(&s, 32, 6), [0, 0, 0, 0]);
}

#[test]
fn nesting_under_a_translucent_shape_matches_inside() {
    let nested = scene("@target t.png 64x64\nrect 40x40 at:32,32 fill:#0000ff4d name:glass\n  circle 6 fill:#000000 name:dot\n");
    let inside = scene("@target t.png 64x64\nrect 40x40 at:32,32 fill:#0000ff4d name:glass\ninside @glass\n  circle 6 at:32,32 fill:#000000 name:dot\n");
    assert_eq!(pixel(&nested, 32, 32), [0, 0, 0, 255]);
    assert_eq!(pixel(&nested, 32, 32), pixel(&inside, 32, 32));
}

#[test]
fn abs_of_a_number_is_a_number_and_abs_of_a_vector_is_elementwise() {
    let s = scene(
        "@target t.png 64x64\nlet a = abs(-3)\nrect 10x10 at:a,10 fill:#3366cc name:r\nrect 4x4 at:abs(-4,-6) fill:#3366cc name:v\ncircle abs(-2) at:20,20 fill:#3366cc name:c\nexpect abs(-3) + 1 == 4\n",
    );
    near(at(node(&s, "r")), (3.0, 10.0));
    near(at(node(&s, "v")), (4.0, 6.0));
}
