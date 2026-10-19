//! `artscript explain`: the evaluated node tree with paths, canvas bounds and resolved paint.

use std::collections::BTreeSet;
use std::fmt::Write;

use artscript_ir::{Effect, EffectOp, MaskSource, Matrix, Node, NodeKind, Paint, Rect, Scene};

use crate::value::num;

fn paint(p: &Paint) -> String {
    let ends = |stops: &[artscript_ir::Stop]| match (stops.first(), stops.last()) {
        (Some(a), Some(b)) => format!("{}..{}", a.color.hex(), b.color.hex()),
        _ => String::new(),
    };
    match p {
        Paint::Solid(c) => c.hex(),
        Paint::Linear { stops, .. } => format!("linear({})", ends(stops)),
        Paint::Radial { stops, radius, .. } => format!("radial({} r:{})", ends(stops), num(*radius as f64)),
        Paint::Conic { stops, .. } => format!("conic({})", ends(stops)),
        Paint::Field { stops, .. } => format!("field({})", ends(stops)),
        Paint::Image { image, .. } => format!("image({}x{})", image.width(), image.height()),
    }
}

fn effect(e: &Effect) -> String {
    match &e.op {
        EffectOp::Blur { sigma } => format!("blur {}", num(*sigma as f64)),
        EffectOp::Shadow { dx, dy, blur, color } => format!("shadow {},{} blur:{} {}", num(*dx as f64), num(*dy as f64), num(*blur as f64), color.hex()),
        EffectOp::Inner { dx, dy, blur, color } => format!("inner {},{} blur:{} {}", num(*dx as f64), num(*dy as f64), num(*blur as f64), color.hex()),
        EffectOp::Glow { radius, color, strength: 1.0 } => format!("glow {} {}", num(*radius as f64), color.hex()),
        EffectOp::Glow { radius, color, strength } => format!("glow {} {} strength:{}", num(*radius as f64), color.hex(), num(*strength as f64)),
        EffectOp::Rim { width, color, .. } => format!("rim {} {}", num(*width as f64), color.hex()),
        EffectOp::Adjust { hue, sat, light } => format!("adjust {} {} {}", num(*hue as f64), num(*sat as f64), num(*light as f64)),
        EffectOp::Outline { width, color, sides } => {
            let kept: Vec<&str> = [(sides.top, "top"), (sides.right, "right"), (sides.bottom, "bottom"), (sides.left, "left")]
                .into_iter()
                .filter_map(|(on, word)| on.then_some(word))
                .collect();
            let only = if sides.is_all() { String::new() } else { format!(" sides:{}", kept.join(",")) };
            format!("outline {} {}{only}", num(*width as f64), color.hex())
        }
        EffectOp::Grain { strength, .. } => format!("grain {}", num(*strength as f64)),
        EffectOp::Merge { inputs, .. } => format!("merge ({})", inputs.len()),
    }
}

fn bounds_text(b: Rect) -> String {
    format!("{},{} {}x{}", num(b.left as f64), num(b.top as f64), num(b.width() as f64), num(b.height() as f64))
}

/// `stamp` labels a node that sits in its host's `stamps`; any other node is labelled by its kind.
fn walk(n: &Node, parent: Matrix, depth: usize, seeded: &BTreeSet<String>, out: &mut String, stamp: bool) {
    let pad = "  ".repeat(depth);
    let kind = match &n.kind {
        _ if stamp => "stamp",
        NodeKind::Group { isolate: true, .. } => "layer",
        NodeKind::Group { .. } => "group",
        NodeKind::Shape(_) => "shape",
    };
    let _ = write!(out, "{pad}{}  {kind}", n.path);
    if let Some(b) = n.bounds() {
        let _ = write!(out, "  {}", bounds_text(parent.map_rect(b).0));
    }
    if let NodeKind::Shape(s) = &n.kind {
        if let Some(f) = &s.fill {
            let _ = write!(out, "  fill:{}", paint(&f.paint));
            if !f.marks.is_empty() {
                let _ = write!(out, "  brush:{}", f.marks.len());
            }
        }
        if let Some(st) = &s.stroke {
            let _ = write!(out, "  stroke:{},{}", paint(&st.paint), num(st.width as f64));
            if let Some(r) = &st.ribbon {
                let _ = write!(out, "  brush:{}", r.marks.len());
            }
        }
        if s.blur > 0.0 {
            let _ = write!(out, "  blur:{}", num(s.blur as f64));
        }
        if !s.stamps.is_empty() {
            let _ = write!(out, "  stamps:{}", s.stamps.len());
        }
    }
    if n.opacity < 1.0 {
        let _ = write!(out, "  a:{}", num(n.opacity as f64));
    }
    if n.clip.is_some() {
        out.push_str("  clipped");
    }
    if n.mask.is_some() {
        out.push_str("  masked");
    }
    for e in &n.effects {
        let _ = write!(out, "  [fx {}]", effect(e));
    }
    if seeded.contains(&n.path) {
        out.push_str("  seed");
    }
    let _ = writeln!(out, "  ({})", n.origin);
    let world = parent * n.transform;
    if let Some(mask) = &n.mask
        && let MaskSource::Nodes(nodes) = &mask.source
    {
        for c in nodes {
            walk(c, world, depth + 1, seeded, out, false);
        }
    }
    if let NodeKind::Shape(s) = &n.kind {
        for set in s.stamps.chunk_by(|a, b| set_of(&a.path) == set_of(&b.path)) {
            fold(set, world, depth + 1, seeded, out);
        }
    }
    for c in n.descendants().skip(stamps_of(n)) {
        walk(c, world, depth + 1, seeded, out, false);
    }
}

fn stamps_of(n: &Node) -> usize {
    match &n.kind {
        NodeKind::Shape(s) => s.stamps.len(),
        NodeKind::Group { .. } => 0,
    }
}

/// `vine.leaves` of `vine.leaves.6`.
fn set_of(path: &str) -> &str {
    path.rsplit_once('.').map_or(path, |(set, _)| set)
}

/// One set of stamps. A run of three or more prints as one line with the set's bounds, then its
/// first and last stamp in full, so a 400-leaf vine stays readable.
fn fold(set: &[Node], world: Matrix, depth: usize, seeded: &BTreeSet<String>, out: &mut String) {
    if set.len() <= 2 {
        for c in set {
            walk(c, world, depth, seeded, out, true);
        }
        return;
    }
    let (first, last) = (&set[0], &set[set.len() - 1]);
    let pad = "  ".repeat(depth);
    let index = |n: &Node| n.path.rsplit_once('.').map_or("", |(_, i)| i).to_string();
    let _ = write!(out, "{pad}{}.{}..{}  stamp x{}", set_of(&first.path), index(first), index(last), set.len());
    if let Some(b) = artscript_ir::union_bounds(set) {
        let _ = write!(out, "  {}", bounds_text(world.map_rect(b).0));
    }
    let _ = writeln!(out, "  ({})", first.origin);
    walk(first, world, depth, seeded, out, true);
    walk(last, world, depth, seeded, out, true);
}

pub fn explain(scene: &Scene, seeded: &BTreeSet<String>) -> String {
    let mut out = format!("canvas {}x{}", num(scene.width as f64), num(scene.height as f64));
    if let Some(bg) = scene.background {
        let _ = write!(out, " bg:{}", bg.hex());
    }
    out.push('\n');
    for c in &scene.children {
        walk(c, Matrix::new_identity(), 0, seeded, &mut out, false);
    }
    out
}
