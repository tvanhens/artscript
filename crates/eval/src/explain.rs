//! `artscript explain`: the evaluated node tree with paths, canvas bounds and resolved paint.

use std::collections::BTreeSet;
use std::fmt::Write;

use artscript_ir::{Effect, EffectOp, Matrix, Node, NodeKind, Paint, Scene};

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
    }
}

fn effect(e: &Effect) -> String {
    match &e.op {
        EffectOp::Blur { sigma } => format!("blur {}", num(*sigma as f64)),
        EffectOp::Shadow { dx, dy, blur, color } => format!("shadow {},{} blur:{} {}", num(*dx as f64), num(*dy as f64), num(*blur as f64), color.hex()),
        EffectOp::Inner { dx, dy, blur, color } => format!("inner {},{} blur:{} {}", num(*dx as f64), num(*dy as f64), num(*blur as f64), color.hex()),
        EffectOp::Glow { radius, color } => format!("glow {} {}", num(*radius as f64), color.hex()),
        EffectOp::Rim { width, color, .. } => format!("rim {} {}", num(*width as f64), color.hex()),
        EffectOp::Adjust { hue, sat, light } => format!("adjust {} {} {}", num(*hue as f64), num(*sat as f64), num(*light as f64)),
        EffectOp::Outline { width, color } => format!("outline {} {}", num(*width as f64), color.hex()),
        EffectOp::Grain { strength, .. } => format!("grain {}", num(*strength as f64)),
        EffectOp::Merge { inputs, .. } => format!("merge ({})", inputs.len()),
    }
}

fn walk(n: &Node, parent: Matrix, depth: usize, seeded: &BTreeSet<String>, out: &mut String) {
    let pad = "  ".repeat(depth);
    let kind = match &n.kind {
        NodeKind::Group { isolate: true, .. } => "layer",
        NodeKind::Group { .. } => "group",
        NodeKind::Shape(_) => "shape",
    };
    let _ = write!(out, "{pad}{}  {kind}", n.path);
    if let Some(b) = n.bounds() {
        let b = parent.map_rect(b).0;
        let _ = write!(out, "  {},{} {}x{}", num(b.left as f64), num(b.top as f64), num(b.width() as f64), num(b.height() as f64));
    }
    if let NodeKind::Shape(s) = &n.kind {
        if let Some(f) = &s.fill {
            let _ = write!(out, "  fill:{}", paint(f));
        }
        if let Some(st) = &s.stroke {
            let _ = write!(out, "  stroke:{},{}", paint(&st.paint), num(st.width as f64));
        }
        if s.blur > 0.0 {
            let _ = write!(out, "  blur:{}", num(s.blur as f64));
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
    for c in n.children() {
        walk(c, world, depth + 1, seeded, out);
    }
}

pub fn explain(scene: &Scene, seeded: &BTreeSet<String>) -> String {
    let mut out = format!("canvas {}x{}", num(scene.width as f64), num(scene.height as f64));
    if let Some(bg) = scene.background {
        let _ = write!(out, " bg:{}", bg.hex());
    }
    out.push('\n');
    for c in &scene.children {
        walk(c, Matrix::new_identity(), 0, seeded, &mut out);
    }
    out
}
