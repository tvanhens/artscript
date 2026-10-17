mod common;

use std::fs;
use std::path::PathBuf;

use artscript_ir::{
    Blend, Color, Effect, EffectInput, EffectOp, Field, Matrix, Node, NodeKind, NoiseKind, Paint, Path,
    Point, Scene, Shape, Stop,
};
use skia_safe::PathBuilder;
use artscript_render::{Backend, render_png};
use common::pixel;

#[test]
fn showcase_png() {
    let scene = showcase();
    let png = render_png(&scene, 2.0, Backend::Cpu).expect("showcase");
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/render-samples/showcase.png");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, &png).unwrap();
    let body = pixel(&png, 320, 360);
    assert!(body[0] > 180 && body[3] == 255, "body {body:?}");
    assert_eq!(pixel(&png, 4, 4), [0, 0, 0, 255]);
}

fn showcase() -> Scene {
    let skin_hi = color(255, 214, 186);
    let skin_lo = color(224, 112, 86);
    let ink = color(92, 42, 48);
    Scene {
        width: 360.0,
        height: 420.0,
        background: Some(color(0, 0, 0)),
        children: vec![
            ground(),
            body(skin_hi, skin_lo, ink),
            stem(),
            leaf(false),
            leaf(true),
            rock(),
        ],
    }
}

fn ground() -> Node {
    let mut node = shape(
        "ground",
        Path::oval(skia_safe::Rect::from_xywh(78.0, 298.0, 200.0, 32.0), None),
        Paint::Solid(Color { r: 0.12, g: 0.04, b: 0.06, a: 0.85 }),
    );
    if let NodeKind::Shape(s) = &mut node.kind {
        s.blur = 14.0;
    }
    node
}

fn body(hi: Color, lo: Color, ink: Color) -> Node {
    let mut node = shape(
        "peach",
        peach_path(),
        Paint::Linear {
            from: Point::new(90.0, 90.0),
            to: Point::new(250.0, 300.0),
            stops: vec![Stop { pos: 0.0, color: hi }, Stop { pos: 1.0, color: lo }],
        },
    );
    if let NodeKind::Shape(shape) = &mut node.kind {
        shape.children.push(highlight());
        shape.children.push(cel());
    }
    node.effects = vec![
        fx(EffectOp::Rim { width: 16.0, dir: Point::new(1.0, 1.0), color: Color { r: 1.0, g: 0.96, b: 0.9, a: 0.85 } }),
        fx(EffectOp::Grain { strength: 0.07, scale: 1.6, seed: 11 }),
        fx(EffectOp::Outline { width: 2.5, color: ink }),
    ];
    node
}

fn highlight() -> Node {
    let mut node = shape(
        "peach.light",
        Path::oval(skia_safe::Rect::from_xywh(108.0, 132.0, 62.0, 46.0), None),
        Paint::Solid(Color { r: 1.0, g: 0.97, b: 0.92, a: 0.7 }),
    );
    if let NodeKind::Shape(s) = &mut node.kind {
        s.blur = 12.0;
    }
    node
}

fn cel() -> Node {
    let mut node = shape(
        "peach.shade",
        Path::oval(skia_safe::Rect::from_xywh(175.0, 175.0, 140.0, 155.0), None),
        Paint::Solid(Color { r: 0.72, g: 0.38, b: 0.32, a: 0.9 }),
    );
    node.blend = Blend::Multiply;
    if let NodeKind::Shape(shape) = &mut node.kind {
        shape.blur = 14.0;
    }
    node
}

fn stem() -> Node {
    shape(
        "stem",
        Path::oval(skia_safe::Rect::from_xywh(169.0, 80.0, 12.0, 30.0), None),
        Paint::Solid(color(110, 72, 42)),
    )
}

fn leaf(right: bool) -> Node {
    let (name, rect) = if right {
        ("leaf.r", skia_safe::Rect::from_xywh(176.0, 70.0, 78.0, 28.0))
    } else {
        ("leaf.l", skia_safe::Rect::from_xywh(96.0, 78.0, 78.0, 28.0))
    };
    let mut node = shape(name, Path::oval(rect, None), Paint::Solid(color(72, 150, 78)));
    node.transform = Matrix::rotate_deg_pivot(if right { -18.0 } else { 16.0 }, (if right { 176.0 } else { 174.0 }, 92.0));
    node
}

fn rock() -> Node {
    shape(
        "rock",
        Path::oval(skia_safe::Rect::from_xywh(286.0, 300.0, 48.0, 36.0), None),
        Paint::Field {
            field: Field::Fbm {
                base: Box::new(Field::Noise { kind: NoiseKind::Worley, scale: 10.0, seed: 5 }),
                octaves: 3,
                ridged: false,
            },
            stops: vec![
                Stop { pos: 0.0, color: color(70, 64, 58) },
                Stop { pos: 0.55, color: color(150, 136, 120) },
                Stop { pos: 1.0, color: color(214, 198, 176) },
            ],
        },
    )
}

fn peach_path() -> Path {
    let mut b = PathBuilder::new();
    b.move_to((180.0, 96.0));
    b.cubic_to((250.0, 90.0), (300.0, 150.0), (292.0, 210.0));
    b.cubic_to((286.0, 270.0), (230.0, 320.0), (170.0, 308.0));
    b.cubic_to((100.0, 294.0), (58.0, 240.0), (70.0, 170.0));
    b.cubic_to((80.0, 112.0), (120.0, 86.0), (168.0, 108.0));
    b.cubic_to((172.0, 96.0), (176.0, 96.0), (180.0, 96.0));
    b.close();
    b.detach()
}

fn shape(path: &str, geometry: Path, fill: Paint) -> Node {
    Node {
        path: path.into(),
        origin: "sample:1".into(),
        transform: Matrix::new_identity(),
        opacity: 1.0,
        blend: Blend::Normal,
        clip: None,
        mask: None,
        effects: vec![],
        kind: NodeKind::Shape(Box::new(Shape {
            geometry,
            fill: Some(fill),
            stroke: None,
            blur: 0.0,
            children: vec![],
        })),
    }
}

fn fx(op: EffectOp) -> Effect {
    Effect { op, input: EffectInput::Previous, name: None }
}

fn color(r: u8, g: u8, b: u8) -> Color {
    Color { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a: 1.0 }
}
