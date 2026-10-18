mod common;

use artscript_ir::{
    Cap, Effect, EffectInput, EffectOp, Join, Mask, MaskSource, Matrix, Node, NodeKind, Paint, Path,
    Rect, Stroke,
};
use skia_safe::PathBuilder;
use artscript_render::{Backend, render_rgba};
use common::{color, effect, scene, solid};

const SIDE: f32 = 240.0;

fn square() -> Node {
    solid(
        "sq",
        Path::rect(Rect::from_xywh(100.0, 100.0, 40.0, 40.0), None),
        color(0, 0, 255, 255),
    )
}

fn with(op: EffectOp) -> Node {
    let mut n = square();
    n.effects.push(effect(op));
    n
}

fn shape_blur(sigma: f32) -> Node {
    let mut n = square();
    if let NodeKind::Shape(s) = &mut n.kind {
        s.blur = sigma;
    }
    n
}

/// Bounds of the CPU pixels whose stored alpha is nonzero.
fn inked(node: &Node) -> Rect {
    let (w, h, px) =
        render_rgba(&scene(SIDE, SIDE, vec![node.clone()]), 1.0, Backend::Cpu).unwrap();
    let mut out: Option<Rect> = None;
    for y in 0..h {
        for x in 0..w {
            if px[((y * w + x) * 4 + 3) as usize] > 0 {
                let p = Rect::from_xywh(x as f32, y as f32, 1.0, 1.0);
                out = Some(out.map_or(p, |mut r| {
                    r.join(p);
                    r
                }));
            }
        }
    }
    let ink = out.expect("something is drawn");
    assert!(
        ink.left > 0.0 && ink.top > 0.0 && ink.right < SIDE && ink.bottom < SIDE,
        "the canvas cut the ink"
    );
    ink
}

/// Asserts every inked pixel lies inside the outer extent, give or take one pixel of
/// anti-aliasing, and returns how far the extent's loosest side lies past the ink.
fn covers(name: &str, node: &Node) -> f32 {
    let outer = node
        .outer_bounds(&Matrix::new_identity(), &|_| None)
        .unwrap();
    let ink = inked(node);
    let slack = [
        ink.left - outer.left,
        ink.top - outer.top,
        outer.right - ink.right,
        outer.bottom - ink.bottom,
    ];
    assert!(
        slack.iter().all(|s| *s >= -1.0),
        "{name}: ink {ink:?} leaves the outer extent {outer:?}"
    );
    slack.into_iter().fold(f32::MIN, f32::max)
}

#[test]
fn no_blurred_pixel_lies_past_the_outer_extent() {
    for sigma in [1.0, 2.0, 3.0, 5.0, 8.0, 12.0, 20.0] {
        covers(&format!("blur {sigma}"), &with(EffectOp::Blur { sigma }));
        covers(&format!("shape blur {sigma}"), &shape_blur(sigma));
        for a in [255, 102, 26] {
            for strength in [1.0, 0.5, 4.0] {
                covers(
                    &format!("glow {sigma} a{a} strength{strength}"),
                    &with(EffectOp::Glow {
                        radius: sigma,
                        color: color(255, 255, 0, a),
                        strength,
                    }),
                );
            }
        }
        covers(
            &format!("shadow {sigma}"),
            &with(EffectOp::Shadow {
                dx: 6.0,
                dy: -9.0,
                blur: sigma,
                color: color(0, 0, 0, 255),
            }),
        );
    }
}

#[test]
fn the_outer_extent_of_a_small_blur_is_tight() {
    for sigma in [1.0, 2.0, 3.0] {
        let cases = [
            ("blur", with(EffectOp::Blur { sigma })),
            ("shape blur", shape_blur(sigma)),
            (
                "glow",
                with(EffectOp::Glow {
                    radius: sigma,
                    color: color(255, 255, 0, 255),
                    strength: 1.0,
                }),
            ),
            (
                "strong glow",
                with(EffectOp::Glow {
                    radius: sigma,
                    color: color(255, 255, 0, 255),
                    strength: 4.0,
                }),
            ),
            (
                "faint glow",
                with(EffectOp::Glow {
                    radius: sigma,
                    color: color(255, 255, 0, 26),
                    strength: 1.0,
                }),
            ),
            (
                "shadow",
                with(EffectOp::Shadow {
                    dx: 6.0,
                    dy: -9.0,
                    blur: sigma,
                    color: color(0, 0, 0, 255),
                }),
            ),
        ];
        for (name, node) in &cases {
            let loose = covers(name, node);
            assert!(
                loose <= 1.5,
                "{name} {sigma}: the extent lies {loose} px past the ink"
            );
        }
    }
    let outline = with(EffectOp::Outline {
        width: 3.0,
        color: color(0, 0, 0, 255),
        sides: artscript_ir::Sides::ALL,
    });
    let loose = covers("outline", &outline);
    assert!(
        loose <= 1.0,
        "outline: the extent lies {loose} px past the ink"
    );
    let top_only = with(EffectOp::Outline {
        width: 3.0,
        color: color(0, 0, 0, 255),
        sides: artscript_ir::Sides { top: true, right: false, bottom: false, left: false },
    });
    let loose = covers("outline on one side", &top_only);
    assert!(
        loose <= 1.0,
        "outline on one side: the extent lies {loose} px past the ink"
    );
}

#[test]
fn a_faint_glow_reaches_less_than_a_full_one() {
    let at = |a: u8| {
        with(EffectOp::Glow {
            radius: 8.0,
            color: color(255, 255, 0, a),
            strength: 1.0,
        })
        .outer_bounds(&Matrix::new_identity(), &|_| None)
        .unwrap()
    };
    assert!(at(26).left > at(255).left);
    assert_eq!(at(0), Rect::from_xywh(100.0, 100.0, 40.0, 40.0));
}

#[test]
fn an_input_node_keeps_its_whole_glow() {
    let mut glowing = solid(
        "b",
        Path::rect(Rect::from_xywh(190.0, 190.0, 20.0, 20.0), None),
        color(0, 0, 255, 255),
    );
    glowing.effects.push(effect(EffectOp::Glow {
        radius: 40.0,
        color: color(255, 255, 0, 255),
        strength: 1.0,
    }));
    let mut masked = solid(
        "m",
        Path::rect(Rect::from_xywh(195.0, 195.0, 10.0, 10.0), None),
        color(0, 0, 255, 255),
    );
    masked.mask = Some(Box::new(Mask {
        source: MaskSource::Nodes(vec![glowing]),
        luminance: false,
        feather: 0.0,
        invert: false,
    }));
    let reader = Node {
        path: "c".into(),
        kind: NodeKind::Group {
            isolate: true,
            children: vec![],
        },
        effects: vec![Effect {
            op: EffectOp::Merge {
                inputs: vec![EffectInput::Node("b".into())],
                alphas: vec![1.0],
            },
            input: EffectInput::Previous,
            name: None,
        }],
        ..solid("c", Path::new(), color(0, 0, 0, 0))
    };
    let (w, _, px) =
        render_rgba(&scene(400.0, 400.0, vec![masked, reader]), 1.0, Backend::Cpu).unwrap();
    let alpha = px[((200 * w + 280) * 4 + 3) as usize];
    assert!(alpha > 0, "the glow of @b is cut 70 units past its edge");
}

fn stroked(geometry: Path, cap: Cap, join: Join) -> Node {
    let mut n = solid("s", geometry, color(0, 0, 255, 255));
    if let NodeKind::Shape(s) = &mut n.kind {
        s.fill = None;
        s.stroke = Some(Stroke {
            paint: Paint::Solid(color(0, 0, 0, 255)),
            width: 10.0,
            cap,
            join,
            dash: vec![],
        });
    }
    n
}

#[test]
fn the_outer_extent_holds_miter_joins_and_square_caps() {
    let mut spike = PathBuilder::new();
    spike.move_to((100.0, 140.0));
    spike.line_to((120.0, 60.0));
    spike.line_to((140.0, 140.0));
    spike.close();
    let mut diagonal = PathBuilder::new();
    diagonal.move_to((90.0, 90.0));
    diagonal.line_to((150.0, 150.0));
    for (name, node) in [
        ("miter", stroked(spike.snapshot(), Cap::Butt, Join::Miter)),
        ("square cap", stroked(diagonal.snapshot(), Cap::Square, Join::Miter)),
    ] {
        covers(name, &node);
        let mut outlined = node;
        outlined.effects.push(effect(EffectOp::Outline {
            width: 4.0,
            color: color(255, 0, 0, 255),
            sides: artscript_ir::Sides::ALL,
        }));
        covers(&format!("{name} outline"), &outlined);
    }
}

