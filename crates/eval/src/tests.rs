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
    nodes.iter().for_each(|n| walk_node(n, out));
}

fn walk_node<'a>(n: &'a Node, out: &mut Vec<&'a Node>) {
    out.push(n);
    n.descendants().for_each(|c| walk_node(c, out));
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
        NodeKind::Shape(s) => match s.fill.as_ref().map(|f| &f.paint) {
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
        "error[C0101] art/t.art:4:1  [t.png]\n  assert level(@eye_l @eye_r) tol:.5\n  eye_l.cy = 40   eye_r.cy = 43   difference 3 > tol 0.5\n  fix: set .eye_r at:70,40"
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

fn evaluate_entry(files: &[(&str, &str)], source: &str) -> Evaluation {
    let p = project(files);
    let plan = plan(&p);
    let job = plan.jobs.iter().find(|j| j.source == source).unwrap();
    evaluate(&p, job)
}

#[test]
fn use_of_an_entry_file_imports_its_defs_and_skips_its_drawing() {
    let ev = evaluate_entry(
        &[
            ("art/a.art", "@target a.png 10x10\nuse art/b\nrock name:r\n"),
            (
                "art/b.art",
                "@target b.png 10x10\ndef rock\n  rect 1x1 name:stone\nrect 2x2 name:own\n",
            ),
        ],
        "art/a.art",
    );
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
    assert_eq!(paths(&ev.scene.unwrap()), ["r", "r.stone"]);
}

#[test]
fn a_def_that_reads_a_skipped_let_of_an_entry_file_is_e0201() {
    let ev = evaluate_entry(
        &[
            ("art/a.art", "@target a.png 10x10\nuse art/b\nrock name:r\n"),
            (
                "art/b.art",
                "@target b.png 10x10\nlet mode = big\ndef rock\n  if mode == big\n    rect 1x1\nrock name:own\n",
            ),
        ],
        "art/a.art",
    );
    let d = ev
        .diagnostics
        .iter()
        .find(|d| d.code == "E0201")
        .unwrap_or_else(|| panic!("{}", show(&ev.diagnostics)));
    assert_eq!((d.file.as_str(), d.span.line), ("art/b.art", 4));
    let own = evaluate_entry(
        &[("art/b.art", "@target b.png 10x10\nlet mode = big\ndef rock\n  if mode == big\n    rect 1x1\nrock name:own\n")],
        "art/b.art",
    );
    assert!(own.diagnostics.is_empty(), "{}", show(&own.diagnostics));
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
    let outline = |name| shape(&s, name).stroke.as_ref().and_then(|st| st.ink.clone()).expect("tapered ink");
    assert_eq!(
        outline("ink").to_svg(),
        outline("lit").to_svg(),
        "a width list built with each differs from the same literal list"
    );
    assert!(outline("ink").contains(artscript_ir::Point::new(50.0, 3.5)));
    assert!(!outline("ink").contains(artscript_ir::Point::new(50.0, 4.6)));
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
    let Some(Paint::Linear { from, to, stops }) = band.fill.as_ref().map(|f| &f.paint) else {
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
    let Some(Paint::Linear { from, to, .. }) = down.fill.as_ref().map(|f| &f.paint) else {
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
    let Some(Paint::Linear { from, .. }) = angle.fill.as_ref().map(|f| &f.paint) else {
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
fn curve_never_goes_past_its_points() {
    let s = scene(
        "@target t.png 120x80\n\
         curve 10,30 30,43 60,43 90,43 110,20 tension:1 name:foot\n\
         curve 20,20 60,10 100,20 60,60 closed name:loop\n",
    );
    let foot = shape(&s, "foot").geometry.compute_tight_bounds();
    assert!(foot.bottom <= 43.0, "foot bulges to {}", foot.bottom);
    let ring = shape(&s, "loop").geometry.compute_tight_bounds();
    assert_eq!((ring.left, ring.top, ring.right, ring.bottom), (20.0, 10.0, 100.0, 60.0));
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
fn a_closed_width_list_stroke_wraps_without_a_seam_cap() {
    let s = scene(
        "@target t.png 128x128\ncurve 30,30 98,30 98,98 30,98 closed stroke:#000,[2 14 2 14] tension:0 name:a\n",
    );
    let ink = shape(&s, "a").stroke.as_ref().and_then(|st| st.ink.as_ref()).expect("tapered ink");
    let inside = |x: f32, y: f32| ink.contains(artscript_ir::Point::new(x, y));
    assert!(inside(30.0, 30.5) && !inside(30.0, 26.0), "the first knot is 2 wide with no cap");
    assert!(inside(33.0, 64.0) && !inside(35.0, 64.0), "the closing side runs from 14 to 2");
    assert!(inside(64.0, 33.0) && !inside(64.0, 35.0), "the first side runs from 2 to 14");
    assert!(!inside(64.0, 64.0), "the ink is a ring");
}

#[test]
fn a_width_list_stroke_inks_every_contour_of_a_path() {
    let s = scene(
        "@target t.png 128x64\npath \"M 10 20 L 60 20 M 10 44 L 60 44\" stroke:#000,[2 10] taper:0 name:a\n",
    );
    let ink = shape(&s, "a").stroke.as_ref().and_then(|st| st.ink.as_ref()).expect("tapered ink");
    let inside = |x: f32, y: f32| ink.contains(artscript_ir::Point::new(x, y));
    assert!(inside(35.0, 20.0) && inside(35.0, 44.0), "both contours are inked");
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
fn node_names_may_take_reserved_words_and_read_back_qualified() {
    let s = scene(
        "@target t.png 64x64\ndef face\n  ellipse 4.5,7 fill:#888888 name:mask\nface at:32,32 name:f\n  .mask fill:#ff0000\n\
         circle 2 at:@f.mask.cx,10 name:if\nassert @f.mask.cx ~= 32\nassert @if.cx ~= 32\n",
    );
    assert_eq!(fill_hex(node(&s, "f.mask")), "#ff0000");
}

#[test]
fn positional_names_may_take_reserved_words_and_read_back_qualified() {
    let s = scene(
        "@target t.png 64x64\nlayer mask\n  circle 20 at:32,32 fill:#336699\n\
         group light\n  rect 10x10\n  anchor shader 5,7\n\
         assert @mask.cx ~= 32\nassert @light.shader.x ~= 5\n",
    );
    assert_eq!(fill_hex(node(&s, "mask.circle~1")), "#336699");
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

const BROKEN: (&str, &str) = (
    "lib/x/thing.art",
    "style soft a:.5\nfinish sheen\n  fx grain .05\nlet sky = #8cf\ndef thing\n  rect 1x(2) fill:p.sky name:r\n",
);

fn codes_at(ev: &Evaluation) -> Vec<String> {
    ev.diagnostics
        .iter()
        .map(|d| format!("{} {}", d.code, d.file))
        .collect()
}

#[test]
fn a_library_that_fails_to_parse_is_reported_once_at_the_library() {
    for src in [
        "@target t.png 64x64\nuse x/thing { thing }\nthing at:32,32 name:t\n",
        "@target t.png 64x64\nuse x/thing\nthing at:32,32 name:t\n",
        "@target t.png 64x64\nuse x/thing { soft }\nrect 2x2 style:soft name:t\n",
        "@target t.png 64x64\nuse x/thing { sheen }\nrect 2x2 name:t\n  fx sheen\n",
        "@target t.png 64x64\nuse x/thing\nrect 2x2 name:t\n  fx sheen\n",
        "@target t.png 64x64\nuse x/thing as m\nrect 2x2 fill:m.sky name:t\n",
        "@target t.png 64x64\nuse x/thing { sky }\nrect 2x2 fill:sky.1 name:t\n",
    ] {
        let ev = run_with(src, &[BROKEN]);
        assert_eq!(codes_at(&ev), ["E0101 lib/x/thing.art"], "{src}\n{}", show(&ev.diagnostics));
    }
}

#[test]
fn a_broken_manifest_palette_is_reported_once_at_the_palette() {
    let p = Project::from_sources(
        &format!("{MANIFEST}\n[defaults]\npalette = \"palette/house\"\n"),
        [
            (
                "art/t.art".to_string(),
                "@target t.png 10x10\ncircle 1 fill:p.sky name:c\n".to_string(),
            ),
            (
                "lib/palette/house.art".to_string(),
                "palette house\n  color sky 1x(2)\n".to_string(),
            ),
        ]
        .into(),
    )
    .unwrap();
    let plan = plan(&p);
    let ev = evaluate(&p, &plan.jobs[0]);
    assert_eq!(codes_at(&ev), ["E0101 lib/palette/house.art"], "{}", show(&ev.diagnostics));
}

#[test]
fn a_play_of_an_anim_from_a_broken_library_is_quiet() {
    let lib = ("lib/x/moves.art", "anim bob dur:1s\n  .t at:key(0 4)\nlet x = 1x(2)\n");
    let ev = run_with(
        "@target t.png 64x64\nuse x/moves { bob }\nrect 2x2 name:t\nplay bob\n",
        &[lib],
    );
    assert_eq!(codes_at(&ev), ["E0101 lib/x/moves.art"], "{}", show(&ev.diagnostics));
}

#[test]
fn a_typo_beside_a_broken_import_is_still_an_unknown_verb() {
    let ev = run_with(
        "@target t.png 64x64\nuse x/thing { thing }\nthng at:32,32 name:t\n",
        &[BROKEN],
    );
    assert_eq!(
        codes_at(&ev),
        ["E0101 lib/x/thing.art", "E0301 art/t.art"],
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
    let Some(Paint::Solid(color)) = shape(&s, "cap").fill.as_ref().map(|f| &f.paint) else {
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
    let Some(Paint::Solid(color)) = shape(&s, "cap").fill.as_ref().map(|f| &f.paint) else {
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
fn a_library_let_and_a_parent_palette_are_not_unused() {
    let manifest = "[project]\nout = \"out\"\nlib = [\"lib\"]\n\n[lint]\nraw-color = \"off\"\nunused = \"warn\"\noff-canvas = \"off\"\n";
    let files = [
        (
            "art/t.art".to_string(),
            "@target t.png 64x64\nuse grid { tile cell }\nuse elite { elite }\nlet spare = 1\ncell\nrect 10x10 at:tile/2,tile/2 fill:elite.sky name:r\n".to_string(),
        ),
        (
            "lib/grid.art".to_string(),
            "let tile = 64\nlet _pad = 3\ndef cell\n  let gap = 2\n  rect 4x4\n".to_string(),
        ),
        (
            "lib/house.art".to_string(),
            "palette house\n  color sky #446688\n".to_string(),
        ),
        (
            "lib/elite.art".to_string(),
            "use house { house }\n\npalette elite from house\n  ramp sky #9a8fb8 5\n".to_string(),
        ),
    ]
    .into();
    let p = Project::from_sources(manifest, files).unwrap();
    let plan = plan(&p);
    assert!(plan.diagnostics.is_empty(), "{:?}", plan.diagnostics);
    let ev = evaluate(&p, &plan.jobs[0]);
    let unused: Vec<String> = ev
        .diagnostics
        .iter()
        .filter(|d| d.code == "L0103")
        .map(|d| format!("{} {}", d.file, d.message))
        .collect();
    assert_eq!(
        unused,
        [
            "art/t.art `let spare` is never used",
            "lib/grid.art `let _pad` is never used",
            "lib/grid.art `let gap` is never used",
        ],
        "{}",
        show(&ev.diagnostics)
    );
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

fn hidden_children(src: &str) -> Vec<Diagnostic> {
    let manifest = "[project]\nout = \"out\"\nlib = [\"lib\"]\n\n[lint]\nraw-color = \"off\"\nunused = \"off\"\n";
    let p = Project::from_sources(
        manifest,
        [("art/t.art".to_string(), src.to_string())].into(),
    )
    .unwrap();
    let plan = plan(&p);
    assert!(plan.diagnostics.is_empty(), "{:?}", plan.diagnostics);
    crate::sample(&p, &plan.jobs[0])
        .diagnostics
        .into_iter()
        .filter(|d| d.code == "L0105")
        .collect()
}

#[test]
fn a_child_inside_a_tapered_stroke_is_not_hidden() {
    let hidden = hidden_children(
        "@target t.png 128x64\ncurve -40,0 40,0 fill:none stroke:#000,[2 20 2] at:64,32 name:line\n  circle 3 at:0,6 name:dot\n",
    );
    assert!(hidden.is_empty(), "{}", show(&hidden));
}

#[test]
fn a_child_its_parent_shape_hides_is_reported_and_a_visible_one_is_not() {
    let hidden = hidden_children(
        "@target t.png 64x64\ndef face\n  circle 11 at:1,-15 name:skull\n    circle 2 at:-3,-15 name:eye_l\n    circle 2 at:5,-15 name:eye_r\n    circle 3 at:0,2 name:mouth\nface at:32,48 name:f\n",
    );
    let nodes: Vec<_> = hidden.iter().map(|d| d.node.as_str()).collect();
    assert_eq!(nodes, ["f.eye_l", "f.eye_r"], "{}", show(&hidden));
    assert_eq!(hidden[0].span.line, 4);
    assert!(
        hidden[0].message.contains("`f.skull`"),
        "{}",
        hidden[0].message
    );
}

#[test]
fn a_scatter_is_reported_only_when_every_copy_is_hidden() {
    let partly = hidden_children(
        "@target t.png 64x64\ncircle 10 at:32,32 name:blob\n  each i in 0..4\n    circle 2 at:i*12-18,0 name:dot\n",
    );
    assert!(partly.is_empty(), "{}", show(&partly));
    let all = hidden_children(
        "@target t.png 64x64\ncircle 10 at:32,32 name:blob\n  each i in 0..4\n    circle 2 at:i*4,20 name:dot\n",
    );
    assert_eq!(all.len(), 1, "{}", show(&all));
    assert!(
        all[0]
            .notes
            .iter()
            .any(|n| n == "also in dot.1, dot.2, dot.3"),
        "{:?}",
        all[0].notes
    );
}

#[test]
fn a_child_hidden_in_every_copy_of_its_parent_is_reported_once_at_its_column() {
    let hidden = hidden_children(
        "@target t.png 200x200\ncircle 80 at:100,100 name:host\n  each k, c in [0 100]\n    ellipse 20,20 at:c-40,0 name:inner\n      circle 4 at:150,0\n",
    );
    assert_eq!(hidden.len(), 1, "{}", show(&hidden));
    assert_eq!((hidden[0].span.line, hidden[0].span.col), (5, 7));
    assert_eq!(hidden[0].node, "inner.0.circle~1");
    assert!(
        hidden[0].notes.iter().any(|n| n == "also in inner.1.circle~1"),
        "{:?}",
        hidden[0].notes
    );
    let partly = hidden_children(
        "@target t.png 200x200\ncircle 80 at:100,100 name:host\n  each c in [0 60]\n    ellipse 20,20 at:c-40,0 name:inner\n      circle 4 at:c,0\n",
    );
    assert!(partly.is_empty(), "{}", show(&partly));
}

#[test]
fn a_child_hidden_on_only_some_frames_is_not_reported() {
    let some = hidden_children(
        "@target t.gif 64x64 bg:#ffffff dur:1s fps:4\nanim slide dur:1s\n  .dot at:key(0 30),0\ncircle 10 at:32,32 name:blob\n  circle 2 name:dot\nplay slide\n",
    );
    assert!(some.is_empty(), "{}", show(&some));
    let every = hidden_children(
        "@target t.gif 64x64 bg:#ffffff dur:1s fps:4\nanim slide dur:1s\n  .dot at:key(20 30),0\ncircle 10 at:32,32 name:blob\n  circle 2 name:dot\nplay slide\n",
    );
    assert_eq!(every.len(), 1, "{}", show(&every));
}

fn unknown_targets(src: &str, libs: &[(&str, &str)]) -> Vec<Diagnostic> {
    run_with(src, libs)
        .diagnostics
        .into_iter()
        .filter(|d| d.code == "L0106")
        .collect()
}

const TWO_LABELS: &str = "@target a.png 8x8 as:villain\n@target b.png 8x8 as:hero\n";

#[test]
fn comparing_target_with_a_path_warns_and_names_the_labels() {
    let hits = unknown_targets(
        &format!(
            "{TWO_LABELS}if target == \"a.png\"\n  circle 2\nif target != \"a.png\"\n  circle 3\n"
        ),
        &[],
    );
    let lines: Vec<u32> = hits.iter().map(|d| d.span.line).collect();
    assert_eq!(lines, [3, 5], "{}", show(&hits));
    let d = &hits[0];
    assert_eq!(d.severity, artscript_syntax::Severity::Warning);
    assert_eq!(d.message, "`target` is never `\"a.png\"` in this file");
    assert_eq!(d.notes, ["its targets are labeled hero, villain"]);
    assert_eq!(
        d.fix.as_deref(),
        Some("compare `target` with an `as:` label, such as `target == hero`")
    );
}

#[test]
fn comparing_target_with_one_of_its_labels_is_quiet() {
    let hits = unknown_targets(
        &format!(
            "{TWO_LABELS}if target == hero\n  circle 2\nif target == \"villain\"\n  circle 3\nlet lead = target != hero\n"
        ),
        &[],
    );
    assert!(hits.is_empty(), "{}", show(&hits));
}

#[test]
fn target_is_none_only_when_a_target_has_no_label() {
    let src = "if target == none\n  circle 2\n";
    let partly = unknown_targets(
        &format!("@target a.png 8x8 as:hero\n@target b.png 8x8\n{src}"),
        &[],
    );
    assert!(partly.is_empty(), "{}", show(&partly));
    let labeled = unknown_targets(&format!("{TWO_LABELS}{src}"), &[]);
    assert_eq!(labeled.len(), 1, "{}", show(&labeled));
    assert_eq!(labeled[0].message, "`target` is never `none` in this file");
    let unlabeled = unknown_targets("@target a.png 8x8\nif target == hero\n  circle 2\n", &[]);
    assert_eq!(unlabeled.len(), 1, "{}", show(&unlabeled));
    assert_eq!(
        unlabeled[0].notes,
        ["no `@target` here sets `as:`, so `target` is always `none`"]
    );
    assert_eq!(
        unlabeled[0].fix.as_deref(),
        Some("add `as:<label>` to a `@target`")
    );
}

#[test]
fn a_name_compared_with_target_is_quiet_because_its_value_is_unknown() {
    let hits = unknown_targets(
        &format!(
            "{TWO_LABELS}let lead = hero\nif target == lead\n  circle 2\neach v in [hero villain]\n  if target == v\n    circle 3\nuse names\nif target == star\n  circle 4\n"
        ),
        &[("lib/names.art", "let star = villain\n")],
    );
    assert!(hits.is_empty(), "{}", show(&hits));
}

#[test]
fn unknown_target_off_is_silent() {
    let manifest = "[project]\nout = \"out\"\n\n[lint]\nunknown-target = \"off\"\n";
    let p = Project::from_sources(
        manifest,
        [(
            "art/t.art".to_string(),
            format!("{TWO_LABELS}if target == \"a.png\"\n  circle 2\n"),
        )]
        .into(),
    )
    .unwrap();
    let plan = plan(&p);
    assert!(plan.diagnostics.is_empty(), "{:?}", plan.diagnostics);
    let ev = evaluate(&p, &plan.jobs[0]);
    assert!(
        ev.diagnostics.iter().all(|d| d.code != "L0106"),
        "{}",
        show(&ev.diagnostics)
    );
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
fn a_gif_scale_two_refines_a_tapered_stroke() {
    let out = gif_samples(
        "@target t.gif 40x40 scale:1,2 bg:#ffffff dur:1s fps:2\ncurve 0,0 20,0 40,0 stroke:#111,4 taper:1 name:ink\n",
    );
    assert!(out.diagnostics.is_empty(), "{}", show(&out.diagnostics));
    let n = |scale: f32| {
        let frames = &out.at(scale).unwrap_or_else(|| panic!("no sample at {scale}")).frames;
        assert_eq!(frames.len(), 2, "{scale}");
        let ink = shape(&frames[0].scene, "ink").stroke.as_ref().and_then(|st| st.ink.as_ref());
        ink.expect("tapered ink").count_points()
    };
    assert!(n(2.0) > n(1.0), "{} vs {}", n(2.0), n(1.0));
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
fn an_explicit_seed_draws_the_same_noise_in_any_node_and_file() {
    let lib = "def blot\n  rect 20x20 fill:simplex(16 seed:1) -> [#000 #fff] name:fill\n";
    let s = lib_scene(
        "@target t.png 100x100\nuse d { blot }\n\
         rect 20x20 fill:simplex(16 seed:1) -> [#000 #fff] name:a\n\
         rect 20x20 fill:simplex(16 seed:1) -> [#000 #fff] name:b\n\
         blot name:c\n\
         rect 20x20 fill:simplex(16 seed:2) -> [#000 #fff] name:d\n",
        lib,
    );
    let fill = |path| format!("{:?}", shape(&s, path).fill);
    assert_eq!(fill("a"), fill("b"));
    assert_eq!(fill("a"), fill("c.fill"));
    assert_ne!(fill("a"), fill("d"));
}

#[test]
fn an_unnamed_node_with_an_explicit_seed_does_not_warn_l0104() {
    let ev = eval_lint("@target t.png 40x40\ncircle 4 at:rand(0 10 seed:3),0\n", "");
    assert!(
        ev.diagnostics.iter().all(|d| d.code != "L0104"),
        "{}",
        show(&ev.diagnostics)
    );
}

/// Every unseeded draw a def body can make: a field fill, a `let` and a draw-site `rand`, a
/// `blob`, `jitter:`, and `fx grain`.
const BLOT_LIB: &str = "\
def blot
  rect 40x40 fill:fbm(simplex(6)) -> [#000 #fff] name:field
  let r = rand(3 8)
  circle r at:rand(-10 10),rand(-10 10) fill:#ff0000 name:dot
  blob 8 wobble:.4 fill:#00ff00 name:lump
  rect 30x4 at:0,15 jitter:1.5,8 fill:#0000ff name:bar
  fx grain .3
";

/// The 48x48 pixels centered on `cx`,`cy`.
fn crop(scene: &Scene, cx: i32, cy: i32) -> Vec<u8> {
    let (w, h, buf) =
        artscript_render::render_rgba(scene, 1.0, artscript_render::Backend::Cpu).unwrap();
    assert!(cx >= 24 && cy >= 24 && cx + 24 <= w as i32 && cy + 24 <= h as i32);
    let mut out = Vec::new();
    for y in cy - 24..cy + 24 {
        let row = (y as usize * w as usize + (cx - 24) as usize) * 4;
        out.extend_from_slice(&buf[row..row + 48 * 4]);
    }
    out
}

#[test]
fn two_instances_with_the_same_seed_draw_the_same_pixels() {
    let s = lib_scene(
        "@target t.png 300x96 bg:#ffffff\nuse d { blot }\n\
         blot at:48,48 seed:7 name:a\n\
         group name:g at:96,0\n  blot at:48,48 seed:7 name:b\n\
         blot at:240,48 seed:8 name:c\n",
        BLOT_LIB,
    );
    let (a, b, c) = (crop(&s, 48, 48), crop(&s, 144, 48), crop(&s, 240, 48));
    assert!(a == b, "seed:7 drew different pixels at a and g.b");
    assert!(a != c, "seed:7 and seed:8 drew the same pixels");
}

#[test]
fn a_seeded_copy_draws_the_same_pixels_off_the_8px_grid() {
    for dx in [98, 100, 101] {
        let s = lib_scene(
            &format!(
                "@target t.png 300x96 bg:#ffffff\nuse d {{ blot }}\n\
                 blot at:48,48 seed:7 name:a\nblot at:{},48 seed:7 name:b\n",
                48 + dx
            ),
            BLOT_LIB,
        );
        assert!(crop(&s, 48, 48) == crop(&s, 48 + dx, 48), "seed:7 drew different pixels {dx}px over");
    }
}

#[test]
fn a_seeded_noise_fill_draws_the_same_pixels_off_the_8px_grid() {
    let s = scene(
        "@target t.png 200x64 bg:#ffffff\n\
         rect 40x40 at:32,32 fill:fbm(simplex(6 seed:3)) -> [#000 #fff] name:a\n\
         rect 40x40 at:132,32 fill:fbm(simplex(6 seed:3)) -> [#000 #fff] name:b\n",
    );
    assert!(crop(&s, 32, 32) == crop(&s, 132, 32), "the noise fill drew different pixels 100px over");
}

#[test]
fn a_brushed_copy_draws_the_same_pixels_off_the_8px_grid() {
    let s = scene(
        "@target t.png 200x64 bg:#ffffff\nuse std/brushes { dry }\n\
         rect 40x40 at:32,32 fill:#3366cc brush:dry(strength:.6) seed:7 name:a\n\
         rect 40x40 at:133,32 fill:#3366cc brush:dry(strength:.6) seed:7 name:b\n",
    );
    assert!(crop(&s, 32, 32) == crop(&s, 133, 32), "the brush drew different pixels 101px over");
}

#[test]
fn a_seeded_instance_matches_across_calling_files_and_the_asset_entry() {
    let wrap = "use d { blot }\ndef wrap\n  blot seed:7 name:w\n    fx grain .3\n";
    let libs = [("lib/d.art", BLOT_LIB), ("lib/e.art", wrap)];
    let ev = run_with(
        "@target t.png 200x96 bg:#ffffff\nuse d { blot }\nuse e { wrap }\n\
         blot at:48,48 seed:7 name:a\n  fx grain .3\nwrap at:144,48 name:x\n",
        &libs,
    );
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
    let s = ev.scene.unwrap();
    assert!(
        crop(&s, 48, 48) == crop(&s, 144, 48),
        "the call in lib/e.art drew different pixels"
    );

    let asset = run_with(
        "@target t.png 48x48 bg:#ffffff\nuse d { blot }\nblot at:24,24 seed:7 name:block\n  fx grain .3\n",
        &libs,
    );
    assert!(asset.diagnostics.is_empty(), "{}", show(&asset.diagnostics));
    assert!(
        crop(&asset.scene.unwrap(), 24, 24) == crop(&s, 48, 48),
        "the asset entry file drew different pixels"
    );
}

#[test]
fn a_seeded_instance_seeds_slot_content_from_the_seed_in_any_file() {
    let fill = |indent: &str| format!("{indent}top:\n{indent}  circle 4 at:rand(0 40),0 name:c\n");
    let wrap = format!(
        "use d {{ holder }}\ndef wrap\n  holder seed:7 name:w\n{}",
        fill("    ")
    );
    let libs = [
        ("lib/d.art", "def holder\n  slot top\n"),
        ("lib/e.art", wrap.as_str()),
    ];
    let ev = run_with(
        &format!(
            "@target t.png 96x48\nuse d {{ holder }}\nuse e {{ wrap }}\n\
             holder seed:7 name:a\n{}wrap name:x\nholder seed:8 name:b\n{}",
            fill("  "),
            fill("  ")
        ),
        &libs,
    );
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
    let s = ev.scene.unwrap();
    assert_eq!(at(node(&s, "a.top.c")), at(node(&s, "x.w.top.c")));
    assert_ne!(at(node(&s, "a.top.c")), at(node(&s, "b.top.c")));
}

#[test]
fn an_unnamed_instance_with_a_seed_does_not_warn_l0104() {
    let src = |call: &str| {
        format!(
            "@target t.png 40x40\ndef speck\n  rect 20x20 fill:#888888\n  fx grain .3\n{call}\n"
        )
    };
    let seeded = eval_lint(&src("speck seed:7"), "");
    assert!(
        seeded.diagnostics.is_empty(),
        "{}",
        show(&seeded.diagnostics)
    );
    let bare = eval_lint(&src("speck"), "");
    assert!(
        bare.diagnostics
            .iter()
            .any(|d| d.code == "L0104" && d.span.line == 5),
        "{}",
        show(&bare.diagnostics)
    );
}

#[test]
fn a_def_with_a_seed_parameter_still_receives_seed() {
    let s = scene(
        "@target t.png 100x100\ndef tile seed:1\n  rect 20x20 fill:simplex(6 seed:seed) -> [#000 #fff] name:f\n\
         tile seed:5 name:a\nrect 20x20 fill:simplex(6 seed:5) -> [#000 #fff] name:r\n",
    );
    assert_eq!(
        format!("{:?}", shape(&s, "a.f").fill),
        format!("{:?}", shape(&s, "r").fill)
    );
}

#[test]
fn an_instance_seed_that_is_not_a_number_is_e0320() {
    let ev = run_with(
        "@target t.png 40x40\ndef speck\n  rect 20x20 fill:#888888\nspeck seed:#ff0000 name:s\n",
        &[],
    );
    let d = diag(&ev, "E0320");
    assert_eq!(d.span.line, 4);
}

const PALETTE_MANIFEST: &str = "[project]\nout = \"out\"\nlib = [\"lib\"]\n\n[defaults]\npalette = \"palette/house\"\n\n[lint]\nraw-color = \"off\"\nunused = \"off\"\noff-canvas = \"off\"\npositional-seed = \"off\"\n";

const HOUSE: &str = "palette house\n  color sky #111111\n";

const ELITE: &str = "palette elite\n  color sky #222222\n\npalette hollow\n  color sky #333333\n";

const PAINT_LIB: &str = "\
palette inner
  color sky #333333

def swatch
  rect 10x10 fill:p.sky name:a

def eye iris:p.sky
  circle 5 fill:iris name:i

def holder
  slot top

def wrap
  swatch name:s
  swatch palette:inner name:t
";

fn palette_project(manifest: &str, src: &str, libs: &[(&str, &str)]) -> Project {
    let mut files = vec![
        ("art/t.art".to_string(), src.to_string()),
        ("lib/palette/house.art".to_string(), HOUSE.to_string()),
    ];
    files.extend(libs.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    Project::from_sources(manifest, files.into_iter().collect()).unwrap()
}

fn palette_run(src: &str, libs: &[(&str, &str)]) -> Evaluation {
    let p = palette_project(PALETTE_MANIFEST, src, libs);
    let plan = plan(&p);
    assert!(plan.diagnostics.is_empty(), "{:?}", plan.diagnostics);
    evaluate(&p, &plan.jobs[0])
}

fn palette_scene(src: &str) -> Scene {
    let src = format!("{src}\n{ELITE}");
    let ev = palette_run(&src, &[("lib/d.art", PAINT_LIB)]);
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
    ev.scene.unwrap()
}

#[test]
fn an_instance_palette_recolors_the_def_body_and_its_parameter_defaults() {
    let s = palette_scene(
        "@target t.png 100x100\nuse d { swatch eye }\n\
         swatch name:plain\nswatch palette:elite name:swapped\n\
         eye name:plain_eye\neye palette:elite name:swapped_eye\n",
    );
    assert_eq!(fill_hex(node(&s, "plain.a")), "#111111");
    assert_eq!(fill_hex(node(&s, "swapped.a")), "#222222");
    assert_eq!(fill_hex(node(&s, "plain_eye.i")), "#111111");
    assert_eq!(fill_hex(node(&s, "swapped_eye.i")), "#222222");
}

#[test]
fn an_argument_written_at_the_call_keeps_the_callers_palette() {
    let s = palette_scene(
        "@target t.png 100x100\nuse d { eye }\neye palette:elite iris:p.sky name:e\n",
    );
    assert_eq!(fill_hex(node(&s, "e.i")), "#111111");
}

#[test]
fn a_target_palette_recolors_every_draw_under_it() {
    let s = palette_scene(
        "@target t.png 100x100 palette:elite\nuse d { swatch holder }\n\
         rect 4x4 fill:p.sky name:direct\nswatch name:inst\n\
         each i in 0..2\n  rect 4x4 fill:p.sky name:looped\n\
         if true\n  rect 4x4 fill:p.sky name:cond\n\
         holder name:h\n  top:\n    rect 4x4 fill:p.sky name:filled\n",
    );
    for path in ["direct", "inst.a", "looped.0", "looped.1", "cond", "h.top.filled"] {
        assert_eq!(fill_hex(node(&s, path)), "#222222", "{path}");
    }
}

#[test]
fn an_instance_palette_reaches_slot_content_written_in_the_caller() {
    let s = palette_scene(
        "@target t.png 100x100\nuse d { holder }\n\
         holder palette:elite name:h\n  top:\n    rect 4x4 fill:p.sky name:filled\n\
         holder name:g\n  top:\n    rect 4x4 fill:p.sky name:filled\n",
    );
    assert_eq!(fill_hex(node(&s, "h.top.filled")), "#222222");
    assert_eq!(fill_hex(node(&s, "g.top.filled")), "#111111");
}

#[test]
fn an_inner_instance_palette_replaces_an_outer_one() {
    let s = palette_scene(
        "@target t.png 100x100\nuse d { wrap swatch }\n\
         wrap palette:elite name:w\nswatch palette:hollow name:x\n",
    );
    assert_eq!(fill_hex(node(&s, "w.s.a")), "#222222");
    assert_eq!(fill_hex(node(&s, "w.t.a")), "#333333");
    assert_eq!(fill_hex(node(&s, "x.a")), "#333333");

    let s = palette_scene(
        "@target t.png 100x100 palette:elite\nuse d { swatch }\n\
         swatch name:outer\nswatch palette:hollow name:inner\n",
    );
    assert_eq!(fill_hex(node(&s, "outer.a")), "#222222");
    assert_eq!(fill_hex(node(&s, "inner.a")), "#333333");
}

#[test]
fn a_file_that_binds_p_itself_keeps_its_own_p() {
    let libs = [
        (
            "lib/own.art",
            "palette p\n  color sky #444444\ndef mine\n  rect 10x10 fill:p.sky name:a\n",
        ),
        (
            "lib/imp.art",
            "use palette/house as p\ndef theirs\n  rect 10x10 fill:p.sky name:a\n",
        ),
    ];
    let src = format!(
        "@target t.png 100x100 palette:elite\nuse own {{ mine }}\nuse imp {{ theirs }}\n\
         mine name:m\nmine palette:elite name:n\ntheirs palette:elite name:t\n\n{ELITE}"
    );
    let ev = palette_run(&src, &libs);
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
    let s = ev.scene.unwrap();
    assert_eq!(fill_hex(node(&s, "m.a")), "#444444");
    assert_eq!(fill_hex(node(&s, "n.a")), "#444444");
    assert_eq!(fill_hex(node(&s, "t.a")), "#111111");
}

#[test]
fn an_entry_file_top_level_let_p_keeps_its_own_p_in_its_defs() {
    let src = format!(
        "@target t.png 100x100 palette:elite\n{ELITE}\nlet p = hollow\n\
         def mine\n  rect 10x10 fill:p.sky name:a\n\
         mine name:m\nrect 10x10 fill:p.sky name:r\n"
    );
    let ev = palette_run(&src, &[]);
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
    let s = ev.scene.unwrap();
    assert_eq!(fill_hex(node(&s, "r")), "#333333");
    assert_eq!(fill_hex(node(&s, "m.a")), "#333333");
}

#[test]
fn an_instance_palette_that_is_not_a_palette_is_e0320() {
    let src = format!(
        "@target t.png 40x40\nuse d {{ swatch }}\nswatch name:ok\nswatch palette:#ff0000 name:bad\n\n{ELITE}"
    );
    let ev = palette_run(&src, &[("lib/d.art", PAINT_LIB)]);
    let d = diag(&ev, "E0320");
    assert_eq!((d.span.line, d.file.as_str()), (4, "art/t.art"));
    assert!(d.fix.as_deref().is_some_and(|f| f.contains("palette:elite")));
}

#[test]
fn a_target_palette_that_is_not_a_palette_is_reported_at_the_target() {
    let ev = palette_run("@target t.png 40x40 palette:3\nrect 4x4 fill:p.sky name:r\n", &[]);
    let d = diag(&ev, "E0206");
    assert_eq!((d.span.line, d.file.as_str()), (1, "art/t.art"));
    assert!(d.fix.as_deref().is_some_and(|f| f.contains("palette:")));
}

#[test]
fn a_palette_passed_as_a_module_parameter_hints_at_the_palette_key() {
    let lib = "def blob_t pal:p\n  rect 10x10 fill:pal.sky name:a\n";
    let src = format!(
        "@target t.png 40x40\nuse d {{ blob_t }}\nblob_t pal:elite name:b\n\n{ELITE}"
    );
    let ev = palette_run(&src, &[("lib/d.art", lib)]);
    let d = diag(&ev, "E0320");
    assert_eq!(d.span.line, 3);
    let fix = d.fix.as_deref().unwrap();
    assert!(fix.contains("palette:elite"), "{fix}");
    assert!(!fix.contains(".art"), "{fix}");
}

#[test]
fn a_palette_key_counts_as_reading_the_palette_it_names() {
    let manifest = PALETTE_MANIFEST.replace("unused = \"off\"\n", "");
    let src = "@target t.png 40x40 palette:elite\nuse d { swatch }\nuse palette/elite as elite\nuse palette/hollow as hollow\nswatch palette:hollow name:s\n";
    let libs = [
        ("lib/palette/elite.art", "palette elite\n  color sky #222222\n"),
        ("lib/palette/hollow.art", "palette hollow\n  color sky #333333\n"),
        ("lib/d.art", PAINT_LIB),
    ];
    let p = palette_project(&manifest, src, &libs);
    let plan = plan(&p);
    let ev = evaluate(&p, &plan.jobs[0]);
    assert!(
        ev.diagnostics.iter().all(|d| d.code != "L0103"),
        "{}",
        show(&ev.diagnostics)
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
    nodes.iter().for_each(|n| walk_deep_node(n, out));
}

fn walk_deep_node<'a>(n: &'a Node, out: &mut Vec<&'a Node>) {
    out.push(n);
    if let Some(mask) = &n.mask
        && let artscript_ir::MaskSource::Nodes(inner) = &mask.source
    {
        walk_deep(inner, out);
    }
    n.descendants().for_each(|c| walk_deep_node(c, out));
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
        "@target t.png 80x40\neach i in 0..2\n  group name:row\n    each j in 0..2\n      circle 4 at:(i * 20 + j * 5),0 name:gem\nassert @gem.1.0.cx ~= 20\n",
    );
    assert!(paths(&s).contains(&"gem.1.0".to_string()));

    let marked = scene(
        "@target t.png 80x40\neach i in 0..2\n  group name:row\n    circle 4 at:i * 10,0 name:mark\nassert @mark.1.cx ~= 10\n",
    );
    assert!(paths(&marked).iter().any(|p| p == "mark.1"));

    let sized = scene(
        "@target t.png 80x40\neach i in 0..2\n  group name:row\n    circle (8 + i * 10) name:a\n    circle (8 + i * 10) name:b\nassert same_size(@row.0.*)\n",
    );
    assert!(paths(&sized).contains(&"a.0".to_string()));

    let missed = run_with(
        "@target t.png 40x40\neach i in 0..2\n  group name:row\n    if i == 0\n      circle 4 name:mark\ncopy @mark.1\n",
        &[],
    );
    let d = diag(&missed, "E0350");
    assert_eq!(d.file, "art/t.art");
    assert_eq!(d.span.line, 6);
    assert!(d.fix.is_some(), "{d}");
}

#[test]
fn a_reference_into_an_instance_skips_its_groups() {
    let s = scene_with(
        "@target t.png 64x64\nuse g { thing }\nthing at:20,20 name:t\nexpect @t.box.w == 10\n",
        &[(
            "lib/g.art",
            "def thing\n  group at:0,0 name:grp\n    rect 10x10 fill:#ff0000 name:box\n",
        )],
    );
    assert!(paths(&s).contains(&"t.box".to_string()), "{:?}", paths(&s));
}

#[test]
fn a_reference_through_a_group_names_the_key() {
    let cases = [
        (
            "@target t.png 80x40\neach i in 0..2\n  group name:row\n    each j in 0..2\n      circle 4 at:(i * 20 + j * 5),0 name:gem\nassert @row.1.gem.0.cx ~= 20\n",
            "row.1.gem.0.cx",
            "write @gem.1.0.cx",
        ),
        (
            "@target t.png 40x40\ngroup name:grp\n  group\n    rect 10x10 name:box\nlet w = @grp.box.w\n",
            "grp.box.w",
            "write @box.w",
        ),
    ];
    for (src, written, fix) in cases {
        let ev = run_with(src, &[]);
        let d = diag(&ev, "E0350");
        assert_eq!(d.file, "art/t.art");
        assert!(
            d.message.contains(&format!("`@{written}` hops through")),
            "{d}"
        );
        assert_eq!(d.fix.as_deref(), Some(fix), "{d}");
    }

    let lib = (
        "lib/g.art",
        "def thing\n  group name:grp\n    rect 10x10 fill:#ff0000 name:box\n",
    );
    let ev = run_with(
        "@target t.png 64x64\nuse g { thing }\nthing at:20,20 name:t\nexpect @t.grp.box.w == 10\n",
        &[lib],
    );
    let d = diag(&ev, "E0350");
    assert_eq!((d.file.as_str(), d.span.line), ("art/t.art", 4));
    assert_eq!(d.fix.as_deref(), Some("write @t.box.w"), "{d}");

    let missing = run_with(
        "@target t.png 64x64\nuse g { thing }\nthing at:20,20 name:t\nexpect @t.lid.w == 10\n",
        &[lib],
    );
    let d = diag(&missing, "E0350");
    assert!(d.notes.iter().any(|n| n == "parts: grp, box"), "{d}");

    scene(
        "@target t.png 40x40\ngroup name:body\n  rect 10x10\n  anchor neck 5,7\nassert @body.neck.x ~= 5\n",
    );
}

#[test]
fn at_takes_an_anchor_reference_like_its_x_and_y() {
    let s = scene(concat!(
        "@target t.png 100x100\n",
        "group name:body at:30,20 rot:90\n",
        "  rect 10x10\n",
        "  anchor neck 5,7\n",
        "circle 2 at:@body.neck name:a\n",
        "circle 2 at:@body.neck.x,@body.neck.y name:b\n",
        "group at:10,10 rot:30 scale:2\n",
        "  circle 2 at:@body.neck name:c\n",
        "  circle 2 at:@body.neck.x,@body.neck.y name:d\n",
    ));
    near(at(node(&s, "a")), (23.0, 25.0));
    let b = at(node(&s, "b"));
    near(at(node(&s, "a")), (f64::from(b.0), f64::from(b.1)));
    let (c, d) = (at(node(&s, "c")), at(node(&s, "d")));
    near(c, (f64::from(d.0), f64::from(d.1)));
    assert!(c.0.abs() + c.1.abs() > 1.0, "{c:?}");
}

#[test]
fn at_takes_an_anchor_through_an_instance() {
    let s = scene(concat!(
        "@target t.png 100x100\n",
        "def hero\n",
        "  rect 10x10\n",
        "  anchor neck 0,-6\n",
        "hero at:40,50 name:hero\n",
        "circle 2 at:@hero.neck name:a\n",
    ));
    near(at(node(&s, "a")), (40.0, 44.0));
}

#[test]
fn a_reference_to_a_later_sibling_resolves_and_paints_at_its_own_line() {
    let s = scene(concat!(
        "@target t.png 100x100\n",
        "circle 2 at:@body.neck name:a\n",
        "group name:body at:30,20\n",
        "  rect 10x10\n",
        "  anchor neck 5,7\n",
    ));
    near(at(node(&s, "a")), (35.0, 27.0));
    let order = paths(&s);
    assert!(order[0] == "a" && order[1] == "body", "{order:?}");

    let s = scene(concat!(
        "@target t.png 100x100\n",
        "def rig\n",
        "  circle 2 at:@body.neck name:dot\n",
        "  group name:body at:30,20\n",
        "    rect 10x10\n",
        "    anchor neck 5,7\n",
        "rig name:r\n",
    ));
    near(at(node(&s, "r.dot")), (35.0, 27.0));
}

#[test]
fn a_chain_of_later_sibling_references_resolves() {
    let s = scene(concat!(
        "@target t.png 100x100\n",
        "circle 2 at:@arm.hand name:palm\n",
        "group name:arm at:@body.shoulder\n",
        "  rect 4x4\n",
        "  anchor hand 3,0\n",
        "group name:body at:30,20\n",
        "  rect 10x10\n",
        "  anchor shoulder -5,0\n",
    ));
    near(at(node(&s, "arm")), (25.0, 20.0));
    near(at(node(&s, "palm")), (28.0, 20.0));
    let order = paths(&s);
    let pos = |p: &str| order.iter().position(|x| x == p).unwrap();
    assert!(pos("palm") < pos("arm") && pos("arm") < pos("body"), "{order:?}");
}

#[test]
fn a_statement_that_waits_for_a_later_sibling_sees_only_the_lets_above_it() {
    let s = scene(concat!(
        "@target t.png 100x100\n",
        "let r = 2\n",
        "circle r at:@body.neck name:a\n",
        "let r = 6\n",
        "circle r at:10,10 name:b\n",
        "group name:body at:30,20\n",
        "  rect 10x10\n",
        "  anchor neck r,0\n",
    ));
    assert_box(&s, "a", 4.0, 4.0);
    assert_box(&s, "b", 12.0, 12.0);
    near(at(node(&s, "a")), (36.0, 20.0));
}

#[test]
fn siblings_that_read_each_other_are_one_e0350_that_suggests_z() {
    let ev = run_with(
        concat!(
            "@target t.png 100x100\n",
            "group name:a at:@b.tip\n",
            "  rect 10x10\n",
            "  anchor tip 5,0\n",
            "group name:b at:@a.top\n",
            "  rect 10x10\n",
            "  anchor top 0,-5\n",
        ),
        &[],
    );
    let errors: Vec<_> = ev
        .diagnostics
        .iter()
        .filter(|d| d.code.starts_with('E'))
        .collect();
    assert_eq!(errors.len(), 1, "{}", show(&ev.diagnostics));
    let d = errors[0];
    assert_eq!((d.code.as_str(), d.span.line), ("E0350", 2), "{d}");
    assert!(d.message.contains("before `b` is drawn"), "{d}");
    assert!(d.fix.as_deref().is_some_and(|f| f.contains("z:")), "{d}");
}

#[test]
fn a_statement_that_waits_on_a_cycle_does_not_take_the_cycle_error() {
    let ev = run_with(
        concat!(
            "@target t.png 100x100\n",
            "circle 2 at:@a.tip name:c\n",
            "group name:a at:@b.tip\n",
            "  rect 10x10\n",
            "  anchor tip 5,0\n",
            "group name:b at:@a.top\n",
            "  rect 10x10\n",
            "  anchor top 0,-5\n",
        ),
        &[],
    );
    let errors: Vec<_> = ev
        .diagnostics
        .iter()
        .filter(|d| d.code.starts_with('E'))
        .collect();
    assert_eq!(errors.len(), 1, "{}", show(&ev.diagnostics));
    assert_eq!((errors[0].code.as_str(), errors[0].span.line), ("E0350", 3), "{}", errors[0]);
}

#[test]
fn a_reference_to_a_name_in_an_untaken_arm_is_still_not_drawn() {
    let ev = run_with(
        "@target t.png 40x40\ncopy @mark\nif 1 == 2\n  circle 4 name:mark\n",
        &[],
    );
    let d = diag(&ev, "E0350");
    assert_eq!(d.span.line, 2, "{d}");
    assert!(d.message.contains("and not drawn"), "{d}");
}

#[test]
fn a_file_play_guard_may_read_a_later_sibling() {
    let s = scene(
        "@target t.png 40x40\nanim bob dur:1s\n  .marker at:10,0\nif @marker\n  play bob\ncircle 4 name:marker\n",
    );
    near(at(node(&s, "marker")), (30.0, 20.0));
}

#[test]
fn an_anim_offset_adds_to_an_anchor_rest_at() {
    let s = scene(concat!(
        "@target t.png 100x100 t:.5s\n",
        "anim bob dur:1s\n",
        "  .dot at:key(0,0 0,-10 ease:linear)\n",
        "group name:body at:30,40\n",
        "  rect 10x10\n",
        "  anchor neck 0,-6\n",
        "circle 2 at:@body.neck name:dot\n",
        "play bob\n",
    ));
    near(at(node(&s, "dot")), (30.0, 29.0));
}

#[test]
fn an_anim_offset_on_a_node_rest_at_reports_the_node_once() {
    let ev = run_with(
        concat!(
            "@target t.png 40x40\n",
            "anim bob dur:1s\n",
            "  .dot at:key(0,0 0,-10)\n",
            "rect 10x10 name:body\n",
            "circle 2 at:@body name:dot\n",
            "play bob\n",
        ),
        &[],
    );
    let errors: Vec<_> = ev.diagnostics.iter().filter(|d| d.code.starts_with('E')).collect();
    assert_eq!(errors.len(), 1, "{}", show(&ev.diagnostics));
    assert!(errors[0].message.contains("`at:@body` is a node, not a point"), "{}", errors[0]);
}

#[test]
fn at_names_the_node_when_the_reference_is_not_a_point() {
    let ev = run_with(
        "@target t.png 40x40\nrect 10x10 name:body\ncircle 2 at:@body\n",
        &[],
    );
    let d = diag(&ev, "E0320");
    assert!(d.message.contains("`at:@body` is a node, not a point"), "{d}");
    assert!(d.fix.as_deref().unwrap_or("").contains("at:@body.neck"), "{d}");
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
fn a_grid_copy_reads_references_in_its_own_cell() {
    let s = scene(
        "@target t.png 64x64\ncircle 4 at:32,32 fill:#ffffff name:hub\ngrid 2x1 cell:20,20\n  circle 2 at:@hub.cx,@hub.cy fill:#ffffff name:dot\n  copy @hub fill:#ffffff name:echo\nassert @dot.0.cx ~= @hub.cx\nassert @dot.1.cx ~= @hub.cx\nassert @echo.0.cx ~= @hub.cx\nassert @echo.1.cx ~= @hub.cx\n",
    );
    assert!(paths(&s).contains(&"dot.1".to_string()));
}

#[test]
fn a_later_grid_copy_without_a_cell_reads_in_its_own_cell() {
    let later = scene(
        "@target t.png 64x64\ncircle 4 at:32,32 fill:#ffffff name:hub\ngrid 2x1\n  if col == 1\n    circle 2 at:@hub.cx,@hub.cy fill:#ffffff name:dot\n  else\n    circle 2 fill:#ffffff name:dot\nassert @dot.1.cx ~= @hub.cx\n",
    );
    assert!(paths(&later).contains(&"dot.1".to_string()));
}

#[test]
fn the_first_copy_of_a_grid_without_a_cell_cannot_read_outside_it() {
    for (src, line) in [
        (
            "@target t.png 64x64\ncircle 4 at:32,32 fill:#ffffff name:hub\ngrid 2x1\n  circle 2 at:@hub.cx,@hub.cy fill:#ffffff name:dot\n",
            4,
        ),
        (
            "@target t.png 64x64\ngrid 2x1\n  circle 2 fill:#ffffff name:base\n  circle 1 at:@canvas.cx,0 fill:#ffffff name:dot\n",
            4,
        ),
        (
            "@target t.png 64x64\ncircle 4 at:32,32 fill:#ffffff name:hub\ngrid 2x1\n  copy @hub fill:#ffffff name:echo\n",
            4,
        ),
    ] {
        let ev = run_with(src, &[]);
        let hits: Vec<_> = ev.diagnostics.iter().filter(|d| d.code == "E0352").collect();
        assert_eq!(hits.len(), 1, "{}", show(&ev.diagnostics));
        assert_eq!(hits[0].file, "art/t.art");
        assert_eq!(hits[0].span.line, line);
        assert!(hits[0].fix.as_deref().unwrap().contains("cell:"), "{hits:?}");
    }
}

#[test]
fn a_grid_without_a_cell_reads_its_own_copy() {
    let s = scene(
        "@target t.png 80x40\ndef gem\n  circle 4 fill:#ffffff name:mark\n  circle 1 at:@mark.cx,@mark.cy fill:#ffffff name:dot\ncircle 8 fill:#ffffff name:hub\ngrid 2x1 gap:4\n  circle (@hub.w / 4) at:0,0 fill:#ffffff name:base\n  circle 1 at:@base.cx,@base.cy fill:#ffffff name:dot\n  gem at:0,0 name:gem\nassert @dot.0.cx ~= @base.0.cx\nassert @dot.1.cx ~= @base.1.cx\nassert @base.1.cx - @base.0.cx ~= 12\nassert @gem.1.dot.cx ~= @gem.1.mark.cx\ngrid 1x1\n  circle 1 at:@hub.cx,@hub.cy fill:#ffffff name:one\nassert @one.0.cx ~= @hub.cx\n",
    );
    assert!(paths(&s).contains(&"gem.1.dot".to_string()));
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
    assert_eq!(manifest.lint_level("raw-color"), LintLevel::Off);
    assert_eq!(manifest.lint_level("unused"), LintLevel::Error);
    for (src, line, col) in [
        ("[project]\nname = \"x\"\n\n[lint]\nraw-color = \"loud\"\n", 5, 13),
        ("[project]\n\n[lint]\nunused = 1\n", 4, 10),
        ("\nlint = 1\n", 2, 8),
        ("[project]\nname = \"x\"\n\n[lint\nraw-color = \"warn\"\n", 4, 6),
    ] {
        let err = Manifest::parse(src).unwrap_err();
        assert_eq!(err.code, "E0200");
        assert_eq!((err.span.line, err.span.col), (line, col), "{src:?}: {}", err.message);
    }
}

#[test]
fn manifest_rejects_what_it_would_otherwise_ignore() {
    use crate::Manifest;

    for (src, line, says) in [
        ("[lint]\noffcanvas = \"off\"\n", 2, "no lint `offcanvas`"),
        ("[project]\nnmae = \"x\"\n", 2, "no key `nmae`"),
        ("[defaults]\npallete = \"p\"\n", 2, "no key `pallete`"),
        ("[lints]\nunused = \"off\"\n", 1, "unknown field `lints`"),
        ("[build]\nbackends = \"cpu\"\n", 2, "unknown field `backends`"),
        ("[defaults]\nbg = \"red\"\n", 2, "bg"),
        ("[defaults]\nscale = [1, \"2\"]\n", 2, "scale"),
        ("[defaults]\nscale = 0\n", 2, "scale"),
        ("[project]\nlib = [1]\n", 2, "lib"),
        ("[project]\nout = 1\n", 2, "out"),
    ] {
        let err = Manifest::parse(src).unwrap_err();
        assert_eq!(err.code, "E0200");
        assert_eq!(err.span.line, line, "{src:?}: {}", err.message);
        assert!(err.message.contains(says), "{src:?}: {}", err.message);
    }
    let ok = Manifest::parse("[defaults]\nbg = \"#f4f1ea\"\nscale = 2\n").unwrap();
    assert_eq!((ok.bg.as_deref(), ok.scale), (Some("#f4f1ea"), vec![2.0]));
}

#[test]
fn manifest_palette_must_be_in_a_lib_root() {
    let manifest = "[project]\nlib = [\"lib\"]\n\n[defaults]\npalette = \"palette/nope\"\n";
    let err = Project::from_sources(manifest, Default::default()).unwrap_err();
    assert_eq!((err.code.as_str(), err.span.line), ("E0200", 5), "{}", err.message);
    assert!(err.message.contains("palette/nope"), "{}", err.message);
    let files = [("lib/palette/nope.art".to_string(), "palette nope\n  color a #000000\n".to_string())];
    assert!(Project::from_sources(manifest, files.into()).is_ok());
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
fn an_inside_or_outside_block_blends_with_what_is_below_it() {
    let draws = "  rect 20x20 at:10,10 fill:#ffffff\n  rect 40x20 at:20,10 fill:#80c0ff blend:multiply\n";
    let head = "@target t.png 40x20 bg:#010001\nrect 40x20 at:20,10 fill:none name:frame\ncircle 1 at:-9,-9 fill:none name:away\n";
    let plain = scene(&format!("{head}group\n{draws}"));
    let inside = scene(&format!("{head}inside @frame\n{draws}"));
    let outside = scene(&format!("{head}outside @away\n{draws}"));
    assert_eq!(pixel(&plain, 35, 10), [1, 0, 1, 255]);
    assert_eq!(pixel(&inside, 35, 10), pixel(&plain, 35, 10), "inside isolated its block");
    assert_eq!(pixel(&outside, 35, 10), pixel(&plain, 35, 10), "outside isolated its block");
    assert_eq!(pixel(&inside, 5, 10), pixel(&plain, 5, 10));
}

fn premul(p: [u8; 4]) -> [i32; 4] {
    let a = p[3] as i32;
    [p[0] as i32 * a / 255, p[1] as i32 * a / 255, p[2] as i32 * a / 255, a]
}

fn assert_near(got: [u8; 4], want: [u8; 4], tol: i32, what: &str) {
    let (g, w) = (premul(got), premul(want));
    assert!(g.iter().zip(w).all(|(g, w)| (g - w).abs() <= tol), "{what}: got {got:?}, want {want:?}");
}

#[test]
fn a_feathered_inside_or_outside_block_blends_with_what_is_below_it() {
    let draws = "  rect 20x20 at:10,10 fill:#ffffff\n  rect 40x20 at:20,10 fill:#80c0ff blend:multiply\n";
    let head = "@target t.png 40x20 bg:#010001\nrect 40x20 at:20,10 fill:none name:frame\ncircle 1 at:-9,-9 fill:none name:away\n";
    let plain = scene(&format!("{head}group\n{draws}"));
    let inside = scene(&format!("{head}inside @frame feather:2\n{draws}"));
    let outside = scene(&format!("{head}outside @away feather:2\n{draws}"));
    assert_eq!(pixel(&plain, 35, 10), [1, 0, 1, 255]);
    assert_near(pixel(&inside, 35, 10), pixel(&plain, 35, 10), 1, "inside isolated its block");
    assert_near(pixel(&outside, 35, 10), pixel(&plain, 35, 10), 1, "outside isolated its block");
}

#[test]
fn a_feathered_inside_block_counts_a_translucent_canvas_once_in_its_edge() {
    let draws = "rect 20x20 at:10,10 fill:none name:frame\ninside @frame feather:2\n  rect 40x6 at:20,3 fill:#ffffff80\n  rect 40x6 at:20,10 fill:#ff0000\n";
    let alone = scene(&format!("@target t.png 40x20\n{draws}"));
    let over = scene(&format!("@target t.png 40x20 bg:#3366cc80\n{draws}"));
    let bg = premul([0x33, 0x66, 0xcc, 0x80]);
    for y in [3, 10, 17] {
        for x in [16, 18, 20, 22] {
            let src = premul(pixel(&alone, x, y));
            let want: Vec<i32> = (0..4).map(|i| src[i] + (bg[i] * (255 - src[3]) + 127) / 255).collect();
            let got = premul(pixel(&over, x, y));
            assert!(
                got.iter().zip(&want).all(|(g, w)| (g - w).abs() <= 2),
                "at {x},{y}: got {got:?}, want {want:?}, the block alone over the canvas"
            );
        }
    }
}

#[test]
fn a_feathered_inside_block_fades_its_blend_into_the_canvas() {
    let head = "rect 20x20 at:10,10 fill:none name:frame\n";
    let coverage = scene(&format!("@target t.png 40x20\n{head}inside @frame feather:2\n  rect 40x20 at:20,10 fill:#ffffff\n"));
    let shade = "  rect 40x20 at:20,10 fill:#80c0ff blend:multiply\n";
    let gray = "@target t.png 40x20 bg:#808080\n";
    let plain = scene(&format!("{gray}{head}group\n{shade}"));
    let soft = scene(&format!("{gray}{head}inside @frame feather:2\n{shade}"));
    for x in [16, 18, 20, 22] {
        let m = pixel(&coverage, x, 10)[3] as f32 / 255.0;
        let full = pixel(&plain, x, 10);
        let want = full.map(|c| (128.0 + (c as f32 - 128.0) * m).round() as u8);
        assert_near(pixel(&soft, x, 10), [want[0], want[1], want[2], 255], 2, &format!("coverage {m} at {x},10"));
    }
}

#[test]
fn an_svg_inside_block_clips_its_blended_child_not_the_block() {
    let s = scene("@target t.svg 40x20 bg:#010001\nrect 40x20 at:20,10 fill:none name:frame\ninside @frame name:view\n  group\n    rect 40x20 at:20,10 fill:#80c0ff blend:multiply name:shade\n");
    let svg = artscript_render::render_svg(&s, artscript_ir::Profile::Svg, 1.0, artscript_render::Backend::Cpu).unwrap();
    let tag = |part: &str| svg.lines().find(|l| l.contains(part)).unwrap_or_else(|| panic!("no {part} in {svg}"));
    assert!(!tag("id=\"view\"").contains("clip-path"), "a clip-path on the block isolates it: {svg}");
    assert!(tag("mix-blend-mode:multiply").contains("clip-path"), "the blended child lost the clip: {svg}");
}

#[test]
fn an_svg_feathered_inside_block_masks_its_blended_child_not_the_block() {
    for clip in ["inside @frame feather:2", "outside @frame feather:2"] {
        let s = scene(&format!("@target t.svg 40x20 bg:#010001\nrect 30x20 at:15,10 fill:none name:frame\n{clip} name:view\n  group\n    rect 40x20 at:20,10 fill:#80c0ff blend:multiply name:shade\n"));
        let svg = artscript_render::render_svg(&s, artscript_ir::Profile::Svg, 1.0, artscript_render::Backend::Cpu).unwrap();
        let tag = |part: &str| svg.lines().find(|l| l.contains(part)).unwrap_or_else(|| panic!("no {part} in {svg}"));
        assert!(!tag("id=\"view\"").contains("mask="), "{clip}: a mask on the block isolates it: {svg}");
        assert!(tag("mix-blend-mode:multiply").contains("mask="), "{clip}: the blended child lost the mask: {svg}");
    }
}

#[test]
fn an_svg_feathered_inside_block_masks_its_overlapping_normal_children_as_one() {
    let s = scene("@target t.svg 40x20 bg:#ffffff\nrect 20x20 at:10,10 fill:none name:frame\ninside @frame feather:2\n  rect 40x20 at:20,10 fill:#ff0000 name:under\n  rect 40x20 at:20,10 fill:#0000ff name:over\n  rect 4x4 at:5,2 fill:#80c0ff blend:multiply name:shade\n");
    let svg = artscript_render::render_svg(&s, artscript_ir::Profile::Svg, 1.0, artscript_render::Backend::Cpu).unwrap();
    let lines: Vec<_> = svg.lines().collect();
    let at = |name: &str| lines.iter().position(|l| l.contains(&format!("{name}\""))).unwrap_or_else(|| panic!("no {name} in {svg}"));
    let (under, over) = (at("under"), at("over"));
    assert!(lines[under - 1].contains("<g mask="), "the run has no mask of its own: {svg}");
    assert!(lines[under..=over].iter().all(|l| !l.contains("mask=")), "each normal child fades on its own, so the lower one shows through: {svg}");
    assert!(lines[at("shade")].contains("mask="), "the blended child lost the mask: {svg}");
}

#[test]
fn an_svg_feathered_clip_writes_its_blur_filter_outside_the_mask_path() {
    for clip in ["inside @frame feather:2", "outside @frame feather:2"] {
        let s = scene(&format!("@target t.svg 40x20\nrect 30x20 at:15,10 fill:none name:frame\n{clip}\n  rect 40x20 at:20,10 fill:#80c0ff\n"));
        let svg = artscript_render::render_svg(&s, artscript_ir::Profile::Svg, 1.0, artscript_render::Backend::Cpu).unwrap();
        assert!(svg.contains("<feGaussianBlur"), "{clip}: no blur: {svg}");
        assert!(
            svg.lines().all(|l| !l.contains("<filter") || l.trim_start().starts_with("<filter")),
            "{clip}: a filter element inside another tag: {svg}"
        );
    }
}

#[test]
fn an_svg_outside_block_masks_by_luminance_so_its_black_hole_hides() {
    for feather in ["", " feather:2"] {
        let s = scene(&format!("@target t.svg 60x40\ncircle 10 at:30,20 fill:none name:hole\noutside @hole{feather}\n  rect 60x40 at:30,20 fill:#80c0ff\n"));
        let svg = artscript_render::render_svg(&s, artscript_ir::Profile::Svg, 1.0, artscript_render::Backend::Cpu).unwrap();
        let mask = svg.lines().find(|l| l.contains("<mask")).unwrap_or_else(|| panic!("no mask in {svg}"));
        assert!(svg.contains("fill=\"#000000\""), "outside{feather}: no black hole: {svg}");
        assert!(mask.contains("mask-type=\"luminance\""), "outside{feather}: an alpha mask keeps the black hole: {svg}");
    }
}

#[test]
fn an_svg_inside_block_clips_a_baked_child_in_canvas_coordinates() {
    for (feather, def) in [("", "<clipPath id=\"inside-1.rect-1-clip\">"), (" feather:2", "<mask id=\"inside-1.rect-1-pushed\"")] {
        let s = scene(&format!("@target t.svg 60x40 bg:#808080\nrect 30x30 at:30,20 fill:none name:frame\ninside @frame{feather}\n  rect 50x36 at:30,20 fill:#80c0ff blend:multiply\n    fx adjust hue:10\n"));
        let svg = artscript_render::render_svg(&s, artscript_ir::Profile::Svg, 1.0, artscript_render::Backend::Cpu).unwrap();
        assert!(svg.contains("<image x=\"5\" y=\"2\""), "inside{feather}: the child did not bake: {svg}");
        let clip = svg.split(def).nth(1).unwrap_or_else(|| panic!("inside{feather}: no {def} in {svg}"));
        let clip = &clip[..clip.find("</").unwrap()];
        assert!(clip.contains("M 15 5L 45 5L 45 35L 15 35Z") && !clip.contains("transform"), "inside{feather}: the clip is not in the tile's coordinates: {svg}");
    }
}

#[test]
fn abs_of_a_number_is_a_number_and_abs_of_a_vector_is_elementwise() {
    let s = scene(
        "@target t.png 64x64\nlet a = abs(-3)\nrect 10x10 at:a,10 fill:#3366cc name:r\nrect 4x4 at:abs(-4,-6) fill:#3366cc name:v\ncircle abs(-2) at:20,20 fill:#3366cc name:c\nexpect abs(-3) + 1 == 4\n",
    );
    near(at(node(&s, "r")), (3.0, 10.0));
    near(at(node(&s, "v")), (4.0, 6.0));
}

#[test]
fn extra_positional_items_on_a_builtin_verb_are_e0321() {
    let ev = run_with(
        "@target t.png 64x64\nlet w = 30\nlet h = 6\nrect w x h at:32,32 name:r\n",
        &[],
    );
    let d = diag(&ev, "E0321");
    assert!(
        d.message.contains("`rect`") && d.message.contains("`x`"),
        "{d}"
    );
    assert_eq!(d.span.line, 4);
    assert!(
        d.fix.as_deref().unwrap_or_default().contains("rect w,h"),
        "{d}"
    );

    for (src, verb, extra) in [
        ("circle 5 9", "circle", "9"),
        ("rect 30x6 7", "rect", "7"),
        ("group g\n  repeat 3 4\n    circle 2", "repeat", "4"),
        ("group g\n  grid 2x2 3\n    circle 2", "grid", "3"),
        ("layer bg 5\n  circle 2", "layer", "5"),
        ("group g\n  mirror x 2\n    circle 2", "mirror", "2"),
    ] {
        let ev = run_with(&format!("@target t.png 64x64\n{src}\n"), &[]);
        let d = diag(&ev, "E0321");
        assert!(
            d.message.contains(&format!("`{verb}`")) && d.message.contains(&format!("`{extra}`")),
            "{src}: {d}"
        );
    }
}

#[test]
fn an_unknown_key_on_a_builtin_verb_is_e0321() {
    let ev = run_with(
        "@target t.png 64x64\ncircle 6 at:32,32 fil:#7fd4a8\nrect 10,10 at:10,10 strok:#7fd4a8,2 nmae:box\n",
        &[],
    );
    let found: Vec<(u32, String, String)> = ev
        .diagnostics
        .iter()
        .filter(|d| d.code == "E0321")
        .map(|d| (d.span.line, d.message.clone(), d.fix.clone().unwrap_or_default()))
        .collect();
    let want = [
        (2, "`circle` has no key `fil`", "did you mean `fill:`?"),
        (3, "`rect` has no key `strok`", "did you mean `stroke:`?"),
        (3, "`rect` has no key `nmae`", "did you mean `name:`?"),
    ];
    assert_eq!(
        found,
        want.map(|(l, m, f)| (l, m.to_string(), f.to_string())),
        "{}",
        show(&ev.diagnostics)
    );

    let ev = run_with("@target t.png 64x64\npoly 6 r:20 inner:.5\n", &[]);
    let d = diag(&ev, "E0321");
    assert_eq!(d.message, "`poly` has no key `inner`");

    let ev = run_with("@target t.png 64x64\ngroup g\n  repeat 3 colour:4\n    circle 2\n", &[]);
    let d = diag(&ev, "E0321");
    assert_eq!(d.message, "`repeat` has no key `colour`");
    assert_eq!(d.span.line, 3);
    assert!(d.notes.iter().any(|n| n.contains("step")), "{d}");

    for src in [
        "star 5 r:20 inner:.5 round:2 jitter:1,20 seed:3",
        "blob 20 seed:4 wobble:.2 points:9",
        "curve 0,0 20,-15 40,0 closed tension:0 round:2 taper:.3",
        "rect 10x10 r:2 stroke:#000000,2 cap:butt join:miter dash:4,2 blur:1 a:.5 blend:multiply z:front feather:1 name:box",
        "group g\n  grid 2x2 gap:2 cell:4x4\n    circle 2",
        "group g\n  repeat 3 step:4,0 rot:10\n    circle 2",
        "group g style:ink fill:#000000\n  circle 2",
    ] {
        let ev = run_with(&format!("@target t.png 64x64\nstyle ink a:1\n{src}\n"), &[]);
        assert!(
            ev.diagnostics.iter().all(|d| d.code != "E0321"),
            "{src}: {}",
            show(&ev.diagnostics)
        );
    }
}

#[test]
fn an_unknown_key_on_an_fx_line_is_e0321() {
    let ev = run_with(
        "@target t.png 64x64\n\
         finish surface\n  fx rim 3 dri:1,1\n\
         rect 30 at:32,32 fill:#7fd4a8\n  fx glow 4 bogus:1 colour:#ffffff\n  fx outline 4 brush:dry\n  fx surface\n  fx blur 2 color:#000000\n",
        &[],
    );
    let found: Vec<(u32, String, String)> = ev
        .diagnostics
        .iter()
        .filter(|d| d.code == "E0321")
        .map(|d| (d.span.line, d.message.clone(), d.fix.clone().unwrap_or_default()))
        .collect();
    let list = "use one of the listed keys";
    let want = [
        (3, "`fx rim` has no key `dri`", "did you mean `dir:`?"),
        (5, "`fx glow` has no key `bogus`", list),
        (5, "`fx glow` has no key `colour`", "did you mean `color:`?"),
        (6, "`fx outline` has no key `brush`", list),
        (8, "`fx blur` has no key `color`", list),
    ];
    assert_eq!(
        found,
        want.map(|(l, m, f)| (l, m.to_string(), f.to_string())),
        "{}",
        show(&ev.diagnostics)
    );
    let glow = ev.diagnostics.iter().find(|d| d.message.ends_with("`bogus`")).unwrap();
    assert!(glow.notes.iter().any(|n| n.contains("strength")), "{glow}");

    for line in [
        "fx blur 2 blur:3 of:self as:soft",
        "fx shadow 0,4 blur:4 color:#000000 a:.5",
        "fx inner 0,4 blur:4 color:#000000 a:.5",
        "fx glow 8 strength:2 color:#ffffff a:.5",
        "fx rim 3 dir:1,1 color:#ffffff a:.5",
        "fx adjust hue:10 sat:-.2 light:.05",
        "fx outline 4 sides:top color:#000000 a:.5",
        "fx grain .05 scale:1.5 seed:3",
        "fx blur 2 as:b\n  fx merge self b a:.4,1",
    ] {
        let ev = run_with(&format!("@target t.png 64x64\nrect 30 at:32,32 fill:#7fd4a8\n  {line}\n"), &[]);
        assert!(
            ev.diagnostics.iter().all(|d| d.code != "E0321"),
            "{line}: {}",
            show(&ev.diagnostics)
        );
    }
}

#[test]
fn paint_on_a_boolean_operand_is_e0321() {
    let ev = run_with(
        "@target t.png 64x64\n\
         def _dot fill:#000000\n  circle 3 fill:fill\n\
         subtract fill:#88aaff name:wall\n  rect 60,60 at:32,32\n  ellipse 20,6 at:32,14 fill:#33aa33 a:.5\n  \
         each i in 0..2\n    circle 3 at:10*i,50 blend:screen\n      fx glow 2\n  _dot fill:#ffffff\n\
         union at:10,10\n  circle 4 stroke:#000000,2\n",
        &[],
    );
    let mut found: Vec<(u32, String, String)> = ev
        .diagnostics
        .iter()
        .filter(|d| d.code == "E0321")
        .map(|d| (d.span.line, d.message.clone(), d.fix.clone().unwrap_or_default()))
        .collect();
    found.sort();
    found.dedup();
    let wall = "to paint over the result, draw it in `inside @wall`";
    let ignored = |key: &str, verb: &str| {
        format!("{key} on an operand of `{verb}` is ignored; an operand gives only its geometry")
    };
    let want = [
        (6, ignored("`a:`", "subtract"), wall),
        (6, ignored("`fill:`", "subtract"), wall),
        (8, ignored("`blend:`", "subtract"), wall),
        (9, ignored("`fx glow`", "subtract"), wall),
        (
            12,
            ignored("`stroke:`", "union"),
            "to paint over the result, name the `union` and draw it in `inside @<name>`",
        ),
    ];
    assert_eq!(
        found,
        want.map(|(l, m, f)| (l, m, f.to_string())),
        "{}",
        show(&ev.diagnostics)
    );
}

#[test]
fn a_group_passes_its_paint_keys_to_its_children() {
    let s = scene("@target t.png 64x64\ngroup g fill:#ff0000\n  circle 4 name:a\n  circle 4 fill:#0000ff name:b\n");
    assert_eq!(fill_hex(node(&s, "a")), "#ff0000");
    assert_eq!(fill_hex(node(&s, "b")), "#0000ff");

    let ev = run_with("@target t.png 64x64\ncanvas fil:#000000\n", &[]);
    let d = diag(&ev, "E0321");
    assert_eq!(d.message, "`canvas` has no key `fil`");
    assert_eq!(d.fix.as_deref(), Some("did you mean `fill:`?"));
}

#[test]
fn a_vector_is_a_size_and_filtered_words_still_pass() {
    let s = scene(
        "@target t.png 64x64\nlet w = 30\nlet h = 6\nrect w,h at:32,32 name:r\nlayer bg\n  group g at:32,32\n    mirror x\n      circle 4 at:10,0 name:ear\ncurve 0,0 10,-5 20,0 30,5 40,0 name:wave\ncurve 0,0 10,10 20,0 closed name:loop\n",
    );
    let b = shape(&s, "r").geometry.compute_tight_bounds();
    assert_eq!((b.width(), b.height()), (30.0, 6.0));
    assert!(paths(&s).contains(&"ear~m".to_string()));
    assert!(paths(&s).iter().any(|p| p.ends_with("wave")));
    assert!(paths(&s).iter().any(|p| p.ends_with("loop")));
}

#[test]
fn a_reference_inside_a_def_reads_in_the_def_s_local_frame() {
    scene(
        "@target t.png 64x64\n\
         def post\n  rect 4x20 at:0,-10 fill:#000000 name:pole\n  anchor top 0,-20\n\
         def pair\n  post name:a\n  circle 3 at:@a.top.x,@a.top.y fill:#000000 name:knob\n\
         pair at:32,50 name:p1\n\
         pair at:20,40 scale:-1,1 rot:90 name:p2\n\
         assert @p1.knob.cx == @p1.a.top.x\n\
         assert @p1.knob.cy == @p1.a.top.y\n\
         assert @p2.knob.cx == @p2.a.top.x\n\
         assert @p2.knob.cy == @p2.a.top.y\n",
    );
}

#[test]
fn a_reference_inside_an_offset_group_reads_in_the_group_s_frame() {
    scene(
        "@target t.png 64x64\n\
         group at:32,50 name:g\n  rect 4x4 at:-10,0 fill:#000000 name:a\n  circle 2 at:@a.x,@a.cy fill:#000000 name:b\n\
         assert @b.cx == @a.x\n\
         assert @b.cy == @a.cy\n",
    );
}

#[test]
fn canvas_inside_a_transformed_group_reads_in_the_group_s_frame() {
    scene(
        "@target t.png 64x64\n\
         group at:10,6 scale:.5 name:g\n  circle canvas.w/4 at:canvas.cx,canvas.cy fill:#000000 name:c\n\
         assert @c.cx == canvas.cx\n\
         assert @c.cy == canvas.cy\n\
         assert @c.w == canvas.w/2\n",
    );
}

#[test]
fn a_child_with_no_at_sits_at_its_moved_group_s_origin() {
    scene(
        "@target t.png 64x64\n\
         group at:48,48 name:g\n  circle 5 fill:#000000 name:dot\n  rect 4x4 at:50%,0% fill:#000000 name:pct\n\
         layer l at:16,40\n  rect 6 fill:#000000 name:sq\n\
         assert @dot.cx == 48\n\
         assert @dot.cy == 48\n\
         assert @pct.cx == 48\n\
         assert @pct.cy == 16\n\
         assert @sq.cx == 16\n\
         assert @sq.cy == 40\n",
    );
}

#[test]
fn a_style_before_at_keeps_the_moved_group_s_frame() {
    scene(
        "@target t.png 64x64\n\
         style ink stroke:#000000,2\n\
         group style:ink at:48,48 name:g\n  circle 5 fill:#000000 name:dot\n\
         assert @dot.cx == 48\n\
         assert @dot.cy == 48\n",
    );
}

#[test]
fn a_mask_with_no_at_in_a_moved_group_covers_its_content() {
    scene(
        "@target t.png 64x64\n\
         group at:48,16 name:g\n  mask\n    rect 10x10 fill:#ffffff\n  circle 8 fill:#000000 name:dot\n\
         assert @g.w == 10\n\
         assert @g.cx == 48\n",
    );
}

#[test]
fn an_anim_that_moves_a_group_with_no_at_carries_its_children() {
    let s = scene(
        "@target t.png 64x64\n\
         anim slide dur:1s\n  .g at:20,0\n\
         group name:g\n  circle 5 fill:#000000 name:dot\n\
         play slide\n",
    );
    near(at(node(&s, "g")), (20.0, 0.0));
    near(at(node(&s, "dot")), (32.0, 32.0));
}

fn off_canvas_hits(src: &str) -> Vec<Diagnostic> {
    let manifest = "[project]\nout = \"out\"\nlib = [\"lib\"]\n\n[lint]\nraw-color = \"off\"\nunused = \"off\"\n";
    let p = Project::from_sources(
        manifest,
        [("art/t.art".to_string(), src.to_string())].into(),
    )
    .unwrap();
    let plan = plan(&p);
    let ev = evaluate(&p, &plan.jobs[0]);
    ev.diagnostics
        .into_iter()
        .filter(|d| d.code == "L0102")
        .collect()
}

#[test]
fn off_canvas_measures_a_shape_after_its_mask() {
    let hits = off_canvas_hits(
        "@target t.png 64x64\ngroup at:32,32 name:cell\n  mask\n    rect 40x40 at:0,0 fill:#4af name:m\n  rect 80x20 at:0,0 fill:#4af name:bar\n",
    );
    assert!(hits.is_empty(), "{}", show(&hits));

    let hits = off_canvas_hits(
        "@target t.png 64x64\ngroup at:32,32 name:cell\n  mask\n    rect 72x40 at:0,0 fill:#4af name:m\n  rect 80x20 at:0,0 fill:#4af name:bar\n",
    );
    assert_eq!(hits.len(), 1, "{}", show(&hits));
    assert_eq!(hits[0].node, "bar");
    assert!(
        hits[0].notes.iter().any(|n| n == "bounds -4,22 72x20"),
        "{}",
        show(&hits)
    );
}

#[test]
fn off_canvas_measures_a_shape_after_an_inside_clip() {
    let hits = off_canvas_hits(
        "@target t.png 64x64\nrect 40x40 at:32,32 fill:#4af name:x\ninside @x\n  rect 80x20 at:32,32 fill:#4af name:bar\n",
    );
    assert!(hits.is_empty(), "{}", show(&hits));

    let hits = off_canvas_hits(
        "@target t.png 64x64\nrect 40x40 at:32,32 fill:#4af name:x\noutside @x\n  rect 80x20 at:32,32 fill:#4af name:bar\n",
    );
    assert_eq!(hits.len(), 1, "{}", show(&hits));
}

const CUT_FX: &str = "@target t.png 64x64\nrect 10x20 at:32,55 fill:#4af name:r\n  fx outline 3\nrect 40x10 at:32,8 fill:#4af name:bar\n  fx glow 12\n";

#[test]
fn off_canvas_measures_what_effects_draw() {
    let plain = off_canvas_hits(
        "@target t.png 64x64\nrect 10x20 at:32,55 fill:#4af name:r\nrect 40x10 at:32,8 fill:#4af name:bar\n",
    );
    assert!(plain.is_empty(), "{}", show(&plain));

    let hits = off_canvas_hits(CUT_FX);
    let nodes: Vec<&str> = hits.iter().map(|d| d.node.as_str()).collect();
    assert_eq!(nodes, ["r", "bar"], "{}", show(&hits));
    assert_eq!(hits[0].span.line, 2);
    assert_eq!(hits[1].span.line, 4);
}

#[test]
fn off_canvas_measures_an_effect_that_reads_another_node() {
    let hits = off_canvas_hits(
        "@target t.png 64x64\nrect 40x10 at:32,8 fill:#4af name:bar\ncircle 4 at:32,40 fill:#4af name:dot\n  fx glow 12 of:@bar\n",
    );
    assert_eq!(hits.len(), 1, "{}", show(&hits));
    assert_eq!(hits[0].node, "dot");

    let hits = off_canvas_hits(
        "@target t.png 64x64\ncircle 4 at:32,40 fill:#4af name:dot\n  fx glow 40 of:@dot\n",
    );
    assert!(hits.is_empty(), "{}", show(&hits));
}

#[test]
fn outer_measures_what_effects_draw_and_plain_refs_do_not() {
    let s = scene(
        "@target t.png 100x100\n\
         rect 10x20 at:30,30 fill:#4af name:r\n  fx outline 3\n\
         rect 40x10 at:50,70 fill:#4af name:g\n  fx glow 4 a:1\n\
         rect 10x10 at:70,20 fill:#4af name:s\n  fx shadow 0,6 blur:2 color:#000\n\
         circle 1 at:@r.outer.right,@r.outer.bottom name:pr\n\
         circle 1 at:@r.right,@r.bottom name:vr\n\
         circle 1 at:@g.outer.left,@g.outer.bottom name:pg\n\
         circle 1 at:@g.left,@g.bottom name:vg\n\
         circle 1 at:@s.outer.top,@s.outer.bottom name:ps\n",
    );
    let glow = 4.0 * 2.884_402_7;
    let shadow = 2.0 * 2.884_402_7;
    near(at(node(&s, "pr")), (38.0, 43.0));
    near(at(node(&s, "vr")), (35.0, 40.0));
    near(at(node(&s, "pg")), (30.0 - glow, 75.0 + glow));
    near(at(node(&s, "vg")), (30.0, 75.0));
    near(at(node(&s, "ps")), (15.0, 31.0 + shadow));
}

#[test]
fn glow_strength_defaults_to_one_and_a_negative_one_is_an_error() {
    let s = scene("@target t.png 64x64\ncircle 4 at:32,32 fill:#4af name:a\n  fx glow 3\ncircle 4 at:32,32 fill:#4af name:b\n  fx glow 3 strength:2.5\n");
    let strength = |n: &str| match node(&s, n).effects[0].op {
        artscript_ir::EffectOp::Glow { strength, .. } => strength,
        ref op => panic!("{op:?}"),
    };
    assert_eq!((strength("a"), strength("b")), (1.0, 2.5));
    let ev = run_with("@target t.png 64x64\ncircle 4 at:32,32 fill:#4af\n  fx glow 3 strength:-1\n", &[]);
    let d = ev.diagnostics.iter().find(|d| d.code == "E0320").unwrap_or_else(|| panic!("{}", show(&ev.diagnostics)));
    assert_eq!(d.span.line, 3, "{d}");
}

#[test]
fn a_strong_faint_glow_reaches_as_far_as_its_scaled_alpha() {
    let s = scene(
        "@target t.png 100x100\n\
         rect 40x10 at:50,50 fill:#4af name:full\n  fx glow 4 a:1\n\
         rect 40x10 at:50,50 fill:#4af name:strong\n  fx glow 4 a:.25 strength:40\n\
         rect 40x10 at:50,50 fill:#4af name:lifted\n  fx glow 4 a:.1 strength:4\n\
         rect 40x10 at:50,50 fill:#4af name:faint\n  fx glow 4 a:.4\n\
         circle 1 at:@full.outer.left,0 name:pf\n\
         circle 1 at:@strong.outer.left,0 name:ps\n\
         circle 1 at:@lifted.outer.left,0 name:pl\n\
         circle 1 at:@faint.outer.left,0 name:pa\n",
    );
    let left = |n: &str| at(node(&s, n)).0;
    assert_eq!(left("ps"), left("pf"), "a halo clamped to opaque reaches as far as a full-alpha one");
    assert_eq!(left("pl"), left("pa"), "a:.1 strength:4 reaches as far as a:.4");
}

#[test]
fn inside_outer_fails_where_plain_inside_passes() {
    let ev = run_with(
        &format!("{CUT_FX}assert inside(@bar canvas margin:3)\n"),
        &[],
    );
    assert!(
        ev.diagnostics.iter().all(|d| d.code != "C0101"),
        "{}",
        show(&ev.diagnostics)
    );

    let ev = run_with(&format!("{CUT_FX}assert inside(@bar.outer canvas)\n"), &[]);
    let failed: Vec<_> = ev
        .diagnostics
        .iter()
        .filter(|d| d.code.starts_with('C'))
        .collect();
    assert_eq!(failed.len(), 1, "{}", show(&ev.diagnostics));
    assert_eq!(failed[0].span.line, 6);

    let ev = run_with(&format!("{CUT_FX}assert @bar.outer.top >= 0\n"), &[]);
    let failed: Vec<_> = ev
        .diagnostics
        .iter()
        .filter(|d| d.code.starts_with('C'))
        .collect();
    assert_eq!(failed.len(), 1, "{}", show(&ev.diagnostics));
    assert!(
        failed[0]
            .notes
            .iter()
            .any(|n| n.starts_with("bar.outer.top = -")),
        "{:?}",
        failed[0].notes
    );
}

#[test]
fn outer_follows_an_effect_that_reads_another_node() {
    let s = scene(
        "@target t.png 100x100\n\
         rect 40x10 at:50,30 fill:#4af name:bar\n\
         circle 4 at:50,70 fill:#4af name:dot\n  fx glow 4 a:1 of:@bar\n\
         circle 1 at:@dot.outer.cx,@dot.outer.top name:probe\n",
    );
    near(at(node(&s, "probe")), (50.0, 25.0 - 4.0 * 2.884_402_7));

    let ev = run_with(&format!("{CUT_FX}assert @bar.outer.foo > 0\n"), &[]);
    assert!(
        ev.diagnostics
            .iter()
            .any(|d| d.code == "E0350" && d.message.contains("`.outer.foo`")),
        "{}",
        show(&ev.diagnostics)
    );
}

#[test]
fn copy_of_an_outer_ref_is_its_box() {
    let s = scene(
        "@target t.png 100x100\nrect 20x10 at:50,50 fill:#4af name:r\n  fx outline 3\ncopy @r.outer fill:#4af name:c\n",
    );
    let b = node(&s, "c").bounds().unwrap();
    assert_eq!((b.left, b.top, b.right, b.bottom), (37.0, 42.0, 63.0, 58.0));
}

#[test]
fn the_outer_extent_grows_only_on_the_sides_an_outline_draws() {
    let s = scene(
        "@target t.png 100x100\nrect 20x10 at:50,50 fill:#4af name:r\n  fx outline 3 sides:top,left\ncopy @r.outer fill:#4af name:c\n",
    );
    let b = node(&s, "c").bounds().unwrap();
    assert_eq!((b.left, b.top, b.right, b.bottom), (37.0, 42.0, 60.0, 55.0));
}

#[test]
fn a_part_named_outer_wins_over_the_outer_extent() {
    let s = scene(
        "@target t.png 100x100\n\
         def badge\n  circle 10 fill:#4af name:outer\n  circle 2 at:0,30 fill:#4af name:pin\n\
         badge at:50,40 name:b\n  fx glow 8\n\
         circle 1 at:@b.outer.cx,@b.outer.bottom name:probe\n",
    );
    near(at(node(&s, "probe")), (50.0, 50.0));
}

#[test]
fn a_cut_group_glow_is_reported_once_at_the_group() {
    let hits = off_canvas_hits(
        "@target t.png 64x64\ngroup at:32,10 name:g\n  fx glow 12\n  rect 40x10 at:0,0 fill:#4af name:a\n  rect 10x4 at:0,2 fill:#4af name:b\n",
    );
    assert_eq!(hits.len(), 1, "{}", show(&hits));
    assert_eq!(hits[0].node, "g");
    assert_eq!(hits[0].span.line, 2);

    let hits = off_canvas_hits(
        "@target t.png 64x64\ngroup at:32,10 name:g\n  fx glow 12\n  rect 40x30 at:0,0 fill:#4af name:a\n",
    );
    assert_eq!(hits.len(), 1, "{}", show(&hits));
    assert_eq!(hits[0].node, "a");
}

#[test]
fn an_ancestor_clip_cuts_a_glow_and_the_node_s_own_clip_does_not() {
    let hits = off_canvas_hits(
        "@target t.png 64x64\nrect 20x20 at:32,32 fill:#4af name:x\ninside @x\n  rect 20x10 at:32,40 fill:#4af name:bar\n    fx glow 12\n",
    );
    assert!(hits.is_empty(), "{}", show(&hits));

    let hits = off_canvas_hits(
        "@target t.png 64x64\nrect 20x20 at:32,32 fill:#4af name:x\ninside @x name:clip\n  fx glow 12\n  rect 20x10 at:32,40 fill:#4af name:bar\n",
    );
    assert_eq!(hits.len(), 1, "{}", show(&hits));
    assert_eq!(hits[0].node, "clip");
}

#[test]
fn a_clipped_group_is_measured_inside_its_clip_and_mask() {
    let ev = run_with(
        "@target t.png 64x64\ncircle 10 at:32,32 fill:#4af name:disc\ninside @disc name:clip\n  rect 4x60 at:32,40 fill:#222 name:bar\nassert inside(@clip canvas margin:3)\nassert @clip.h == 20\n",
        &[],
    );
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));

    let ev = run_with(
        "@target t.png 64x64\ngroup at:32,32 name:cell\n  mask\n    rect 20x20 at:0,0 fill:#fff\n  rect 4x60 at:0,8 fill:#222 name:bar\nassert inside(@cell canvas margin:3)\nassert @cell.h == 20\n",
        &[],
    );
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));

    let ev = run_with(
        "@target t.png 64x64\nrect 20x20 at:32,32 fill:#4af name:x\noutside @x name:clip\n  rect 4x60 at:32,32 fill:#222 name:bar\nassert @clip.h == 60\n",
        &[],
    );
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
}

#[test]
fn name_listings_in_notes_are_sorted() {
    let defs = "def zeta\n  circle 1\ndef alpha\n  circle 1\ndef mid\n  circle 1\n";
    let ev = run_with(&format!("@target t.png 10x10\n{defs}nope 1\n"), &[]);
    assert!(
        diag(&ev, "E0301")
            .notes
            .contains(&"components in scope: alpha, mid, zeta".to_string()),
        "{:?}",
        diag(&ev, "E0301").notes
    );

    let styles = "style zeta fill:#fff\nstyle alpha fill:#fff\nstyle mid fill:#fff\n";
    let ev = run_with(&format!("@target t.png 10x10\n{styles}circle 1 style:nope\n"), &[]);
    assert!(
        diag(&ev, "E0307")
            .notes
            .contains(&"styles: alpha, mid, zeta".to_string()),
        "{:?}",
        diag(&ev, "E0307").notes
    );
}

#[test]
fn an_unbounded_point_count_is_e0320() {
    for src in ["poly 1/0 r:10", "star 1/0", "poly 1000000"] {
        let ev = run_with(&format!("@target t.png 40x40\n{src}\n"), &[]);
        assert_eq!(diag(&ev, "E0320").message, format!("`{}` needs a point count up to 100000", &src[..4]));
    }
    scene("@target t.png 40x40\npoly 6 r:10\nstar 5 r:10\n");
}

#[test]
fn a_comma_dash_dashes_the_stroke() {
    let s = scene("@target t.svg 40x40\ncurve 4,20 36,20 stroke:#000,2 dash:4,2\n");
    let svg = artscript_render::render_svg(&s, artscript_ir::Profile::Vector, 1.0, artscript_render::Backend::Cpu).unwrap();
    assert!(svg.contains("stroke-dasharray=\"4,2\""), "{svg}");
    let ev = run_with("@target t.png 40x40\ncurve 4,20 36,20 stroke:#000,2 dash:red\n", &[]);
    assert_eq!(diag(&ev, "E0320").message, "`dash:` takes lengths");
}

#[test]
fn an_odd_dash_list_repeats_in_png_as_in_svg() {
    let png = |dash: &str| {
        let s = scene(&format!("@target t.png 60x20\ncurve 2,10 58,10 stroke:#000,2 dash:{dash}\n"));
        artscript_render::render_png(&s, 1.0, artscript_render::Backend::Cpu).unwrap()
    };
    assert_eq!(png("[4 2 3]"), png("[4 2 3 4 2 3]"));
}

#[test]
fn rect_radii_are_one_number_or_four() {
    scene("@target t.png 40x40\nrect 20x20 at:20,20 r:4\nrect 20x20 at:20,20 r:4,4,0,0\nrect 20x20 at:20,20 r:[1 2 3 4]\n");
    let ev = run_with("@target t.png 40x40\nrect 20x20 r:4,4\n", &[]);
    assert_eq!(diag(&ev, "E0320").message, "`rect` needs one corner radius or four");
}

#[test]
fn a_transform_that_is_not_finite_or_not_a_vector_is_e0320() {
    for (attr, message) in [
        ("scale:1/0", "this node's transform is not finite"),
        ("at:1/0,0", "this node's transform is not finite"),
        ("skew:0,-90", "`skew:` must be between -90 and 90 degrees"),
        ("scale:red", "`scale:` expects a vector like 4,2, got a word"),
    ] {
        let ev = run_with(&format!("@target t.png 40x40\ncurve 0,0 10,10 stroke:#000,[2 4] {attr}\n"), &[]);
        assert_eq!(diag(&ev, "E0320").message, message, "{attr}");
    }
    scene("@target t.png 40x40\nrect 4x4 at:20,20 scale:2 skew:30,-45\n");
}

#[test]
fn a_field_below_a_threshold_masks_the_complement() {
    let alpha = |mask: &str| {
        let s = scene(&format!(
            "@target t.png 32x32\nrect 32x32 at:16,16 fill:#fff\n  mask {mask}\n"
        ));
        let (_, _, px) = artscript_render::render_rgba(&s, 1.0, artscript_render::Backend::Cpu).unwrap();
        px.chunks(4).map(|p| p[3] as i32).collect::<Vec<_>>()
    };
    let (above, below, flipped) = (
        alpha("simplex(8) > .4"),
        alpha("simplex(8) < .4"),
        alpha(".4 > simplex(8)"),
    );
    assert_eq!(below, flipped, "`.4 > f` is `f < .4`");
    assert!(above.contains(&255) && below.contains(&255));
    let mismatched = above.iter().zip(&below).filter(|(a, b)| (*a + *b - 255).abs() > 2).count();
    assert!(mismatched < 16, "{mismatched} pixels are in both masks or neither");
}

#[test]
fn an_off_patch_on_one_copy_hides_that_copy_and_counts_as_a_match() {
    let def = "@target t.png 64x64\ndef comp4\n  repeat 5\n    circle 3 at:10,0 name:dot8\n";
    for src in [
        format!("{def}comp4 name:b at:32,32\n  .dot8.1 off\n"),
        format!("{def}anim hide dur:1s\n  .dot8.1 off\ncomp4 name:b at:32,32\n  play hide\n"),
        format!("{def}anim hide dur:1s\n  .b.dot8.1 off\ncomp4 name:b at:32,32\nplay hide\n"),
    ] {
        let s = scene(&src);
        for i in [0, 2, 3, 4] {
            node(&s, &format!("b.dot8.{i}"));
        }
        assert!(!paths(&s).iter().any(|p| p == "b.dot8.1"), "{src}\n{:?}", paths(&s));
    }
}

#[test]
fn a_final_star_patch_skips_anchors() {
    let def = "@target t.png 64x64\ndef comp\n  poly 4 name:tail7\n  anchor anc8 38,6\n";
    for src in [
        format!("{def}comp name:b at:32,32\n  .* a:.5\n"),
        format!("{def}anim fade dur:1s\n  .* a:.5\ncomp name:b at:32,32\n  play fade\n"),
    ] {
        let s = scene(&src);
        assert!((node(&s, "b.tail7").opacity - 0.5).abs() < 1e-4, "{src}");
    }

    let only = run_with(
        "@target t.png 64x64\ndef comp\n  anchor anc8 38,6\ncomp name:b at:32,32\n  .* a:.5\n",
        &[],
    );
    let hits: Vec<_> = only.diagnostics.iter().filter(|d| d.code == "E0340").collect();
    assert_eq!(hits.len(), 1, "{}", show(&only.diagnostics));
    assert_eq!(hits[0].span.line, 5);
    assert!(hits[0].message.contains("matches nothing"), "{hits:?}");
}

#[test]
fn a_path_is_closed_only_when_every_subpath_ends_in_z() {
    let s = scene(
        "@target t.png 64x64\n\
         path \"M-24 0 Q-12 -24 0 0 T24 0\" stroke:#111,3 name:wave\n\
         path \"M0 0 L10 0 L10 10 Z\" name:tri\n\
         path \"M0 0 L10 0 L10 10 z M20 0 L30 0\" name:mixed\n\
         path \"M0 0 L10 0 L10 10\" name:bare\n",
    );
    assert!(shape(&s, "wave").fill.is_none());
    assert!(shape(&s, "mixed").fill.is_none());
    assert_eq!(fill_hex(node(&s, "tri")), "#8a8a8a");
    let bare = shape(&s, "bare");
    assert!(bare.fill.is_none());
    assert!(bare.stroke.is_some(), "an open path gets the open-curve default stroke");
}

#[test]
fn a_failure_under_each_loop_copy_is_reported_once() {
    let ev = run_with(
        "@target t.png 64x64\n\ndef dot r:6\n  circle r fill:#000 name:disc\n\neach i in 0..3\n  dot at:12+i*20,32 name:d\n    rect 4,20 fill:#000\n",
        &[],
    );
    let d = &ev.diagnostics;
    assert_eq!(d.len(), 1, "{}", show(d));
    assert_eq!((d[0].code.as_str(), d[0].node.as_str()), ("E0330", "d.0"));
    assert!(d[0].notes.contains(&"also in d.1, d.2".to_string()), "{}", show(d));
}

#[test]
fn a_failure_in_a_def_body_is_reported_once_for_every_instance() {
    let ev = run_with(
        "@target t.png 64x64\n\ndef dot r:6\n  circle r fill:#000 name:disc\n  pointy 3\n\neach i in 0..3\n  dot at:12+i*20,32 name:d\n",
        &[],
    );
    let d = &ev.diagnostics;
    assert_eq!(d.len(), 1, "{}", show(d));
    assert_eq!((d[0].code.as_str(), d[0].span.line), ("E0301", 5));
    assert!(d[0].notes.contains(&"also in d.1, d.2".to_string()), "{}", show(d));
}

#[test]
fn a_gif_names_each_failing_loop_copy_once() {
    let src = "@target t.gif 20x20 bg:#ffffff dur:1s fps:4\ndef dot k:0\n  circle 2\n  assert t > k\neach i in 0..3\n  dot k:i*.25 name:d\n";
    let p = project(&[("art/t.art", src)]);
    let plan = plan(&p);
    let sampled = crate::sample(&p, &plan.jobs[0]);
    let mut named: Vec<String> = Vec::new();
    for d in sampled.diagnostics.iter().filter(|d| d.code == "C0101") {
        named.push(d.node.clone());
        for n in d.notes.iter().filter_map(|n| n.strip_prefix("also in ")) {
            named.extend(n.split(", ").map(String::from));
        }
    }
    named.sort();
    assert_eq!(named, ["d.0", "d.1", "d.2"], "{}", show(&sampled.diagnostics));
}

#[test]
fn a_gif_folds_loop_copies_that_fail_on_every_frame() {
    let src = "@target t.gif 64x64 bg:#ffffff dur:1s fps:4\ndef dot r:6\n  circle r fill:#000 name:disc\n  pointy 3\neach i in 0..3\n  dot at:12+i*20,32 name:d\n";
    let p = project(&[("art/t.art", src)]);
    let plan = plan(&p);
    let sampled = crate::sample(&p, &plan.jobs[0]);
    let d = &sampled.diagnostics;
    assert_eq!(d.len(), 1, "{}", show(d));
    assert_eq!((d[0].code.as_str(), d[0].node.as_str()), ("E0301", "d.0"));
    assert!(d[0].notes.contains(&"also in d.1, d.2".to_string()), "{}", show(d));
}

fn outline_sides(src: &str) -> artscript_ir::Sides {
    let s = scene(&format!("@target t.png 100x100\nrect 20x20 name:c\n  fx outline 3 {src}\n"));
    node(&s, "c")
        .effects
        .iter()
        .find_map(|e| match e.op {
            EffectOp::Outline { sides, .. } => Some(sides),
            _ => None,
        })
        .expect("outline")
}

#[test]
fn outline_sides_names_the_sides_it_draws_on() {
    use artscript_ir::Sides;
    assert_eq!(outline_sides(""), Sides::ALL);
    assert_eq!(outline_sides("sides:top"), Sides { top: true, right: false, bottom: false, left: false });
    assert_eq!(
        outline_sides("sides:top,left"),
        Sides { top: true, right: false, bottom: false, left: true }
    );
    assert_eq!(outline_sides("sides:left,top"), outline_sides("sides:top,left"));
    assert_eq!(outline_sides("sides:top,right,bottom,left"), Sides::ALL);
}

#[test]
fn outline_sides_rejects_what_is_not_a_side_word() {
    for bad in ["sides:up", "sides:top,lef", "sides:3", "sides:[]", "sides:#ff0000", "sides:\"top\""] {
        let ev = run_with(
            &format!("@target t.png 100x100\nrect 20x20 name:c\n  fx outline 3 {bad}\n"),
            &[],
        );
        let d = diag(&ev, "E0320");
        assert_eq!(d.span.line, 3, "{bad}: {d}");
        assert!(d.message.contains("sides"), "{bad}: {d}");
    }
}

fn outline_mask<'a>(s: &'a Scene, path: &str) -> Option<&'a artscript_ir::Mask> {
    node(s, path)
        .effects
        .iter()
        .find_map(|e| match &e.op {
            EffectOp::Outline { mask, .. } => Some(mask.as_deref()),
            _ => None,
        })
        .expect("outline")
}

#[test]
fn fx_outline_takes_a_mask_written_like_a_node_mask_with_feather_on_the_mask() {
    use artscript_ir::MaskSource;
    let s = scene(
        "@target t.png 100x100\ncircle 8 name:m\nrect 20x20 name:bare\n  fx outline 3\nrect 20x20 name:block\n  fx glow 2\n  fx outline 3 feather:2\n    mask\n      rect 4x4\n      circle 2\nrect 20x20 name:value\n  fx outline 3\n    mask @m\nrect 20x20 name:field\n  fx outline 3\n    mask simplex(8) > .5\n",
    );
    assert!(outline_mask(&s, "bare").is_none());
    let block = outline_mask(&s, "block").unwrap();
    assert_eq!(block.feather, 2.0);
    assert!(!block.invert && !block.luminance);
    match &block.source {
        MaskSource::Nodes(n) => {
            assert_eq!(n.len(), 2);
            assert_eq!(n[0].path, "block.fx~2.mask~1.rect~1");
        }
        other => panic!("{other:?}"),
    }
    let value = outline_mask(&s, "value").unwrap();
    assert_eq!(value.feather, 0.0);
    assert!(matches!(&value.source, MaskSource::Nodes(n) if n.len() == 1));
    let field = outline_mask(&s, "field").unwrap();
    assert!(
        matches!(&field.source, MaskSource::Field { threshold: Some(t), .. } if (t - 0.5).abs() < 1e-6)
    );
}

#[test]
fn an_outline_mask_sits_in_the_node_frame_like_its_node_mask() {
    use artscript_ir::MaskSource;
    let s = scene(
        "@target t.png 100x100\nrect 20x20 at:50,50 rot:30 name:c\n  mask\n    circle 4\n  fx outline 3\n    mask\n      circle 4\n",
    );
    let c = node(&s, "c");
    let MaskSource::Nodes(own) = &c.mask.as_ref().unwrap().source else {
        panic!("node mask")
    };
    let MaskSource::Nodes(ring) = &outline_mask(&s, "c").unwrap().source else {
        panic!("outline mask")
    };
    assert_eq!(own[0].bounds(), ring[0].bounds());
    assert_eq!(own[0].transform, ring[0].transform);
}

#[test]
fn feather_on_fx_outline_needs_a_mask() {
    let ev = run_with("@target t.png 100x100\nrect 20x20 name:c\n  fx outline 3 feather:2\n", &[]);
    let d = diag(&ev, "E0320");
    assert_eq!(d.span.line, 3, "{d}");
    assert!(d.message.contains("feather"), "{d}");
    let ev = run_with("@target t.png 100x100\nrect 20x20 name:c\n  fx glow 3 feather:2\n", &[]);
    assert_eq!(diag(&ev, "E0321").span.line, 3);
}

#[test]
fn a_bad_outline_mask_is_reported_at_the_mask_line() {
    let ev = run_with("@target t.png 100x100\nrect 20x20 name:c\n  fx outline 3\n    mask 4\n", &[]);
    let d = diag(&ev, "E0320");
    assert_eq!(d.span.line, 4, "{d}");
    assert!(outline_mask(ev.scene.as_ref().unwrap(), "c").is_none());
}

#[test]
fn an_outline_field_mask_seeds_from_its_own_site() {
    use artscript_ir::{Field, MaskSource};
    let src = "@target t.png 100x100\nrect 20x20 name:a\n  rect 2x2\n  fx outline 3\n    mask simplex(8) > .5\nrect 20x20 name:b\n  fx outline 3\n    mask simplex(8) > .5\n";
    let seed = |s: &Scene, p: &str| match &outline_mask(s, p).unwrap().source {
        MaskSource::Field { field: Field::Noise { seed, .. }, .. } => *seed,
        other => panic!("{other:?}"),
    };
    let once = scene(src);
    let twice = scene(src);
    assert_eq!(seed(&once, "a"), seed(&twice, "a"));
    assert_ne!(seed(&once, "a"), seed(&once, "b"));
}

#[test]
fn explain_lists_an_outline_mask_and_its_nodes() {
    let ev = run_with(
        "@target t.png 40x40\ncircle 8 name:body\n  fx outline 2 feather:3\n    mask\n      circle 3 name:keep\n",
        &[],
    );
    let text = crate::explain(ev.scene.as_ref().unwrap(), &ev.seeded);
    assert!(text.contains("[fx outline 2 #201802 mask feather:3]"), "{text}");
    assert!(text.contains("\n  keep  shape"), "{text}");
}

#[test]
fn a_def_fx_outline_mask_names_and_numbers_its_nodes() {
    use artscript_ir::MaskSource;
    let s = scene(
        "@target t.png 100x100\ndef badge\n  circle 10\n  fx outline 2\n    mask\n      rect 4x4 at:(rand(16) - 8),0\n      rect 4x4 at:(rand(16) - 8),0\n      rect 4x4 at:(rand(16) - 8),0 name:keep\nbadge at:30,50 name:b\n",
    );
    let MaskSource::Nodes(n) = &outline_mask(&s, "b").unwrap().source else {
        panic!("outline mask")
    };
    let paths: Vec<&str> = n.iter().map(|n| n.path.as_str()).collect();
    assert_eq!(paths, ["b.fx~1.mask~1.rect~1", "b.fx~1.mask~1.rect~2", "b.keep"]);
    assert_ne!(at(&n[0]), at(&n[1]));
}

#[test]
fn an_fx_line_is_numbered_by_its_source_position_whatever_an_earlier_branch_does() {
    use artscript_ir::MaskSource;
    let src = |guard: &str| {
        format!(
            "@target t.png 100x100\ncircle 20 at:50,50 name:c\n  if {guard}\n    fx glow 2\n  fx outline 3\n    mask\n      rect 4x4 at:(rand(16) - 8),0\n"
        )
    };
    let rect = |s: &Scene| match &outline_mask(s, "c").unwrap().source {
        MaskSource::Nodes(n) => (n[0].path.clone(), at(&n[0])),
        other => panic!("{other:?}"),
    };
    let taken = scene(&src("true"));
    let skipped = scene(&src("false"));
    assert_eq!(rect(&taken).0, "c.fx~2.mask~1.rect~1");
    assert_eq!(rect(&taken), rect(&skipped));
}

#[test]
fn a_call_block_fx_line_counts_after_the_def_s_own_lines() {
    use artscript_ir::MaskSource;
    let s = scene(
        "@target t.png 100x100\ndef badge\n  circle 10\n  fx outline 2\n    mask\n      rect 4x4\nbadge at:30,50 name:b\n  fx outline 3\n    mask\n      circle 2\n",
    );
    let masks: Vec<String> = node(&s, "b")
        .effects
        .iter()
        .filter_map(|e| match &e.op {
            EffectOp::Outline { mask: Some(m), .. } => match &m.source {
                MaskSource::Nodes(n) => Some(n[0].path.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(masks, ["b.fx~1.mask~1.rect~1", "b.fx~2.mask~1.circle~1"]);
}

fn noise_wrap(s: &Scene, path: &str) -> Option<[f32; 2]> {
    match shape(s, path).fill.as_ref().map(|f| &f.paint) {
        Some(Paint::Field { field: artscript_ir::Field::Noise { wrap, .. }, .. }) => *wrap,
        other => panic!("{path} has fill {other:?}"),
    }
}

#[test]
fn wrap_sets_a_period_per_axis_on_a_noise_field() {
    let s = scene(
        "@target t.png 100x100\n\
         rect 20x20 fill:simplex(16 wrap:64) -> [#000 #fff] name:a\n\
         rect 20x20 fill:worley(16 wrap:64,32) -> [#000 #fff] name:b\n\
         rect 20x20 fill:value(16) -> [#000 #fff] name:c\n",
    );
    assert_eq!(noise_wrap(&s, "a"), Some([64.0, 64.0]));
    assert_eq!(noise_wrap(&s, "b"), Some([64.0, 32.0]));
    assert_eq!(noise_wrap(&s, "c"), None);
}

#[test]
fn a_wrap_that_is_not_a_positive_number_or_pair_is_e0320_at_its_line() {
    for bad in ["wrap:tile", "wrap:0", "wrap:-64", "wrap:64,0", "wrap:50%"] {
        let ev = run_with(
            &format!("@target t.png 40x40\ncircle 10 name:spacer\nrect 20x20 fill:perlin(16 {bad}) -> [#000 #fff] name:a\n"),
            &[],
        );
        let d = diag(&ev, "E0320");
        assert_eq!(d.span.line, 3, "{bad}: {d}");
        assert!(d.message.contains("wrap"), "{bad}: {d}");
    }
}

#[test]
fn fx_under_a_taken_if_applies_to_the_draw_in_source_order() {
    let src = |on: bool| {
        format!(
            "@target t.png 100x100\nlet on = {on}\ncircle 10 name:c\n  fx shadow 0,4\n  if on\n    fx blur 2\n  else\n    fx glow 3\n  fx outline 3\n"
        )
    };
    let s = scene(&src(true));
    assert_eq!(fx_ops(node(&s, "c")), ["Shadow", "Blur", "Outline"]);
    let written = scene(
        "@target t.png 100x100\ncircle 10 name:c\n  fx shadow 0,4\n  fx blur 2\n  fx outline 3\n",
    );
    assert_eq!(fx_debug(&s, "c"), fx_debug(&written, "c"));
    let s = scene(&src(false));
    assert_eq!(fx_ops(node(&s, "c")), ["Shadow", "Glow", "Outline"]);

    let s = scene(
        "@target t.png 100x100\nlet on = false\nlayer l\n  circle 10 name:c\n  if on\n    fx blur 2\n  else if true\n    if true\n      fx outline 2\n",
    );
    assert_eq!(fx_ops(node(&s, "l")), ["Outline"]);
    assert!(node(&s, "c").effects.is_empty());
}

#[test]
fn fx_under_an_if_in_a_def_body_or_call_block_applies_to_the_instance() {
    let s = scene(
        "@target t.png 100x100\nlet on = true\ndef badge glow:false\n  circle 10\n  fx shadow 0,4\n  if glow\n    fx glow 3\nbadge name:a\nbadge glow:true name:b\n  if on\n    fx outline 2\n",
    );
    assert_eq!(fx_ops(node(&s, "a")), ["Shadow"]);
    assert_eq!(fx_ops(node(&s, "b")), ["Shadow", "Glow", "Outline"]);
}

#[test]
fn fx_under_an_if_reads_the_lets_of_its_branch() {
    let s = scene(
        "@target t.png 100x100\nlet w = 1\ncircle 10 name:c\n  if true\n    let w = 5\n    fx blur w\ndef badge\n  circle 4\n  if true\n    let w = 5\n    fx blur w\nbadge name:a\nbadge name:b\n  if true\n    let w = 5\n    fx glow w\n",
    );
    let written = scene(
        "@target t.png 100x100\ncircle 10 name:c\n  fx blur 5\ndef badge\n  circle 4\n  fx blur 5\nbadge name:a\nbadge name:b\n  fx glow 5\n",
    );
    for path in ["c", "a", "b"] {
        assert_eq!(fx_debug(&s, path), fx_debug(&written, path), "{path}");
    }
}

#[test]
fn fx_under_an_if_on_a_boolean_operand_is_e0321() {
    let ev = run_with(
        "@target t.png 64x64\nsubtract name:wall\n  rect 60,60 at:32,32\n  circle 10 at:32,32\n    if true\n      fx glow 2\n",
        &[],
    );
    let d = diag(&ev, "E0321");
    assert_eq!(
        d.message,
        "`fx glow` on an operand of `subtract` is ignored; an operand gives only its geometry"
    );
    assert_eq!(d.span.line, 6);
}

fn outputs(src: &str) -> crate::OutputScenes {
    let p = project(&[("art/t.art", src)]);
    let plan = plan(&p);
    assert!(plan.diagnostics.is_empty(), "{:?}", plan.diagnostics);
    crate::evaluate_outputs(&p, &plan.jobs[0])
}

fn width_at(out: &crate::OutputScenes, scale: f32, path: &str) -> f64 {
    let scene = &out.at(scale).unwrap_or_else(|| panic!("no scene at {scale}")).scene;
    f64::from(shape(scene, path).geometry.compute_tight_bounds().width())
}

#[test]
fn scale_reads_each_png_outputs_own_scale() {
    let out = outputs("@target t.png 40x40 scale:.5,1,2.5\ncircle 5*scale name:c\n");
    assert!(out.diagnostics.is_empty(), "{}", show(&out.diagnostics));
    assert_eq!(out.scenes.len(), 3);
    for scale in [0.5, 1.0, 2.5] {
        let w = width_at(&out, scale, "c");
        assert!((w - 10.0 * f64::from(scale)).abs() < 0.05, "{scale}: {w}");
    }
}

#[test]
fn scale_is_the_number_the_target_wrote() {
    let out = outputs(
        "@target t.png 40x40 scale:1.1\nlet r = if scale == 1.1 then 6 else 3\ncircle r name:c\n",
    );
    assert!(out.diagnostics.is_empty(), "{}", show(&out.diagnostics));
    assert_eq!(width_at(&out, 1.1, "c"), 12.0);
}

#[test]
fn scale_is_one_for_an_svg_whatever_scale_it_lists() {
    let p = project(&[("art/t.art", "@target t.svg 40x40 scale:1,2\ncircle 5*scale name:c\n")]);
    let plan = plan(&p);
    let ev = evaluate(&p, &plan.jobs[0]);
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
    let s = ev.scene.unwrap();
    assert_eq!(shape(&s, "c").geometry.compute_tight_bounds().width(), 10.0);
}

fn gif_samples(src: &str) -> crate::OutputSamples {
    let p = project(&[("art/t.art", src)]);
    let plan = plan(&p);
    assert!(plan.diagnostics.is_empty(), "{:?}", plan.diagnostics);
    crate::sample_outputs(&p, &plan.jobs[0])
}

#[test]
fn scale_reads_each_gif_outputs_own_scale() {
    let out = gif_samples("@target t.gif 40x40 scale:.5,2 bg:#ffffff dur:1s fps:2\ncircle 5*scale name:c\n");
    assert!(out.diagnostics.is_empty(), "{}", show(&out.diagnostics));
    assert_eq!(out.samples.len(), 2);
    for scale in [0.5, 2.0] {
        let frames = &out.at(scale).unwrap_or_else(|| panic!("no sample at {scale}")).frames;
        assert!(!frames.is_empty(), "{scale}");
        for f in frames {
            let w = f64::from(shape(&f.scene, "c").geometry.compute_tight_bounds().width());
            assert!((w - 10.0 * f64::from(scale)).abs() < 0.05, "{scale}: {w}");
        }
    }
}

#[test]
fn a_parameter_named_scale_wins_over_the_output_scale() {
    let out = outputs("@target t.png 40x40 scale:2\ndef dot scale:3\n  circle scale name:c\ndot name:d\n");
    assert!(out.diagnostics.is_empty(), "{}", show(&out.diagnostics));
    assert_eq!(width_at(&out, 2.0, "d.c"), 6.0);
}

#[test]
fn diagnostics_from_every_scale_are_kept_and_a_shared_one_appears_once() {
    let out = outputs(
        "@target t.png 40x40 scale:1,2\nexpect scale in 0..1.5 \"scale stays small\"\nexpect 5 in 0..1 \"always\"\n",
    );
    let always = out.diagnostics.iter().filter(|d| d.message.contains("always")).count();
    let small = out.diagnostics.iter().filter(|d| d.message.contains("scale stays small")).count();
    assert_eq!((always, small), (1, 1), "{}", show(&out.diagnostics));
}

#[test]
fn each_scale_has_the_rig_its_own_scene_placed() {
    let out = outputs(
        "@target t.png 40x40 scale:1,2 pivot:@body.hip\ndef doll\n  circle 4\n  anchor hip 0,5*scale\ndoll at:10,10 name:body\n",
    );
    assert!(out.diagnostics.is_empty(), "{}", show(&out.diagnostics));
    assert_eq!(out.at(1.0).unwrap().rig.pivot, Some((10.0, 15.0)));
    assert_eq!(out.at(2.0).unwrap().rig.pivot, Some((10.0, 20.0)));
}

#[test]
fn a_let_named_scale_wins_over_the_output_scale() {
    let out = outputs("@target t.png 40x40 scale:2\nlet scale = 3\ncircle scale name:c\n");
    assert!(out.diagnostics.is_empty(), "{}", show(&out.diagnostics));
    assert_eq!(width_at(&out, 2.0, "c"), 6.0);
}

#[test]
fn a_library_let_named_scale_wins_inside_its_own_defs() {
    let ev = run_with(
        "@target t.png 40x40 scale:2\nuse lib/dots\ndot name:d\n",
        &[("lib/dots.art", "let scale = 3\ndef dot\n  circle scale name:c\n")],
    );
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
    let s = ev.scene.unwrap();
    assert_eq!(shape(&s, "d.c").geometry.compute_tight_bounds().width(), 6.0);
}

#[test]
fn a_patch_under_an_instance_palette_reads_that_palette() {
    let s = palette_scene(
        "@target t.png 100x100\nuse d { swatch }\nswatch palette:elite name:s\n  .a fill:p.sky\n  fx grain .1\nswatch name:u\n  .a fill:p.sky\n",
    );
    assert_eq!(fill_hex(node(&s, "s.a")), "#222222");
    assert_eq!(fill_hex(node(&s, "u.a")), "#111111");
}

#[test]
fn each_binds_a_loop_index_before_the_value() {
    let s = scene(
        "@target t.png 80x80\nlet list = [5 6 7]\nlayer l\n  each i, x in list\n    circle 1 at:i*10,x\n",
    );
    assert_eq!(at(node(&s, "l.circle~1.0")), (0.0, 5.0));
    assert_eq!(at(node(&s, "l.circle~1.1")), (10.0, 6.0));
    assert_eq!(at(node(&s, "l.circle~1.2")), (20.0, 7.0));
}

#[test]
fn the_loop_index_counts_positions_over_a_range() {
    let s = scene("@target t.png 80x80\neach i, x in 4..7\n  circle 1 at:i,x\n");
    assert_eq!(at(node(&s, "circle~1.0")), (0.0, 4.0));
    assert_eq!(at(node(&s, "circle~1.2")), (2.0, 6.0));
}

#[test]
fn each_expr_binds_a_loop_index() {
    scene(
        "@target t.png 80x80\nlet pts = [each i, x in [5 6 7] -> i,x]\nexpect pts == [0,5 1,6 2,7]\ncircle 1\n",
    );
}

#[test]
fn a_loop_index_inside_a_def_body_and_nested_loops() {
    let s = scene(
        "@target t.png 80x80\ndef dots\n  each i, x in [1 2]\n    circle 1 at:i,x name:dot\ndots name:d\neach r, _a in [0 0]\n  each c, _b in [0 0 0]\n    rect 1 at:c,r name:cell\n",
    );
    assert_eq!(at(node(&s, "d.dot.1")), (1.0, 2.0));
    assert_eq!(at(node(&s, "cell.1.2")), (2.0, 1.0));
}

#[test]
fn adding_a_loop_index_keeps_the_seeds_inside_the_loop() {
    let plain = scene("@target t.png 80x80\neach x in 0..3\n  circle 1 at:rand(0 40),x\n");
    let indexed = scene("@target t.png 80x80\neach i, x in 0..3\n  circle 1 at:rand(0 40),x\n");
    for k in 0..3 {
        let path = format!("circle~1.{k}");
        assert_eq!(at(node(&plain, &path)), at(node(&indexed, &path)), "{path}");
    }
    assert_ne!(
        at(node(&plain, "circle~1.0")).0,
        at(node(&plain, "circle~1.1")).0
    );
}

#[test]
fn a_list_is_indexed_by_an_expression() {
    let s = scene(
        "@target t.png 80x80\nlet tones = [#111111 #222222 #333333]\nlet row = 4\nlet i = 1\ncircle 2 fill:tones.{mod(row 3)} name:a\ncircle 2 fill:tones.{i+1} name:b\ncircle 2 fill:tones.{i} name:c\ncircle 2 fill:tones.2 name:d\n",
    );
    assert_eq!(fill_hex(node(&s, "a")), "#222222");
    assert_eq!(fill_hex(node(&s, "b")), "#333333");
    assert_eq!(fill_hex(node(&s, "c")), "#222222");
    assert_eq!(fill_hex(node(&s, "d")), "#333333");
}

#[test]
fn an_index_inside_each_reads_the_loop_names() {
    let s = scene(
        "@target t.png 80x80\nlet tones = [#111111 #222222 #333333]\neach i, x in 0..4\n  circle 2 fill:tones.{mod(i 3)} name:dot\n",
    );
    let fills: Vec<String> = (0..4).map(|k| fill_hex(node(&s, &format!("dot.{k}")))).collect();
    assert_eq!(fills, ["#111111", "#222222", "#333333", "#111111"]);
}

#[test]
fn an_index_that_is_not_a_name_or_a_number_is_e0320() {
    let ev = run_with(
        "@target t.png 40x40\nlet tones = [1 2 3]\nlet v = 1,2\ncircle tones.{v}\n",
        &[],
    );
    let d = diag(&ev, "E0320");
    assert_eq!(d.message, "an index is a name or a number, got a vector like 4,2");
    assert_eq!(
        d.fix.as_deref(),
        Some("write a number or a name inside `{}`, such as `list.{i}`")
    );
}

#[test]
fn a_vector_answers_0_and_1_as_x_and_y() {
    let s = scene("@target t.png 60x60\nlet v = 10,20\ncircle 4 at:v.0,v.1 name:dot\n");
    assert_eq!(at(node(&s, "dot")), (10.0, 20.0));
    let ev = run_with("@target t.png 60x60\nlet v = 10,20\ncircle 4 at:v.2,0\n", &[]);
    let d = diag(&ev, "E0350");
    assert_eq!(d.message, "a vector has no member `2`");
}

#[test]
fn an_unbound_name_in_an_index_is_the_member_name() {
    let ev = run_with("@target t.png 40x40\nlet tones = [1 2 3]\ncircle tones.{nope}\n", &[]);
    let d = diag(&ev, "E0350");
    assert_eq!(d.message, "a list has no member `nope`");
}

#[test]
fn an_index_picks_one_copy_of_a_family() {
    let s = scene(
        "@target t.png 80x80\nlet k = 1\neach i in 0..3\n  circle 2 at:i*10+5,10 name:dot\ncircle 2 at:@dot.{k}.cx,30 name:a\ncircle 2 at:@dot.{k+1}.cx,40 name:b\n",
    );
    assert_eq!(at(node(&s, "a")), (15.0, 30.0));
    assert_eq!(at(node(&s, "b")), (25.0, 40.0));
}

#[test]
fn a_vector_branch_of_an_if_evaluates_in_a_let() {
    let s = scene(
        "@target t.png 80x80\nlet k = 3\nlet p = if k < 19 then k*32,200 else 576,300\nlet q = if k > 19 then k*32,200 else 576,300\ncircle 2 at:p name:a\ncircle 2 at:q name:b\n",
    );
    assert_eq!(at(node(&s, "a")), (96.0, 200.0));
    assert_eq!(at(node(&s, "b")), (576.0, 300.0));
}

#[test]
fn a_comma_before_a_vector_sum_is_e0320_with_the_parenthesized_fix() {
    let fix = "wrap the vector in parentheses, as in `(a,b) + v`, because `+` binds tighter than `,`";
    for (line, key) in [
        ("circle 2 at:(a),(b) + v", "at"),
        ("circle 2 at:a,b + v", "at"),
        ("circle 2 at:a + v,b", "at"),
        ("rect 4x4 at:8,8 pivot:a,b + v", "pivot"),
        ("repeat 3 step:a,b + v\n  circle 2", "step"),
        ("grid 2x2 cell:a,b + v\n  circle 2", "cell"),
        ("circle 4 fill:radial(#fff #000 at:a,b + v)", "at"),
        ("circle 4 fill:conic(#fff #000 at:a,b + v)", "at"),
        ("rect 4x4 scale:a,b + v", "scale"),
        ("rect 4x4 skew:a,b + v", "skew"),
        ("circle 4\n  fx rim 3 dir:a,b + v", "dir"),
    ] {
        let ev = run_with(
            &format!("@target t.png 40x40\nlet a = 3\nlet b = 4\nlet v = 10,10\n{line}\n"),
            &[],
        );
        let d = diag(&ev, "E0320");
        assert_eq!(d.message, format!("`{key}:` expects a vector like 4,2, got a list"), "{line}");
        assert_eq!(d.fix.as_deref(), Some(fix), "{line}");
    }
}

#[test]
fn a_parenthesized_vector_sum_is_a_vector_in_every_vector_key() {
    let s = scene(
        "@target t.png 40x40\nlet a = 3\nlet b = 4\nlet v = 10,10\ncircle 2 at:(a,b) + v name:c\nrepeat 2 step:(a,b) + v\n  circle 2 name:r\ngrid 2x2 cell:(a,b) + v\n  circle 2 name:g\nrect 4x4 at:5,5 pivot:(a,b) + v scale:(1,2) + v name:p\n",
    );
    assert_eq!(at(node(&s, "c")), (13.0, 14.0));
    assert_eq!(at(node(&s, "r.1")), (33.0, 34.0));
}

#[test]
fn a_vector_key_that_is_not_a_vector_names_the_key() {
    for (line, message, fix) in [
        ("rect 4x4 pivot:red", "`pivot:` expects a vector like 4,2, got a word", "write `pivot:4,2`"),
        ("repeat 3 step:[1 2 3]\n  circle 2", "`step:` expects a vector like 4,2, got a list", "write `step:4,2`"),
        ("grid 2x2 cell:red\n  circle 2", "`cell:` expects a vector like 4,2, got a word", "write `cell:4,2`"),
        ("circle 4 fill:radial(#fff #000 at:red)", "`at:` expects a vector like 4,2, got a word", "write `at:4,2`"),
        ("circle 4\n  fx rim 3 dir:red", "`dir:` expects a vector like 4,2, got a word", "write `dir:4,2`"),
        ("repeat 3 step:10%,2\n  circle 2", "`step:` takes numbers, not percents", "write `step:4,2` in canvas units"),
        ("curve 0,0 20,0 stroke:#000,[2 4] taper:red", "`taper:` expects a vector like 4,2, got a word", "write `taper:4,2`"),
    ] {
        let ev = run_with(&format!("@target t.png 40x40\n{line}\n"), &[]);
        let d = diag(&ev, "E0320");
        assert_eq!(d.message, message, "{line}");
        assert_eq!(d.fix.as_deref(), Some(fix), "{line}");
    }
}

#[test]
fn concat_joins_lists_and_ranges_in_order() {
    scene(
        "@target t.png 10x10\nexpect concat([1 2] [3]) == [1 2 3]\nexpect concat(0..2 [5] 7..9) == [0 1 5 7 8]\nexpect concat([0,0 1,1] [2,2]) == [0,0 1,1 2,2]\nexpect concat([] [1]) == [1]\ncircle 1\n",
    );
}

#[test]
fn concat_reports_what_is_not_a_list() {
    for (call, message, fix) in [
        (
            "concat([1] 2)",
            "`concat` argument 2 must be a list, got a number",
            "write `concat(a b)` with lists or ranges, and `[v]` for one item",
        ),
        (
            "concat([1] 3,4)",
            "`concat` argument 2 must be a list, got a vector like 4,2",
            "write `concat(a b)` with lists or ranges, and `[v]` for one item",
        ),
        ("concat([1])", "`concat` joins two or more lists", "write `concat(a b)`"),
        ("concat()", "`concat` joins two or more lists", "write `concat(a b)`"),
    ] {
        let ev = run_with(&format!("@target t.png 10x10\nlet xs = {call}\ncircle 1\n"), &[]);
        let d = diag(&ev, "E0320");
        assert_eq!(d.message, message, "{call}");
        assert_eq!(d.fix.as_deref(), Some(fix), "{call}");
    }
}

#[test]
fn a_curve_takes_points_and_lists_of_points_in_order() {
    let geometry = |src: &str| {
        let s = scene(&format!("@target t.png 80x80\nlet pts = [10,10 50,10]\n{src} name:c\n"));
        shape(&s, "c").geometry.to_svg()
    };
    let literal = geometry("curve 10,10 50,10 50,50 0,50");
    assert_eq!(geometry("curve pts 50,50 0,50"), literal);
    assert_eq!(geometry("curve 10,10 [50,10 50,50] 0,50"), literal);
    assert_eq!(geometry("curve concat(pts [50,50 0,50])"), literal);
    assert_eq!(geometry("curve pts [50,50] [0,50]"), literal);
    assert_eq!(geometry("curve [10,10 50,10 50,50 0,50]"), literal);
}

#[test]
fn a_curve_item_that_is_not_a_point_still_names_the_points_it_needs() {
    let ev = run_with("@target t.png 80x80\nlet pts = [10,10 50,10]\ncurve pts red\n", &[]);
    let d = diag(&ev, "E0320");
    assert!(d.message.contains("points like 0,0 20,-15"), "{d}");
}

#[test]
fn a_seed_that_is_not_a_number_is_e0320_at_its_line() {
    let brush = ("lib/brushes.art", "brush dry\n  sksl \"\"\"\n    float4 shade(Mark m) { return float4(m.paint.rgb, cover(m.edge, m)); }\n  \"\"\"\n");
    let stamp = ("lib/garden.art", "stamp dots spacing:5\n  circle 1 fill:#000000\n");
    let blot = ("lib/d.art", "def blot\n  rect 10x10 fill:#000000 name:fill\n");
    let none: &[(&str, &str)] = &[];
    let cases = [
        ("circle 4 seed:\"a\" name:n", "a string", none),
        ("rect 20x20 jitter:0,60 seed:\"a\" name:n", "a string", none),
        ("rect 20x20 fill:#000000 seed:foo name:n", "a word", none),
        ("circle rand(4 8 seed:\"a\") name:n", "a string", none),
        ("circle pick([4 8] seed:foo) name:n", "a word", none),
        ("rect 20x20 fill:simplex(16 seed:\"a\") -> [#000000 #ffffff] name:n", "a string", none),
        ("blob 20 seed:\"a\" name:n", "a string", none),
        ("rect 20x20 jitter:1.5,60 seed:\"a\" name:n", "a string", none),
        ("rect 20x20 name:n\n  fx grain .3 seed:\"a\"", "a string", none),
        ("use brushes { dry }\nrect 20x20 fill:#ff0000 brush:dry seed:\"a\" name:n", "a string", &[brush]),
        ("use garden { dots }\ncurve 0,9 30,9 stroke:#000000,2 stamp:dots seed:\"a\" name:n", "a string", &[stamp]),
        ("use d { blot }\nblot seed:\"a\" name:n", "a string", &[blot]),
    ];
    for (body, kind, libs) in cases {
        let ev = run_with(&format!("@target t.png 40x40\n{body}\n"), libs);
        let line = 2 + body.lines().count() as u32 - 1;
        let d = diag(&ev, "E0320");
        assert_eq!(d.message, format!("`seed:` takes a number, got {kind}"), "{body}: {d}");
        assert_eq!(d.span.line, line, "{body}: {d}");
        assert_eq!(ev.diagnostics.len(), 1, "{body}: {}", show(&ev.diagnostics));
    }
}

fn contour_len(s: &Scene, path: &str) -> f64 {
    skia_safe::ContourMeasureIter::new(&shape(s, path).geometry, false, None)
        .map(|m| f64::from(m.length()))
        .sum()
}

fn contour_count(s: &Scene, path: &str) -> usize {
    skia_safe::ContourMeasureIter::new(&shape(s, path).geometry, false, None).count()
}

fn ends(s: &Scene, path: &str) -> ((f32, f32), (f32, f32)) {
    let pts = shape(s, path).geometry.points().to_vec();
    let (a, z) = (pts[0], pts[pts.len() - 1]);
    ((a.x, a.y), (z.x, z.y))
}

#[test]
fn edge_of_a_circle_runs_clockwise_from_three_oclock() {
    let s = scene("@target t.png 100x100\ncircle 20 at:50,50 name:c\nedge @c to:.25 name:e\n");
    let (start, end) = ends(&s, "e");
    near(start, (70.0, 50.0));
    near(end, (50.0, 70.0));
    assert!(shape(&s, "e").fill.is_none() && shape(&s, "e").stroke.is_some());
}

#[test]
fn edge_of_a_rect_starts_at_the_top_left_and_runs_along_the_top() {
    let s = scene("@target t.png 100x100\nrect 40x20 at:50,50 name:r\nedge @r to:.25 name:e\n");
    let (start, end) = ends(&s, "e");
    near(start, (30.0, 40.0));
    near(end, (60.0, 40.0));
}

#[test]
fn edge_of_a_rounded_rect_starts_at_the_middle_of_the_top_left_arc() {
    let s = scene(
        "@target t.png 100x100\nrect 40x20 r:6 at:50,50 name:r\nedge @r to:.1 name:e\ncopy @r name:c\nedge @c to:.1 name:ec\nedge @r name:all\n",
    );
    assert_eq!(contour_count(&s, "all"), 1);
    assert!((contour_len(&s, "all") - contour_len(&s, "r")).abs() < 1e-3);
    let mid = 36.0 - 6.0 * std::f64::consts::FRAC_1_SQRT_2;
    let (start, _) = ends(&s, "e");
    assert!(
        (f64::from(start.0) - mid).abs() < 0.05 && (f64::from(start.1) - (mid + 10.0)).abs() < 0.05,
        "{start:?}"
    );
    let (copied, _) = ends(&s, "ec");
    near(copied, (f64::from(start.0), f64::from(start.1)));
}

#[test]
fn edge_of_a_poly_starts_at_the_top_vertex_even_when_rounded() {
    let s = scene(
        "@target t.png 100x100\npoly 5 r:20 at:50,50 name:p\nedge @p to:.2 name:e\npoly 5 r:20 round:6 at:50,50 name:q\nedge @q to:.2 name:f\n",
    );
    near(ends(&s, "e").0, (50.0, 30.0));
    let (start, _) = ends(&s, "f");
    assert!((start.0 - 50.0).abs() < 0.05 && start.1 > 31.0 && start.1 < 36.0, "{start:?}");
}

#[test]
fn edge_follows_the_referenced_nodes_transform() {
    let s = scene(
        "@target t.png 100x100\ngroup g at:10,10\n  circle 20 at:40,40 rot:90 name:c\n  edge @c to:.25 name:e\n",
    );
    let (start, end) = ends(&s, "e");
    near(start, (40.0, 60.0));
    near(end, (20.0, 40.0));
}

#[test]
fn edge_wraps_through_the_start_and_from_equal_to_is_empty() {
    let s = scene(
        "@target t.png 100x100\ncircle 20 at:50,50 name:c\nedge @c from:.9 to:.1 name:wrap\nedge @c name:all\nedge @c from:.4 to:.4 name:none\nedge @c from:.2 to:.6 name:mid\n",
    );
    let whole = contour_len(&s, "c");
    let len = |p: &str| contour_len(&s, p) / whole;
    assert!((len("wrap") - 0.2).abs() < 1e-3, "{}", len("wrap"));
    assert!((len("all") - 1.0).abs() < 1e-3);
    assert!((len("mid") - 0.4).abs() < 1e-3);
    assert_eq!(contour_count(&s, "wrap"), 1);
    assert_eq!(contour_count(&s, "none"), 0);
    let angle = 0.9 * std::f64::consts::TAU;
    let start = ends(&s, "wrap").0;
    let want = (50.0 + 20.0 * angle.cos(), 50.0 + 20.0 * angle.sin());
    assert!(
        (f64::from(start.0) - want.0).hypot(f64::from(start.1) - want.1) < 0.1,
        "{start:?} != {want:?}"
    );
}

#[test]
fn edge_of_a_path_spans_its_contours_in_order() {
    let s = scene(
        "@target t.png 100x100\npath \"M0 0 L10 0 M20 0 L30 0\" name:p\nedge @p from:.25 to:.75 name:e\n",
    );
    assert_eq!(contour_count(&s, "e"), 2);
    assert!((contour_len(&s, "e") - 10.0).abs() < 1e-3);
    let pts = shape(&s, "e").geometry.points().to_vec();
    assert_eq!((pts[0].x, pts[pts.len() - 1].x), (5.0, 25.0));
}

#[test]
fn edge_of_a_jittered_shape_lies_on_the_jittered_contour() {
    let s = scene("@target t.png 100x100\ncircle 20 at:50,50 jitter:3,30 seed:1 name:c\nedge @c name:e\n");
    let world = node(&s, "c").transform;
    let drawn = crate::geom::sample(&shape(&s, "c").geometry.make_transform(&world), 0.25);
    let max_off = |path: &artscript_ir::Path| {
        crate::geom::sample(path, 1.0)
            .iter()
            .flat_map(|c| c.pos.iter())
            .map(|p| {
                drawn
                    .iter()
                    .flat_map(|c| c.pos.iter())
                    .map(|q| (p.0 - q.0).hypot(p.1 - q.1))
                    .fold(f64::MAX, f64::min)
            })
            .fold(0.0, f64::max)
    };
    assert!(max_off(&shape(&s, "e").geometry) < 0.3);
    let circle = crate::geom::ellipse(20.0, 20.0).make_transform(&world);
    assert!(max_off(&circle) > 0.5, "the jitter moved the contour");
}

#[test]
fn edge_takes_stroke_keys_like_an_open_curve() {
    let s = scene(
        "@target t.png 100x100\ncircle 20 at:50,50 name:c\nedge @c to:.5 stroke:#000000,[1 6 1] taper:.2 name:e\nedge @c to:.5 name:plain\n",
    );
    assert!(shape(&s, "e").stroke.as_ref().unwrap().ink.is_some());
    let plain = shape(&s, "plain");
    assert_eq!(plain.stroke.as_ref().unwrap().width, 2.0);
    assert!(plain.fill.is_none());
}

#[test]
fn edge_errors_name_what_it_needs() {
    let cases = [
        ("edge", "`edge` needs a node reference"),
        ("edge 4", "`edge` needs a node reference"),
        ("edge @g", "`edge` reads one shape's contour"),
        ("edge @c from:-.1", "`from:` is a fraction from 0 to 1, got -0.1"),
        ("edge @c to:1.5", "`to:` is a fraction from 0 to 1, got 1.5"),
        ("edge @c to:\"a\"", "`to:` is a fraction from 0 to 1, got a string"),
    ];
    for (line, message) in cases {
        let ev = run_with(
            &format!("@target t.png 40x40\ncircle 4 name:c\ngroup g\n  circle 4\n{line}\n"),
            &[],
        );
        let d = diag(&ev, "E0320");
        assert_eq!(d.message, message, "{line}");
        assert_eq!(ev.diagnostics.len(), 1, "{line}: {}", show(&ev.diagnostics));
    }
    let ev = run_with("@target t.png 40x40\ncircle 4 name:c\nedge @c to:2\n", &[]);
    let d = diag(&ev, "E0320");
    assert_eq!((d.span.line, d.span.col), (3, 9), "{d}");
    let ev = run_with("@target t.png 40x40\nedge @nope\n", &[]);
    assert_eq!(ev.diagnostics.len(), 1, "{}", show(&ev.diagnostics));
}

const ARM_LIB: &str = "\
def hero_arm len:20 tone:#3366cc
  rect len,6 fill:tone name:bone
  circle 4 at:len/2,0 fill:#ffffff name:hand
  circle rand(3 9) at:0,0 fill:#000000 name:dot
  anchor tip len/2+4,0
  slot glove at:len/2,0

def arm_l = hero_arm tone:#ff0000
";

#[test]
fn an_alias_instance_has_the_targets_anchors_parts_and_slots() {
    let ev = run_with(
        "@target t.png 100x100\nuse arm\narm_l at:50,50 name:a\n  glove:\n    circle 2 fill:#00ff00\n  .hand fill:#0000ff\ncircle 2 at:@a.tip name:m\n",
        &[("lib/arm.art", ARM_LIB)],
    );
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
    let s = ev.scene.unwrap();
    assert_eq!(fill_hex(node(&s, "a.bone")), "#ff0000");
    assert_eq!(fill_hex(node(&s, "a.hand")), "#0000ff");
    assert!(paths(&s).iter().any(|p| p == "a.glove.circle~1"), "{:?}", paths(&s));
    near(at(node(&s, "m")), (64.0, 50.0));
}

#[test]
fn an_alias_call_fills_the_open_parameters_and_cannot_pass_a_fixed_one() {
    let s = scene_with(
        "@target t.png 100x100\nuse arm\narm_l 30 at:50,50 name:a\narm_l len:40 at:50,50 name:b\n",
        &[("lib/arm.art", ARM_LIB)],
    );
    near(at(node(&s, "a.hand")), (15.0, 0.0));
    near(at(node(&s, "b.hand")), (20.0, 0.0));

    let ev = run_with(
        "@target t.png 100x100\nuse arm\narm_l tone:#000000 at:50,50 name:a\n",
        &[("lib/arm.art", ARM_LIB)],
    );
    let d = diag(&ev, "E0321");
    assert_eq!(d.message, "`arm_l` fixes `tone`");
    assert_eq!((d.file.as_str(), d.span.line), ("art/t.art", 3));
    assert!(d.fix.as_deref().unwrap_or_default().contains("tone:"), "{d}");

    let ev = run_with(
        "@target t.png 100x100\nuse arm\narm_l 30 50 at:50,50 name:a\n",
        &[("lib/arm.art", ARM_LIB)],
    );
    assert_eq!(diag(&ev, "E0321").message, "`arm_l` takes 1 positional argument(s)");
}

#[test]
fn an_alias_instance_draws_the_same_as_the_direct_call() {
    let via_alias = scene_with(
        "@target t.png 100x100\nuse arm\narm_l at:50,50 name:a\n",
        &[("lib/arm.art", ARM_LIB)],
    );
    let direct = scene_with(
        "@target t.png 100x100\nuse arm\nhero_arm tone:#ff0000 at:50,50 name:a\n",
        &[("lib/arm.art", ARM_LIB)],
    );
    assert_eq!(paths(&via_alias), paths(&direct));
    let dot = |s: &Scene| node(s, "a.dot").bounds().unwrap().width();
    assert_eq!(dot(&via_alias), dot(&direct));
}

#[test]
fn an_unnamed_alias_instance_keys_and_seeds_as_the_direct_call() {
    let via_alias = scene_with(
        "@target t.png 100x100\ndef pebble r:4 tone:#3366cc\n  circle r+rand(0 9) fill:tone\ndef red = pebble tone:#ff0000\nred at:50,50\npebble at:20,20\n",
        &[],
    );
    let direct = scene_with(
        "@target t.png 100x100\ndef pebble r:4 tone:#3366cc\n  circle r+rand(0 9) fill:tone\npebble tone:#ff0000 at:50,50\npebble at:20,20\n",
        &[],
    );
    let widths = |s: &Scene| -> Vec<(String, f32)> {
        paths(s)
            .into_iter()
            .map(|p| (p.clone(), node(s, &p).bounds().unwrap().width()))
            .collect()
    };
    assert_eq!(widths(&via_alias).len(), 4, "{:?}", paths(&via_alias));
    assert_eq!(widths(&via_alias), widths(&direct));
}

#[test]
fn a_library_let_sees_an_alias_declared_above_it() {
    let lib = (
        "lib/rig.art",
        "use arm\ndef arm_r = hero_arm tone:#00ff00\nlet arm = arm_r\ndef rig\n  arm name:near\n",
    );
    let s = scene_with(
        "@target t.png 100x100\nuse rig\nrig at:50,50 name:r\n",
        &[lib, ("lib/arm.art", ARM_LIB)],
    );
    assert_eq!(fill_hex(node(&s, "r.near.bone")), "#00ff00");
    near(at(node(&s, "r.near.hand")), (10.0, 0.0));
}

#[test]
fn an_alias_chains_through_aliases_and_imports_and_may_precede_its_target() {
    let big = ("lib/big.art", "use arm\ndef arm_big = arm_l len:40\n");
    let s = scene_with(
        "@target t.png 100x100\nuse big\nuse arm { hero_arm }\ndef mine = hero_arm tone:#00ff00\narm_big at:50,50 name:a\nmine at:50,50 name:b\ndef early = late\nlate at:10,10 name:c\ndef late\n  rect 4x4 name:box\n",
        &[big, ("lib/arm.art", ARM_LIB)],
    );
    assert_eq!(fill_hex(node(&s, "a.bone")), "#ff0000");
    near(at(node(&s, "a.hand")), (20.0, 0.0));
    assert_eq!(fill_hex(node(&s, "b.bone")), "#00ff00");
    assert!(paths(&s).iter().any(|p| p == "c.box"), "{:?}", paths(&s));

    let ev = run_with(
        "@target t.png 100x100\nuse big\narm_big len:3\n",
        &[big, ("lib/arm.art", ARM_LIB)],
    );
    assert_eq!(diag(&ev, "E0321").message, "`arm_big` fixes `len`");
}

#[test]
fn an_alias_reports_its_own_mistakes_at_the_alias_line() {
    let ev = run_with(
        "@target t.png 100x100\nuse arm\n\ndef bad = hero_arm wat:1\ndef twice = arm_l tone:#000000\n",
        &[("lib/arm.art", ARM_LIB)],
    );
    let ds: Vec<&Diagnostic> = ev.diagnostics.iter().filter(|d| d.code == "E0321").collect();
    assert_eq!(ds.len(), 2, "{}", show(&ev.diagnostics));
    assert_eq!(ds[0].message, "`hero_arm` has no parameter `wat`");
    assert_eq!(ds[0].span.line, 4);
    assert_eq!(ds[1].message, "`arm_l` fixes `tone`");
    assert_eq!(ds[1].span.line, 5);

    let ev = run_with(
        "@target t.png 100x100\ndef a = b\ndef b = a\npalette q\n  color ink #000000\ndef c = q\ndef d = nothing\n",
        &[],
    );
    let mut ds: Vec<&Diagnostic> = ev.diagnostics.iter().filter(|d| d.code == "E0323").collect();
    ds.sort_by_key(|d| d.span.line);
    assert_eq!(ds.len(), 2, "{}", show(&ev.diagnostics));
    assert_eq!(ds[0].message, "`a` is an alias of itself");
    assert_eq!(ds[0].notes, ["through a -> b -> a"]);
    assert_eq!(ds[0].span.line, 2);
    assert_eq!(ds[1].message, "`q` is a palette, not a def");
    assert_eq!(ds[1].span.line, 6);
    assert_eq!(diag(&ev, "E0301").message, "unknown def `nothing`");

    let ev = run_with(
        "@target t.png 100x100\ndef early = late_alias\ndef late_alias = nothing\nearly at:1,1 name:e\n",
        &[],
    );
    assert_eq!(ev.diagnostics.len(), 1, "{}", show(&ev.diagnostics));
    assert_eq!(diag(&ev, "E0301").span.line, 3);
}

#[test]
fn an_alias_fixed_argument_follows_the_instance_palette_like_a_default() {
    let s = palette_scene(
        "@target t.png 100x100\nuse d { eye }\ndef sky_eye = eye iris:p.sky\nsky_eye name:plain\nsky_eye palette:elite name:swapped\n",
    );
    assert_eq!(fill_hex(node(&s, "plain.i")), "#111111");
    assert_eq!(fill_hex(node(&s, "swapped.i")), "#222222");
}

const HALF_LIB: &str = "fn half(x) = x/2\nfn quarter(x) = half(half(x))\n";

#[test]
fn a_fn_computes_a_value_from_its_arguments_and_other_fns() {
    let s = scene(
        "@target t.png 100x100\nfn half(x) = x/2\nfn quarter(x) = half(half(x))\ncircle 4 at:half(40),quarter(80) name:c\ndef dot k:1\n  circle 2 at:half(k),0 name:in\ndot k:12 name:d\n",
    );
    near(at(node(&s, "c")), (20.0, 20.0));
    near(at(node(&s, "d.in")), (6.0, 0.0));
}

#[test]
fn a_fn_reads_its_files_top_level_lets_and_palettes() {
    let s = scene(
        "@target t.png 100x100\nlet base = 10\npalette q\n  color ink #112233\n  color hot #ff0000\nfn tone(k) = if k > base then q.hot else q.ink\nrect 4x4 fill:tone(5) name:cold\nrect 4x4 fill:tone(50) name:warm\n",
    );
    assert_eq!(fill_hex(node(&s, "cold")), "#112233");
    assert_eq!(fill_hex(node(&s, "warm")), "#ff0000");
}

#[test]
fn p_in_a_fn_body_is_the_palette_of_the_call_site() {
    let s = palette_scene(
        "@target t.png 100x100\nuse d { eye }\nfn tint() = p.sky\ndef dot\n  circle 5 fill:tint() name:i\ndot name:plain\ndot palette:elite name:swapped\n",
    );
    assert_eq!(fill_hex(node(&s, "plain.i")), "#111111");
    assert_eq!(fill_hex(node(&s, "swapped.i")), "#222222");
}

#[test]
fn use_imports_a_fn_by_item_list_and_through_an_alias() {
    let libs = [("lib/m.art", HALF_LIB)];
    let ev = run_with("@target t.png 100x100\nuse m { quarter }\ncircle 4 at:quarter(80),0 name:c\n", &libs);
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
    near(at(node(ev.scene.as_ref().unwrap(), "c")), (20.0, 0.0));

    let ev = run_with("@target t.png 100x100\nuse m as lib\nlet q = lib.quarter\ncircle 4 at:q(80),0 name:c\n", &libs);
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
    near(at(node(ev.scene.as_ref().unwrap(), "c")), (20.0, 0.0));
}

#[test]
fn a_fn_call_with_the_wrong_arguments_is_e0320() {
    let ev = run_with("@target t.png 100x100\nfn half(x) = x/2\ncircle 4 at:half(1 2),0\n", &[]);
    let d = diag(&ev, "E0320");
    assert_eq!(d.message, "`half` takes 1 argument, got 2");
    assert_eq!(d.fix.as_deref(), Some("write `half(x)`"));
    assert_eq!(d.span.line, 3);

    let ev = run_with("@target t.png 100x100\nfn half(x) = x/2\ncircle 4 at:half(x:2),0\n", &[]);
    let d = diag(&ev, "E0320");
    assert_eq!(d.message, "`half` takes positional arguments only, got `x:`");
    assert_eq!(d.fix.as_deref(), Some("write `half(x)`"));
}

#[test]
fn a_fn_body_cannot_read_a_ref_or_draw_a_random_value() {
    let ev = run_with(
        "@target t.png 100x100\ncircle 4 name:a\nfn w(k) = @a.w * k\nfn jit(k) = k + rand(0 1)\nfn one(xs) = pick(xs)\ncircle jit(4)\n",
        &[],
    );
    assert_eq!(ev.diagnostics.len(), 3, "{}", show(&ev.diagnostics));
    let mut ds: Vec<&Diagnostic> = ev.diagnostics.iter().filter(|d| d.code == "E0324").collect();
    ds.sort_by_key(|d| d.span.line);
    assert_eq!(ds.len(), 3, "{}", show(&ev.diagnostics));
    assert_eq!(ds[0].message, "fn `w` reads `@a.w`");
    assert_eq!(ds[0].span.line, 3);
    assert_eq!(ds[1].message, "fn `jit` calls `rand`");
    assert_eq!(ds[1].fix.as_deref(), Some("pass the random value in as an argument"));
    assert_eq!(ds[1].span.line, 4);
    assert_eq!(ds[2].message, "fn `one` calls `pick`");
    assert_eq!(ds[2].fix.as_deref(), Some("pass the random value in as an argument"));
}

#[test]
fn a_fn_reached_through_a_parameter_or_a_let_cannot_call_itself() {
    for src in [
        "@target t.png 100x100\nfn f(g) = g(g)\ncircle f(f)\n",
        "@target t.png 100x100\nfn f(x) = g(x)\nlet g = f\ncircle f(1)\n",
    ] {
        let ev = run_with(src, &[]);
        assert_eq!(ev.diagnostics.len(), 1, "{}", show(&ev.diagnostics));
        let d = diag(&ev, "E0325");
        assert_eq!(d.message, "fn `f` calls itself");
    }
}

#[test]
fn a_fn_cannot_call_itself_directly_or_through_another_fn() {
    let ev = run_with(
        "@target t.png 100x100\nfn down(n) = if n > 0 then down(n - 1) else 0\nfn ping(n) = pong(n)\nfn pong(n) = ping(n)\nfn fine(n) = n\ncircle down(3) at:ping(1),fine(2) name:c\n",
        &[],
    );
    let mut ds: Vec<&Diagnostic> = ev.diagnostics.iter().filter(|d| d.code == "E0325").collect();
    ds.sort_by_key(|d| d.span.line);
    assert_eq!(ds.len(), 3, "{}", show(&ev.diagnostics));
    assert_eq!(ds[0].message, "fn `down` calls itself");
    assert_eq!(ds[0].notes, ["through down -> down"]);
    assert_eq!(ds[0].span.line, 2);
    assert_eq!(ds[1].message, "fn `ping` calls itself");
    assert_eq!(ds[1].notes, ["through ping -> pong -> ping"]);
    assert_eq!(ds[2].message, "fn `pong` calls itself");
    assert_eq!(ds[2].span.line, 4);
    assert!(!ev.diagnostics.iter().any(|d| d.code == "E0301"), "{}", show(&ev.diagnostics));

    let ev = run_with("@target t.png 100x100\nfn down(down) = down - 1\ncircle 4 at:down(5),0 name:c\n", &[]);
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
    near(at(node(ev.scene.as_ref().unwrap(), "c")), (4.0, 0.0));
}

#[test]
fn a_random_value_passed_to_a_fn_is_seeded_at_the_draw_site() {
    let src = "@target t.png 100x100\nfn bump(x r) = x + r\nrepeat 3\n  circle 2 at:bump(10 rand(0 1)),i*10 name:c\n";
    let first = scene(src);
    let second = scene(src);
    let xs: Vec<f32> = (0..3).map(|i| at(node(&first, &format!("c.{i}"))).0).collect();
    let again: Vec<f32> = (0..3).map(|i| at(node(&second, &format!("c.{i}"))).0).collect();
    assert_eq!(xs, again);
    assert!(xs.iter().all(|x| (10.0..11.0).contains(x)), "{xs:?}");
    assert!(xs[0] != xs[1] || xs[1] != xs[2], "{xs:?}");
}

const TILE: &str = "@target t.png 16x16\ndef tile kind:one_of(floor_top fill fill_b)\n  if kind == fill\n    circle 2 name:filled\n  if kind == floor_top\n    circle 3 name:floor\n";

fn errors(ev: &Evaluation) -> Vec<&Diagnostic> {
    ev.diagnostics.iter().filter(|d| d.code == "E0320").collect()
}

#[test]
fn a_word_outside_one_of_is_an_error_at_the_call_naming_the_allowed_words() {
    for call in ["tile kind:flor_top", "tile flor_top"] {
        let ev = run_with(&format!("{TILE}{call}\n"), &[]);
        let es = errors(&ev);
        assert_eq!(es.len(), 1, "{call}: {}", show(&ev.diagnostics));
        assert_eq!(es[0].span.line, 7, "{call}");
        assert!(
            es[0].message.contains("floor_top, fill, fill_b") && es[0].message.contains("flor_top"),
            "{}",
            es[0].message
        );
        assert!(es[0].fix.is_some());
    }
}

#[test]
fn a_number_for_a_one_of_parameter_is_an_error() {
    let ev = run_with(&format!("{TILE}tile kind:3\n"), &[]);
    assert_eq!(errors(&ev).len(), 1, "{}", show(&ev.diagnostics));
}

#[test]
fn an_alias_fixed_word_outside_one_of_is_an_error() {
    let ev = run_with(&format!("{TILE}def wall = tile kind:flor_top\nwall\n"), &[]);
    assert_eq!(errors(&ev).len(), 1, "{}", show(&ev.diagnostics));
}

#[test]
fn a_word_inside_one_of_is_accepted_and_the_first_word_is_the_default() {
    let s = scene(&format!("{TILE}tile name:a\ntile kind:fill name:b\ntile fill_b name:c\n"));
    let all = paths(&s);
    assert!(all.iter().any(|p| p == "a.floor"), "{all:?}");
    assert!(all.iter().any(|p| p == "b.filled"), "{all:?}");
    assert!(!all.iter().any(|p| p.starts_with("c.")), "{all:?}");
}

#[test]
fn a_plain_word_default_still_accepts_any_word() {
    let ev = run_with(
        "@target t.png 16x16\ndef tile kind:floor_top\n  circle 2\ntile kind:flor_top\n",
        &[],
    );
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
}

#[test]
fn a_malformed_one_of_is_one_error_at_the_def() {
    for decl in ["one_of()", "one_of(1 2)", "one_of(a b c:1)"] {
        let ev = run_with(
            &format!("@target t.png 16x16\ndef tile kind:{decl}\n  circle 2\ntile kind:a\ntile kind:b\n"),
            &[],
        );
        let es = errors(&ev);
        assert_eq!(es.len(), 1, "{decl}: {}", show(&ev.diagnostics));
        assert!(es[0].message.contains("one_of"), "{}", es[0].message);
        assert_eq!(es[0].span.line, 2, "{decl}: {}", show(&ev.diagnostics));
    }
}

#[test]
fn a_malformed_one_of_in_an_uncalled_library_def_is_reported_in_the_library() {
    let lib = ("lib/tiles.art", "def tile kind:one_of(1 2)\n  circle 2\n");
    let ev = run_with("@target t.png 16x16\nuse tiles\ncircle 2\n", &[lib]);
    let es = errors(&ev);
    assert_eq!(es.len(), 1, "{}", show(&ev.diagnostics));
    assert!(es[0].file.ends_with("tiles.art"), "{}", show(&ev.diagnostics));
    assert_eq!(es[0].span.line, 1, "{}", show(&ev.diagnostics));
}

#[test]
fn one_of_outside_a_parameter_default_is_an_unknown_function() {
    let ev = run_with("@target t.png 16x16\nlet k = one_of(a b)\ncircle 2\n", &[]);
    assert!(
        ev.diagnostics.iter().any(|d| d.message.contains("one_of")),
        "{}",
        show(&ev.diagnostics)
    );
}

fn unknown_words(src: &str, libs: &[(&str, &str)]) -> Vec<Diagnostic> {
    run_with(src, libs)
        .diagnostics
        .into_iter()
        .filter(|d| d.code == "L0107")
        .collect()
}

#[test]
fn comparing_a_one_of_parameter_with_another_word_warns_and_names_the_words() {
    let hits = unknown_words(
        &format!("{TILE}  if flor == kind\n    circle 1\n  if kind != flor\n    circle 1\n"),
        &[],
    );
    let lines: Vec<u32> = hits.iter().map(|d| d.span.line).collect();
    assert_eq!(lines, [7, 9], "{}", show(&hits));
    let d = &hits[0];
    assert_eq!(d.severity, artscript_syntax::Severity::Warning);
    assert_eq!(d.message, "`kind` is never `flor`");
    assert_eq!(d.notes, ["`kind` is one of floor_top, fill, fill_b"]);
    assert!(d.fix.is_some());
}

#[test]
fn comparing_a_one_of_parameter_with_its_words_or_other_names_is_quiet() {
    let hits = unknown_words(
        &format!(
            "{TILE}  let other = fill\n  if kind == other\n    circle 1\n  if kind == fill_b\n    circle 1\n"
        ),
        &[],
    );
    assert!(hits.is_empty(), "{}", show(&hits));
    let rebound = unknown_words(
        "@target t.png 16x16\ndef tile kind:one_of(a b)\n  let kind = c\n  if kind == c\n    circle 1\n",
        &[],
    );
    assert!(rebound.is_empty(), "{}", show(&rebound));
}

#[test]
fn unknown_word_off_is_silent() {
    let manifest = "[project]\nout = \"out\"\n\n[lint]\nunknown-word = \"off\"\n";
    let p = Project::from_sources(
        manifest,
        [(
            "art/t.art".to_string(),
            format!("{TILE}  if kind == flor\n    circle 1\n"),
        )]
        .into(),
    )
    .unwrap();
    let plan = plan(&p);
    assert!(plan.diagnostics.is_empty(), "{:?}", plan.diagnostics);
    let ev = evaluate(&p, &plan.jobs[0]);
    assert!(ev.diagnostics.iter().all(|d| d.code != "L0107"), "{}", show(&ev.diagnostics));
}

mod brush;
mod stamp;

#[test]
fn edge_to_the_end_of_an_open_curve_is_one_piece() {
    for k in 0..20 {
        let mut src = format!("@target t.png 100x100\ncurve 0,0 30,40 70,10 {},90 name:c\n", 60 + k);
        for i in 1..100 {
            src.push_str(&format!("edge @c from:.{i:02} name:e{i}\n"));
        }
        let s = scene(&src);
        for i in 1..100 {
            let moves = skia_safe::path::Iter::new(&shape(&s, &format!("e{i}")).geometry, false)
                .filter(|(verb, _)| *verb == skia_safe::path::Verb::Move)
                .count();
            assert_eq!(moves, 1, "curve {k}, from:.{i:02}");
        }
    }
}

#[test]
fn fill_stroke_and_blur_on_an_instance_point_at_palette_and_fx_blur() {
    let lib = [("lib/arm.art", ARM_LIB)];
    for (line, key) in [
        ("hero_arm fill:#ff0000", "fill"),
        ("hero_arm stroke:#000000,2", "stroke"),
        ("arm_l fill:#ff0000", "fill"),
        ("def red = hero_arm fill:#ff0000", "fill"),
    ] {
        let ev = run_with(&format!("@target t.png 100x100\nuse arm\n{line}\n"), &lib);
        let d = diag(&ev, "E0321");
        assert!(d.message.ends_with(&format!("has no parameter `{key}`")), "{line}: {d}");
        assert_eq!(
            d.fix.as_deref(),
            Some("recolor the instance with `palette:`, or give `hero_arm` a color parameter"),
            "{line}"
        );
    }

    for line in ["hero_arm blur:2", "arm_l blur:2", "def soft = hero_arm blur:2"] {
        let ev = run_with(&format!("@target t.png 100x100\nuse arm\n{line}\n"), &lib);
        let d = diag(&ev, "E0321");
        assert_eq!(
            d.fix.as_deref(),
            Some("blur the instance with an `fx blur N` line under it"),
            "{line}"
        );
    }

    let ev = run_with(
        "@target t.png 100x100\ndef dot fill:#ff0000 blur:0\n  circle 4 fill:fill blur:blur\ndot fill:#00ff00 blur:1\n",
        &[],
    );
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
}
