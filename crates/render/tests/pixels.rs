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
    frame.effects.push(effect(EffectOp::Outline { width: 4.0, color: color(0, 0, 0, 255), sides: artscript_ir::Sides::ALL }));
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
    node.effects.push(effect(EffectOp::Outline { width: 6.0, color: color(0, 0, 0, 255), sides: artscript_ir::Sides::ALL }));
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
    node.effects.push(effect(EffectOp::Outline { width: 6.0, color: color(0, 0, 0, 255), sides: artscript_ir::Sides::ALL }));
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
        node.effects.push(effect(EffectOp::Outline { width: 3.0, color: color(0, 0, 0, 255), sides: artscript_ir::Sides::ALL }));
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
        group.effects.push(effect(EffectOp::Outline { width: 3.0, color: color(0, 0, 0, 255), sides: artscript_ir::Sides::ALL }));
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

fn haloed(halo: Option<EffectOp>, outline: bool) -> artscript_ir::Scene {
    let mut node = solid("disc", circle_at(32.0, 32.0, 18.0), color(79, 127, 224, 255));
    node.effects.extend(halo.map(effect));
    if outline {
        node.effects.push(effect(EffectOp::Outline { width: 4.0, color: color(12, 0, 72, 255), sides: artscript_ir::Sides::ALL }));
    }
    scene(64.0, 64.0, vec![node])
}

fn glow() -> EffectOp {
    EffectOp::Glow { radius: 10.0, color: color(232, 179, 58, 255), strength: 1.0 }
}

#[test]
fn glow_strength_multiplies_the_halo_alpha_up_to_opaque() {
    let alpha_at = |strength: f32, x: i32| {
        let halo = EffectOp::Glow { radius: 4.0, color: color(232, 179, 58, 255), strength };
        let png = render_png(&haloed(Some(halo), false), 1.0, Backend::Cpu).unwrap();
        i32::from(pixel(&png, x, 32)[3])
    };
    let faint = alpha_at(1.0, 9);
    assert!((10..60).contains(&faint), "the plain halo is faint at x=9: {faint}");
    let strong = alpha_at(4.0, 9);
    assert!((strong - 4 * faint).abs() <= 4, "strength 4 gives {strong}, want about {}", 4 * faint);
    assert_eq!(alpha_at(20.0, 11), 255, "a strong halo clamps to opaque");
    assert_eq!(alpha_at(0.0, 11), 0, "strength 0 draws no halo");
}

fn blur() -> EffectOp {
    EffectOp::Blur { sigma: 4.0 }
}

fn drop_shadow() -> EffectOp {
    EffectOp::Shadow { dx: 0.0, dy: 6.0, blur: 8.0, color: color(160, 30, 30, 128) }
}

fn outline_ring_stays_on_top_of(halo: EffectOp) {
    for scale in [1, 2] {
        let render = |s| render_png(&s, scale as f32, Backend::Cpu).unwrap();
        let at = |png: &[u8], x: i32| pixel(png, x * scale, 32 * scale);
        let plain = render(haloed(None, true));
        let ringed = render(haloed(Some(halo.clone()), true));
        let bare = render(haloed(Some(halo.clone()), false));
        let (got, want) = (at(&ringed, 12), at(&plain, 12));
        assert!(got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= 1), "ring at {scale}x is {got:?}, want {want:?}");
        assert_eq!(at(&ringed, 32), at(&bare, 32), "interior at {scale}x");
        let past = at(&bare, 6);
        assert!(past[3] > 0, "the halo reaches past the ring at {scale}x");
        assert_eq!(at(&ringed, 6), past, "past the ring at {scale}x");
    }
}

#[test]
fn outline_ring_stays_on_top_of_an_earlier_glow() {
    outline_ring_stays_on_top_of(glow());
}

#[test]
fn outline_ring_stays_on_top_of_an_earlier_blur() {
    outline_ring_stays_on_top_of(blur());
}

#[test]
fn outline_ring_stays_on_top_of_an_earlier_shadow() {
    outline_ring_stays_on_top_of(drop_shadow());
}

#[test]
fn nested_outline_rings_stay_on_top_of_an_earlier_glow() {
    let rings = |halo: bool| {
        let mut s = haloed(halo.then(glow), true);
        s.children[0].effects.push(effect(EffectOp::Outline { width: 8.0, color: color(200, 30, 30, 255), sides: artscript_ir::Sides::ALL }));
        render_png(&s, 1.0, Backend::Cpu).unwrap()
    };
    let (plain, ringed) = (rings(false), rings(true));
    let bare = render_png(&haloed(Some(glow()), false), 1.0, Backend::Cpu).unwrap();
    for x in [12, 8] {
        let (got, want) = (pixel(&ringed, x, 32), pixel(&plain, x, 32));
        assert!(got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= 1), "at {x},32 got {got:?}, want {want:?}");
    }
    assert_eq!(pixel(&ringed, 3, 32), pixel(&bare, 3, 32), "past the outer ring");
}

#[test]
fn an_earlier_effect_that_stays_inside_the_shape_keeps_the_outlined_edge() {
    let unchanged = EffectOp::Adjust { hue: 0.0, sat: 0.0, light: 0.0 };
    let plain = render_png(&haloed(None, true), 1.0, Backend::Cpu).unwrap();
    let adjusted = render_png(&haloed(Some(unchanged), true), 1.0, Backend::Cpu).unwrap();
    for x in 8..20 {
        let (got, want) = (pixel(&adjusted, x, 32), pixel(&plain, x, 32));
        assert!(got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= 1), "at {x},32 got {got:?}, want {want:?}");
    }
}

#[test]
fn svg_outline_ring_is_drawn_again_over_an_earlier_blur_or_shadow() {
    for halo in [blur(), drop_shadow()] {
        let svg = render_svg(&haloed(Some(halo), true), Profile::Svg, 1.0, Backend::Cpu).unwrap();
        let body = svg.lines().find(|l| l.contains("fill=\"#4f7fe0\"")).expect(&svg);
        let d = body.split("d=\"").nth(1).and_then(|s| s.split('"').next()).expect(&svg);
        let content_at = svg.find(body).unwrap();
        let ring_at = svg.rfind("stroke-linejoin=\"round\"").expect(&svg);
        assert!(ring_at > content_at, "{svg}");
        let filter_end = svg[content_at..].find("</g>").unwrap() + content_at;
        assert!(ring_at > filter_end, "{svg}");
        let open = "<g mask=\"url(#";
        let over = svg[..ring_at].rfind(open).expect(&svg);
        assert!(over > filter_end, "{svg}");
        let mid = svg[over + open.len()..].split(')').next().unwrap();
        let mask = svg.split(&format!("<mask id=\"{mid}\"")).nth(1).and_then(|s| s.split("</mask>").next()).expect(&svg);
        assert!(mask.contains(&format!("d=\"{d}\" fill=\"#000000\"")), "{svg}");
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
    node.effects.push(effect(EffectOp::Outline { width: 3.0, color: color(0, 0, 0, 255), sides: artscript_ir::Sides::ALL }));
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
fn svg_node_mask_with_feather_or_invert_is_well_formed() {
    for (feather, invert) in [(5.0, false), (0.0, true), (5.0, true)] {
        let mut bar = solid("bar", Path::rect(skia_safe::Rect::from_xywh(-24.0, -24.0, 48.0, 48.0), None), color(0, 0, 255, 255));
        bar.mask = Some(Box::new(artscript_ir::Mask {
            source: artscript_ir::MaskSource::Nodes(vec![solid("window", circle_at(0.0, 0.0, 20.0), red())]),
            luminance: false,
            feather,
            invert,
        }));
        let svg = render_svg(&scene(96.0, 96.0, vec![bar]), Profile::Svg, 1.0, Backend::Cpu).unwrap();
        let doc = roxmltree::Document::parse(&svg).unwrap_or_else(|e| panic!("{e}\n{svg}"));
        let group = doc.descendants().find(|n| n.has_tag_name("mask")).and_then(|m| m.children().find(|n| n.has_tag_name("g"))).expect(&svg);
        let filter = group.attribute("filter").expect(&svg);
        let id = filter.trim_start_matches("url(#").trim_end_matches(')');
        assert!(doc.descendants().any(|n| n.has_tag_name("filter") && n.attribute("id") == Some(id)), "{svg}");
    }
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
    node.effects.push(effect(EffectOp::Outline { width: 6.0, color: color(0, 0, 0, 255), sides: artscript_ir::Sides::ALL }));
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
    leaf.effects.push(effect(EffectOp::Outline { width: 4.0, color: color(10, 40, 25, 255), sides: artscript_ir::Sides::ALL }));
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
                field: Field::Noise { kind: NoiseKind::Value, scale: 6.0, seed: 4, wrap: None },
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
        bar.effects.push(effect(EffectOp::Outline { width: 3.0, color: color(0, 0, 0, 255), sides: artscript_ir::Sides::ALL }));
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
        let field = Field::Noise { kind: NoiseKind::Simplex, scale: 8.0, seed: 1, wrap: None };
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
            s.children[0].effects.push(effect(EffectOp::Outline { width: 3.0, color: color(0, 0, 0, 255), sides: artscript_ir::Sides::ALL }));
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
            field: Field::Noise { kind, scale: 16.0, seed: 1, wrap: None },
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

#[test]
fn svg_feathered_clip_with_a_node_mask_is_well_formed() {
    for (feather, invert) in [(3.0, false), (3.0, true)] {
        let mut bar = solid("bar", Path::rect(skia_safe::Rect::from_xywh(-24.0, -24.0, 48.0, 48.0), None), color(0, 0, 255, 255));
        bar.clip = Some(artscript_ir::Clip { path: circle_at(0.0, 0.0, 10.0), feather, invert });
        bar.mask = Some(Box::new(artscript_ir::Mask {
            source: artscript_ir::MaskSource::Nodes(vec![solid("window", circle_at(0.0, 0.0, 20.0), red())]),
            luminance: false,
            feather: 0.0,
            invert: false,
        }));
        let svg = render_svg(&scene(96.0, 96.0, vec![bar]), Profile::Svg, 1.0, Backend::Cpu).unwrap();
        let doc = roxmltree::Document::parse(&svg).unwrap_or_else(|e| panic!("{e}\n{svg}"));
        let bar = doc.descendants().find(|n| n.attribute("id") == Some("bar")).expect(&svg);
        assert!(bar.attribute("mask").is_some(), "{svg}");
    }
}

fn sided_tile(sides: artscript_ir::Sides, turn: f32) -> Vec<u8> {
    let mut node = solid("tile", Path::rect(skia_safe::Rect::from_xywh(30.0, 30.0, 40.0, 40.0), None), color(255, 255, 255, 255));
    node.transform = skia_safe::Matrix::rotate_deg_pivot(turn, (50.0, 50.0));
    node.effects.push(effect(EffectOp::Outline { width: 6.0, color: color(0, 0, 0, 255), sides }));
    render_png(&scene(100.0, 100.0, vec![node]), 1.0, Backend::Cpu).unwrap()
}

const TOP_ONLY: artscript_ir::Sides = artscript_ir::Sides { top: true, right: false, bottom: false, left: false };

#[test]
fn outline_on_the_top_side_leaves_the_other_three_bare() {
    let png = sided_tile(TOP_ONLY, 0.0);
    assert_eq!(pixel(&png, 50, 50), [255, 255, 255, 255]);
    assert_eq!(pixel(&png, 50, 27), [0, 0, 0, 255]);
    assert_eq!(pixel(&png, 28, 50), [0, 0, 0, 0]);
    assert_eq!(pixel(&png, 72, 50), [0, 0, 0, 0]);
    assert_eq!(pixel(&png, 50, 73), [0, 0, 0, 0]);
}

#[test]
fn a_one_sided_band_ends_square_at_the_edges_of_the_box() {
    let png = sided_tile(TOP_ONLY, 0.0);
    assert_eq!(pixel(&png, 30, 27), [0, 0, 0, 255]);
    assert_eq!(pixel(&png, 69, 27), [0, 0, 0, 255]);
    assert_eq!(pixel(&png, 29, 27), [0, 0, 0, 0]);
    assert_eq!(pixel(&png, 70, 27), [0, 0, 0, 0]);
    assert_eq!(pixel(&png, 28, 28), [0, 0, 0, 0]);
    assert_eq!(pixel(&png, 30, 24), [0, 0, 0, 255]);
    assert_eq!(pixel(&png, 30, 23), [0, 0, 0, 0]);
}

#[test]
fn two_adjacent_sides_leave_the_corner_between_them_filled() {
    let top_left = artscript_ir::Sides { top: true, right: false, bottom: false, left: true };
    let png = sided_tile(top_left, 0.0);
    assert_eq!(pixel(&png, 27, 27), [0, 0, 0, 255]);
    assert_eq!(pixel(&png, 27, 50), [0, 0, 0, 255]);
    assert_eq!(pixel(&png, 50, 27), [0, 0, 0, 255]);
    assert_eq!(pixel(&png, 72, 50), [0, 0, 0, 0]);
    assert_eq!(pixel(&png, 50, 73), [0, 0, 0, 0]);
    assert_eq!(pixel(&png, 72, 27), [0, 0, 0, 0]);
}

#[test]
fn all_sides_draws_the_full_ring() {
    let png = sided_tile(artscript_ir::Sides::ALL, 0.0);
    for (x, y) in [(50, 27), (28, 50), (72, 50), (50, 73), (27, 27)] {
        assert_eq!(pixel(&png, x, y)[3], 255, "{x},{y}");
    }
}

#[test]
fn a_rotated_node_keeps_its_sides_in_its_own_frame() {
    let png = sided_tile(TOP_ONLY, 90.0);
    assert_eq!(pixel(&png, 73, 50), [0, 0, 0, 255]);
    assert_eq!(pixel(&png, 50, 27), [0, 0, 0, 0]);
    assert_eq!(pixel(&png, 27, 50), [0, 0, 0, 0]);
    assert_eq!(pixel(&png, 50, 73), [0, 0, 0, 0]);
}

#[test]
fn svg_bakes_a_one_sided_outline_like_the_png() {
    let mut node = solid("tile", Path::rect(skia_safe::Rect::from_xywh(30.0, 30.0, 40.0, 40.0), None), color(255, 255, 255, 255));
    node.effects.push(effect(EffectOp::Outline { width: 6.0, color: color(0, 0, 0, 255), sides: TOP_ONLY }));
    let scene = scene(100.0, 100.0, vec![node]);
    assert!(raster_only_features(&scene).iter().any(|u| u.feature == "fx outline sides"));
    assert!(render_svg(&scene, Profile::Vector, 1.0, Backend::Cpu).is_err());
    let svg = render_svg(&scene, Profile::Svg, 1.0, Backend::Cpu).unwrap();
    assert!(svg.contains("<image"), "{svg}");
}

fn ramp_tile(field: Field, w: f32, h: f32) -> Vec<f64> {
    let mut node = solid("tile", Path::rect(skia_safe::Rect::from_xywh(0.0, 0.0, w, h), None), red());
    if let NodeKind::Shape(shape) = &mut node.kind {
        shape.fill = Some(Paint::Field {
            field,
            stops: vec![
                artscript_ir::Stop { pos: 0.0, color: color(0, 0, 0, 255) },
                artscript_ir::Stop { pos: 1.0, color: color(255, 255, 255, 255) },
            ],
        });
    }
    let png = render_png(&scene(w, h, vec![node]), 1.0, Backend::Cpu).unwrap();
    rgba(&png).iter().map(|p| p[0] as f64).collect()
}

struct LineSteps {
    seam: f64,
    interior: Vec<f64>,
}

/// Mean absolute red step from each line of pixels to the next along `axis`, with the step
/// from the last line back to the first set apart as the wrap seam.
fn line_steps(reds: &[f64], w: usize, h: usize, axis: char) -> LineSteps {
    let at = |x: usize, y: usize| reds[y * w + x];
    let (lines, across) = if axis == 'x' { (w, h) } else { (h, w) };
    let step = |i: usize, j: usize| match axis {
        'x' => (at(i, j) - at((i + 1) % w, j)).abs(),
        _ => (at(j, i) - at(j, (i + 1) % h)).abs(),
    };
    let mut all: Vec<f64> = (0..lines)
        .map(|i| (0..across).map(|j| step(i, j)).sum::<f64>() / across as f64)
        .collect();
    let seam = all.pop().unwrap();
    LineSteps { seam, interior: all }
}

fn assert_seam_hides(name: &str, field: Field, w: usize, h: usize) {
    let reds = ramp_tile(field, w as f32, h as f32);
    for axis in ['x', 'y'] {
        let LineSteps { seam, interior } = line_steps(&reds, w, h, axis);
        let mean = interior.iter().sum::<f64>() / interior.len() as f64;
        let max = interior.iter().cloned().fold(0.0, f64::max);
        assert!(mean > 0.5, "{name}: interior {axis} step {mean:.1} is flat");
        assert!(seam <= max * 1.1, "{name}: {axis} seam {seam:.1} against steepest interior line {max:.1}");
        assert!(seam < mean * 2.0, "{name}: {axis} seam {seam:.1} against mean interior line {mean:.1}");
    }
}
#[test]
fn a_wrapped_field_tiles_across_its_seam_like_its_interior() {
    let wrapped = |kind, scale| Field::Noise { kind, scale, seed: 1, wrap: Some([64.0, 64.0]) };
    let fields = [
        ("fbm(simplex)", Field::Fbm { base: Box::new(wrapped(NoiseKind::Simplex, 16.0)), octaves: 4, ridged: false }),
        ("fbm(perlin)", Field::Fbm { base: Box::new(wrapped(NoiseKind::Perlin, 16.0)), octaves: 4, ridged: false }),
        ("simplex", wrapped(NoiseKind::Simplex, 16.0)),
        ("perlin", wrapped(NoiseKind::Perlin, 16.0)),
        ("worley", wrapped(NoiseKind::Worley, 16.0)),
        ("value", wrapped(NoiseKind::Value, 16.0)),
        ("ridged(value) snapped from 14", Field::Fbm { base: Box::new(wrapped(NoiseKind::Value, 14.0)), octaves: 3, ridged: true }),
    ];
    for (name, field) in fields {
        assert_seam_hides(name, field, 64, 64);
    }
}

#[test]
fn a_wrapped_field_with_two_periods_tiles_on_each_axis() {
    let field = Field::Noise { kind: NoiseKind::Perlin, scale: 10.0, seed: 2, wrap: Some([64.0, 32.0]) };
    assert_seam_hides("perlin 64,32", field, 64, 32);
}

#[test]
fn a_wrapped_field_is_the_same_in_every_window_of_its_period() {
    let field = Field::Noise { kind: NoiseKind::Simplex, scale: 16.0, seed: 3, wrap: Some([64.0, 64.0]) };
    let reds = ramp_tile(field, 128.0, 128.0);
    let at = |x: usize, y: usize| reds[y * 128 + x];
    let mut off = 0;
    for y in 0..64 {
        for x in 0..64 {
            for (dx, dy) in [(64, 0), (0, 64), (64, 64)] {
                if (at(x, y) - at(x + dx, y + dy)).abs() > 2.0 {
                    off += 1;
                }
            }
        }
    }
    assert_eq!(off, 0, "{off} pixel pairs one period apart differ");
}

#[test]
fn wrapped_noise_fills_the_ramp_without_clipping_its_ends() {
    for kind in [NoiseKind::Simplex, NoiseKind::Perlin, NoiseKind::Worley, NoiseKind::Value] {
        let reds = ramp_tile(Field::Noise { kind, scale: 16.0, seed: 1, wrap: Some([192.0, 192.0]) }, 192.0, 192.0);
        let clipped = reds.iter().filter(|&&r| r == 0.0 || r == 255.0).count() as f64 / reds.len() as f64;
        let (lo, hi) = reds.iter().fold((255.0_f64, 0.0_f64), |(lo, hi), &r| (lo.min(r), hi.max(r)));
        assert!(clipped < 0.01, "{kind:?} puts {:.1}% of pixels on a ramp end", clipped * 100.0);
        assert!(lo < 20.0 && hi > 225.0, "{kind:?} spans only {lo}..{hi} of the ramp");
    }
}
