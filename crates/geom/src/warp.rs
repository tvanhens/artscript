//! A resolved [`Warp`] mapped through Skia. Every function on a stage is one `match` into the
//! kind's own function, so a new kind is one `WarpStage` variant plus one block of functions here.

use std::f32::consts::FRAC_PI_2;

use ramose_ir::warp::Stage;
use ramose_ir::{Axis, Corners, Cylinder, Warp, WarpStage};
use skia_safe::matrix::Member;
use skia_safe::path::Verb;
use skia_safe::{Contains, Matrix, Path, PathBuilder, Point, Point3, Rect};

use crate::{ToIr, ToSk};

/// How a composed stack draws a flat image.
#[derive(Clone, Copy, Debug)]
pub enum Placer {
    Matrix(Matrix),
    Grid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WarpError {
    /// An earlier cylinder hid everything, so this stage's `src` is empty.
    Empty(usize),
}

/// Maps through a [`Warp`]. Every method is the identity on an empty warp.
pub trait WarpGeom: Sized {
    /// Binds `stages` to `reference`, threading each stage's `map_rect` of its `src` into the
    /// next `src`.
    fn resolve(stages: &[WarpStage], reference: Rect) -> Result<Self, WarpError>;
    fn map_rect(&self, r: Rect) -> Rect;
    fn map_point(&self, p: Point) -> Option<Point>;
    fn map_path(&self, path: &Path, tol: f32) -> Path;
    /// The part of `within` whose corners map through every stage: stage 0's own cut, then
    /// shrunk toward the reference while a later stage still drops a corner. When a later stage
    /// hides part of the reference itself, the result is stage 0's cut.
    fn visible(&self, within: Rect) -> Rect;
    /// The product of each stage's largest magnification over the image of `within` so far.
    fn max_stretch(&self, within: Rect) -> f32;
    /// [`Placer::Matrix`] when every stage is a matrix, multiplied in application order; else
    /// [`Placer::Grid`].
    fn placer(&self) -> Placer;
    /// (stage index, dropped rect) for every strip a stage cuts.
    fn cuts(&self) -> Vec<(usize, Rect)>;
}

impl WarpGeom for Warp {
    fn resolve(stages: &[WarpStage], reference: Rect) -> Result<Warp, WarpError> {
        let mut src = reference;
        let mut out = Vec::with_capacity(stages.len());
        for (i, &kind) in stages.iter().enumerate() {
            if src.is_empty() {
                return Err(WarpError::Empty(i));
            }
            let stage = Stage { kind, src: src.to_ir() };
            src = stage_map_rect(&stage, src);
            out.push(stage);
        }
        Ok(Warp::from_stages(out))
    }

    fn map_rect(&self, r: Rect) -> Rect {
        self.stages().iter().fold(r, |r, s| stage_map_rect(s, r))
    }

    fn map_point(&self, p: Point) -> Option<Point> {
        self.stages().iter().try_fold(p, |p, s| stage_map_point(s, p))
    }

    fn map_path(&self, path: &Path, tol: f32) -> Path {
        match self.stages().split_first() {
            None => path.clone(),
            Some((first, rest)) => rest
                .iter()
                .fold(stage_map_path(first, path, tol), |p, s| stage_map_path(s, &p, tol)),
        }
    }

    fn visible(&self, within: Rect) -> Rect {
        let Some((first, rest)) = self.stages().split_first() else {
            return within;
        };
        let src = first.src.to_sk();
        let cut = stage_visible(first, within);
        if rest.is_empty() || maps_corners(self, cut) || !maps_corners(self, src) {
            return cut;
        }
        let mix = |t: f32| {
            let m = |a: f32, b: f32| a + (b - a) * t;
            Rect::new(
                m(src.left, cut.left),
                m(src.top, cut.top),
                m(src.right, cut.right),
                m(src.bottom, cut.bottom),
            )
        };
        let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
        for _ in 0..24 {
            let mid = 0.5 * (lo + hi);
            if maps_corners(self, mix(mid)) {
                lo = mid
            } else {
                hi = mid
            }
        }
        mix(lo)
    }

    fn max_stretch(&self, within: Rect) -> f32 {
        let mut box_ = within;
        let mut stretch = 1.0;
        for s in self.stages() {
            stretch *= stage_max_stretch(s, box_);
            box_ = stage_map_rect(s, box_);
        }
        stretch
    }

    fn placer(&self) -> Placer {
        let mut m = Matrix::new_identity();
        for s in self.stages() {
            match stage_as_matrix(s) {
                Some(stage) => m = stage * m,
                None => return Placer::Grid,
            }
        }
        Placer::Matrix(m)
    }

    fn cuts(&self) -> Vec<(usize, Rect)> {
        self.stages()
            .iter()
            .enumerate()
            .flat_map(|(i, s)| stage_cut(s).into_iter().map(move |r| (i, r)))
            .collect()
    }
}

fn maps_corners(warp: &Warp, r: Rect) -> bool {
    r.to_quad(None).iter().all(|&p| warp.map_point(p).is_some())
}

/// Exact box of the warped `src`-frame rect `r`, cut to [`stage_visible`] first.
fn stage_map_rect(s: &Stage, r: Rect) -> Rect {
    match &s.kind {
        WarpStage::Corners(c) => corners_map_rect(c, s.src.to_sk(), r),
        WarpStage::Cylinder(c) => cylinder_map_rect(c, r),
    }
}

/// `None` past the cylinder's visible half or behind the corners horizon.
fn stage_map_point(s: &Stage, p: Point) -> Option<Point> {
    match &s.kind {
        WarpStage::Corners(c) => corners_map_point(c, s.src.to_sk(), p),
        WarpStage::Cylinder(c) => cylinder_map_point(c, p),
    }
}

/// The path cut to [`stage_visible`] and mapped. A curved kind flattens each contour so the
/// mapped polyline stays within `tol` (`src` units) of the true image.
fn stage_map_path(s: &Stage, path: &Path, tol: f32) -> Path {
    match &s.kind {
        WarpStage::Corners(c) => corners_map_path(c, s.src.to_sk(), path),
        WarpStage::Cylinder(c) => cylinder_map_path(c, path, tol),
    }
}

/// The part of `within` that maps. `src` itself always maps.
fn stage_visible(s: &Stage, within: Rect) -> Rect {
    match &s.kind {
        WarpStage::Corners(c) => corners_visible(c, s.src.to_sk(), within),
        WarpStage::Cylinder(c) => cylinder_visible(c, within),
    }
}

/// The largest local magnification over `within`, at least 1.
fn stage_max_stretch(s: &Stage, within: Rect) -> f32 {
    match &s.kind {
        WarpStage::Corners(c) => corners_max_stretch(c, s.src.to_sk(), within),
        WarpStage::Cylinder(_) => 1.0,
    }
}

/// `Some` when the stage is exactly a 3x3 matrix.
fn stage_as_matrix(s: &Stage) -> Option<Matrix> {
    match &s.kind {
        WarpStage::Corners(c) => Some(corners_matrix(c, s.src.to_sk())),
        WarpStage::Cylinder(_) => None,
    }
}

/// The parts of `src` this stage drops, for the `warp-cut` lint. Empty when nothing is hidden.
fn stage_cut(s: &Stage) -> Vec<Rect> {
    match &s.kind {
        WarpStage::Corners(_) => Vec::new(),
        WarpStage::Cylinder(c) => cylinder_cut(c, s.src.to_sk()),
    }
}

/// How far toward the horizon a padded box may reach, as a fraction of the way from a corner
/// that maps to the horizon behind it.
const HORIZON_STOP: f32 = 0.5;

/// The unit square's corners in the same order as a quad.
const UNIT: [Point; 4] = [
    Point { x: 0.0, y: 0.0 },
    Point { x: 1.0, y: 0.0 },
    Point { x: 1.0, y: 1.0 },
    Point { x: 0.0, y: 1.0 },
];

/// Maps the corners of `src` (tl tr br bl) onto the quad.
fn corners_matrix(c: &Corners, src: Rect) -> Matrix {
    let quad = c.quad().map(|p| p.to_sk());
    let unit_to_quad = Matrix::from_poly_to_poly(&UNIT, &quad).expect("try_new proved the quad is a projective target");
    let src_to_unit = Matrix::scale((1.0 / src.width(), 1.0 / src.height())) * Matrix::translate((-src.left, -src.top));
    unit_to_quad * src_to_unit
}

fn corners_map_rect(c: &Corners, src: Rect, r: Rect) -> Rect {
    let m = corners_matrix(c, src);
    let pts = corners_visible(c, src, r).to_quad(None).map(|p| m.map_point(p));
    bounds_of(&pts)
}

fn corners_map_point(c: &Corners, src: Rect, p: Point) -> Option<Point> {
    let m = corners_matrix(c, src);
    (depth(&m, p) > 0.0).then(|| m.map_point(p)).filter(|p| p.is_finite())
}

fn corners_map_path(c: &Corners, src: Rect, path: &Path) -> Path {
    clip_to(path, corners_visible(c, src, path.compute_tight_bounds())).with_transform(&corners_matrix(c, src))
}

/// Shrinks `within` toward `src`, which maps by construction, until every corner is in front
/// of the horizon. The shrink stops at [`HORIZON_STOP`] of the way so the farthest corner
/// still maps to a finite point instead of running off.
fn corners_visible(c: &Corners, src: Rect, within: Rect) -> Rect {
    let m = corners_matrix(c, src);
    let t = src
        .to_quad(None)
        .iter()
        .zip(within.to_quad(None))
        .map(|(&a, b)| {
            let (w0, w1) = (depth(&m, a), depth(&m, b));
            if w1 > 0.0 { 1.0 } else { HORIZON_STOP * w0 / (w0 - w1) }
        })
        .fold(1.0_f32, f32::min);
    let mix = |a: f32, b: f32| a + (b - a) * t;
    Rect::new(
        mix(src.left, within.left),
        mix(src.top, within.top),
        mix(src.right, within.right),
        mix(src.bottom, within.bottom),
    )
}

/// The largest singular value of the homography's Jacobian over `within`, sampled at the
/// corners and edge midpoints, where it is extremal.
fn corners_max_stretch(c: &Corners, src: Rect, within: Rect) -> f32 {
    let m = corners_matrix(c, src);
    let r = corners_visible(c, src, within);
    let (cx, cy) = (0.5 * (r.left + r.right), 0.5 * (r.top + r.bottom));
    let samples = [
        (r.left, r.top),
        (cx, r.top),
        (r.right, r.top),
        (r.right, cy),
        (r.right, r.bottom),
        (cx, r.bottom),
        (r.left, r.bottom),
        (r.left, cy),
    ];
    samples.iter().map(|&(x, y)| stretch_at(&m, x, y)).fold(1.0, f32::max)
}

/// `(u, v)`: the coordinate that bends around the cylinder and the one along its axis.
fn split(axis: Axis, p: Point) -> (f32, f32) {
    match axis {
        Axis::Y => (p.x, p.y),
        Axis::X => (p.y, p.x),
    }
}

fn join(axis: Axis, u: f32, v: f32) -> Point {
    match axis {
        Axis::Y => Point::new(u, v),
        Axis::X => Point::new(v, u),
    }
}

/// The rect's extent along the bending coordinate.
fn span(axis: Axis, r: Rect) -> (f32, f32) {
    match axis {
        Axis::Y => (r.left, r.right),
        Axis::X => (r.top, r.bottom),
    }
}

fn with_span(axis: Axis, r: Rect, lo: f32, hi: f32) -> Rect {
    match axis {
        Axis::Y => Rect::new(lo, r.top, hi, r.bottom),
        Axis::X => Rect::new(r.left, lo, r.right, hi),
    }
}

/// Half the visible circumference: how far from the axis content still faces the viewer.
fn half(c: &Cylinder) -> f32 {
    FRAC_PI_2 * c.r()
}

fn bend(c: &Cylinder, u: f32) -> f32 {
    c.r() * (u.clamp(-half(c), half(c)) / c.r()).sin()
}

fn cylinder_map_rect(c: &Cylinder, r: Rect) -> Rect {
    let (lo, hi) = span(c.axis(), cylinder_visible(c, r));
    with_span(c.axis(), r, bend(c, lo), bend(c, hi))
}

fn cylinder_map_point(c: &Cylinder, p: Point) -> Option<Point> {
    let (u, v) = split(c.axis(), p);
    (u.abs() <= half(c)).then(|| join(c.axis(), bend(c, u), v))
}

fn cylinder_map_path(c: &Cylinder, path: &Path, tol: f32) -> Path {
    let flat = clip_to(path, cylinder_visible(c, path.compute_tight_bounds()));
    let step = chord_step(c, tol);
    let mut out = PathBuilder::new();
    out.set_fill_type(path.fill_type());
    for (points, closed) in flatten(&flat, step, tol) {
        let mapped: Vec<Point> = points
            .iter()
            .map(|&p| {
                let (u, v) = split(c.axis(), p);
                join(c.axis(), bend(c, u), v)
            })
            .collect();
        out.add_polygon(&mapped, closed);
    }
    out.detach()
}

/// The longest chord whose sag from the surface stays within `tol`: a chord of length `s`
/// sags `s^2 / 8r`.
fn chord_step(c: &Cylinder, tol: f32) -> f32 {
    (8.0 * c.r() * tol).sqrt()
}

fn cylinder_visible(c: &Cylinder, within: Rect) -> Rect {
    let (lo, hi) = span(c.axis(), within);
    with_span(c.axis(), within, lo.clamp(-half(c), half(c)), hi.clamp(-half(c), half(c)))
}

/// The strips of `src` past the visible half, on each side the content reaches.
fn cylinder_cut(c: &Cylinder, src: Rect) -> Vec<Rect> {
    let (lo, hi) = span(c.axis(), src);
    let half = half(c);
    let mut strips = Vec::new();
    if lo < -half {
        strips.push(with_span(c.axis(), src, lo, hi.min(-half)));
    }
    if hi > half {
        strips.push(with_span(c.axis(), src, lo.max(half), hi));
    }
    strips
}

/// `w` of the homogeneous image of `p`: positive in front of the horizon.
fn depth(m: &Matrix, p: Point) -> f32 {
    m.map_homogeneous_point(Point3::new(p.x, p.y, 1.0)).z
}

fn bounds_of(pts: &[Point]) -> Rect {
    pts.iter().fold(Rect::new(pts[0].x, pts[0].y, pts[0].x, pts[0].y), |r, p| {
        Rect::new(r.left.min(p.x), r.top.min(p.y), r.right.max(p.x), r.bottom.max(p.y))
    })
}

/// `path` cut to `keep`, untouched when it already lies inside.
fn clip_to(path: &Path, keep: Rect) -> Path {
    if keep.contains(path.compute_tight_bounds()) {
        return path.clone();
    }
    skia_safe::op(path, &Path::rect(keep, None), skia_safe::PathOp::Intersect).unwrap_or_default()
}

/// The largest singular value of the Jacobian of the homography `m` at `(x, y)`.
fn stretch_at(m: &Matrix, x: f32, y: f32) -> f32 {
    let g = |k: Member| f64::from(m[k]);
    let (x, y) = (f64::from(x), f64::from(y));
    let nx = g(Member::ScaleX) * x + g(Member::SkewX) * y + g(Member::TransX);
    let ny = g(Member::SkewY) * x + g(Member::ScaleY) * y + g(Member::TransY);
    let w = g(Member::Persp0) * x + g(Member::Persp1) * y + g(Member::Persp2);
    let ww = w * w;
    let j11 = (g(Member::ScaleX) * w - g(Member::Persp0) * nx) / ww;
    let j12 = (g(Member::SkewX) * w - g(Member::Persp1) * nx) / ww;
    let j21 = (g(Member::SkewY) * w - g(Member::Persp0) * ny) / ww;
    let j22 = (g(Member::ScaleY) * w - g(Member::Persp1) * ny) / ww;
    let a = ((j11 + j22).powi(2) + (j12 - j21).powi(2)).sqrt();
    let b = ((j11 - j22).powi(2) + (j12 + j21).powi(2)).sqrt();
    (0.5 * (a + b)) as f32
}

/// Each contour of `path` as a polyline and whether it closes. Every segment is split into
/// pieces no longer than `step`, and a curve into enough pieces that its chords stay within
/// `tol` of it, so vertices survive and the image of each piece is flat enough to draw straight.
fn flatten(path: &Path, step: f32, tol: f32) -> Vec<(Vec<Point>, bool)> {
    let (step, tol) = (step.max(1e-4), tol.max(1e-4));
    let mut contours = Vec::new();
    let mut points: Vec<Point> = Vec::new();
    let mut closed = false;
    let mut flush = |points: &mut Vec<Point>, closed: bool| {
        if points.len() > 1 {
            contours.push((std::mem::take(points), closed));
        }
        points.clear();
    };
    let pieces = |points: &mut Vec<Point>, at: &dyn Fn(f32) -> Point, n: usize| {
        for i in 1..=n {
            points.push(at(i as f32 / n as f32));
        }
    };
    let mut iter = skia_safe::path::Iter::new(path, false);
    while let Some((verb, p)) = iter.next() {
        match verb {
            Verb::Move => {
                flush(&mut points, closed);
                closed = false;
                points.push(p[0]);
            }
            Verb::Line => {
                let n = ((p[1] - p[0]).length() / step).ceil().max(1.0) as usize;
                pieces(&mut points, &|t| p[0] + (p[1] - p[0]) * t, n);
            }
            Verb::Quad | Verb::Conic => {
                let w = if verb == Verb::Conic { iter.conic_weight().unwrap_or(1.0) } else { 1.0 };
                let n = curve_pieces(&p, 2.0, step, tol);
                pieces(&mut points, &|t| conic_at(p[0], p[1], p[2], w, t), n);
            }
            Verb::Cubic => {
                let n = curve_pieces(&p, 6.0, step, tol);
                pieces(&mut points, &|t| cubic_at(p[0], p[1], p[2], p[3], t), n);
            }
            Verb::Close => closed = true,
            Verb::Done => {}
        }
    }
    flush(&mut points, closed);
    contours
}

/// How many chords keep a curve within `tol`: the chord error is bounded by `k * dd / (8 n^2)`
/// with `dd` the largest second difference of the control points (`k` 2 for a quad, 6 for a
/// cubic), and each chord also stays shorter than `step`.
fn curve_pieces(p: &[Point], k: f32, step: f32, tol: f32) -> usize {
    let dd = p.windows(3).map(|w| (w[0] - w[1] * 2.0 + w[2]).length()).fold(0.0, f32::max);
    let length: f32 = p.windows(2).map(|w| (w[1] - w[0]).length()).sum();
    let for_curve = (k * dd / (8.0 * tol)).sqrt().ceil();
    let for_step = (length / step).ceil();
    for_curve.max(for_step).max(1.0) as usize
}

fn conic_at(p0: Point, p1: Point, p2: Point, w: f32, t: f32) -> Point {
    let (a, b, c) = ((1.0 - t) * (1.0 - t), 2.0 * w * (1.0 - t) * t, t * t);
    (p0 * a + p1 * b + p2 * c) * (1.0 / (a + b + c))
}

fn cubic_at(p0: Point, p1: Point, p2: Point, p3: Point, t: f32) -> Point {
    let s = 1.0 - t;
    p0 * (s * s * s) + p1 * (3.0 * s * s * t) + p2 * (3.0 * s * t * t) + p3 * (t * t * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(x: f32, y: f32) -> ramose_ir::Point {
        ramose_ir::Point::new(x, y)
    }

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-2
    }

    fn same(a: Rect, b: Rect) -> bool {
        near(a.left, b.left) && near(a.top, b.top) && near(a.right, b.right) && near(a.bottom, b.bottom)
    }

    const QUAD: [ramose_ir::Point; 4] = [
        ramose_ir::Point::new(412.0, 180.0),
        ramose_ir::Point::new(530.0, 196.0),
        ramose_ir::Point::new(526.0, 330.0),
        ramose_ir::Point::new(408.0, 322.0),
    ];

    fn corners(quad: [ramose_ir::Point; 4]) -> WarpStage {
        WarpStage::Corners(Corners::try_new(quad).unwrap())
    }

    fn cylinder(r: f32, axis: Axis) -> WarpStage {
        WarpStage::Cylinder(Cylinder::try_new(r, axis).unwrap())
    }

    #[test]
    fn a_cylinder_box_is_2r_sin_of_half_the_width_over_r_and_keeps_its_height() {
        let src = Rect::new(-60.0, -25.0, 60.0, 25.0);
        let warp = Warp::resolve(&[cylinder(60.0, Axis::Y)], src).unwrap();
        let b = warp.map_rect(src);
        let half = 60.0 * (60.0_f32 / 60.0).sin();
        assert!(same(b, Rect::new(-half, -25.0, half, 25.0)), "{b:?}");
        let on_x = Warp::resolve(&[cylinder(60.0, Axis::X)], src).unwrap();
        let b = on_x.map_rect(src);
        let half = 60.0 * (25.0_f32 / 60.0).sin();
        assert!(same(b, Rect::new(-60.0, -half, 60.0, half)), "{b:?}");
        assert!(matches!(warp.placer(), Placer::Grid));
    }

    #[test]
    fn a_cylinder_hides_content_past_its_visible_half() {
        let src = Rect::new(-20.0, 0.0, 20.0, 5.0);
        let warp = Warp::resolve(&[cylinder(10.0, Axis::Y)], src).unwrap();
        let half = FRAC_PI_2 * 10.0;
        assert!(same(warp.visible(src), Rect::new(-half, 0.0, half, 5.0)));
        assert!(same(warp.map_rect(src), Rect::new(-10.0, 0.0, 10.0, 5.0)));
        assert_eq!(warp.map_point(Point::new(half + 0.1, 1.0)), None);
        assert!(warp.map_point(Point::new(half - 0.1, 1.0)).is_some());
        let cuts = warp.cuts();
        assert_eq!(cuts.len(), 2);
        assert!(same(cuts[0].1, Rect::new(-20.0, 0.0, -half, 5.0)), "{cuts:?}");
        assert!(same(cuts[1].1, Rect::new(half, 0.0, 20.0, 5.0)), "{cuts:?}");
        assert_eq!(cuts[0].0, 0);
        let fits = Warp::resolve(&[cylinder(60.0, Axis::Y)], src).unwrap();
        assert!(fits.cuts().is_empty());
    }

    #[test]
    fn a_stage_whose_input_a_cylinder_emptied_is_an_error() {
        let src = Rect::new(100.0, 0.0, 120.0, 5.0);
        let err = Warp::resolve(&[cylinder(10.0, Axis::Y), corners(QUAD)], src);
        assert_eq!(err, Err(WarpError::Empty(1)));
    }

    #[test]
    fn a_cylinder_path_lands_in_the_cylinder_box() {
        let src = Rect::new(-60.0, -25.0, 60.0, 25.0);
        let warp = Warp::resolve(&[cylinder(60.0, Axis::Y)], src).unwrap();
        let mapped = warp.map_path(&Path::rect(src, None), 0.1);
        assert!(same(mapped.compute_tight_bounds(), warp.map_rect(src)), "{:?}", mapped.compute_tight_bounds());
        let circle = Path::circle((0.0, 0.0), 50.0, None);
        let mapped = warp.map_path(&circle, 0.1);
        let b = mapped.compute_tight_bounds();
        let x = 60.0 * (50.0_f32 / 60.0).sin();
        assert!(same(b, Rect::new(-x, -50.0, x, 50.0)), "{b:?}");
        let vertices = mapped.points();
        assert!(vertices.len() > 16, "{}", vertices.len());
        for p in vertices {
            let unbent = 60.0 * (p.x / 60.0).asin();
            let radius = (unbent * unbent + p.y * p.y).sqrt();
            assert!((radius - 50.0).abs() < 0.2, "{p:?} unbends to radius {radius}");
        }
    }

    #[test]
    fn corners_map_the_src_corners_onto_the_quad() {
        let src = Rect::new(0.0, 0.0, 40.0, 200.0);
        let warp = Warp::resolve(&[corners(QUAD)], src).unwrap();
        for (from, to) in src.to_quad(None).iter().zip(QUAD) {
            let p = warp.map_point(*from).unwrap();
            assert!(near(p.x, to.x) && near(p.y, to.y), "{from:?} -> {p:?}, want {to:?}");
        }
        assert!(same(warp.map_rect(src), Rect::new(408.0, 180.0, 530.0, 330.0)));
        let mapped = warp.map_path(&Path::rect(src, None), 0.1);
        assert!(same(mapped.compute_tight_bounds(), Rect::new(408.0, 180.0, 530.0, 330.0)));
        assert!(matches!(warp.placer(), Placer::Matrix(_)));
        assert!(warp.cuts().is_empty());
        assert!(warp.max_stretch(src) > 1.0);
    }

    #[test]
    fn a_cylinder_then_corners_resolves_the_second_src_to_the_cylinder_box() {
        let src = Rect::new(-20.0, -50.0, 20.0, 50.0);
        let warp = Warp::resolve(&[cylinder(60.0, Axis::Y), corners(QUAD)], src).unwrap();
        let bent = Warp::resolve(&[cylinder(60.0, Axis::Y)], src).unwrap().map_rect(src);
        assert!(same(warp.stages()[1].src.to_sk(), bent), "{:?}", warp.stages()[1].src);
        assert_eq!(warp.reference(), Some(src.to_ir()));
        assert!(same(warp.map_rect(src), Rect::new(408.0, 180.0, 530.0, 330.0)), "{:?}", warp.map_rect(src));
        assert!(matches!(warp.placer(), Placer::Grid));
    }

    #[test]
    fn an_empty_warp_is_the_identity() {
        let warp = Warp::default();
        assert!(warp.is_none());
        let r = Rect::new(1.0, 2.0, 3.0, 4.0);
        assert_eq!(warp.map_rect(r), r);
        assert_eq!(warp.visible(r), r);
        assert_eq!(warp.map_point(Point::new(5.0, 6.0)), Some(Point::new(5.0, 6.0)));
        assert_eq!(warp.max_stretch(r), 1.0);
        assert!(matches!(warp.placer(), Placer::Matrix(m) if m.is_identity()));
        assert_eq!(warp.reference(), None);
    }

    #[test]
    fn padding_past_the_corners_horizon_is_cut_back_to_a_finite_box() {
        // A strong perspective: the top edge is a fifth of the bottom edge.
        let quad = [pt(40.0, 0.0), pt(60.0, 0.0), pt(100.0, 100.0), pt(0.0, 100.0)];
        let src = Rect::new(0.0, 0.0, 100.0, 100.0);
        let warp = Warp::resolve(&[corners(quad)], src).unwrap();
        let padded = src.with_outset((0.0, 400.0));
        let vis = warp.visible(padded);
        assert!(src.contains(vis) || vis.contains(src));
        assert!(vis.top > padded.top, "{vis:?}");
        let mapped = warp.map_rect(padded);
        assert!(mapped.is_finite() && mapped.is_sorted(), "{mapped:?}");
    }
}
