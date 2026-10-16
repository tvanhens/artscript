mod common;

use artscript_ir::{Field, NoiseKind, Paint, Path, Point, Profile};
use artscript_render::{render_png, render_svg, raster_only_features, RenderError};
use common::{circle_at, color, effect, pixel, red, scene, solid};

use artscript_ir::EffectOp;

#[test]
fn red_circle_center_is_red_and_corner_is_clear() {
    let scene = scene(80.0, 80.0, vec![solid("dot", circle_at(40.0, 40.0, 20.0), red())]);
    let png = render_png(&scene, 1.0).unwrap();
    assert_eq!(pixel(&png, 40, 40), [255, 0, 0, 255]);
    assert_eq!(pixel(&png, 0, 0), [0, 0, 0, 0]);
}

#[test]
fn nested_child_never_paints_outside_parent() {
    let mut parent = solid("body", circle_at(40.0, 40.0, 20.0), red());
    let child = solid("ink", Path::rect(skia_safe::Rect::from_xywh(0.0, 0.0, 80.0, 80.0), None), color(0, 0, 255, 255));
    match &mut parent.kind {
        artscript_ir::NodeKind::Shape(shape) => shape.children.push(child),
        _ => unreachable!(),
    }
    let png = render_png(&scene(80.0, 80.0, vec![parent]), 1.0).unwrap();
    assert_eq!(pixel(&png, 40, 40), [0, 0, 255, 255]);
    assert_eq!(pixel(&png, 8, 40), [0, 0, 0, 0]);
    assert_eq!(pixel(&png, 40, 8), [0, 0, 0, 0]);
}

#[test]
fn outline_ring_is_round() {
    let mut node = solid("mark", circle_at(60.0, 60.0, 20.0), color(255, 255, 255, 255));
    node.effects.push(effect(EffectOp::Outline { width: 6.0, color: color(0, 0, 0, 255) }));
    let png = render_png(&scene(140.0, 140.0, vec![node]), 1.0).unwrap();
    assert_eq!(pixel(&png, 60, 60), [255, 255, 255, 255]);
    let axis = outer_radius(&png, 60.0, 60.0, 1.0, 0.0);
    let diag = outer_radius(&png, 60.0, 60.0, std::f32::consts::FRAC_1_SQRT_2, std::f32::consts::FRAC_1_SQRT_2);
    assert!((axis - diag).abs() <= 1.0, "axis {axis} diag {diag}");
    assert!((axis - 26.0).abs() <= 2.0, "radius {axis}");
    let ring = pixel(&png, 83, 60);
    assert!(ring[3] > 200, "ring {ring:?}");
    assert!(ring[0] < 40, "ring color {ring:?}");
    assert_eq!(pixel(&png, 110, 60), [0, 0, 0, 0]);
}

#[test]
fn outline_width_scales_with_the_canvas() {
    let mut node = solid("mark", circle_at(60.0, 60.0, 20.0), color(255, 255, 255, 255));
    node.effects.push(effect(EffectOp::Outline { width: 6.0, color: color(0, 0, 0, 255) }));
    let png = render_png(&scene(140.0, 140.0, vec![node]), 2.0).unwrap();
    assert_eq!(pixel(&png, 120, 120), [255, 255, 255, 255]);
    let axis = outer_radius(&png, 120.0, 120.0, 1.0, 0.0);
    let diag = outer_radius(&png, 120.0, 120.0, std::f32::consts::FRAC_1_SQRT_2, std::f32::consts::FRAC_1_SQRT_2);
    assert!((axis - diag).abs() <= 2.0, "axis {axis} diag {diag}");
    assert!((axis - 52.0).abs() <= 4.0, "radius {axis}");
}

#[test]
fn rim_brightens_the_bottom_right_edge() {
    let mut node = solid("disc", circle_at(50.0, 50.0, 30.0), color(140, 140, 140, 255));
    node.effects.push(effect(EffectOp::Rim {
        width: 12.0,
        dir: Point::new(1.0, 1.0),
        color: color(255, 255, 255, 255),
    }));
    let png = render_png(&scene(120.0, 120.0, vec![node]), 1.0).unwrap();
    let center = pixel(&png, 50, 50);
    let lit = pixel(&png, 68, 68);
    let shade = pixel(&png, 32, 32);
    let sum = |p: [u8; 4]| p[0] as i32 + p[1] as i32 + p[2] as i32;
    assert!(sum(lit) > sum(center) + 40, "lit {lit:?} center {center:?}");
    assert!(sum(shade) < sum(lit) - 40, "shade {shade:?} lit {lit:?}");
    assert!(lit[3] > 200 && shade[3] > 200);
}

#[test]
fn grain_changes_only_opaque_pixels() {
    let plain = solid("disc", circle_at(40.0, 40.0, 24.0), color(180, 140, 120, 255));
    let mut gritty = plain.clone();
    gritty.effects.push(effect(EffectOp::Grain { strength: 0.45, scale: 3.0, seed: 9 }));
    let a = render_png(&scene(80.0, 80.0, vec![plain]), 1.0).unwrap();
    let b = render_png(&scene(80.0, 80.0, vec![gritty]), 1.0).unwrap();
    assert_eq!(pixel(&a, 40, 40), [180, 140, 120, 255]);
    assert_ne!(pixel(&b, 40, 40), pixel(&a, 40, 40));
    assert_eq!(pixel(&a, 2, 2), [0, 0, 0, 0]);
    assert_eq!(pixel(&b, 2, 2), [0, 0, 0, 0]);
    assert_eq!(pixel(&b, 40, 40)[3], 255);
}

#[test]
fn png_bytes_are_deterministic() {
    let scene = scene(80.0, 80.0, vec![solid("dot", circle_at(40.0, 40.0, 20.0), red())]);
    let a = render_png(&scene, 1.0).unwrap();
    let b = render_png(&scene, 1.0).unwrap();
    assert_eq!(a, b);
    assert_eq!(pixel(&a, 40, 40), [255, 0, 0, 255]);
}

#[test]
fn svg_emits_gradient_and_bakes_field_fill() {
    let mut disc = solid("disc", circle_at(20.0, 20.0, 12.0), red());
    if let artscript_ir::NodeKind::Shape(shape) = &mut disc.kind {
        shape.fill = Some(Paint::Linear {
            from: Point::new(8.0, 8.0),
            to: Point::new(32.0, 32.0),
            stops: vec![
                artscript_ir::Stop { pos: 0.0, color: red() },
                artscript_ir::Stop { pos: 1.0, color: color(0, 0, 255, 255) },
            ],
        });
    }
    let rock = {
        let mut node = solid("rock", circle_at(70.0, 20.0, 10.0), red());
        if let artscript_ir::NodeKind::Shape(shape) = &mut node.kind {
            shape.fill = Some(Paint::Field {
                field: Field::Noise { kind: NoiseKind::Value, scale: 6.0, seed: 4 },
                stops: vec![
                    artscript_ir::Stop { pos: 0.0, color: color(90, 80, 70, 255) },
                    artscript_ir::Stop { pos: 1.0, color: color(210, 190, 160, 255) },
                ],
            });
        }
        node
    };
    let scene = scene(100.0, 40.0, vec![disc, rock]);
    let svg = render_svg(&scene, Profile::Svg, 1.0).unwrap();
    assert!(svg.contains("linearGradient"), "{svg}");
    assert!(svg.contains("gradientUnits=\"userSpaceOnUse\""), "{svg}");
    assert!(svg.contains("id=\"disc\""), "{svg}");
    assert!(svg.contains("<image"), "{svg}");
    assert!(svg.contains("data:image/png;base64,"), "{svg}");
    assert!(svg.contains("id=\"rock\""), "{svg}");

    let hits = raster_only_features(&scene);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].node, "rock");
    assert_eq!(hits[0].feature, "field fill");

    match render_svg(&scene, Profile::Vector, 1.0) {
        Err(RenderError::NotVector(list)) => {
            assert_eq!(list.len(), 1);
            assert_eq!(list[0].node, "rock");
            assert_eq!(list[0].feature, "field fill");
        }
        other => panic!("expected NotVector, got {other:?}"),
    }
}

fn outer_radius(png: &[u8], cx: f32, cy: f32, ux: f32, uy: f32) -> f32 {
    let mut last = 0.0;
    let mut t = 0.0;
    while t < 50.0 {
        let x = (cx + ux * t).round() as i32;
        let y = (cy + uy * t).round() as i32;
        if pixel(png, x, y)[3] > 128 {
            last = t;
        }
        t += 0.5;
    }
    last
}
