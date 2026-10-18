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
fn a_fill_none_parent_still_clips_and_shows_its_children() {
    let mut frame = solid("frame", Path::rect(skia_safe::Rect::from_xywh(12.0, 12.0, 40.0, 40.0), None), red());
    let child = solid("dot", circle_at(32.0, 32.0, 30.0), color(0, 0, 255, 255));
    match &mut frame.kind {
        NodeKind::Shape(shape) => {
            shape.fill = None;
            shape.children.push(child);
        }
        _ => unreachable!(),
    }
    let png = render_png(&scene(64.0, 64.0, vec![frame]), 1.0, Backend::Cpu).unwrap();
    assert_eq!(pixel(&png, 32, 32), [0, 0, 255, 255]);
    assert_eq!(pixel(&png, 14, 32), [0, 0, 255, 255]);
    assert_eq!(pixel(&png, 32, 6), [0, 0, 0, 0]);
    assert_eq!(pixel(&png, 6, 32), [0, 0, 0, 0]);
}

#[test]
fn an_opaque_child_of_a_translucent_parent_stays_opaque() {
    let mut glass = solid("glass", Path::rect(skia_safe::Rect::from_xywh(12.0, 12.0, 40.0, 40.0), None), color(0, 0, 255, 77));
    let dot = solid("dot", circle_at(32.0, 32.0, 6.0), color(0, 0, 0, 255));
    match &mut glass.kind {
        NodeKind::Shape(shape) => shape.children.push(dot),
        _ => unreachable!(),
    }
    let png = render_png(&scene(64.0, 64.0, vec![glass]), 1.0, Backend::Cpu).unwrap();
    assert_eq!(pixel(&png, 32, 32), [0, 0, 0, 255]);
    assert_eq!(pixel(&png, 16, 16), [0, 0, 255, 77]);
}

#[test]
fn outline_traces_the_children_a_fill_none_parent_shows() {
    let mut frame = solid("frame", Path::rect(skia_safe::Rect::from_xywh(20.0, 20.0, 40.0, 40.0), None), red());
    let child = solid("dot", circle_at(40.0, 40.0, 30.0), color(0, 0, 255, 255));
    match &mut frame.kind {
        NodeKind::Shape(shape) => {
            shape.fill = None;
            shape.children.push(child);
        }
        _ => unreachable!(),
    }
    frame.effects.push(effect(EffectOp::Outline { width: 4.0, color: color(0, 0, 0, 255) }));
    let png = render_png(&scene(80.0, 80.0, vec![frame]), 1.0, Backend::Cpu).unwrap();
    assert_eq!(pixel(&png, 40, 40), [0, 0, 255, 255]);
    assert_eq!(pixel(&png, 18, 40), [0, 0, 0, 255]);
    assert_eq!(pixel(&png, 12, 40), [0, 0, 0, 0]);
}

#[test]
fn svg_isolates_a_shape_with_children() {
    let mut parent = solid("body", circle_at(40.0, 40.0, 20.0), red());
    let child = solid("ink", circle_at(40.0, 40.0, 8.0), color(0, 0, 255, 255));
    match &mut parent.kind {
        NodeKind::Shape(shape) => shape.children.push(child),
        _ => unreachable!(),
    }
    let svg = render_svg(&scene(80.0, 80.0, vec![parent]), Profile::Svg, 1.0, Backend::Cpu).unwrap();
    assert!(svg.contains("<g id=\"body\" style=\"isolation:isolate\""), "{svg}");
}

#[test]
fn svg_nesting_clip_keeps_an_even_odd_parent_hole() {
    let mut ring = skia_safe::PathBuilder::new();
    ring.add_circle((40.0, 40.0), 20.0, None);
    ring.add_circle((40.0, 40.0), 8.0, None);
    ring.set_fill_type(skia_safe::PathFillType::EvenOdd);
    let mut parent = solid("ring", ring.detach(), red());
    let child = solid("ink", circle_at(40.0, 40.0, 30.0), color(0, 0, 255, 255));
    match &mut parent.kind {
        NodeKind::Shape(shape) => shape.children.push(child),
        _ => unreachable!(),
    }
    let svg = render_svg(&scene(80.0, 80.0, vec![parent]), Profile::Svg, 1.0, Backend::Cpu).unwrap();
    let clip = svg.split("<clipPath id=\"ring-nest\">").nth(1).unwrap().split("</clipPath>").next().unwrap();
    assert!(clip.contains("clip-rule=\"evenodd\""), "{svg}");
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

fn glass(outline: bool) -> artscript_ir::Scene {
    let rect = Path::rect(skia_safe::Rect::from_xywh(12.0, 12.0, 40.0, 40.0), None);
    let mut node = solid("glass", rect, color(0, 0, 255, 128));
    if outline {
        node.effects.push(effect(EffectOp::Outline { width: 3.0, color: color(0, 0, 0, 255) }));
    }
    scene(64.0, 64.0, vec![node])
}

#[test]
fn outline_ring_keeps_its_alpha_around_a_translucent_fill() {
    let png = render_png(&glass(true), 1.0, Backend::Cpu).unwrap();
    assert_eq!(pixel(&png, 10, 32), [0, 0, 0, 255]);
    assert_eq!(pixel(&png, 32, 54), [0, 0, 0, 255]);
}

#[test]
fn outline_shows_nothing_inside_a_translucent_fill() {
    let outlined = render_png(&glass(true), 1.0, Backend::Cpu).unwrap();
    let bare = render_png(&glass(false), 1.0, Backend::Cpu).unwrap();
    for (x, y) in [(32, 32), (13, 32), (50, 50)] {
        assert_eq!(pixel(&outlined, x, y), pixel(&bare, x, y), "at {x},{y}");
    }
}

fn translucent_group(outline: bool) -> artscript_ir::Scene {
    let tinted = solid("tinted", circle_at(24.0, 32.0, 10.0), color(0, 0, 255, 128));
    let mut faded = solid("faded", circle_at(56.0, 32.0, 10.0), color(255, 0, 0, 255));
    faded.opacity = 0.5;
    let mut group = solid("pair", circle_at(0.0, 0.0, 1.0), red());
    group.kind = NodeKind::Group { isolate: false, children: vec![tinted, faded] };
    if outline {
        group.effects.push(effect(EffectOp::Outline { width: 3.0, color: color(0, 0, 0, 255) }));
    }
    scene(80.0, 64.0, vec![group])
}

#[test]
fn outline_around_translucent_children_keeps_its_alpha() {
    let outlined = render_png(&translucent_group(true), 1.0, Backend::Cpu).unwrap();
    let bare = render_png(&translucent_group(false), 1.0, Backend::Cpu).unwrap();
    assert_eq!(pixel(&outlined, 12, 32), [0, 0, 0, 255]);
    assert_eq!(pixel(&outlined, 67, 32), [0, 0, 0, 255]);
    for (x, y) in [(24, 32), (56, 32)] {
        assert_eq!(pixel(&outlined, x, y), pixel(&bare, x, y), "at {x},{y}");
    }
}

#[test]
fn svg_outline_on_a_translucent_fill_is_not_faded() {
    let svg = render_svg(&glass(true), Profile::Svg, 1.0, Backend::Cpu).unwrap();
    let ink = svg.lines().find(|l| l.contains("stroke-linejoin=\"round\"")).expect(&svg);
    assert!(ink.contains("fill=\"#000000\""), "{svg}");
    assert!(!ink.contains("opacity"), "{svg}");
    let body = svg.lines().find(|l| l.contains("fill=\"#0000ff\"")).expect(&svg);
    let d = body.split("d=\"").nth(1).and_then(|s| s.split('"').next()).expect(&svg);
    let mask = svg.split("<mask").nth(1).and_then(|s| s.split("</mask>").next()).expect(&svg);
    assert!(mask.contains(&format!("d=\"{d}\" fill=\"#000000\"")), "{svg}");
}

#[test]
fn svg_outline_mask_covers_a_ring_far_from_the_origin() {
    let rect = Path::rect(skia_safe::Rect::from_xywh(20000.0, -30000.0, 40.0, 40.0), None);
    let mut node = solid("far", rect, color(0, 0, 255, 128));
    node.effects.push(effect(EffectOp::Outline { width: 3.0, color: color(0, 0, 0, 255) }));
    let svg = render_svg(&scene(64.0, 64.0, vec![node]), Profile::Svg, 1.0, Backend::Cpu).unwrap();
    let mask = svg.split("<mask").nth(1).and_then(|s| s.split('>').next()).expect(&svg);
    let attr = |name: &str| -> f32 {
        let key = format!(" {name}=\"");
        mask.split(&key).nth(1).and_then(|s| s.split('"').next()).and_then(|v| v.parse().ok()).expect(&svg)
    };
    let (x, y, w, h) = (attr("x"), attr("y"), attr("width"), attr("height"));
    assert!(x <= 19997.0 && x + w >= 20043.0, "{svg}");
    assert!(y <= -30003.0 && y + h >= -29957.0, "{svg}");
}

fn assert_mask_region_covers(svg: &str, left: f32, top: f32, right: f32, bottom: f32) {
    let masks: Vec<&str> = svg.split("<mask").skip(1).filter_map(|s| s.split('>').next()).collect();
    assert!(!masks.is_empty(), "{svg}");
    for mask in masks {
        let attr = |name: &str| -> f32 {
            let key = format!(" {name}=\"");
            mask.split(&key).nth(1).and_then(|s| s.split('"').next()).and_then(|v| v.parse().ok()).expect(svg)
        };
        let (x, y, w, h) = (attr("x"), attr("y"), attr("width"), attr("height"));
        assert!(x <= left && x + w >= right && y <= top && y + h >= bottom, "{mask}");
    }
}

#[test]
fn svg_node_mask_region_covers_a_shape_placed_away_from_the_origin() {
    let window = solid("window", circle_at(0.0, 0.0, 20.0), red());
    let mut bar = solid("bar", Path::rect(skia_safe::Rect::from_xywh(-24.0, -24.0, 48.0, 48.0), None), color(0, 0, 255, 255));
    bar.transform = skia_safe::Matrix::translate((48.0, 48.0));
    bar.mask = Some(Box::new(artscript_ir::Mask {
        source: artscript_ir::MaskSource::Nodes(vec![window]),
        luminance: false,
        feather: 0.0,
        invert: false,
    }));
    let svg = render_svg(&scene(96.0, 96.0, vec![bar]), Profile::Svg, 1.0, Backend::Cpu).unwrap();
    assert_mask_region_covers(&svg, -20.0, -20.0, 20.0, 20.0);
}

#[test]
fn svg_outside_clip_region_covers_a_shape_placed_away_from_the_origin() {
    let mut bar = solid("bar", Path::rect(skia_safe::Rect::from_xywh(-24.0, -24.0, 48.0, 48.0), None), color(0, 0, 255, 255));
    bar.transform = skia_safe::Matrix::translate((48.0, 48.0));
    bar.clip = Some(artscript_ir::Clip { path: circle_at(0.0, 0.0, 10.0), feather: 0.0, invert: true });
    let svg = render_svg(&scene(96.0, 96.0, vec![bar]), Profile::Svg, 1.0, Backend::Cpu).unwrap();
    assert_mask_region_covers(&svg, -24.0, -24.0, 24.0, 24.0);
}

#[test]
fn svg_mask_region_covers_a_mitered_stroke_tip() {
    let mut b = skia_safe::PathBuilder::new();
    b.move_to((0.0, -30.0));
    b.line_to((12.0, 10.0));
    b.line_to((-12.0, 10.0));
    b.close();
    let mut node = solid("tip", b.detach(), color(0, 0, 255, 255));
    node.clip = Some(artscript_ir::Clip { path: circle_at(0.0, 0.0, 2.0), feather: 0.0, invert: true });
    match &mut node.kind {
        NodeKind::Shape(shape) => {
            shape.stroke = Some(artscript_ir::Stroke {
                paint: Paint::Solid(red()),
                width: 8.0,
                cap: artscript_ir::Cap::Butt,
                join: artscript_ir::Join::Miter,
                dash: vec![],
            })
        }
        _ => unreachable!(),
    }
    let svg = render_svg(&scene(96.0, 96.0, vec![node]), Profile::Svg, 1.0, Backend::Cpu).unwrap();
    let miter = 4.0 / (12.0_f32 / 40.0).atan().sin();
    assert_mask_region_covers(&svg, -12.0, -30.0 - miter, 12.0, 10.0);
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

#[test]
fn a_baked_svg_tile_keeps_a_blurred_mask_fringe() {
    let mut edge = solid("m", Path::rect(skia_safe::Rect::from_xywh(22.0, 22.0, 20.0, 20.0), None), red());
    if let NodeKind::Shape(shape) = &mut edge.kind {
        shape.blur = 4.0;
    }
    let mut cell = solid("bar", Path::rect(skia_safe::Rect::from_xywh(12.0, 2.0, 40.0, 60.0), None), red());
    cell.effects.push(effect(EffectOp::Grain { strength: 0.05, scale: 1.0, seed: 1 }));
    cell.mask = Some(Box::new(artscript_ir::Mask {
        source: artscript_ir::MaskSource::Nodes(vec![edge]),
        luminance: false,
        feather: 0.0,
        invert: false,
    }));
    let svg = render_svg(&scene(64.0, 64.0, vec![cell]), Profile::Svg, 1.0, Backend::Cpu).unwrap();
    assert!(svg.contains("<image x=\"10\" y=\"10\" width=\"44\" height=\"44\""), "{svg}");
}

fn masked_bar(mask: artscript_ir::MaskSource, outline: bool) -> artscript_ir::Scene {
    let mut bar = solid("bar", Path::rect(skia_safe::Rect::from_xywh(10.0, 30.0, 60.0, 20.0), None), color(255, 255, 255, 255));
    bar.mask = Some(Box::new(artscript_ir::Mask { source: mask, luminance: false, feather: 0.0, invert: false }));
    if outline {
        bar.effects.push(effect(EffectOp::Outline { width: 3.0, color: color(0, 0, 0, 255) }));
    }
    scene(80.0, 80.0, vec![bar])
}

#[test]
fn outline_traces_the_shape_a_node_mask_keeps() {
    let window = solid("window", Path::rect(skia_safe::Rect::from_xywh(30.0, 20.0, 20.0, 40.0), None), red());
    let png = render_png(&masked_bar(artscript_ir::MaskSource::Nodes(vec![window]), true), 1.0, Backend::Cpu).unwrap();
    assert_eq!(pixel(&png, 40, 40), [255, 255, 255, 255]);
    assert_eq!(pixel(&png, 28, 40), [0, 0, 0, 255]);
    assert_eq!(pixel(&png, 40, 28), [0, 0, 0, 255]);
    assert_eq!(pixel(&png, 20, 40), [0, 0, 0, 0]);
    assert_eq!(pixel(&png, 60, 40), [0, 0, 0, 0]);
}

#[test]
fn outline_shows_nothing_through_a_field_mask() {
    let half = || artscript_ir::MaskSource::Field { field: Field::Const(0.5), threshold: None };
    let outlined = render_png(&masked_bar(half(), true), 1.0, Backend::Cpu).unwrap();
    let bare = render_png(&masked_bar(half(), false), 1.0, Backend::Cpu).unwrap();
    for (x, y) in [(40, 40), (12, 32), (68, 48)] {
        assert_eq!(pixel(&outlined, x, y), pixel(&bare, x, y), "at {x},{y}");
    }
    assert_eq!(pixel(&outlined, 8, 40), [0, 0, 0, 255]);
}

fn outline_hugs_what_a_thresholded_field_keeps(invert: bool) {
    let blobs = |outline: bool| {
        let field = Field::Noise { kind: NoiseKind::Simplex, scale: 8.0, seed: 1 };
        let mut s = masked_bar(artscript_ir::MaskSource::Field { field, threshold: Some(0.5) }, outline);
        s.children[0].mask.as_mut().unwrap().invert = invert;
        s
    };
    let outlined = render_png(&blobs(true), 1.0, Backend::Cpu).unwrap();
    let bare = render_png(&blobs(false), 1.0, Backend::Cpu).unwrap();
    let px = |png: &[u8]| (0..80).flat_map(|y| (0..80).map(move |x| (x, y))).map(|(x, y)| ((x, y), pixel(png, x, y))).collect::<Vec<_>>();
    let (outlined, bare) = (px(&outlined), px(&bare));
    let kept: Vec<(i32, i32)> = bare.iter().filter(|(_, p)| p[3] > 0).map(|(at, _)| *at).collect();
    assert!(!kept.is_empty() && kept.len() < 60 * 20, "the field keeps some of the bar, {} px", kept.len());
    let (mut near, mut far) = (0, 0);
    for (((x, y), got), (_, under)) in outlined.iter().zip(&bare) {
        let gap = kept.iter().map(|(kx, ky)| (((kx - x).pow(2) + (ky - y).pow(2)) as f32).sqrt()).fold(f32::MAX, f32::min);
        if gap == 0.0 {
            assert_eq!(got, under, "kept pixel {x},{y} shows the bar");
        } else if gap <= 2.0 {
            near += 1;
            assert_eq!(*got, [0, 0, 0, 255], "pixel {x},{y} is {gap} px from a kept pixel and is ink");
        } else if gap > 4.5 {
            far += 1;
            assert_eq!(*got, [0, 0, 0, 0], "pixel {x},{y} is {gap} px from every kept pixel and is clear");
        }
    }
    assert!(near > 0 && far > 0, "near {near} far {far}");
}

#[test]
fn outline_hugs_what_a_thresholded_field_mask_keeps() {
    outline_hugs_what_a_thresholded_field_keeps(false);
}

#[test]
fn outline_hugs_what_an_inverted_thresholded_field_mask_keeps() {
    outline_hugs_what_a_thresholded_field_keeps(true);
}

#[test]
fn outline_shows_nothing_through_a_feathered_mask_edge() {
    let window = || solid("window", Path::rect(skia_safe::Rect::from_xywh(30.0, 20.0, 20.0, 40.0), None), red());
    let feathered = |outline: bool| {
        let mut s = masked_bar(artscript_ir::MaskSource::Nodes(vec![window()]), outline);
        s.children[0].mask.as_mut().unwrap().feather = 3.0;
        s
    };
    let outlined = render_png(&feathered(true), 1.0, Backend::Cpu).unwrap();
    let bare = render_png(&feathered(false), 1.0, Backend::Cpu).unwrap();
    for (x, y) in [(31, 40), (49, 40), (40, 31)] {
        assert_eq!(pixel(&outlined, x, y), pixel(&bare, x, y), "at {x},{y}");
    }
}

#[test]
fn outline_shows_nothing_through_a_feathered_clip_edge() {
    let clipped = |outline: bool| {
        let mut s = scene(80.0, 80.0, vec![solid("bar", Path::rect(skia_safe::Rect::from_xywh(10.0, 30.0, 60.0, 20.0), None), color(255, 255, 255, 255))]);
        s.children[0].clip = Some(artscript_ir::Clip {
            path: Path::rect(skia_safe::Rect::from_xywh(30.0, 20.0, 20.0, 40.0), None),
            feather: 3.0,
            invert: false,
        });
        if outline {
            s.children[0].effects.push(effect(EffectOp::Outline { width: 3.0, color: color(0, 0, 0, 255) }));
        }
        s
    };
    let outlined = render_png(&clipped(true), 1.0, Backend::Cpu).unwrap();
    let bare = render_png(&clipped(false), 1.0, Backend::Cpu).unwrap();
    for (x, y) in [(31, 40), (49, 40)] {
        assert_eq!(pixel(&outlined, x, y), pixel(&bare, x, y), "at {x},{y}");
    }
}

fn noise_ramp(kind: NoiseKind) -> Vec<u8> {
    let mut node = solid("noise", Path::rect(skia_safe::Rect::from_xywh(0.0, 0.0, 192.0, 192.0), None), red());
    if let NodeKind::Shape(shape) = &mut node.kind {
        shape.fill = Some(Paint::Field {
            field: Field::Noise { kind, scale: 16.0, seed: 1 },
            stops: vec![
                artscript_ir::Stop { pos: 0.0, color: color(0, 0, 0, 255) },
                artscript_ir::Stop { pos: 1.0, color: color(255, 255, 255, 255) },
            ],
        });
    }
    render_png(&scene(192.0, 192.0, vec![node]), 1.0, Backend::Cpu).unwrap()
}

#[test]
fn noise_fills_the_ramp_without_clipping_its_ends() {
    for kind in [NoiseKind::Simplex, NoiseKind::Perlin, NoiseKind::Worley, NoiseKind::Value] {
        let reds: Vec<u8> = rgba(&noise_ramp(kind)).iter().map(|p| p[0]).collect();
        let clipped = reds.iter().filter(|&&r| r == 0 || r == 255).count() as f64 / reds.len() as f64;
        let (lo, hi) = (*reds.iter().min().unwrap(), *reds.iter().max().unwrap());
        assert!(clipped < 0.01, "{kind:?} puts {:.1}% of pixels on a ramp end", clipped * 100.0);
        assert!(lo < 20 && hi > 225, "{kind:?} spans only {lo}..{hi} of the ramp");
    }
}
