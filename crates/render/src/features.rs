//! Raster-only census. SVG bakes a node that appears here. Vector reports the same list.

use artscript_ir::{Blend, EffectInput, EffectOp, MaskSource, Node, NodeKind, Paint, Scene};

use crate::Unsupported;
use crate::effects::fill_shows;

pub fn raster_only_features(scene: &Scene) -> Vec<Unsupported> {
    let mut out = Vec::new();
    walk(&scene.children, &mut out);
    out
}

fn walk(nodes: &[Node], out: &mut Vec<Unsupported>) {
    for node in nodes {
        visit(node, out);
    }
}

fn visit(node: &Node, out: &mut Vec<Unsupported>) {
    features_of(node, out);
    if let Some(mask) = &node.mask
        && let MaskSource::Nodes(inner) = &mask.source
    {
        walk(inner, out);
    }
    for child in node.descendants() {
        visit(child, out);
    }
}

/// Whether SVG replaces this whole node with one tile.
pub fn bakes(node: &Node) -> bool {
    has(node, node_features) || (has(node, paint_features) && !splits_stamps(node))
}

/// Whether SVG bakes only this shape's own fill and stroke, and writes its stamps as elements over
/// the tile: a brushed or field-painted host whose stamps would otherwise be baked with it.
pub fn bakes_paint(node: &Node) -> bool {
    !has(node, node_features) && has(node, paint_features) && splits_stamps(node)
}

/// A host whose children clip to a brushed fill's coverage bakes whole, since an SVG clip path
/// cannot follow that coverage.
fn splits_stamps(node: &Node) -> bool {
    matches!(&node.kind, NodeKind::Shape(shape)
        if !shape.stamps.is_empty() && (shape.children.is_empty() || shape.coverage_clip().is_none()))
}

fn has(node: &Node, features: fn(&Node, &mut Vec<Unsupported>)) -> bool {
    let mut tmp = Vec::new();
    features(node, &mut tmp);
    !tmp.is_empty()
}


fn features_of(node: &Node, out: &mut Vec<Unsupported>) {
    node_features(node, out);
    paint_features(node, out);
}

/// What covers the node's whole content: its blend, mask, and effects.
fn node_features(node: &Node, out: &mut Vec<Unsupported>) {
    if node.blend == Blend::Erase {
        push(out, node, "blend erase");
    }
    if let Some(mask) = &node.mask
        && matches!(mask.source, MaskSource::Field { .. })
    {
        push(out, node, "field mask");
    }
    for effect in &node.effects {
        if let Some(name) = effect_feature(&effect.op) {
            push(out, node, name);
        } else if matches!(effect.input, EffectInput::Node(_)) {
            push(out, node, "fx node");
        }
    }
    let outlined = node.effects.iter().any(|e| matches!(e.op, EffectOp::Outline { width, .. } if width > 0.0));
    if outlined && traces_a_brushed_fill(node) {
        push(out, node, "fx outline brush");
    }
}

/// Whether the silhouette `fx outline` traces under `node` holds a brushed fill, whose ring
/// follows the brush's coverage. Walks what `effects::gather` walks.
fn traces_a_brushed_fill(node: &Node) -> bool {
    let own = match &node.kind {
        NodeKind::Shape(shape) => {
            shape.coverage_clip().is_some()
                || shape.stamps.iter().any(traces_a_brushed_fill)
                || (!fill_shows(shape) && shape.children.iter().any(traces_a_brushed_fill))
        }
        NodeKind::Group { children, .. } => children.iter().any(traces_a_brushed_fill),
    };
    own || node.mask.as_ref().is_some_and(|mask| {
        !mask.invert && matches!(&mask.source, MaskSource::Nodes(nodes) if nodes.iter().any(traces_a_brushed_fill))
    })
}

/// What only the shape's own fill and stroke need.
fn paint_features(node: &Node, out: &mut Vec<Unsupported>) {
    if let NodeKind::Shape(shape) = &node.kind {
        if let Some(fill) = &shape.fill {
            if !fill.marks.is_empty() {
                push(out, node, "brush fill");
            }
            paint_feature(node, &fill.paint, true, out);
        }
        if let Some(stroke) = &shape.stroke {
            if stroke.ribbon.is_some() {
                push(out, node, "brush stroke");
            }
            paint_feature(node, &stroke.paint, false, out);
        }
    }
}

fn paint_feature(node: &Node, paint: &Paint, fill: bool, out: &mut Vec<Unsupported>) {
    match paint {
        Paint::Field { .. } => push(out, node, if fill { "field fill" } else { "field stroke" }),
        Paint::Conic { .. } => push(out, node, if fill { "conic fill" } else { "conic stroke" }),
        _ => {}
    }
}

fn effect_feature(op: &EffectOp) -> Option<&'static str> {
    match op {
        EffectOp::Outline { sides, .. } if !sides.is_all() => Some("fx outline sides"),
        EffectOp::Inner { .. } => Some("fx inner"),
        EffectOp::Rim { .. } => Some("fx rim"),
        EffectOp::Grain { .. } => Some("fx grain"),
        EffectOp::Adjust { .. } => Some("fx adjust"),
        EffectOp::Glow { .. } => Some("fx glow"),
        EffectOp::Merge { .. } => Some("fx merge"),
        EffectOp::Blur { .. } | EffectOp::Shadow { .. } | EffectOp::Outline { .. } => None,
    }
}

fn push(out: &mut Vec<Unsupported>, node: &Node, feature: &str) {
    out.push(Unsupported {
        node: node.path.clone(),
        feature: feature.to_string(),
    });
}
