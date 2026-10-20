#![allow(dead_code)]

use std::sync::Arc;

use ramose_brush::{MainKind, Program, UniformKind, UniformValue};
use ramose_ir::{
    AreaMark, Blend, Color, Effect, EffectInput, EffectOp, Fill, Matrix, Node, NodeKind, Paint, Path, Scene, Shape,
    Stage, Warp,
};
use skia_safe::{AlphaType, ColorType, Data, Image, ImageInfo};

pub fn scene(w: f32, h: f32, children: Vec<Node>) -> Scene {
    Scene { width: w, height: h, background: None, children }
}

pub fn solid(path: &str, geometry: Path, fill: Color) -> Node {
    Node {
        path: path.into(),
        origin: "test:1".into(),
        transform: Matrix::new_identity(),
        warp: Warp::default(),
        opacity: 1.0,
        blend: Blend::Normal,
        clip: None,
        mask: None,
        effects: vec![],
        kind: NodeKind::Shape(Box::new(Shape {
            geometry,
            fill: Some(ramose_ir::Fill::plain(Paint::Solid(fill))),
            stroke: None,
            blur: 0.0,
            stamps: vec![],
            children: vec![],
        })),
    }
}

pub fn color(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: a as f32 / 255.0,
    }
}

pub fn pixel(png: &[u8], x: i32, y: i32) -> [u8; 4] {
    let image = Image::from_encoded(Data::new_copy(png)).expect("png");
    let w = image.width();
    let h = image.height();
    let info = ImageInfo::new((w, h), ColorType::RGBA8888, AlphaType::Unpremul, None);
    let row = w as usize * 4;
    let mut buf = vec![0u8; row * h as usize];
    assert!(
        image.read_pixels(&info, &mut buf, row, (0, 0), skia_safe::image::CachingHint::Disallow),
        "read pixels"
    );
    let i = y as usize * row + x as usize * 4;
    [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
}

pub fn red() -> Color {
    color(255, 0, 0, 255)
}

pub fn circle_at(cx: f32, cy: f32, r: f32) -> Path {
    Path::circle((cx, cy), r, None)
}

pub fn effect(op: EffectOp) -> Effect {
    Effect { op, input: EffectInput::Previous, name: None }
}


/// A stamp: an isolated group placed by `transform` in its host's frame, holding `children`.
pub fn stamp(path: &str, transform: Matrix, children: Vec<Node>) -> Node {
    Node {
        path: path.into(),
        origin: "test:1".into(),
        transform,
        warp: Warp::default(),
        opacity: 1.0,
        blend: Blend::Normal,
        clip: None,
        mask: None,
        effects: vec![],
        kind: NodeKind::Group { isolate: true, children },
    }
}

/// Puts `stamps` and `children` on the shape `host`.
pub fn with_stamps(host: &mut Node, stamps: Vec<Node>, children: Vec<Node>) {
    match &mut host.kind {
        NodeKind::Shape(shape) => {
            shape.stamps = stamps;
            shape.children = children;
        }
        _ => unreachable!("the host is a shape"),
    }
}

/// One checked area-brush stage from its `shade` body.
pub fn stage(body: &str, params: &[(&str, UniformKind)], uniforms: &[(&str, UniformValue)], seed: u32) -> Stage {
    let params: Vec<(String, UniformKind)> = params.iter().map(|(n, k)| (n.to_string(), *k)).collect();
    let program = Program::assemble(MainKind::Area, &params, body);
    ramose_brush::check(&program).unwrap_or_else(|e| panic!("{e:?}"));
    Stage {
        program: Arc::new(program),
        uniforms: uniforms.iter().map(|(n, v)| (n.to_string(), *v)).collect(),
        seed,
    }
}

pub fn mark(stage: Stage, reach: f32) -> AreaMark {
    AreaMark { stage, dir_deg: 0.0, cos_sin: [1.0, 0.0], reach }
}

/// A shape whose fill is `paint` shaded by `marks`.
pub fn brushed(path: &str, geometry: Path, paint: Paint, marks: Vec<AreaMark>) -> Node {
    let mut node = solid(path, geometry, red());
    if let NodeKind::Shape(shape) = &mut node.kind {
        shape.fill = Some(Fill { paint, marks });
    }
    node
}
