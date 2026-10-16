//! Evaluator behaviour tests over in-memory projects.

use artscript_ir::{Node, NodeKind, Paint, Scene};
use artscript_syntax::Diagnostic;

use crate::{Evaluation, Project, evaluate, plan};

const MANIFEST: &str = "[project]\nout = \"out\"\nlib = [\"lib\"]\n\n[lint]\nraw-color = \"off\"\nunused = \"off\"\noff-canvas = \"off\"\n";

fn project(files: &[(&str, &str)]) -> Project {
    let files = files.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
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
    ds.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n")
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
    all.into_iter().find(|n| n.path == path).unwrap_or_else(|| panic!("no node {path} in {:?}", paths(s)))
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
    assert_eq!(paths(&s), ["bg", "bg.circle", "bg.circle2", "bg.box", "bg.circle#0", "bg.circle#1", "bg.circle#2"]);
    assert_eq!(at(node(&s, "bg.circle#2")), (20.0, 0.0));
}

#[test]
fn shape_name_is_registered_before_its_children_so_copy_can_reach_it() {
    let s = scene("@target t.png 100x100\ncircle 20 at:50,50 name:white\n  subtract\n    copy @white\n    copy @white at:10,0\n");
    let sub = node(&s, "white.subtract");
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
        "@target t.png 100x100\nuse face\nface at:50,50 name:f\n  .a rot:-20\n  .mouth off\n  .c.white.pupil fill:#ff0000\n  .a iris:#00ff00\n",
        &[lib, ("lib/eye.art", EYE_LIB)],
    );
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
    let s = ev.scene.unwrap();
    let a = node(&s, "f.a");
    let (sin, cos) = (a.transform.skew_y(), a.transform.scale_x());
    assert!((sin.atan2(cos).to_degrees() + 20.0).abs() < 1e-3);
    assert!(!paths(&s).iter().any(|p| p == "f.mouth"));
    assert_eq!(fill_hex(node(&s, "f.c.white.pupil")), "#ff0000");
    assert_eq!(fill_hex(node(&s, "f.a.white.pupil")), "#00ff00");
    assert_eq!(fill_hex(node(&s, "f.c.white")), "#ffffff");

    let head = ("lib/head.art", "use face\ndef head\n  face name:f\n");
    let ev = run_with("@target t.png 100x100\nuse head\nhead\n  .f.c iris:#0000ff\n", &[head, lib, ("lib/eye.art", EYE_LIB)]);
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
    let s = ev.scene.unwrap();
    assert_eq!(fill_hex(node(&s, "head.f.c.white.pupil")), "#0000ff");
    assert_eq!(fill_hex(node(&s, "head.f.a.white.pupil")), "#3366cc");
}

#[test]
fn unmatched_patch_is_e0340_listing_parts() {
    let ev = run_with("@target t.png 100x100\nuse eye\neye\n  .iris off\n", &[("lib/eye.art", EYE_LIB)]);
    let d = ev.diagnostics.iter().find(|d| d.code == "E0340").unwrap_or_else(|| panic!("{}", show(&ev.diagnostics)));
    assert!(d.notes.iter().any(|n| n.contains("white")), "{d}");
}

#[test]
fn z_behind_moves_a_node_first() {
    let s = scene("@target t.png 100x100\nlayer l\n  circle 5 name:a\n  circle 5 name:b\n  circle 5 name:c z:behind\n");
    assert_eq!(paths(&s), ["l", "l.c", "l.a", "l.b"]);
}

#[test]
fn slot_content_resolves_host_parts_and_needs_checks_them() {
    let ok = run_with("@target t.png 100x100\nuse eye\neye 10 at:50,50\n  lashes:\n    lash\n", &[("lib/eye.art", EYE_LIB)]);
    assert!(ok.diagnostics.is_empty(), "{}", show(&ok.diagnostics));
    assert!(paths(ok.scene.as_ref().unwrap()).iter().any(|p| p == "eye.lashes.lash"), "{:?}", paths(ok.scene.as_ref().unwrap()));

    let lib = "def blank\n  circle 4 name:ball\n  slot top\n\ndef lash needs:@white\n  curve 0,0 4,-4\n";
    let ev = run_with("@target t.png 100x100\nuse b\nblank\n  top:\n    lash\n", &[("lib/b.art", lib)]);
    let d = ev.diagnostics.iter().find(|d| d.code == "E0311").unwrap_or_else(|| panic!("{}", show(&ev.diagnostics)));
    assert_eq!(d.message, "lash needs @white, but the host `blank` has no part named white");
    assert_eq!(d.notes, ["parts available: ball"]);
}

#[test]
fn mirror_draws_a_flipped_copy_with_m_suffix() {
    let s = scene("@target t.png 100x100\ngroup g at:50,50\n  mirror x\n    circle 4 at:10,0 name:ear\n");
    let all = paths(&s);
    assert!(all.contains(&"g.mirror.ear".to_string()) && all.contains(&"g.mirror.ear_m".to_string()), "{all:?}");
    let m = node(&s, "g.mirror.ear_m");
    let b = m.transform.map_rect(m.bounds().map(|r| m.transform.invert().unwrap().map_rect(r).0).unwrap()).0;
    assert_eq!(b.center_x().round(), -10.0);
}

#[test]
fn repeat_and_grid_make_indexed_copies() {
    let s = scene("@target t.png 100x100\ngroup g at:50,50\n  repeat 4 rot:90\n    rect 2x10 at:0,-20 name:petal\n");
    let petals: Vec<String> = paths(&s).into_iter().filter(|p| p.contains("petal")).collect();
    assert_eq!(petals, ["g.repeat.petal#0", "g.repeat.petal#1", "g.repeat.petal#2", "g.repeat.petal#3"]);

    let s = scene("@target t.png 100x100\ngroup g\n  grid 3x2 gap:2\n    rect 10x10 name:tile\n");
    let tiles: Vec<&Node> = {
        let mut all = Vec::new();
        walk(&s.children, &mut all);
        all.into_iter().filter(|n| n.path.contains("tile")).collect()
    };
    assert_eq!(tiles.len(), 6);
    let xs: Vec<f32> = tiles.iter().map(|n| at(n).0).collect();
    assert_eq!(xs, [38.0, 50.0, 62.0, 38.0, 50.0, 62.0]);
    let ys: Vec<f32> = tiles.iter().map(|n| at(n).1).collect();
    assert_eq!(ys, [44.0, 44.0, 44.0, 56.0, 56.0, 56.0]);
}

#[test]
fn level_failure_is_c0101_with_measurements_and_a_fix() {
    let ev = run_with(
        "@target t.png 100x100\ncircle 5 at:30,40 name:eye_l\ncircle 5 at:70,43 name:eye_r\nassert level(@eye_l @eye_r) tol:.5\n",
        &[],
    );
    assert_eq!(ev.diagnostics.len(), 1, "{}", show(&ev.diagnostics));
    let d = &ev.diagnostics[0];
    assert_eq!(d.to_string(), "error[C0101] art/t.art:4:1\n  assert level(@eye_l @eye_r) tol:.5\n  eye_l.cy = 40   eye_r.cy = 43   difference 3 > tol 0.5\n  fix: set .eye_r at:70,40");
}

#[test]
fn expect_failure_is_warning_c0201() {
    let ev = run_with(
        "@target t.png 100x100\nrect 10x10 name:head\nrect 40x10 name:body\nexpect @head.w / @body.w in .9..1.4 \"head as wide as the body\"\n",
        &[],
    );
    let d = &ev.diagnostics[0];
    assert_eq!((d.code.as_str(), d.severity), ("C0201", artscript_syntax::Severity::Warning));
    assert_eq!(d.notes, ["head as wide as the body", "head.w = 10", "body.w = 40", "0.25 is outside 0.9..1.4"]);
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
    assert_eq!(ev.diagnostics.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(), ["E0201"], "{}", show(&ev.diagnostics));
}

#[test]
fn drawing_in_a_library_is_e0204() {
    let ev = run_with("@target t.png 10x10\nuse bad\n", &[("lib/bad.art", "circle 4\n")]);
    let d = ev.diagnostics.iter().find(|d| d.code == "E0204").unwrap_or_else(|| panic!("{}", show(&ev.diagnostics)));
    assert_eq!(d.file, "lib/bad.art");
}

#[test]
fn param_kind_mismatch_is_e0320() {
    let ev = run_with("@target t.png 10x10\nuse eye\neye size:#ff0000\n", &[("lib/eye.art", EYE_LIB)]);
    let d = ev.diagnostics.iter().find(|d| d.code == "E0320").unwrap_or_else(|| panic!("{}", show(&ev.diagnostics)));
    assert!(d.fix.as_deref().unwrap_or_default().contains("number"), "{d}");
}

#[test]
fn raw_color_lint_skips_palettes() {
    let p = Project::from_sources(
        "[project]\nout = \"out\"\nlib = [\"lib\"]\n",
        [("art/t.art".to_string(), "@target t.png 10x10\npalette x\n  a #ff0000\ncircle 3 fill:x.a\nrect 2x2 fill:#00ff00\n".to_string())].into(),
    )
    .unwrap();
    let plan = plan(&p);
    let ev = evaluate(&p, &plan.jobs[0]);
    let lints: Vec<(&str, u32)> = ev.diagnostics.iter().map(|d| (d.code.as_str(), d.span.line)).collect();
    assert_eq!(lints, [("L0101", 5)]);
}
