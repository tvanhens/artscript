mod common;

use common::{brushed, circle_at, color, effect, mark, red, scene, solid, stage};
use ramose_geom::{NodeGeom, ToIr, WarpGeom};
use ramose_ir::{Axis, Blend, Corners, Cylinder, EffectOp, Node, NodeKind, Paint, Profile, Sides, Warp, WarpStage};
use skia_safe::{Matrix, Path, Point, Rect};
use ramose_render::{
    Backend, RenderError, gpu_available, raster_only_features, render_png, render_svg,
};
use skia_safe::Contains;

struct Raster {
    w: i32,
    h: i32,
    px: Vec<u8>,
}

impl Raster {
    fn decode(png: &[u8]) -> Raster {
        let image = skia_safe::Image::from_encoded(skia_safe::Data::new_copy(png)).expect("png");
        let (w, h) = (image.width(), image.height());
        let info = skia_safe::ImageInfo::new(
            (w, h),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Unpremul,
            None,
        );
        let mut px = vec![0u8; (w * h * 4) as usize];
        assert!(image.read_pixels(
            &info,
            &mut px,
            (w * 4) as usize,
            (0, 0),
            skia_safe::image::CachingHint::Disallow
        ));
        Raster { w, h, px }
    }

    fn at(&self, x: i32, y: i32) -> [u8; 4] {
        let i = ((y * self.w + x) * 4) as usize;
        [self.px[i], self.px[i + 1], self.px[i + 2], self.px[i + 3]]
    }

    fn at_point(&self, p: Point) -> [u8; 4] {
        self.at(p.x.floor() as i32, p.y.floor() as i32)
    }

    fn painted_box(&self) -> Option<Rect> {
        let mut found: Option<Rect> = None;
        for y in 0..self.h {
            for x in 0..self.w {
                if self.at(x, y)[3] > 0 {
                    let here = Rect::new(x as f32, y as f32, x as f32 + 1.0, y as f32 + 1.0);
                    found = Some(found.map_or(here, |mut r| {
                        r.join(here);
                        r
                    }));
                }
            }
        }
        found
    }

    fn runs(&self, y: i32) -> Vec<(i32, i32)> {
        runs_of((0..self.w).map(|x| self.at(x, y)[3]))
    }

    fn column_runs(&self, x: i32) -> Vec<(i32, i32)> {
        runs_of((0..self.h).map(|y| self.at(x, y)[3]))
    }

    fn partial(&self, y: i32, from: i32, to: i32) -> usize {
        (from..to)
            .filter(|&x| (16..240).contains(&self.at(x, y)[3]))
            .count()
    }
}

fn runs_of(alphas: impl Iterator<Item = u8>) -> Vec<(i32, i32)> {
    let mut runs = Vec::new();
    let mut start = None;
    for (i, alpha) in alphas.map(Some).chain([None]).enumerate() {
        let i = i as i32;
        match (start, alpha.is_some_and(|a| a > 128)) {
            (None, true) => start = Some(i),
            (Some(s), false) => {
                runs.push((s, i));
                start = None;
            }
            _ => {}
        }
    }
    runs
}

fn corners(quad: [(f32, f32); 4]) -> WarpStage {
    WarpStage::Corners(Corners::try_new(quad.map(|(x, y)| Point::new(x, y).to_ir())).unwrap())
}

fn cylinder(r: f32) -> WarpStage {
    WarpStage::Cylinder(Cylinder::try_new(r, Axis::Y).unwrap())
}

fn warped(mut node: Node, stages: &[WarpStage]) -> Node {
    node.warp = Warp::resolve(stages, node.reference_box().unwrap()).unwrap();
    node
}

fn within(inner: Rect, outer: Rect, slack: f32) -> bool {
    outer.with_outset((slack, slack)).contains(inner)
}

fn same_box(a: Rect, b: Rect, slack: f32) -> bool {
    within(a, b, slack) && within(b, a, slack)
}

const QUAD: [(f32, f32); 4] = [(50.0, 20.0), (150.0, 30.0), (140.0, 170.0), (60.0, 160.0)];

fn centroid(quad: [(f32, f32); 4]) -> Point {
    let sum = quad
        .iter()
        .fold(Point::new(0.0, 0.0), |s, &(x, y)| s + Point::new(x, y));
    sum * 0.25
}

fn toward(p: Point, anchor: Point, by: f32) -> Point {
    let d = anchor - p;
    p + d * (by / d.length())
}

fn side() -> Node {
    solid(
        "side",
        Path::rect(Rect::from_xywh(0.0, 0.0, 40.0, 100.0), None),
        red(),
    )
}

#[test]
fn corners_land_the_content_box_on_the_four_points() {
    let node = warped(side(), &[corners(QUAD)]);
    let bounds = node.bounds().unwrap();
    let png = render_png(&scene(200.0, 200.0, vec![node]), 1.0, Backend::Cpu).unwrap();
    let img = Raster::decode(&png);
    let center = centroid(QUAD);
    for (i, &(x, y)) in QUAD.iter().enumerate() {
        let corner = Point::new(x, y);
        let inside = img.at_point(toward(corner, center, 3.0));
        assert_eq!(inside, [255, 0, 0, 255], "just inside corner {i}");
        let (nx, ny) = QUAD[(i + 1) % 4];
        let mid = (corner + Point::new(nx, ny)) * 0.5;
        let outside = img.at_point(toward(mid, center, -3.0));
        assert_eq!(outside[3], 0, "just outside edge {i}");
    }
    let painted = img.painted_box().unwrap();
    assert!(
        same_box(painted, bounds, 1.0),
        "painted {painted:?}, bounds {bounds:?}"
    );
}

#[test]
fn a_cylinder_compresses_stripes_toward_its_edges_and_hides_the_back() {
    let mut stripes = skia_safe::PathBuilder::new();
    for cx in [-90.0, -60.0, -30.0, 0.0, 30.0, 60.0, 90.0] {
        stripes.add_rect(Rect::new(cx - 5.0, -20.0, cx + 5.0, 20.0), None, None);
    }
    let mut node = solid("label", stripes.snapshot(), red());
    node.transform = Matrix::translate((100.0, 50.0)).to_ir();
    let node = warped(node, &[cylinder(50.0)]);
    let png = render_png(&scene(200.0, 100.0, vec![node]), 2.0, Backend::Cpu).unwrap();
    let img = Raster::decode(&png);
    let runs = img.runs(100);
    assert_eq!(runs.len(), 5, "{runs:?}");
    let width = |r: (i32, i32)| r.1 - r.0;
    let center = |r: (i32, i32)| (r.0 + r.1) as f32 / 2.0;
    assert!(
        width(runs[0]) < width(runs[2]) && width(runs[4]) < width(runs[2]),
        "{runs:?}"
    );
    assert!(
        center(runs[1]) - center(runs[0]) < center(runs[2]) - center(runs[1]),
        "{runs:?}"
    );
    assert!(
        center(runs[4]) - center(runs[3]) < center(runs[3]) - center(runs[2]),
        "{runs:?}"
    );
    let painted = img.painted_box().unwrap();
    assert!(
        painted.left >= 99.0 && painted.right <= 301.0,
        "past the radius: {painted:?}"
    );
    for run in [runs[0], runs[2], runs[4]] {
        let x = (run.0 + run.1) / 2;
        let rows = img.column_runs(x);
        assert_eq!(rows.len(), 1, "column {x}: {rows:?}");
        assert!(
            (rows[0].0 - 60).abs() <= 1 && (rows[0].1 - 140).abs() <= 1,
            "column {x} edges at {rows:?}"
        );
    }
}

#[test]
fn stage_order_matters_and_each_stack_stays_inside_its_bounds() {
    let bent_then_pinned = warped(side(), &[cylinder(100.0), corners(QUAD)]);
    let pinned_then_bent = warped(side(), &[corners(QUAD), cylinder(100.0)]);
    let rendered = [bent_then_pinned, pinned_then_bent].map(|node| {
        let bounds = node.bounds().unwrap();
        (
            render_png(&scene(200.0, 200.0, vec![node]), 1.0, Backend::Cpu).unwrap(),
            bounds,
        )
    });
    assert_ne!(rendered[0].0, rendered[1].0);
    for (png, bounds) in &rendered {
        let painted = Raster::decode(png).painted_box().unwrap();
        assert!(
            within(painted, *bounds, 1.0),
            "painted {painted:?}, bounds {bounds:?}"
        );
    }
}

#[test]
fn a_warped_node_keeps_a_crisp_edge_under_a_scaled_render() {
    let disc = || {
        let mut disc = solid("disc", circle_at(0.0, 0.0, 10.0), color(140, 140, 140, 255));
        disc.transform = Matrix::translate((48.0, 48.0)).to_ir();
        disc
    };
    let same_place = [(-10.0, -10.0), (10.0, -10.0), (10.0, 10.0), (-10.0, 10.0)];
    let cases = [
        ("plain", disc()),
        ("corners", warped(disc(), &[corners(same_place)])),
        ("cylinder", warped(disc(), &[cylinder(1000.0)])),
    ];
    let counts = cases.map(|(name, node)| {
        let png = render_png(&scene(96.0, 96.0, vec![node]), 3.0, Backend::Cpu).unwrap();
        (name, Raster::decode(&png).partial(144, 144 - 30 - 6, 144))
    });
    for (name, partial) in &counts[1..] {
        assert!(
            *partial <= counts[0].1 + 1,
            "{name} edge ramps over {partial} px, plain {}",
            counts[0].1
        );
    }
}

fn warped_scene() -> ramose_ir::Scene {
    let mut label = solid(
        "label",
        Path::rect(Rect::new(-60.0, -20.0, 60.0, 20.0), None),
        color(0, 90, 220, 255),
    );
    label.transform = Matrix::translate((100.0, 140.0)).to_ir();
    scene(
        200.0,
        200.0,
        vec![
            warped(side(), &[corners(QUAD)]),
            warped(label, &[cylinder(50.0)]),
        ],
    )
}

#[test]
fn renders_are_byte_identical_per_backend_and_close_across_them() {
    let s = warped_scene();
    let cpu = render_png(&s, 1.0, Backend::Cpu).unwrap();
    assert_eq!(cpu, render_png(&s, 1.0, Backend::Cpu).unwrap());
    if !gpu_available() {
        return;
    }
    let gpu = render_png(&s, 1.0, Backend::Gpu).unwrap();
    assert_eq!(gpu, render_png(&s, 1.0, Backend::Gpu).unwrap());
    let (a, b) = (Raster::decode(&cpu), Raster::decode(&gpu));
    assert_eq!((a.w, a.h), (b.w, b.h));
    let total: u64 =
        a.px.iter()
            .zip(&b.px)
            .map(|(x, y)| x.abs_diff(*y) as u64)
            .sum();
    let mean = total as f64 / a.px.len() as f64;
    assert!(mean < 2.0, "mean channel difference {mean}");
}

#[test]
fn a_warped_group_isolates_its_children_s_blend_modes() {
    let band = || {
        let mut gray = solid(
            "label.band",
            Path::rect(Rect::new(-30.0, -10.0, 30.0, 10.0), None),
            color(128, 128, 128, 255),
        );
        gray.blend = Blend::Multiply;
        let mut label = solid("label", Path::new(), red());
        label.kind = NodeKind::Group {
            isolate: false,
            children: vec![gray],
        };
        label.transform = Matrix::translate((100.0, 50.0)).to_ir();
        label
    };
    let floor = || {
        solid(
            "floor",
            Path::rect(Rect::from_xywh(0.0, 0.0, 200.0, 100.0), None),
            color(200, 200, 200, 255),
        )
    };
    let flat = render_png(
        &scene(200.0, 100.0, vec![floor(), band()]),
        1.0,
        Backend::Cpu,
    )
    .unwrap();
    assert_eq!(
        Raster::decode(&flat).at(100, 50),
        [100, 100, 100, 255],
        "an unwarped group lets the multiply through"
    );
    let bent = render_png(
        &scene(
            200.0,
            100.0,
            vec![floor(), warped(band(), &[cylinder(200.0)])],
        ),
        1.0,
        Backend::Cpu,
    )
    .unwrap();
    assert_eq!(Raster::decode(&bent).at(100, 50), [128, 128, 128, 255]);
}

#[test]
fn a_brushed_fill_renders_through_a_warp() {
    const IDENTITY: &str =
        "float4 shade(Mark m) { return float4(m.paint.rgb, cover(m.edge, m)); }\n";
    let square = Path::rect(Rect::new(20.0, 20.0, 60.0, 60.0), None);
    let node = brushed(
        "a",
        square,
        Paint::Solid(red()),
        vec![mark(stage(IDENTITY, &[], &[], 1), 0.0)],
    );
    let node = warped(
        node,
        &[corners([
            (80.0, 20.0),
            (160.0, 30.0),
            (150.0, 90.0),
            (90.0, 80.0),
        ])],
    );
    let center = node.warp.map_point(Point::new(40.0, 40.0)).unwrap();
    let png = render_png(&scene(200.0, 100.0, vec![node]), 1.0, Backend::Cpu).unwrap();
    let img = Raster::decode(&png);
    assert_eq!(img.at_point(center), [255, 0, 0, 255]);
    assert_eq!(
        img.at(40, 40)[3],
        0,
        "nothing lands where the flat square was"
    );
}

#[test]
fn fx_outline_rings_the_warped_quad_and_not_the_flat_rect() {
    let mut node = warped(side(), &[corners(QUAD)]);
    node.effects.push(effect(EffectOp::Outline {
        width: 4.0,
        color: color(0, 0, 0, 255),
        sides: Sides::ALL,
        mask: None,
        brush: vec![],
    }));
    let png = render_png(&scene(200.0, 200.0, vec![node]), 1.0, Backend::Cpu).unwrap();
    let img = Raster::decode(&png);
    let center = centroid(QUAD);
    for i in 0..4 {
        let (ax, ay) = QUAD[i];
        let (bx, by) = QUAD[(i + 1) % 4];
        let mid = Point::new((ax + bx) * 0.5, (ay + by) * 0.5);
        let ring = img.at_point(toward(mid, center, -2.0));
        assert!(
            ring[3] > 128 && ring[0] < 40 && ring[1] < 40 && ring[2] < 40,
            "no ring just outside edge {i}: {ring:?}"
        );
    }
    assert_eq!(img.at(20, 50)[3], 0, "the flat rect's place stays clear");
    assert_eq!(img.at(43, 50)[3], 0, "no ring around the flat rect's edge");
}

fn image_box(svg: &str, index: usize) -> Rect {
    let attrs = &svg[svg.match_indices("<image").nth(index).unwrap().0..];
    let num = |name: &str| -> f32 {
        let key = format!(" {name}=\"");
        let start = attrs.find(&key).unwrap() + key.len();
        attrs[start..].split('"').next().unwrap().parse().unwrap()
    };
    Rect::from_xywh(num("x"), num("y"), num("width"), num("height"))
}

#[test]
fn both_kinds_are_raster_only_and_svg_bakes_each_node_to_its_warped_box() {
    let s = warped_scene();
    let census: Vec<String> = raster_only_features(&s)
        .iter()
        .map(|u| format!("{} {}", u.node, u.feature))
        .collect();
    assert_eq!(census, ["side warp corners", "label warp cylinder"]);
    assert!(matches!(
        render_svg(&s, Profile::Vector, 1.0, Backend::Cpu),
        Err(RenderError::NotVector(_))
    ));
    let svg = render_svg(&s, Profile::Svg, 1.0, Backend::Cpu).unwrap();
    assert_eq!(svg.matches("<image").count(), 2, "{svg}");
    for (i, node) in s.children.iter().enumerate() {
        let (tile, bounds) = (image_box(&svg, i), node.bounds().unwrap());
        assert!(
            same_box(tile, bounds, 0.01),
            "{}: tile {tile:?}, bounds {bounds:?}",
            node.path
        );
    }
}
