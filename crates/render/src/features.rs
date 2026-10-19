//! Raster-only census. SVG bakes a node that appears here. Vector reports the same list.

use artscript_ir::{Blend, EffectInput, EffectOp, MaskSource, Node, NodeKind, Paint, Scene};

use crate::Unsupported;

pub fn raster_only_features(scene: &Scene) -> Vec<Unsupported> {
    let mut out = Vec::new();
    walk(&scene.children, &mut out);
    out
}

fn walk(nodes: &[Node], out: &mut Vec<Unsupported>) {
    for node in nodes {
        features_of(node, out);
        if let Some(mask) = &node.mask
            && let MaskSource::Nodes(inner) = &mask.source
        {
            walk(inner, out);
        }
        walk(node.children(), out);
    }
}

pub fn bakes(node: &Node) -> bool {
    let mut tmp = Vec::new();
    features_of(node, &mut tmp);
    !tmp.is_empty()
}

fn features_of(node: &Node, out: &mut Vec<Unsupported>) {
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
