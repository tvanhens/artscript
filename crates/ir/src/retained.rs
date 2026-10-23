use std::mem::size_of;

use super::{
    AreaMark, Clip, Effect, EffectInput, EffectOp, Field, Fill, Mask, MaskSource, Node, NodeKind,
    Paint, Path, Scene, Shape, Stroke,
};

impl Scene {
    /// Bytes the scene keeps on the heap, plus the node structs those buffers own.
    /// The `Scene` value itself is not included.
    pub fn retained_bytes(&self) -> u64 {
        vec_bytes(&self.children, node_bytes)
    }
}

fn node_bytes(node: &Node) -> u64 {
    str_bytes(&node.path)
        + str_bytes(&node.origin)
        + node.clip.as_ref().map(clip_bytes).unwrap_or(0)
        + node
            .mask
            .as_ref()
            .map(|mask| box_bytes(mask.as_ref(), mask_bytes))
            .unwrap_or(0)
        + vec_bytes(&node.effects, effect_bytes)
        + match &node.kind {
            NodeKind::Group { children, .. } => vec_bytes(children, node_bytes),
            NodeKind::Shape(shape) => box_bytes(shape.as_ref(), shape_bytes),
        }
}

fn shape_bytes(shape: &Shape) -> u64 {
    path_bytes(&shape.geometry)
        + shape.fill.as_ref().map(fill_bytes).unwrap_or(0)
        + shape.stroke.as_ref().map(stroke_bytes).unwrap_or(0)
        + vec_bytes(&shape.stamps, node_bytes)
        + vec_bytes(&shape.children, node_bytes)
}

fn clip_bytes(clip: &Clip) -> u64 {
    path_bytes(&clip.path)
}

fn fill_bytes(fill: &Fill) -> u64 {
    paint_bytes(&fill.paint) + vec_bytes(&fill.marks, mark_bytes)
}

/// The shader is shared with eval's declaration through its `Arc`, so no scene owns it.
fn mark_bytes(mark: &AreaMark) -> u64 {
    vec_bytes(&mark.stage.uniforms, |_| 0)
}

fn stroke_bytes(stroke: &Stroke) -> u64 {
    let ribbon = stroke.ribbon.as_ref().map_or(0, |r| {
        vec_bytes(&r.marks, |m| vec_bytes(&m.stage.uniforms, |_| 0))
            + vec_bytes(&r.contours, |c| vec_bytes(&c.knots, |_| 0))
    });
    paint_bytes(&stroke.paint) + vec_bytes(&stroke.dash, |_| 0) + stroke.ink.as_ref().map_or(0, path_bytes) + ribbon
}

fn paint_bytes(paint: &Paint) -> u64 {
    match paint {
        Paint::Solid(_) => 0,
        Paint::Linear { stops, .. } | Paint::Radial { stops, .. } | Paint::Conic { stops, .. } => {
            vec_bytes(stops, |_| 0)
        }
        Paint::Field { field, stops } => field_bytes(field) + vec_bytes(stops, |_| 0),
    }
}

fn field_bytes(field: &Field) -> u64 {
    match field {
        Field::Noise { .. } | Field::Const(_) => 0,
        Field::Fbm { base, .. } => box_bytes(base.as_ref(), field_bytes),
        Field::Unary(_, arg) => box_bytes(arg.as_ref(), field_bytes),
        Field::Binary(_, left, right) => {
            box_bytes(left.as_ref(), field_bytes) + box_bytes(right.as_ref(), field_bytes)
        }
    }
}

fn mask_bytes(mask: &Mask) -> u64 {
    match &mask.source {
        MaskSource::Nodes(nodes) => vec_bytes(nodes, node_bytes),
        MaskSource::Field { field, .. } => field_bytes(field),
    }
}

fn effect_bytes(effect: &Effect) -> u64 {
    effect.name.as_ref().map(str_bytes).unwrap_or(0)
        + input_bytes(&effect.input)
        + match &effect.op {
            EffectOp::Blur { .. }
            | EffectOp::Shadow { .. }
            | EffectOp::Inner { .. }
            | EffectOp::Glow { .. }
            | EffectOp::Rim { .. }
            | EffectOp::Adjust { .. }
            | EffectOp::Outline { .. }
            | EffectOp::Grain { .. } => 0,
            EffectOp::Merge { inputs, alphas } => {
                vec_bytes(inputs, input_bytes) + vec_bytes(alphas, |_| 0)
            }
        }
}

fn input_bytes(input: &EffectInput) -> u64 {
    match input {
        EffectInput::Previous | EffectInput::Source => 0,
        EffectInput::Named(name) | EffectInput::Node(name) => str_bytes(name),
    }
}

fn path_bytes(path: &Path) -> u64 {
    vec_bytes(&path.verbs, |_| 0) + vec_bytes(&path.points, |_| 0) + vec_bytes(&path.weights, |_| 0)
}

fn str_bytes(s: &String) -> u64 {
    s.capacity() as u64
}

fn box_bytes<T>(value: &T, heap: impl Fn(&T) -> u64) -> u64 {
    (size_of::<T>() as u64).saturating_add(heap(value))
}

fn vec_bytes<T>(values: &Vec<T>, heap: impl Fn(&T) -> u64) -> u64 {
    (values.capacity() as u64)
        .saturating_mul(size_of::<T>() as u64)
        .saturating_add(
            values
                .iter()
                .fold(0, |acc, value| acc.saturating_add(heap(value))),
        )
}
