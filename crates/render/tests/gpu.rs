mod common;

use ramose_ir::EffectOp;
use ramose_render::{Backend, gpu_available, render_png};
use common::{circle_at, color, effect, framed, red, scene, solid};

fn decode(png: &[u8]) -> (i32, i32, Vec<u8>) {
    let image = skia_safe::Image::from_encoded(skia_safe::Data::new_copy(png)).expect("png");
    let (w, h) = (image.width(), image.height());
    let info = skia_safe::ImageInfo::new(
        (w, h),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Unpremul,
        None,
    );
    let row = w as usize * 4;
    let mut buf = vec![0u8; row * h as usize];
    assert!(image.read_pixels(&info, &mut buf, row, (0, 0), skia_safe::image::CachingHint::Disallow));
    (w, h, buf)
}

fn layered_scene() -> ramose_ir::Scene {
    let mut ringed = solid("ringed", circle_at(50.0, 60.0, 24.0), red());
    ringed.effects.push(effect(EffectOp::Outline { width: 5.0, color: color(0, 0, 0, 255), sides: ramose_ir::Sides::ALL, mask: None, brush: vec![] }));
    let mut soft = solid("soft", circle_at(110.0, 60.0, 24.0), color(0, 90, 220, 255));
    soft.opacity = 0.6;
    let mut cut = solid("cut", circle_at(50.0, 60.0, 10.0), color(255, 255, 255, 255));
    let keep = solid("keep", skia_safe::Path::rect(skia_safe::Rect::from_xywh(0.0, 0.0, 50.0, 120.0), None), color(255, 255, 255, 255));
    let mask = ramose_ir::Mask { source: ramose_ir::MaskSource::Nodes(vec![keep]), luminance: false, feather: 3.0, invert: false };
    cut.effects.push(effect(EffectOp::Outline { width: 4.0, color: color(0, 0, 0, 255), sides: ramose_ir::Sides::ALL, mask: Some(Box::new(mask)), brush: vec![] }));
    scene(160.0, 120.0, vec![ringed, soft, cut])
}

#[test]
fn gpu_render_is_close_to_cpu_and_repeatable() {
    if !gpu_available() {
        return;
    }
    let scene = layered_scene();
    let cpu = render_png(&scene, &framed(&scene, 1.0), Backend::Cpu).unwrap();
    let gpu = render_png(&scene, &framed(&scene, 1.0), Backend::Gpu).unwrap();
    let (cw, ch, cpu_px) = decode(&cpu);
    let (gw, gh, gpu_px) = decode(&gpu);
    assert_eq!((cw, ch), (gw, gh));
    let total: u64 = cpu_px.iter().zip(&gpu_px).map(|(a, b)| a.abs_diff(*b) as u64).sum();
    let mean = total as f64 / cpu_px.len() as f64;
    assert!(mean < 2.0, "mean channel difference {mean}");
    assert_eq!(gpu, render_png(&scene, &framed(&scene, 1.0), Backend::Gpu).unwrap());
}
