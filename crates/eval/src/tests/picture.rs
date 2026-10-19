//! `image(...)` and `sample(...)` read pictures from the project folder.

use std::path::PathBuf;

use artscript_ir::{Paint, Scene};
use skia_safe::{Color, Color4f, EncodedImageFormat, Paint as SkPaint, surfaces};

use crate::{Evaluation, Project, evaluate, plan};

use super::{diag, node, shape, show};

/// A 4x2 PNG whose left half is red and right half is blue, in a folder of its own.
fn picture_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("artscript-picture-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut surface = surfaces::raster_n32_premul((4, 2)).unwrap();
    for (x, color) in [(0, Color::RED), (2, Color::BLUE)] {
        surface.canvas().draw_irect(skia_safe::IRect::from_xywh(x, 0, 2, 2), &SkPaint::new(Color4f::from(color), None));
    }
    let png = surface.image_snapshot().encode(None, EncodedImageFormat::PNG, None).unwrap();
    std::fs::write(dir.join("two.png"), png.as_bytes()).unwrap();
    dir
}

fn eval_in(dir: PathBuf, src: &str) -> Evaluation {
    let manifest = "[project]\nout = \"out\"\n\n[lint]\nraw-color = \"off\"\nunused = \"off\"\n";
    let files = [("art/t.art".to_string(), src.to_string())].into();
    let mut p = Project::from_sources(manifest, files).unwrap();
    p.root = dir;
    let plan = plan(&p);
    evaluate(&p, &plan.jobs[0])
}

fn scene_in(dir: PathBuf, src: &str) -> Scene {
    let ev = eval_in(dir, src);
    assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
    ev.scene.unwrap()
}

#[test]
fn image_fill_stretches_over_the_bounds_and_cover_keeps_the_aspect() {
    let dir = picture_dir("fill");
    let s = scene_in(
        dir,
        "@target t.png 64x64\nrect 20x20 at:32,32 fill:image(\"two.png\") name:a\nrect 20x20 at:32,32 fill:image(\"two.png\" fit:cover) name:b\n",
    );
    let rect_of = |name: &str| match &shape(&s, name).fill.as_ref().unwrap().paint {
        Paint::Image { rect, .. } => *rect,
        other => panic!("{name} has fill {other:?}"),
    };
    let a = rect_of("a");
    assert_eq!((a.left, a.top, a.right, a.bottom), (-10.0, -10.0, 10.0, 10.0));
    let b = rect_of("b");
    assert_eq!((b.left, b.top, b.right, b.bottom), (-20.0, -10.0, 20.0, 10.0));
}

#[test]
fn sample_averages_a_box_in_linear_light() {
    let s = scene_in(
        picture_dir("sample"),
        "@target t.png 64x64\nlet red = sample(\"two.png\" 0,0 r:0)\nlet mixed = sample(\"two.png\" 2,0 r:1)\nrect 4x4 fill:red name:a\nrect 4x4 fill:mixed name:b\n",
    );
    assert_eq!(super::fill_hex(node(&s, "a")), "#ff0000");
    assert_eq!(super::fill_hex(node(&s, "b")), "#9c00d5");
}

#[test]
fn a_missing_picture_or_position_is_e0320() {
    let missing = eval_in(picture_dir("missing"), "@target t.png 8x8\nrect 4x4 fill:image(\"nope.png\")\n");
    assert!(diag(&missing, "E0320").message.contains("cannot read image \"nope.png\""));
    let outside = eval_in(picture_dir("outside"), "@target t.png 8x8\nrect 4x4 fill:sample(\"two.png\" 9,0)\n");
    assert!(diag(&outside, "E0320").message.contains("outside the 4x2 picture"));
}
