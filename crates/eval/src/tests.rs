//! Evaluator behaviour tests over in-memory projects.

use artscript_ir::{EffectOp, Node, NodeKind, Paint, Scene};
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
fn a_tight_comma_is_one_argument_and_a_real_tuple_stays_a_tuple() {
    let tight = run_with("@target t.png 40x40\ncircle 10 fill:shade(#cc6644,-2) name:tight\n", &[]);
    let d = tight
        .diagnostics
        .iter()
        .find(|d| d.code == "E0320")
        .unwrap_or_else(|| panic!("shade(c,-2) should be E0320, got:\n{}", show(&tight.diagnostics)));
    assert!(d.message.contains("color"), "{d}");

    let level = run_with(
        "@target t.png 40x40\ncircle 5 at:30,40 name:eye_l\ncircle 5 at:70,43 name:eye_r\nassert level(@eye_l,@eye_r) tol:.5\n",
        &[],
    );
    assert!(level.diagnostics.iter().any(|d| d.code == "E0320"), "{}", show(&level.diagnostics));
    assert!(level.diagnostics.iter().all(|d| d.code != "C0101"), "tight comma must not compare two nodes\n{}", show(&level.diagnostics));

    let s = scene("@target t.png 40x40\ncircle 10 fill:shade(#cc6644 -2) name:spaced\ncircle 8 at:14,0 stroke:#112233,2 name:point\ncircle 4 at:min(1 2 0),0 name:low\ncircle 4 at:min([1 2 0]),16 name:list\n");
    assert_ne!(fill_hex(node(&s, "spaced")), "#cc6644");
    let point = node(&s, "point");
    assert_eq!(at(point), (14.0, 0.0));
    let NodeKind::Shape(shape) = &point.kind else { panic!("point is not a shape") };
    let stroke = shape.stroke.as_ref().expect("stroke tuple");
    assert_eq!(stroke.width, 2.0);
    let Paint::Solid(color) = &stroke.paint else { panic!("stroke paint {:?}", shape.stroke) };
    assert_eq!(color.hex(), "#112233");
    assert_eq!(at(node(&s, "low")).0, 0.0);
    assert_eq!(at(node(&s, "list")).0, 0.0);

    let min_comma = run_with("@target t.png 40x40\ncircle 4 at:min(1,2,0),0 name:comma\n", &[]);
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
    assert!((f64::from(b.right) - 10.0).abs() < 1e-4, "right {}", b.right);
    assert!((f64::from(b.top) + 10.0).abs() < 1e-4, "top {}", b.top);
    assert!((f64::from(b.bottom) - 10.0).abs() < 1e-4, "bottom {}", b.bottom);
    let ink = shape(&s, "ink").geometry.to_svg();
    let lit = shape(&s, "lit").geometry.to_svg();
    assert_eq!(ink, lit, "a width list built with each differs from the same literal list");
    assert!(shape(&s, "ink").geometry.contains(artscript_ir::Point::new(50.0, 3.5)));
    assert!(!shape(&s, "ink").geometry.contains(artscript_ir::Point::new(50.0, 4.6)));
}

#[test]
fn each_walks_a_list_of_points() {
    let s = scene("@target t.png 80x80\nlet pts = [0,0 10,4 20,-3]\neach p in pts\n  circle 1 at:p\n");
    assert_eq!(at(node(&s, "circle#0")), (0.0, 0.0));
    assert_eq!(at(node(&s, "circle#1")), (10.0, 4.0));
    assert_eq!(at(node(&s, "circle#2")), (20.0, -3.0));
}

#[test]
fn each_expr_respects_e0306() {
    let bad = run_with("@target t.png 10x10\nlet xs = [each k in 4 -> k]\ncircle 1\n", &[]);
    let d = bad.diagnostics.iter().find(|d| d.code == "E0306").unwrap_or_else(|| panic!("{}", show(&bad.diagnostics)));
    assert!(d.message.contains("number"), "{d}");
    let huge = run_with("@target t.png 10x10\nlet xs = [each k in 0..100001 -> k]\ncircle 1\n", &[]);
    assert!(huge.diagnostics.iter().any(|d| d.code == "E0306" && d.message.contains("100000")), "{}", show(&huge.diagnostics));
    let mixed = run_with("@target t.png 10x10\ncurve [0,0 10,0] 20,0\n", &[]);
    assert!(mixed.diagnostics.iter().any(|d| d.code == "E0320"), "{}", show(&mixed.diagnostics));
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
    let ev = run_with("@target t.png 20x20\nlet p = (#ff0000)+(1,0)\ncircle 1 at:p\n", &[]);
    let d = ev.diagnostics.iter().find(|d| d.code == "E0320").unwrap_or_else(|| panic!("{}", show(&ev.diagnostics)));
    assert!(d.message.contains("color") && d.message.contains("vector"), "{d}");
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
    let NodeKind::Shape(band) = &node(&s, "band").kind else { panic!("band") };
    let Some(Paint::Linear { from, to, stops }) = &band.fill else { panic!("{:?}", band.fill) };
    assert_eq!((from.x, from.y, to.x, to.y), (10.0, 5.0, 70.0, -15.0));
    assert!(stops.iter().any(|st| (st.pos - 0.4).abs() < 1e-4), "stop at .4 missing: {stops:?}");
    let NodeKind::Shape(down) = &node(&s, "down").kind else { panic!("down") };
    let Some(Paint::Linear { from, to, .. }) = &down.fill else { panic!("{:?}", down.fill) };
    assert!((f64::from(from.x)).abs() < 1e-4 && (f64::from(from.y) + 20.0).abs() < 1e-4, "{from:?}");
    assert!((f64::from(to.x)).abs() < 1e-4 && (f64::from(to.y) - 20.0).abs() < 1e-4, "{to:?}");
    let NodeKind::Shape(angle) = &node(&s, "angle").kind else { panic!("angle") };
    let Some(Paint::Linear { from, .. }) = &angle.fill else { panic!("{:?}", angle.fill) };
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
    let has = |path: &str, x: f32, y: f32| shape(&s, path).geometry.contains(artscript_ir::Point::new(x, y));
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
    assert_ne!(shape(&a, "c").geometry.to_svg(), shape(&b, "c").geometry.to_svg());
    let bare = run_with("@target t.png 40x40\ncircle 20 jitter:2\n", &[]);
    let d = bare.diagnostics.iter().find(|d| d.code == "E0320").unwrap_or_else(|| panic!("{}", show(&bare.diagnostics)));
    assert_eq!(d.fix.as_deref(), Some("write `jitter:2,60`"));
}

#[test]
fn scaled_taper_matches_the_same_stroke_in_pixel_space() {
    let unit = png("@target t.png 200x120\ngroup scale:50 at:100,60\n  curve -1,0 0,-.5 1,0 stroke:#000,[.05 .1 .05] taper:.3\n");
    let pixels = png("@target t.png 200x120\ngroup at:100,60\n  curve -50,0 0,-25 50,0 stroke:#000,[2.5 5 2.5] taper:.3\n");
    let fine = png("@target t.png 200x120 scale:1\ngroup at:100,60\n  curve -40,0 0,-20 40,0 stroke:#000,[4 8 4] taper:.3\n");
    let doubled = png("@target t.png 100x60 scale:2\ngroup scale:.5 at:50,30\n  curve -40,0 0,-20 40,0 stroke:#000,[4 8 4] taper:.3\n");
    assert_pixels_close(&unit, &pixels, 0.15);
    assert_pixels_close(&fine, &doubled, 0.15);
}

fn png(src: &str) -> Vec<u8> {
    let ev = run_with(src, &[]);
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
    artscript_render::render_png(&ev.scene.unwrap(), ev_scale(src)).unwrap()
}

fn ev_scale(src: &str) -> f32 {
    if src.contains("scale:2") { 2.0 } else { 1.0 }
}

fn rgba(png: &[u8]) -> (i32, i32, Vec<u8>) {
    let image = skia_safe::Image::from_encoded(skia_safe::Data::new_copy(png)).expect("png");
    let (w, h) = (image.width(), image.height());
    let info = skia_safe::ImageInfo::new((w, h), skia_safe::ColorType::RGBA8888, skia_safe::AlphaType::Unpremul, None);
    let row = w as usize * 4;
    let mut buf = vec![0u8; row * h as usize];
    assert!(image.read_pixels(&info, &mut buf, row, (0, 0), skia_safe::image::CachingHint::Disallow));
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
    assert!(mean < mean_limit, "mean channel delta {mean} over {ink} ink bytes");
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
    let lints: Vec<(&str, u32)> = ev.diagnostics.iter().map(|d| (d.code.as_str(), d.span.line)).collect();
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

    let ev = run_with("@target t.png 10x10\npalette x\n  color ink x2\ncircle 1\n", &[]);
    let d = ev.diagnostics.iter().find(|d| d.code == "E0305").unwrap_or_else(|| panic!("{}", show(&ev.diagnostics)));
    assert_eq!(d.message, "`color` takes a color, got a word");
}

#[test]
fn mask_value_and_mask_block_both_mask_the_draw() {
    let s = scene(
        "@target t.png 100x100\ncircle 10 name:body\ngroup g\n  rect 4x4\n  mask @body\ngroup f\n  mask simplex(30) > .5\n  rect 4x4\ngroup b\n  mask\n    circle 3\n",
    );
    let source = |p: &str| node(&s, p).mask.as_ref().map(|m| &m.source).unwrap_or_else(|| panic!("{p} has no mask"));
    assert!(matches!(source("g"), artscript_ir::MaskSource::Nodes(n) if n.len() == 1));
    assert!(matches!(source("f"), artscript_ir::MaskSource::Field { threshold: Some(t), .. } if (*t - 0.5).abs() < 1e-6));
    assert!(matches!(source("b"), artscript_ir::MaskSource::Nodes(n) if n[0].path == "b.mask.circle"));
}

#[test]
fn def_fx_run_before_call_site_fx_and_the_mask_is_the_call_sites() {
    let lib = ("lib/cup.art", "def cup\n  circle 4\n  fx blur 1\n  slot mask\n");
    let s = scene_with("@target t.png 100x100\nuse cup\ncup\n  fx glow 6\n  mask\n    circle 2\n", &[lib]);
    let cup = node(&s, "cup");
    let ops: Vec<&artscript_ir::EffectOp> = cup.effects.iter().map(|e| &e.op).collect();
    assert!(matches!(ops.as_slice(), [artscript_ir::EffectOp::Blur { .. }, artscript_ir::EffectOp::Glow { .. }]), "{ops:?}");
    assert!(cup.mask.is_some());

    let s = scene_with("@target t.png 100x100\nuse cup\ncup\n  mask:\n    circle 2\n", &[lib]);
    assert!(node(&s, "cup").mask.is_none(), "`mask:` fills the slot, it does not mask");
    node(&s, "cup.mask.circle");
}

#[test]
fn a_fill_line_is_a_draw_with_an_unknown_verb() {
    let ev = run_with("@target t.png 10x10\nfill #fff\n", &[]);
    assert_eq!(ev.diagnostics.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(), ["E0301"], "{}", show(&ev.diagnostics));
}

#[test]
fn slot_fill_under_a_shape_is_still_e0331() {
    let ev = run_with("@target t.png 10x10\ncircle 4\n  head:\n    circle 1\n", &[]);
    assert_eq!(ev.diagnostics.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(), ["E0331"], "{}", show(&ev.diagnostics));
}

fn scene_with(src: &str, libs: &[(&str, &str)]) -> Scene {
    let ev = run_with(src, libs);
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
    ev.scene.unwrap()
}

fn assert_box(s: &Scene, path: &str, w: f64, h: f64) {
    let b = shape(s, path).geometry.compute_tight_bounds();
    let (bw, bh) = (f64::from(b.width()), f64::from(b.height()));
    assert!((bw - w).abs() < 1e-3 && (bh - h).abs() < 1e-3, "{path} is {bw}x{bh}, expected {w}x{h}");
}

fn lib_scene(src: &str, lib: &str) -> Scene {
    let ev = run_with(src, &[("lib/d.art", lib)]);
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
    ev.scene.unwrap()
}

#[test]
fn def_size_box_measures_percents_from_its_own_center() {
    let lib = "def d size:40x20\n  rect 100%x100% name:r\n  circle 50% name:c\n  circle 2 at:0%,0% name:mark\n";
    let s = lib_scene("@target t.png 200x100\nuse d\nd\n", lib);
    assert_box(&s, "d.r", 40.0, 20.0);
    assert_box(&s, "d.c", 20.0, 20.0);
    near(at(node(&s, "d.mark")), (-20.0, -10.0));
}

#[test]
fn caller_percent_size_resolves_against_the_caller_frame() {
    let lib = "def d size:40x20\n  rect size name:r\n  circle size.w*.1 name:c\n";
    let pair = lib_scene("@target t.png 200x100\nuse d\nd 50%x50%\n", lib);
    assert_box(&pair, "d.r", 100.0, 50.0);
    assert_box(&pair, "d.c", 20.0, 20.0);
    let bare = lib_scene("@target t.png 200x100\nuse d\nd 50%\n", "def d size:40x20\n  rect size name:r\n");
    assert_box(&bare, "d.r", 50.0, 50.0);
    let scaled = lib_scene("@target t.png 200x100\nuse d\nd 100%x100% scale:2\n", "def d size:10x10\n  rect 100%x100% name:r\n");
    assert_box(&scaled, "d.r", 100.0, 50.0);
    let square = lib_scene("@target t.png 80x40\nuse d\nd 30\n", "def d size:72x32\n  rect size.w,size.h name:r\n");
    assert_box(&square, "d.r", 30.0, 30.0);
}

#[test]
fn a_square_size_rejects_a_pair() {
    let ev = run_with("@target t.png 80x40\nuse d\nd 30x16\n", &[("lib/d.art", "def d size:10\n  circle size*.5\n")]);
    assert!(ev.diagnostics.iter().any(|d| d.code == "E0320"), "{}", show(&ev.diagnostics));
}

#[test]
fn child_without_at_stays_at_the_instance_origin() {
    let s = lib_scene("@target t.png 200x100\nuse d\nd at:30,16\n", "def d size:80x40\n  circle 5 name:c\n");
    near(at(node(&s, "d")), (30.0, 16.0));
    near(at(node(&s, "d.c")), (0.0, 0.0));
    let placed = lib_scene("@target t.png 200x100\nuse d\nd at:100%,100%\n", "def d size:40x20\n  circle 4 name:c\n");
    near(at(node(&placed, "d")), (200.0, 100.0));
    near(at(node(&placed, "d.c")), (0.0, 0.0));
}

#[test]
fn percent_in_a_def_without_size_is_e0322() {
    let ev = run_with("@target t.png 80x40\nuse d\nblush\n", &[("lib/d.art", "def blush\n  circle 50%\n")]);
    let d = ev.diagnostics.iter().find(|d| d.code == "E0322").unwrap_or_else(|| panic!("{}", show(&ev.diagnostics)));
    assert_eq!(d.message, "`blush` has no size, so 50% has nothing to measure");
    assert_eq!(d.fix.as_deref(), Some("declare the box with `size:`, as in `def blush size:72x32`"));

    let outside = run_with("@target t.png 80x40\nlayer x\ninside @x\n  circle 50%\n", &[]);
    let d = outside.diagnostics.iter().find(|d| d.code == "E0322").unwrap_or_else(|| panic!("{}", show(&outside.diagnostics)));
    assert_eq!(d.message, "50% has nothing to measure");
}

#[test]
fn slot_fill_percents_use_the_host_box() {
    let lib = "def host size:40x20\n  slot hole\n";
    let s = lib_scene("@target t.png 200x100\nuse d\nhost\n  hole:\n    rect 100%x100% name:r\n    circle 2 at:0%,0% name:mark\n", lib);
    assert_box(&s, "host.hole.r", 40.0, 20.0);
    near(at(node(&s, "host.hole.mark")), (-20.0, -10.0));
    let shifted = lib_scene(
        "@target t.png 200x100\nuse d\nhost\n",
        "def host size:40x20\n  slot hole at:10,0\n    circle 2 at:0%,0% name:mark\n",
    );
    near(at(node(&shifted, "host.hole.mark")), (-20.0, -10.0));
}

#[test]
fn unplaced_slot_content_stays_at_the_slot() {
    let lib = "def host size:40x20\n  slot hole at:10,0\n    circle 2 name:own\n";
    let own = lib_scene("@target t.png 200x100\nuse d\nhost\n", lib);
    near(at(node(&own, "host.hole.own")), (0.0, 0.0));
    let filled = lib_scene("@target t.png 200x100\nuse d\nhost\n  hole:\n    circle 2 name:fill\n", lib);
    near(at(node(&filled, "host.hole.fill")), (0.0, 0.0));
}

#[test]
fn size_is_not_unused_when_only_percents_use_the_box() {
    let manifest = "[project]\nout = \"out\"\nlib = [\"lib\"]\n\n[lint]\nraw-color = \"off\"\nunused = \"warn\"\noff-canvas = \"off\"\n";
    let files = [
        ("art/t.art".to_string(), "@target t.png 80x40\nuse d\nblush\n".to_string()),
        ("lib/d.art".to_string(), "def blush size:40x20 extra:1\n  circle 50%\n".to_string()),
    ]
    .into();
    let p = Project::from_sources(manifest, files).unwrap();
    let plan = plan(&p);
    assert!(plan.diagnostics.is_empty(), "{:?}", plan.diagnostics);
    let ev = evaluate(&p, &plan.jobs[0]);
    let unused: Vec<&str> = ev.diagnostics.iter().filter(|d| d.code == "L0103").map(|d| d.message.as_str()).collect();
    assert!(unused.iter().any(|m| m.contains("`extra`")), "{unused:?}");
    assert!(unused.iter().all(|m| !m.contains("`size`")), "{unused:?}");
}

#[test]
fn absolute_size_still_boxes_a_child_of_a_sizeless_def() {
    let s = lib_scene(
        "@target t.png 80x40\nuse d\nouter\n",
        "def outer\n  inner 20\ndef inner size:10\n  rect 100%x100% name:r\n",
    );
    assert_box(&s, "outer.inner.r", 20.0, 20.0);
    let ev = run_with("@target t.png 80x40\nuse d\nouter\n", &[("lib/d.art", "def outer\n  inner 50%\ndef inner size:10\n  circle 4\n")]);
    let d = ev.diagnostics.iter().find(|d| d.code == "E0322").unwrap_or_else(|| panic!("{}", show(&ev.diagnostics)));
    assert_eq!(d.message, "`outer` has no size, so 50% has nothing to measure");
}

fn fx_ops(n: &Node) -> Vec<String> {
    n.effects
        .iter()
        .map(|e| format!("{:?}", e.op).split([' ', '{']).next().unwrap_or_default().to_string())
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

const SURFACE: &str = "finish surface width:5 a:.55\n  fx rim width dir:1,1 a:a\n  fx grain .035 scale:2\n";

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
    assert_eq!(fx_ops(node(&s, "l.c")), ["Rim", "Grain"]);
    assert!(node(&s, "l").effects.is_empty());
    assert!(node(&s, "l.c.dot").effects.is_empty());
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
    assert_ne!(grain_seed(node(&shared, "cap")), grain_seed(node(&shared, "shimaki")));
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
    let written = scene("@target t.png 100x100\ncircle 10 name:cap\n  fx rim 7 color:#ff0000\n  fx grain .04 scale:2.5\n");
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
    assert_eq!(fx_ops(node(&s, "leaf")), ["Rim", "Grain", "Outline", "Adjust"]);
    let w = run_with(src, &[written]);
    assert!(w.diagnostics.is_empty(), "{}", show(&w.diagnostics));
    assert_eq!(fx_debug(&s, "leaf"), fx_debug(&w.scene.unwrap(), "leaf"));
}

#[test]
fn finish_as_a_node_key_is_e0339_pointing_at_an_fx_line() {
    let ev = run_with(&format!("@target t.png 100x100\n{SURFACE}circle 10 finish:surface\n"), &[]);
    let d = diag(&ev, "E0339");
    assert_eq!(d.span.line, 5);
    assert_eq!(d.fix.as_deref(), Some("write `fx surface` under the node"));

    let ev = run_with(&format!("@target t.png 100x100\n{SURFACE}circle 10 style:surface\n"), &[]);
    assert_eq!(diag(&ev, "E0307").fix.as_deref(), Some("write `fx surface` under the node"));
}

#[test]
fn finish_body_is_plain_built_in_effects() {
    let ev = run_with("@target t.png 100x100\nfinish a\n  fx blur 2 as:soft\n  fx merge soft self of:self\ncircle 4\n", &[]);
    let lines: Vec<(&str, u32)> = ev.diagnostics.iter().map(|d| (d.code.as_str(), d.span.line)).collect();
    assert_eq!(lines, [("E0336", 3), ("E0336", 4)], "{}", show(&ev.diagnostics));

    let ev = run_with(
        &format!("@target t.png 100x100\n{SURFACE}finish outer\n  fx rim 2\n  fx surface\ncircle 4\n"),
        &[],
    );
    let d = diag(&ev, "E0338");
    assert_eq!(d.span.line, 7);
    assert!(d.notes.iter().any(|n| n == "`surface` is a finish, and finishes do not nest"), "{d}");

    let ev = run_with("@target t.png 100x100\nfinish rim\n  fx grain .1\ncircle 4\n", &[]);
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
    assert_eq!(diag(&explicit, "E0212").message, "`finish/surface` does not export `_surface`");

    let style = run_with("@target t.png 100x100\nuse finish/surface { _ink }\ncircle 10 style:_ink\n", &[lib]);
    assert_eq!(diag(&style, "E0212").message, "`finish/surface` does not export `_ink`");

    let wild = run_with("@target t.png 100x100\nuse finish/surface\ncircle 10 name:cap\n  fx _surface\n", &[lib]);
    assert_eq!(diag(&wild, "E0391").message, "unknown effect `_surface`");

    let public = run_with(
        "@target t.png 100x100\nuse finish/surface { surface ink }\ncircle 10 name:cap style:ink\n  fx surface\n",
        &[lib],
    );
    assert!(public.diagnostics.is_empty(), "{}", show(&public.diagnostics));
    let s = public.scene.unwrap();
    assert_eq!(fx_ops(node(&s, "cap")), ["Rim"]);
    let Some(Paint::Solid(color)) = &shape(&s, "cap").fill else { panic!("fill") };
    assert_eq!(color.hex(), "#112233");
}

#[test]
fn explicit_use_binds_a_style_and_a_finish_that_share_a_name() {
    let lib = ("lib/finish/surface.art", "style surface fill:#112233\nfinish surface width:4\n  fx rim width\n");
    let ev = run_with(
        "@target t.png 100x100\nuse finish/surface { surface }\ncircle 10 name:cap style:surface\n  fx surface\n",
        &[lib],
    );
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
    let s = ev.scene.unwrap();
    assert_eq!(fx_ops(node(&s, "cap")), ["Rim"]);
    let Some(Paint::Solid(color)) = &shape(&s, "cap").fill else { panic!("fill") };
    assert_eq!(color.hex(), "#112233");
}

#[test]
fn finish_parameters_are_named_and_typed_by_their_defaults() {
    let ev = run_with(&format!("@target t.png 100x100\n{SURFACE}circle 10\n  fx surface 6\n"), &[]);
    assert!(diag(&ev, "E0321").notes.iter().any(|n| n == "parameters: width, a"));
    let ev = run_with(&format!("@target t.png 100x100\n{SURFACE}circle 10\n  fx surface depth:6\n"), &[]);
    assert_eq!(diag(&ev, "E0321").message, "finish `surface` has no parameter `depth`");
    let ev = run_with(&format!("@target t.png 100x100\n{SURFACE}circle 10\n  fx surface width:#ff0000\n"), &[]);
    assert_eq!(diag(&ev, "E0320").span.line, 6);
}

#[test]
fn non_positive_size_is_e0320() {
    let ev = run_with("@target t.png 40x40\nuse d\nd 0\n", &[("lib/d.art", "def d size:40\n  circle size\n")]);
    let d = ev.diagnostics.iter().find(|d| d.code == "E0320").unwrap_or_else(|| panic!("{}", show(&ev.diagnostics)));
    assert!(d.message.contains("must be positive"), "{d}");
    assert_eq!(d.fix.as_deref(), Some("write `size:40` or `size:72x32`"));
}
