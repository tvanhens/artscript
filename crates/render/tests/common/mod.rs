#![allow(dead_code)]

use artscript_ir::{Blend, Color, Effect, EffectInput, EffectOp, Matrix, Node, NodeKind, Paint, Path, Scene, Shape};
use skia_safe::{AlphaType, ColorType, Data, Image, ImageInfo};

pub fn scene(w: f32, h: f32, children: Vec<Node>) -> Scene {
    Scene { width: w, height: h, background: None, children }
}

pub fn solid(path: &str, geometry: Path, fill: Color) -> Node {
    Node {
        path: path.into(),
        origin: "test:1".into(),
        transform: Matrix::new_identity(),
        opacity: 1.0,
        blend: Blend::Normal,
        clip: None,
        mask: None,
        effects: vec![],
        kind: NodeKind::Shape(Box::new(Shape {
            geometry,
            fill: Some(artscript_ir::Fill::plain(Paint::Solid(fill))),
            stroke: None,
            blur: 0.0,
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

