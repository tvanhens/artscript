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
        [("art/t.art".to_string(), "@target t.png 10x10\npalette x\n  a #ff0000\ncircle 3 fill:x.a\nrect 2x2 fill:#00ff00\n".to_string())].into(),
    )
    .unwrap();
    let plan = plan(&p);
    let ev = evaluate(&p, &plan.jobs[0]);
    let lints: Vec<(&str, u32)> = ev.diagnostics.iter().map(|d| (d.code.as_str(), d.span.line)).collect();
    assert_eq!(lints, [("L0101", 5)]);
}
