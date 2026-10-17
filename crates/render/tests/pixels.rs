mod common;

use artscript_ir::{Field, NodeKind, NoiseKind, Paint, Path, Point, Profile};
use artscript_render::{Backend, render_png, render_svg, raster_only_features, RenderError};
use common::{circle_at, color, effect, pixel, red, scene, solid};

use artscript_ir::EffectOp;

#[test]
fn red_circle_center_is_red_and_corner_is_clear() {
    let scene = scene(80.0, 80.0, vec![solid("dot", circle_at(40.0, 40.0, 20.0), red())]);
    let png = render_png(&scene, 1.0, Backend::Cpu).unwrap();
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
    let png = render_png(&scene(80.0, 80.0, vec![parent]), 1.0, Backend::Cpu).unwrap();
    assert_eq!(pixel(&png, 40, 40), [0, 0, 255, 255]);
    assert_eq!(pixel(&png, 8, 40), [0, 0, 0, 0]);
    assert_eq!(pixel(&png, 40, 8), [0, 0, 0, 0]);
}

#[test]
fn outline_ring_is_round() {
    let mut node = solid("mark", circle_at(60.0, 60.0, 20.0), color(255, 255, 255, 255));
    node.effects.push(effect(EffectOp::Outline { width: 6.0, color: color(0, 0, 0, 255) }));
    let png = render_png(&scene(140.0, 140.0, vec![node]), 1.0, Backend::Cpu).unwrap();
    assert_eq!(pixel(&png, 60, 60), [255, 255, 255, 255]);
    assert_eq!(pixel(&png, 80, 60), [0, 0, 0, 255]);
    assert_eq!(pixel(&png, 85, 60), [0, 0, 0, 244]);
    assert_eq!(pixel(&png, 86, 60), [0, 0, 0, 0]);
    assert_eq!(pixel(&png, 76, 76), [0, 0, 0, 255]);
    assert_eq!(pixel(&png, 78, 78), [0, 0, 0, 86]);
    assert_eq!(pixel(&png, 79, 79), [0, 0, 0, 0]);
    let axis = outer_radius(&png, 60.0, 60.0, 1.0, 0.0);
    let diag = outer_radius(&png, 60.0, 60.0, std::f32::consts::FRAC_1_SQRT_2, std::f32::consts::FRAC_1_SQRT_2);
    assert_eq!(axis, 25.0);
    assert_eq!(diag, 24.5);
}

#[test]
fn svg_outline_is_a_round_stroked_path() {
    let mut node = solid("mark", circle_at(60.0, 60.0, 20.0), color(255, 255, 255, 255));
    node.effects.push(effect(EffectOp::Outline { width: 6.0, color: color(0, 0, 0, 255) }));
    let scene = scene(140.0, 140.0, vec![node]);
    let svg = render_svg(&scene, Profile::Svg, 1.0, Backend::Cpu).unwrap();
    let outline_at = svg.find("stroke-linejoin=\"round\"").expect(&svg);
    let fill_at = svg.find("fill=\"#ffffff\"").expect(&svg);
    assert!(outline_at < fill_at, "{svg}");
    assert!(svg.contains("stroke-linecap=\"round\""), "{svg}");
    assert!(svg.contains("stroke-width=\"12\""), "{svg}");
    assert!(!svg.contains("<image"), "{svg}");
    assert_eq!(raster_only_features(&scene), Vec::new());
    render_svg(&scene, Profile::Vector, 1.0, Backend::Cpu).unwrap();
}

#[test]
fn outline_width_scales_with_the_canvas() {
    let mut node = solid("mark", circle_at(60.0, 60.0, 20.0), color(255, 255, 255, 255));
    node.effects.push(effect(EffectOp::Outline { width: 6.0, color: color(0, 0, 0, 255) }));
    let png = render_png(&scene(140.0, 140.0, vec![node]), 2.0, Backend::Cpu).unwrap();
    assert_eq!(pixel(&png, 120, 120), [255, 255, 255, 255]);
    assert_eq!(pixel(&png, 160, 120), [0, 0, 0, 255]);
    assert_eq!(pixel(&png, 170, 120), [0, 0, 0, 255]);
    assert_eq!(pixel(&png, 171, 120), [0, 0, 0, 249]);
    assert_eq!(pixel(&png, 172, 120), [0, 0, 0, 0]);
    let axis = outer_radius(&png, 120.0, 120.0, 1.0, 0.0);
    let diag = outer_radius(&png, 120.0, 120.0, std::f32::consts::FRAC_1_SQRT_2, std::f32::consts::FRAC_1_SQRT_2);
    assert_eq!(axis, 51.0);
    assert_eq!(diag, 51.5);
}

#[test]
fn rim_brightens_the_bottom_right_edge() {
    let mut node = solid("disc", circle_at(50.0, 50.0, 30.0), color(140, 140, 140, 255));
    node.effects.push(effect(EffectOp::Rim {
        width: 12.0,
        dir: Point::new(1.0, 1.0),
        color: color(255, 255, 255, 255),
    }));
    let png = render_png(&scene(120.0, 120.0, vec![node]), 1.0, Backend::Cpu).unwrap();
    let center = pixel(&png, 50, 50);
    let lit = pixel(&png, 68, 68);
    let shade = pixel(&png, 32, 32);
    let sum = |p: [u8; 4]| p[0] as i32 + p[1] as i32 + p[2] as i32;
    assert!(sum(lit) > sum(center) + 40, "lit {lit:?} center {center:?}");
    assert!(sum(shade) < sum(lit) - 40, "shade {shade:?} lit {lit:?}");
    assert!(lit[3] > 200 && shade[3] > 200);
}

#[test]
fn grain_on_a_flat_rect_is_not_striped() {
    let png = grain_rect(0.45, 2.2, 7);
    let buf = rgba(&png);
    let (x0, x1, y0, y1) = (24i32, 168, 24, 168);
    let mut cols = Vec::new();
    let mut rows = Vec::new();
    for x in x0..x1 {
        let mut s = 0.0;
        for y in y0..y1 {
            s += buf[idx(x, y)][0] as f64;
        }
        cols.push(s / (y1 - y0) as f64);
    }
    for y in y0..y1 {
        let mut s = 0.0;
        for x in x0..x1 {
            s += buf[idx(x, y)][0] as f64;
        }
        rows.push(s / (x1 - x0) as f64);
    }
    let ratio = variance(&cols) / variance(&rows);
    assert!(
        (0.5..2.0).contains(&ratio),
        "column-mean variance / row-mean variance {ratio} on a square grained rect"
    );
}

#[test]
fn grain_strength_is_a_fixed_overlay() {
    assert_eq!(pixel(&grain_rect(0.0, 2.0, 3), 96, 96), [128, 128, 128, 255]);
    assert_eq!(pixel(&grain_rect(0.5, 2.0, 3), 96, 96), [177, 177, 177, 255]);
    let half = pixel(&grain_rect(0.25, 2.0, 3), 80, 80);
    let full = pixel(&grain_rect(0.5, 2.0, 3), 80, 80);
    let d = |p: [u8; 4]| p[0] as i32 - 128;
    assert!((2 * d(half) - d(full)).abs() <= 1, "half {half:?} full {full:?}");
}

fn grain_rect(strength: f32, scale: f32, seed: u32) -> Vec<u8> {
    let mut node = solid(
        "slab",
        Path::rect(skia_safe::Rect::from_xywh(16.0, 16.0, 160.0, 160.0), None),
        color(128, 128, 128, 255),
    );
    node.effects.push(effect(EffectOp::Grain { strength, scale, seed }));
    render_png(&scene(192.0, 192.0, vec![node]), 1.0, Backend::Cpu).unwrap()
}

fn rgba(png: &[u8]) -> Vec<[u8; 4]> {
    let image = skia_safe::Image::from_encoded(skia_safe::Data::new_copy(png)).expect("png");
    let w = image.width();
    let h = image.height();
    let info = skia_safe::ImageInfo::new((w, h), skia_safe::ColorType::RGBA8888, skia_safe::AlphaType::Unpremul, None);
    let row = w as usize * 4;
    let mut buf = vec![0u8; row * h as usize];
    assert!(image.read_pixels(&info, &mut buf, row, (0, 0), skia_safe::image::CachingHint::Disallow));
    buf.chunks(4).map(|p| [p[0], p[1], p[2], p[3]]).collect()
}

fn idx(x: i32, y: i32) -> usize {
    y as usize * 192 + x as usize
}

fn variance(xs: &[f64]) -> f64 {
    let m = xs.iter().sum::<f64>() / xs.len() as f64;
    xs.iter().map(|v| (v - m) * (v - m)).sum::<f64>() / xs.len() as f64
}

#[test]
fn grain_changes_only_opaque_pixels() {
    let plain = solid("disc", circle_at(40.0, 40.0, 24.0), color(180, 140, 120, 255));
    let mut gritty = plain.clone();
    gritty.effects.push(effect(EffectOp::Grain { strength: 0.45, scale: 3.0, seed: 9 }));
    let a = render_png(&scene(80.0, 80.0, vec![plain]), 1.0, Backend::Cpu).unwrap();
    let b = render_png(&scene(80.0, 80.0, vec![gritty]), 1.0, Backend::Cpu).unwrap();
    assert_eq!(pixel(&a, 40, 40), [180, 140, 120, 255]);
    assert_ne!(pixel(&b, 40, 40), pixel(&a, 40, 40));
    assert_eq!(pixel(&a, 2, 2), [0, 0, 0, 0]);
    assert_eq!(pixel(&b, 2, 2), [0, 0, 0, 0]);
    assert_eq!(pixel(&b, 40, 40)[3], 255);
}

#[test]
fn adjust_keeps_soft_edges_dark() {
    let mut leaf = solid("layer.leaf", circle_at(40.0, 40.0, 24.0), color(30, 70, 40, 255));
    if let NodeKind::Shape(s) = &mut leaf.kind {
        s.blur = 3.0;
    }
    leaf.effects.push(effect(EffectOp::Outline { width: 4.0, color: color(10, 40, 25, 255) }));
    let mut layer = solid("layer", circle_at(0.0, 0.0, 1.0), color(0, 0, 0, 0));
    layer.kind = NodeKind::Group { isolate: true, children: vec![leaf] };
    layer.effects.push(effect(EffectOp::Adjust { hue: 0.0, sat: 0.05, light: -0.06 }));
    let mut canvas = scene(80.0, 80.0, vec![layer]);
    canvas.background = Some(color(40, 20, 20, 255));
    let png = render_png(&canvas, 1.0, Backend::Cpu).unwrap();
    let brightest = rgba(&png).iter().map(|p| p[0].max(p[1]).max(p[2])).max().unwrap();
    assert!(brightest < 80, "an edge pixel brightened to {brightest}");
}

#[test]
fn adjust_leaves_black_black() {
    for (sat, light) in [(-0.2, 0.0), (0.0, -0.12), (-0.35, -0.12), (0.5, 0.0)] {
        let mut node = solid("black", circle_at(40.0, 40.0, 24.0), color(0, 0, 0, 255));
        node.effects.push(effect(EffectOp::Adjust { hue: 30.0, sat, light }));
        let png = render_png(&scene(80.0, 80.0, vec![node]), 1.0, Backend::Cpu).unwrap();
        assert_eq!(pixel(&png, 40, 40), [0, 0, 0, 255], "sat {sat} light {light}");
    }
}

#[test]
fn png_bytes_are_deterministic() {
    let scene = scene(80.0, 80.0, vec![solid("dot", circle_at(40.0, 40.0, 20.0), red())]);
    let a = render_png(&scene, 1.0, Backend::Cpu).unwrap();
    let b = render_png(&scene, 1.0, Backend::Cpu).unwrap();
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
    let svg = render_svg(&scene, Profile::Svg, 1.0, Backend::Cpu).unwrap();
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

    match render_svg(&scene, Profile::Vector, 1.0, Backend::Cpu) {
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
    while t < 60.0 {
        let x = (cx + ux * t).round() as i32;
        let y = (cy + uy * t).round() as i32;
        if pixel(png, x, y)[3] > 128 {
            last = t;
        }
        t += 0.5;
    }
    last
}

#[test]
fn svg_path_data_follows_each_segment() {
    let mut b = skia_safe::PathBuilder::new();
    b.move_to((0.0, 0.0));
    b.line_to((30.0, 0.0));
    b.quad_to((30.0, 30.0), (0.0, 30.0));
    b.close();
    b.move_to((50.0, 0.0));
    b.cubic_to((60.0, -20.0), (80.0, -20.0), (90.0, 0.0));
    b.close();
    let scene = scene(100.0, 100.0, vec![solid("mark", b.detach(), red())]);
    let svg = render_svg(&scene, Profile::Svg, 1.0, Backend::Cpu).unwrap();
    assert!(svg.contains("d=\"M 0 0L 30 0C 30 20 20 30 0 30ZM 50 0C 60 -20 80 -20 90 0Z\""), "{svg}");
}

#[test]
fn svg_circle_stays_on_its_radius() {
    let scene = scene(100.0, 100.0, vec![solid("dot", circle_at(50.0, 50.0, 40.0), red())]);
    let svg = render_svg(&scene, Profile::Svg, 1.0, Backend::Cpu).unwrap();
    let d = svg.split("<path d=\"").nth(1).and_then(|s| s.split('"').next()).expect(&svg);
    let mut ends = 0;
    for segment in d.split(['M', 'C', 'Z']).filter(|s| !s.trim().is_empty()) {
        let nums: Vec<f32> = segment.split_whitespace().map(|n| n.parse().expect(d)).collect();
        let (x, y) = (nums[nums.len() - 2], nums[nums.len() - 1]);
        let r = ((x - 50.0).powi(2) + (y - 50.0).powi(2)).sqrt();
        assert!((r - 40.0).abs() < 0.01, "end {x},{y} is {r} from the center in {d}");
        ends += 1;
    }
    assert!(ends > 4, "{d}");
}
