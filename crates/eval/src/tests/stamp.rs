//! Stamp declarations, calls, the `stamp:` key, placement along the spine, identity and
//! references, seeds, and the diagnostics each failure lands on.

use std::collections::BTreeSet;

use ramose_ir::{Node, NodeKind, Paint, Scene};
use ramose_syntax::Diagnostic;

use crate::explain::explain;

use super::{at, diag, node, paths, run_with, shape, show};

const GARDEN: &str = "\
def leaf size:10 tint:#4a7a3a
  curve 0,0 size*.5,-size*.35 size,0 size*.5,size*.35 closed tension:.6 fill:tint name:blade
  anchor tip size,0

stamp leaves spacing:8+8*pressure tint:#4a7a3a
  let side = if mod(i 2) == 0 then 1 else -1
  leaf 5+7*pressure tint:tint at:0,side*2 rot:side*rand(15 40) name:leaf

stamp dots r:1.5 spacing:5 tint:none
  circle r fill:if tint == none then paint else tint

stamp probe spacing:25
  rect s,len at:0,u*10 fill:#000000 name:r
";

const LIB: (&str, &str) = ("lib/garden.ramo", GARDEN);

fn ok(src: &str) -> Scene {
    let ev = run_with(src, &[LIB]);
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
    ev.scene.unwrap()
}

fn errors(src: &str) -> Vec<Diagnostic> {
    run_with(src, &[LIB]).diagnostics
}

fn first(src: &str, code: &str) -> Diagnostic {
    diag(&run_with(src, &[LIB]), code).clone()
}

fn stamps<'a>(s: &'a Scene, path: &str) -> &'a [Node] {
    &shape(s, path).stamps
}

fn solid(n: &Node) -> ramose_ir::Color {
    match &n.kind {
        NodeKind::Shape(s) => match s.fill.as_ref().map(|f| &f.paint) {
            Some(Paint::Solid(c)) => *c,
            other => panic!("{} has fill {other:?}", n.path),
        },
        _ => panic!("{} is not a shape", n.path),
    }
}

fn x_positions(nodes: &[Node]) -> Vec<f32> {
    nodes.iter().map(|n| (n.transform.translate_x() * 1000.0).round() / 1000.0).collect()
}

#[test]
fn a_stamp_draws_its_block_once_per_site_under_the_host_and_replaces_the_default_stroke() {
    let s = ok("@target t.png 200x100\nuse garden { dots }\ncurve 0,50 50,50 100,50 stamp:dots name:line\ncurve 0,80 100,80 stroke:#ff0000,2 stamp:dots name:inked\n");
    let line = shape(&s, "line");
    assert!(line.stroke.is_none(), "a stamped open path draws no default line");
    assert_eq!(line.stamps.len(), 20);
    assert_eq!(x_positions(&line.stamps)[..3], [2.5, 7.5, 12.5]);
    let first = &line.stamps[0];
    assert_eq!(first.path, "line.dots.0");
    assert!(matches!(first.kind, NodeKind::Group { isolate: false, .. }), "a stamp with no blend inside is a plain group");
    assert_eq!(first.origin, "lib/garden.ramo:9:1");
    assert_eq!(first.descendants().map(|n| n.path.as_str()).collect::<Vec<_>>(), ["line.dots.0.circle~1"]);
    assert!(paths(&s).contains(&"line.dots.19.circle~1".to_string()));
    let inked = shape(&s, "inked");
    assert_eq!(inked.stroke.as_ref().map(|st| st.width), Some(2.0), "a written stroke stays");
    assert_eq!(solid(node(&s, "inked.dots.3.circle~1")).hex(), "#ff0000", "`paint` is the stroke's paint");
}

#[test]
fn the_block_binds_the_site_and_the_frame_follows_the_tangent() {
    let s = ok("@target t.png 200x200\nuse garden { probe }\ncurve 0,10 100,10 stamp:probe name:along\ncurve 10,0 10,100 stamp:probe name:down\n");
    let along = stamps(&s, "along");
    assert_eq!(x_positions(along), [12.5, 37.5, 62.5, 87.5]);
    let r = node(&s, "along.probe.1.r");
    let b = r.bounds().unwrap();
    assert!((b.width() - 37.5).abs() < 1e-3 && (b.height() - 100.0).abs() < 1e-3, "`s` and `len`: {b:?}");
    assert!((b.center_y() - 3.75).abs() < 1e-3, "`u` is s / len: {b:?}");
    let first = &stamps(&s, "down")[0];
    let b = first.bounds().unwrap();
    assert!((first.transform.translate_y() - 12.5).abs() < 1e-3);
    assert!((b.width() - 100.0).abs() < 1e-3 && (b.height() - 12.5).abs() < 1e-3, "x runs along the travel, so `len` lies along y: {b:?}");
}

#[test]
fn pressure_sizes_and_spaces_the_leaves_and_the_sides_alternate() {
    let s = ok("@target t.png 200x100\nuse garden { leaves }\ncurve 0,50 100,50 stroke:#5a3b2a,3 pressure:[1 .2] stamp:leaves name:vine\n");
    let leaves = stamps(&s, "vine");
    let xs = x_positions(leaves);
    assert_eq!(xs[0], 8.0, "half of 8 + 8 * pressure at s = 0");
    let gaps: Vec<f32> = xs.windows(2).map(|w| w[1] - w[0]).collect();
    assert!(gaps.windows(2).all(|g| g[1] < g[0]), "the spacing shrinks with the pressure: {gaps:?}");
    let widths: Vec<f32> = leaves.iter().map(|n| n.bounds().unwrap().width()).collect();
    assert!(widths[0] > widths[widths.len() - 1] * 1.5, "the leaves shrink with the pressure: {widths:?}");
    let ys: Vec<f32> = (0..4).map(|i| at(node(&s, &format!("vine.leaves.{i}.leaf"))).1).collect();
    assert_eq!(ys, [2.0, -2.0, 2.0, -2.0]);
}

#[test]
fn seed_on_the_host_pins_its_stamps_apart_and_moves_nothing_else() {
    let src = |seed: &str| {
        format!("@target t.png 200x100\nuse garden {{ leaves }}\ncurve 0,30 100,30 stroke:#000000,2 stamp:leaves {seed} name:a\ncurve 0,70 100,70 stroke:#000000,2 stamp:leaves name:b\n")
    };
    let rots = |s: &Scene, path: &str| -> Vec<(f32, f32)> {
        stamps(s, path).iter().map(|n| n.descendants().next().unwrap()).map(|leaf| (leaf.transform.scale_x(), leaf.transform.skew_y())).collect()
    };
    let base = ok(&src(""));
    let pinned = ok(&src("seed:7"));
    let twice = ok(&src("seed:7"));
    assert_ne!(rots(&base, "a"), rots(&pinned, "a"), "`seed:` moves the host's leaves");
    assert_eq!(rots(&base, "b"), rots(&pinned, "b"), "and nothing else");
    assert_eq!(rots(&pinned, "a"), rots(&twice, "a"));
    let a = rots(&pinned, "a");
    assert!(a.windows(2).any(|w| w[0] != w[1]), "each stamp draws its own numbers under one seed: {a:?}");
}

#[test]
fn references_reach_a_stamp_its_parts_and_its_measures() {
    let s = ok("@target t.png 200x100\nuse garden { leaves }\ncurve 0,50 100,50 stroke:#000000,2 stamp:leaves name:vine\ncircle 2 at:@vine.leaves.3.leaf.tip fill:#ff0000 name:tip\ncircle 2 at:@vine.leaves.3.cx,@vine.leaves.3.cy fill:#0000ff name:mid\n");
    let leaf = node(&s, "vine.leaves.3");
    let b = leaf.bounds().unwrap();
    let (cx, cy) = (b.center_x(), b.center_y());
    let mid = at(node(&s, "mid"));
    assert!((mid.0 - cx).abs() < 1e-3 && (mid.1 - cy).abs() < 1e-3, "{mid:?} vs {cx},{cy}");
    let tip = at(node(&s, "tip"));
    let leaf_world = leaf.transform * node(&s, "vine.leaves.3.leaf").transform;
    let anchor = leaf_world.map_point((12.0, 0.0));
    assert!((tip.0 - anchor.x).abs() < 1e-3 && (tip.1 - anchor.y).abs() < 1e-3, "{tip:?} vs {anchor:?}");
    let d = first("@target t.png 200x100\nuse garden { leaves }\ncurve 0,50 100,50 stroke:#000000,2 stamp:leaves name:vine\ncircle 2 at:@vine.leaves.40.cx,0 name:far\n", "E0350");
    assert!(d.message.contains("vine.leaves.40"), "{d}");
    let d = first("@target t.png 200x100\nuse garden { leaves }\ncurve 0,50 100,50 stroke:#000000,2 stamp:leaves name:vine\ncircle 2 at:@vine.petals.0.cx,0 name:far\n", "E0350");
    assert!(d.notes.iter().any(|n| n.contains("leaves")), "the stamp sets are listed as parts: {d}");
}

#[test]
fn copy_carries_stamp_like_the_other_paint_keys() {
    let s = ok("@target t.png 200x100\nuse garden { dots }\ncurve 0,30 100,30 stamp:dots name:a\ncopy @a at:0,40 stamp:dots fill:none name:b\ncopy @a at:0,60 fill:none name:c\n");
    assert_eq!(stamps(&s, "b").len(), 20);
    assert!(stamps(&s, "c").is_empty(), "the copy takes `stamp:` from its own line, as it takes `fill:`");
}

#[test]
fn a_closed_contour_spreads_a_constant_spacing_evenly_and_a_varying_one_stops_short_of_the_seam() {
    let s = ok("@target t.png 100x100\nuse garden { dots }\ncircle 10 at:50,50 stamp:dots(spacing:7) name:even\ncircle 10 at:50,50 stamp:dots(spacing:7+0*i) name:walked\ncircle 10 at:50,50 stamp:dots(count:5) name:counted\n");
    let len = 2.0 * std::f32::consts::PI * 10.0;
    let even = stamps(&s, "even");
    assert_eq!(even.len() as f32, (len / 7.0).round());
    let pos: Vec<(f32, f32)> = even.iter().map(|n| (n.transform.translate_x(), n.transform.translate_y())).collect();
    let gaps: Vec<f32> = (0..pos.len()).map(|i| {
        let (a, b) = (pos[i], pos[(i + 1) % pos.len()]);
        ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
    }).collect();
    // Skia measures arc length to about a percent on a circle, so the chords vary that much.
    let mean = gaps.iter().sum::<f32>() / gaps.len() as f32;
    assert!(gaps.iter().all(|g| (g - mean).abs() < mean * 0.03), "even all the way round the seam: {gaps:?}");
    let walked = stamps(&s, "walked");
    let pos: Vec<(f32, f32)> = walked.iter().map(|n| (n.transform.translate_x(), n.transform.translate_y())).collect();
    let (first, last) = (pos[0], pos[pos.len() - 1]);
    let seam = ((first.0 - last.0).powi(2) + (first.1 - last.1).powi(2)).sqrt();
    assert!(seam > 3.5, "no doubled stamp at the seam: {seam}");
    assert_eq!(stamps(&s, "counted").len(), 5);
    let open = ok("@target t.png 100x100\nuse garden { dots }\ncurve 0,50 100,50 stamp:dots(count:4) name:four\n");
    assert_eq!(x_positions(stamps(&open, "four")), [12.5, 37.5, 62.5, 87.5]);
}

#[test]
fn stamp_inherits_from_a_layer_none_clears_it_a_list_stacks_sets_and_name_renames_one() {
    let s = ok("@target t.png 200x100\nuse garden { dots leaves }\nlayer g stamp:dots\n  curve 0,20 100,20 name:a\n  curve 0,40 100,40 stamp:none name:b\n  curve 0,60 100,60 stroke:#000000,2 stamp:[dots leaves(name:small)] name:c\n");
    assert_eq!(stamps(&s, "a").len(), 20);
    assert!(stamps(&s, "b").is_empty());
    let c: Vec<&str> = stamps(&s, "c").iter().map(|n| n.path.as_str()).collect();
    assert_eq!(c[0], "c.dots.0");
    assert_eq!(c[19], "c.dots.19");
    assert!(c[20].starts_with("c.small."), "{c:?}");
}

#[test]
fn a_def_takes_a_stamp_parameter_and_recalls_it_with_overrides() {
    let s = ok("@target t.png 200x100\nuse garden { dots leaves }\ndef vine stamp:dots\n  curve 0,0 100,0 stamp:stamp(spacing:10) name:v\nvine at:0,20 name:a\nvine at:0,40 stamp:none name:b\nvine at:0,60 stamp:leaves name:c\n");
    assert_eq!(stamps(&s, "a.v").len(), 10);
    assert!(stamps(&s, "b.v").is_empty());
    let c = stamps(&s, "c.v");
    assert_eq!(c.len(), 10);
    assert!(c[0].path.starts_with("c.v.leaves."));
    let d = first("@target t.png 200x100\nuse garden { dots }\ndef vine stamp:dots\n  curve 0,0 100,0 stamp:stamp(gap:10) name:v\nvine stamp:none name:b\n", "E0366");
    assert!(d.message.contains("no parameter `gap`"), "a call on the parameter is checked against the default even under `none`: {d}");
}

#[test]
fn the_block_sees_the_enclosing_style_without_stamp_and_the_channels() {
    let s = ok("@target t.png 200x100\nstamp sq spacing:10\n  rect 2 name:r\ngroup fill:#ff0000 stamp:sq pressure:[.5 1]\n  curve 0,50 100,50 stroke:#000000,2 stamp:sq name:a\n");
    let r = node(&s, "a.sq.0.r");
    assert_eq!(solid(r).hex(), "#ff0000", "the layer's fill reaches the block");
    assert!(shape(&s, "a.sq.0.r").stamps.is_empty(), "the block's own shapes are not stamped again");
}

#[test]
fn a_channel_counts_as_read_when_a_stamp_reads_it() {
    ok("@target t.png 200x100\nuse garden { leaves }\ncurve 0,50 100,50 pressure:[.5 1] stamp:leaves name:a\n");
    let d = first("@target t.png 200x100\nuse garden { dots }\ncurve 0,50 100,50 pressure:[.5 1] stamp:dots name:a\n", "E0362");
    assert_eq!((d.span.line, d.span.col), (3, 19), "{d}");
    assert!(d.notes.iter().any(|n| n == "the node's stamps do not read `pressure`"), "{d}");
    ok("@target t.png 200x100\nuse garden { dots }\nlayer g pressure:[.5 1]\n  curve 0,50 100,50 stamp:dots name:a\n");
}

#[test]
fn paint_samples_a_gradient_stroke_at_the_stamp() {
    let s = ok("@target t.png 200x100\nuse garden { dots }\ncurve 0,50 100,50 stroke:grad(right #ff0000 #0000ff),2 stamp:dots name:a\n");
    let first = solid(node(&s, "a.dots.0.circle~1"));
    let last = solid(node(&s, "a.dots.19.circle~1"));
    assert!(first.r > 0.8 && first.b < 0.2, "{first:?}");
    assert!(last.b > 0.8 && last.r < 0.2, "{last:?}");
}

#[test]
fn a_stamp_block_takes_fx_and_a_percent_spacing_measures_the_stroke() {
    let s = ok("@target t.png 200x100\nstamp glowing spacing:150%\n  circle 2 fill:#000000\n  fx glow 2\ncurve 0,50 100,50 stroke:#000000,[4 8] stamp:glowing name:a\n");
    let a = stamps(&s, "a");
    assert_eq!(a[0].effects.len(), 1);
    let gaps: Vec<f32> = x_positions(a).windows(2).map(|w| w[1] - w[0]).collect();
    assert!(gaps[0] > 6.0 && gaps[0] < 7.0, "150% of a width near 4: {gaps:?}");
    assert!(gaps[gaps.len() - 1] > gaps[0], "the spacing grows with the stroke: {gaps:?}");
}

#[test]
fn explain_folds_a_run_of_stamps_and_shows_its_ends() {
    let s = ok("@target t.png 200x100\nuse garden { dots }\ncurve 0,50 100,50 stamp:dots name:line\n");
    let text = explain(&s, &BTreeSet::new());
    assert!(text.contains("line  shape  0,48.5 100x3  stamps:20"), "{text}");
    assert!(text.contains("\n  line.dots.0..19  stamp x20  "), "{text}");
    assert!(text.contains("\n  line.dots.0  stamp  "), "{text}");
    assert!(text.contains("\n  line.dots.19  stamp  "), "{text}");
    assert!(!text.contains("line.dots.1  stamp"), "{text}");
}

#[test]
fn every_stamp_error_lands_at_its_own_line() {
    let src = "\
@target t.png 128x128
use garden { leaves dots }
stamp selfish spacing:6
  circle 2 fill:#000000 stamp:selfish
def vined
  curve 0,0 40,10 80,0 stamp:leaves name:vine
def mid
  circle 2 fill:#000000 stamp:viadef name:c
stamp viadef spacing:6
  mid name:m
curve 0,0 40,10 80,0 stroke:#000000,2 pressure:[.2 1 .4] name:unread
curve 0,20 40,30 80,20 stamp:dots(spacing:0) name:zero
curve 0,40 40,50 80,40 stamp:[dots dots(spacing:3)] name:twice
curve 0,60 40,70 80,60 stamp:dots(spacng:3) name:typo
curve 0,80 40,90 80,80 stamp:selfish name:loop
vined at:0,100 name:v
  .vine.leaves.2 off
curve 0,120 40,120 stamp:dotz name:unknown
curve 0,120 40,120 stamp:dots(r:#ff0000) name:kind
circle 3 at:50,50 stamp:dots(spacing:150%) name:pct
curve 0,120 40,120 stamp:viadef name:through
curve 0,120 40,120 stamp:3 name:number
curve 0,120 40,120 stamp:[] name:empty
circle 0 at:50,50 stamp:dots name:dot
curve 0,120 40,120 stamp:dots(count:0) name:nocount
";
    let ds = errors(src);
    let mut got: Vec<(String, u32, u32)> = ds.iter().map(|d| (d.code.clone(), d.span.line, d.span.col)).collect();
    got.sort_by_key(|(_, line, col)| (*line, *col));
    let want = [
        ("E0369", 4, 25),
        ("E0369", 8, 25),
        ("E0362", 11, 39),
        ("E0368", 12, 35),
        ("E0333", 13, 24),
        ("E0366", 14, 1),
        ("E0340", 17, 3),
        ("E0367", 18, 20),
        ("E0366", 19, 1),
        ("E0368", 20, 30),
        ("E0367", 22, 20),
        ("E0367", 23, 20),
        ("E0368", 24, 19),
        ("E0366", 25, 1),
    ];
    let want: Vec<(String, u32, u32)> = want.iter().map(|(c, l, k)| (c.to_string(), *l, *k)).collect();
    assert_eq!(got, want, "{}", show(&ds));
    let by_line = |line: u32| ds.iter().find(|d| d.span.line == line).unwrap();
    assert!(by_line(4).message.contains("stamp `selfish` places itself"));
    assert_eq!(by_line(4).node, "loop.selfish.0");
    assert!(by_line(8).message.contains("stamp `viadef` places itself"));
    assert!(by_line(11).message.contains("`pressure:` has no effect because nothing on this node reads it"));
    assert!(by_line(12).message.contains("must be a positive number, got 0"));
    assert!(by_line(13).message.contains("`dots` is already a stamp set on this node"));
    assert_eq!(by_line(14).fix.as_deref(), Some("did you mean `spacing:`?"));
    assert_eq!(by_line(18).fix.as_deref(), Some("did you mean `stamp:dots`?"));
    assert!(by_line(20).notes.iter().any(|n| n.contains("this node has no stroke")), "{}", by_line(20));
    assert!(by_line(24).message.contains("nothing to stamp along"));
}

#[test]
fn declaration_errors_are_e0365_and_a_failed_stamp_stays_quiet_at_its_uses() {
    let src = "\
@target t.png 64x64
stamp both spacing:4 count:3
  circle 1 fill:#000000
stamp neither tint:#000000
  circle 1 fill:#000000
stamp shadowing spacing:4 pressure:1
  circle 1 fill:#000000
stamp named spacing:4 name:x
  circle 1 fill:#000000
stamp slotted spacing:4
  slot inner
  anchor tip 0,0
curve 0,0 40,0 stamp:both name:a
curve 0,0 40,0 stamp:neither name:b
";
    let ds = errors(src);
    let got: Vec<(String, u32)> = ds.iter().map(|d| (d.code.clone(), d.span.line)).collect();
    assert_eq!(
        got,
        [("E0365".to_string(), 2), ("E0365".into(), 4), ("E0365".into(), 6), ("E0365".into(), 8), ("E0365".into(), 11), ("E0365".into(), 12)],
        "{}",
        show(&ds)
    );
    assert!(ds[0].message.contains("both `spacing:` and `count:`"));
    assert!(ds[1].message.contains("no `spacing:` or `count:`"));
    assert!(ds[2].message.contains("cannot be named `pressure`"));
    assert!(ds[4].message.contains("a `slot`"));
    assert!(ds[5].message.contains("an `anchor`"));
}

#[test]
fn a_warning_in_a_block_keeps_the_set_going_and_is_reported_once() {
    let ev = run_with("@target t.png 256x256\nstamp beads spacing:10\n  circle 2 fill:#333333 name:bead\n  expect i < 5 \"only five fit\"\ncurve 20,128 236,128 stamp:beads name:line\n", &[]);
    let warned: Vec<&Diagnostic> = ev.diagnostics.iter().filter(|d| d.code == "C0201").collect();
    assert_eq!(warned.len(), 1, "{}", show(&ev.diagnostics));
    assert_eq!(warned[0].node, "line.beads.5");
    assert!(!warned[0].notes.iter().any(|n| n.starts_with("also in")), "{}", warned[0]);
    assert_eq!(ev.diagnostics.len(), 1, "{}", show(&ev.diagnostics));
    assert_eq!(stamps(ev.scene.as_ref().unwrap(), "line").len(), 22, "every bead is placed");
}

#[test]
fn an_unnamed_host_whose_block_names_parts_is_one_e0312_at_the_host() {
    let ds = errors("@target t.png 200x100\nuse garden { leaves }\ncurve 0,50 100,50 stroke:#000000,2 stamp:leaves\n");
    let got: Vec<(&str, &str, u32)> = ds.iter().map(|d| (d.code.as_str(), d.file.as_str(), d.span.line)).collect();
    assert_eq!(got, [("E0312", "art/t.ramo", 3)], "{}", show(&ds));
}

#[test]
fn a_let_or_each_in_a_block_cannot_rebind_a_bound_name() {
    let src = "\
@target t.png 64x64
stamp shadow spacing:10
  let pressure = 1
  circle 2*pressure fill:#000000 name:bead
stamp looped spacing:10
  each i in 0..2
    circle 1 fill:#000000
  group
    each k, angle in [1 2]
      circle 1 fill:#000000
curve 0,10 60,10 pressure:[.1 1] stamp:shadow name:a
curve 0,30 60,30 stamp:looped name:b
";
    let ds = errors(src);
    let got: Vec<(String, u32)> = ds.iter().map(|d| (d.code.clone(), d.span.line)).collect();
    assert_eq!(got, [("E0365".to_string(), 3), ("E0365".into(), 6), ("E0365".into(), 9)], "{}", show(&ds));
    assert!(ds[0].message.contains("`pressure`"), "{}", ds[0]);
}

#[test]
fn an_unpainted_host_gives_its_effects_the_stamps_color() {
    let s = ok("@target t.png 256x256\nuse garden { dots }\ncurve 20,128 236,128 stamp:dots(tint:#cc2222 r:4) name:d\n  fx glow 6\n");
    let text = explain(&s, &BTreeSet::new());
    let line = text.lines().find(|l| l.starts_with("d  ")).unwrap();
    let group = ok("@target t.png 256x256\ngroup name:g\n  circle 4 at:40,60 fill:#cc2222 name:c\n  fx glow 6\n");
    let glow = explain(&group, &BTreeSet::new()).lines().find(|l| l.starts_with("g  ")).unwrap().split("[fx glow 6 ").nth(1).unwrap()[..7].to_string();
    assert!(line.contains(&format!("[fx glow 6 {glow}]")), "the dots color the glow as a group of them does: {line} vs {glow}");
}

#[test]
fn the_rig_carries_anchors_inside_stamps() {
    let ev = run_with("@target t.png 256x256\ndef bud\n  circle 3 fill:#22aa66 name:b\n  anchor tip 0,-3\nstamp buds spacing:40\n  bud name:bud\ncurve 20,128 236,128 stroke:#333333,2 stamp:buds name:vine\n", &[]);
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
    let tip = ev.rig.anchors.get("vine.buds.2.bud.tip").copied();
    assert_eq!(tip, Some((120.0, 125.0)), "{:?}", ev.rig.anchors);
    assert_eq!(ev.rig.anchors.keys().filter(|k| k.starts_with("vine.buds.")).count(), 5, "{:?}", ev.rig.anchors);
}

#[test]
fn a_stamp_is_a_layer_only_when_a_blend_inside_needs_one() {
    let s = ok("@target t.png 200x100\nstamp inked spacing:10\n  circle 2 fill:#000000 blend:multiply\nstamp nested spacing:10\n  group\n    circle 2 fill:#000000 blend:screen\ncurve 0,30 100,30 stamp:inked name:a\ncurve 0,60 100,60 stamp:nested name:b\n");
    for host in ["a", "b"] {
        assert!(stamps(&s, host).iter().all(|n| matches!(n.kind, NodeKind::Group { isolate: true, .. })), "{host}");
    }
}
