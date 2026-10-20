//! `warp:`: a node's non-affine placement, applied to everything it draws before `transform`.
//!
//! Eval resolves each stage against the box it maps once, and the IR stores the resolved stack.
//! The per-kind table is `impl Stage`: every method there is one `match` into the kind's own
//! method, so a new kind is one `WarpStage` variant plus one impl block, and nothing outside this
//! file matches on a kind.

use std::f32::consts::FRAC_PI_2;

use skia_safe::matrix::Member;
use skia_safe::path::Verb;
use skia_safe::{Contains, Matrix, Path, PathBuilder, Point, Point3, Rect};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
}

impl Axis {
    /// `(u, v)`: the coordinate that bends around the cylinder and the one along its axis.
    fn split(self, p: Point) -> (f32, f32) {
        match self {
            Axis::Y => (p.x, p.y),
            Axis::X => (p.y, p.x),
        }
    }

    fn join(self, u: f32, v: f32) -> Point {
        match self {
            Axis::Y => Point::new(u, v),
            Axis::X => Point::new(v, u),
        }
    }

    /// The rect's extent along the bending coordinate.
    fn span(self, r: Rect) -> (f32, f32) {
        match self {
            Axis::Y => (r.left, r.right),
            Axis::X => (r.top, r.bottom),
        }
    }

    fn with_span(self, r: Rect, lo: f32, hi: f32) -> Rect {
        match self {
            Axis::Y => Rect::new(lo, r.top, hi, r.bottom),
            Axis::X => Rect::new(r.left, lo, r.right, hi),
        }
    }
}

/// One stage as the author wrote it. Eval values hold these; [`Warp::resolve`] binds them to a box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WarpStage {
    Corners(Corners),
    Cylinder(Cylinder),
}

/// Four destination points clockwise from the top-left. Only [`Corners::try_new`] builds one, so
/// a stored value is a valid projective target for a rectangle: finite, no three collinear
/// corners, no two edges crossing, and convex. A flipped (counter-clockwise) quad is allowed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Corners {
    quad: [Point; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CornersError {
    /// Two edges cross (a bow tie).
    SelfCrossing,
    /// Zero area, three collinear corners, or a non-finite coordinate.
    Degenerate,
    /// A corner folds inward (a dart). No projective map takes a rectangle onto it without
    /// sending part of the rectangle past the horizon.
    Concave,
}

/// The unit square's corners in the same order as a quad.
const UNIT: [Point; 4] = [
    Point { x: 0.0, y: 0.0 },
    Point { x: 1.0, y: 0.0 },
    Point { x: 1.0, y: 1.0 },
    Point { x: 0.0, y: 1.0 },
];

impl Corners {
    pub fn try_new(quad: [Point; 4]) -> Result<Corners, CornersError> {
        if quad.iter().any(|p| !p.is_finite()) {
            return Err(CornersError::Degenerate);
        }
        let extent = quad
            .iter()
            .flat_map(|p| [p.x.abs(), p.y.abs()])
            .fold(0.0_f32, f32::max)
            .max(1.0);
        let flat = f64::from(extent) * f64::from(extent) * 1e-9;
        let collinear =
            |a: usize, b: usize, c: usize| cross(quad[a], quad[b], quad[c]).abs() <= flat;
        if (0..4).any(|i| collinear(i, (i + 1) % 4, (i + 2) % 4)) {
            return Err(CornersError::Degenerate);
        }
        if crosses(quad[0], quad[1], quad[2], quad[3])
            || crosses(quad[1], quad[2], quad[3], quad[0])
        {
            return Err(CornersError::SelfCrossing);
        }
        let m = Matrix::from_poly_to_poly(&UNIT, &quad).ok_or(CornersError::Degenerate)?;
        if UNIT.iter().any(|&p| depth(&m, p) <= 0.0) {
            return Err(CornersError::Concave);
        }
        Ok(Corners { quad })
    }

    pub fn quad(&self) -> [Point; 4] {
        self.quad
    }

    /// Maps the corners of `src` (tl tr br bl) onto the quad.
    fn matrix(&self, src: Rect) -> Matrix {
        let unit_to_quad = Matrix::from_poly_to_poly(&UNIT, &self.quad)
            .expect("try_new proved the quad is a projective target");
        let src_to_unit = Matrix::scale((1.0 / src.width(), 1.0 / src.height()))
            * Matrix::translate((-src.left, -src.top));
        unit_to_quad * src_to_unit
    }

    fn map_rect(&self, src: Rect, r: Rect) -> Rect {
        let m = self.matrix(src);
        let pts = self.visible(src, r).to_quad(None).map(|p| m.map_point(p));
        bounds_of(&pts)
    }

    fn map_point(&self, src: Rect, p: Point) -> Option<Point> {
        let m = self.matrix(src);
        (depth(&m, p) > 0.0)
            .then(|| m.map_point(p))
            .filter(|p| p.is_finite())
    }

    fn map_path(&self, src: Rect, path: &Path) -> Path {
        clip_to(path, self.visible(src, path.compute_tight_bounds()))
            .with_transform(&self.matrix(src))
    }

    /// Shrinks `within` toward `src`, which maps by construction, until every corner is in front
    /// of the horizon. The shrink stops halfway to the horizon so the farthest corner still maps
    /// to a finite point instead of running off.
    fn visible(&self, src: Rect, within: Rect) -> Rect {
        let m = self.matrix(src);
        let t = src
            .to_quad(None)
            .iter()
            .zip(within.to_quad(None))
            .map(|(&a, b)| {
                let (w0, w1) = (depth(&m, a), depth(&m, b));
                if w1 > 0.0 { 1.0 } else { 0.5 * w0 / (w0 - w1) }
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
    fn max_stretch(&self, src: Rect, within: Rect) -> f32 {
        let m = self.matrix(src);
        let r = self.visible(src, within);
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
        samples
            .iter()
            .map(|&(x, y)| stretch_at(&m, x, y))
            .fold(1.0, f32::max)
    }
}

/// `r` finite and > 0 via [`Cylinder::try_new`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cylinder {
    r: f32,
    axis: Axis,
}

impl Cylinder {
    pub fn try_new(r: f32, axis: Axis) -> Option<Cylinder> {
        (r.is_finite() && r > 0.0).then_some(Cylinder { r, axis })
    }

    pub fn r(&self) -> f32 {
        self.r
    }

    pub fn axis(&self) -> Axis {
        self.axis
    }

    /// Half the visible circumference: how far from the axis content still faces the viewer.
    fn half(&self) -> f32 {
        FRAC_PI_2 * self.r
    }

    fn bend(&self, u: f32) -> f32 {
        self.r * (u.clamp(-self.half(), self.half()) / self.r).sin()
    }

    fn map_rect(&self, r: Rect) -> Rect {
        let (lo, hi) = self.axis.span(self.visible(r));
        self.axis.with_span(r, self.bend(lo), self.bend(hi))
    }

    fn map_point(&self, p: Point) -> Option<Point> {
        let (u, v) = self.axis.split(p);
        (u.abs() <= self.half()).then(|| self.axis.join(self.bend(u), v))
    }

    fn map_path(&self, path: &Path, tol: f32) -> Path {
        let flat = clip_to(path, self.visible(path.compute_tight_bounds()));
        // A chord of length `s` on the cylinder sags `s^2 / 8r` from the surface.
        let step = (8.0 * self.r * tol).sqrt();
        let mut out = PathBuilder::new();
        out.set_fill_type(path.fill_type());
        for (points, closed) in flatten(&flat, step, tol) {
            let mapped: Vec<Point> = points
                .iter()
                .map(|&p| {
                    let (u, v) = self.axis.split(p);
                    self.axis.join(self.bend(u), v)
                })
                .collect();
            out.add_polygon(&mapped, closed);
        }
        out.detach()
    }

    fn visible(&self, within: Rect) -> Rect {
        let (lo, hi) = self.axis.span(within);
        self.axis.with_span(
            within,
            lo.clamp(-self.half(), self.half()),
            hi.clamp(-self.half(), self.half()),
        )
    }

    /// The strips of `src` past the visible half, on each side the content reaches.
    fn cut(&self, src: Rect) -> Vec<Rect> {
        let (lo, hi) = self.axis.span(src);
        let half = self.half();
        let mut strips = Vec::new();
        if lo < -half {
            strips.push(self.axis.with_span(src, lo, hi.min(-half)));
        }
        if hi > half {
            strips.push(self.axis.with_span(src, lo.max(half), hi));
        }
        strips
    }
}

/// A stage bound to the box it maps: `src` is the node's reference box for stage 0 and the
/// previous stage's `map_rect` of its own `src` after that. Built only by [`Warp::resolve`].
#[derive(Clone, Debug, PartialEq)]
pub struct Stage {
    pub kind: WarpStage,
    pub src: Rect,
}

impl Stage {
    /// Exact box of the warped `src`-frame rect `r`, cut to [`Stage::visible`] first.
    pub fn map_rect(&self, r: Rect) -> Rect {
        match &self.kind {
            WarpStage::Corners(c) => c.map_rect(self.src, r),
            WarpStage::Cylinder(c) => c.map_rect(r),
        }
    }

    /// `None` past the cylinder's visible half or behind the corners horizon.
    pub fn map_point(&self, p: Point) -> Option<Point> {
        match &self.kind {
            WarpStage::Corners(c) => c.map_point(self.src, p),
            WarpStage::Cylinder(c) => c.map_point(p),
        }
    }

    /// The path cut to [`Stage::visible`] and mapped. A curved kind flattens each contour so the
    /// mapped polyline stays within `tol` (`src` units) of the true image.
    pub fn map_path(&self, path: &Path, tol: f32) -> Path {
        match &self.kind {
            WarpStage::Corners(c) => c.map_path(self.src, path),
            WarpStage::Cylinder(c) => c.map_path(path, tol),
        }
    }

    /// The part of `within` that maps. `src` itself always maps.
    pub fn visible(&self, within: Rect) -> Rect {
        match &self.kind {
            WarpStage::Corners(c) => c.visible(self.src, within),
            WarpStage::Cylinder(c) => c.visible(within),
        }
    }

    /// The largest local magnification over `within`, at least 1.
    pub fn max_stretch(&self, within: Rect) -> f32 {
        match &self.kind {
            WarpStage::Corners(c) => c.max_stretch(self.src, within),
            WarpStage::Cylinder(_) => 1.0,
        }
    }

    /// `Some` when the stage is exactly a 3x3 matrix.
    pub fn as_matrix(&self) -> Option<Matrix> {
        match &self.kind {
            WarpStage::Corners(c) => Some(c.matrix(self.src)),
            WarpStage::Cylinder(_) => None,
        }
    }

    /// The parts of `src` this stage drops, for the `warp-cut` lint. Empty when nothing is hidden.
    pub fn cut(&self) -> Vec<Rect> {
        match &self.kind {
            WarpStage::Corners(_) => Vec::new(),
            WarpStage::Cylinder(c) => c.cut(self.src),
        }
    }

    /// Census name as E0209 prints it.
    pub fn feature(&self) -> &'static str {
        match &self.kind {
            WarpStage::Corners(_) => "warp corners",
            WarpStage::Cylinder(_) => "warp cylinder",
        }
    }
}

/// How a composed stack draws a flat image.
#[derive(Clone, Copy, Debug)]
pub enum Placer {
    Matrix(Matrix),
    Grid,
}

/// A node's placed warp: resolved stages in application order. Empty is no warp
/// ([`Warp::default`]), and every method is the identity on it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Warp(Vec<Stage>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WarpError {
    /// An earlier cylinder hid everything, so this stage's `src` is empty.
    Empty(usize),
}

impl Warp {
    /// Binds `stages` to `reference`, threading each stage's `map_rect` of its `src` into the
    /// next `src`.
    pub fn resolve(stages: &[WarpStage], reference: Rect) -> Result<Warp, WarpError> {
        let mut src = reference;
        let mut out = Vec::with_capacity(stages.len());
        for (i, &kind) in stages.iter().enumerate() {
            if src.is_empty() {
                return Err(WarpError::Empty(i));
            }
            let stage = Stage { kind, src };
            src = stage.map_rect(src);
            out.push(stage);
        }
        Ok(Warp(out))
    }

    pub fn is_none(&self) -> bool {
        self.0.is_empty()
    }

    pub fn stages(&self) -> &[Stage] {
        &self.0
    }

    /// The box stage 0 maps (`None` when empty).
    pub fn reference(&self) -> Option<Rect> {
        self.0.first().map(|s| s.src)
    }

    pub fn map_rect(&self, r: Rect) -> Rect {
        self.0.iter().fold(r, |r, s| s.map_rect(r))
    }

    pub fn map_point(&self, p: Point) -> Option<Point> {
        self.0.iter().try_fold(p, |p, s| s.map_point(p))
    }

    pub fn map_path(&self, path: &Path, tol: f32) -> Path {
        match self.0.split_first() {
            None => path.clone(),
            Some((first, rest)) => rest
                .iter()
                .fold(first.map_path(path, tol), |p, s| s.map_path(&p, tol)),
        }
    }

    /// The part of `within` whose corners map through every stage: stage 0's own cut, then
    /// shrunk toward the reference while a later stage still drops a corner. When a later stage
    /// hides part of the reference itself, the result is stage 0's cut.
    pub fn visible(&self, within: Rect) -> Rect {
        let Some((first, rest)) = self.0.split_first() else {
            return within;
        };
        let cut = first.visible(within);
        if rest.is_empty() || self.maps_corners(cut) || !self.maps_corners(first.src) {
            return cut;
        }
        let mix = |t: f32| {
            let m = |a: f32, b: f32| a + (b - a) * t;
            Rect::new(
                m(first.src.left, cut.left),
                m(first.src.top, cut.top),
                m(first.src.right, cut.right),
                m(first.src.bottom, cut.bottom),
            )
        };
        let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
        for _ in 0..24 {
            let mid = 0.5 * (lo + hi);
            if self.maps_corners(mix(mid)) {
                lo = mid
            } else {
                hi = mid
            }
        }
        mix(lo)
    }

    fn maps_corners(&self, r: Rect) -> bool {
        r.to_quad(None).iter().all(|&p| self.map_point(p).is_some())
    }

    /// The product of each stage's largest magnification over the image of `within` so far.
    pub fn max_stretch(&self, within: Rect) -> f32 {
        let mut box_ = within;
        let mut stretch = 1.0;
        for s in &self.0 {
            stretch *= s.max_stretch(box_);
            box_ = s.map_rect(box_);
        }
        stretch
    }

    /// [`Placer::Matrix`] when every stage is a matrix, multiplied in application order; else
    /// [`Placer::Grid`].
    pub fn placer(&self) -> Placer {
        let mut m = Matrix::new_identity();
        for s in &self.0 {
            match s.as_matrix() {
                Some(stage) => m = stage * m,
                None => return Placer::Grid,
            }
        }
        Placer::Matrix(m)
    }

    /// (stage index, dropped rect) for every strip a stage cuts.
    pub fn cuts(&self) -> Vec<(usize, Rect)> {
        self.0
            .iter()
            .enumerate()
            .flat_map(|(i, s)| s.cut().into_iter().map(move |r| (i, r)))
            .collect()
    }
}

/// `w` of the homogeneous image of `p`: positive in front of the horizon.
fn depth(m: &Matrix, p: Point) -> f32 {
    m.map_homogeneous_point(Point3::new(p.x, p.y, 1.0)).z
}

fn cross(a: Point, b: Point, c: Point) -> f64 {
    let (abx, aby) = (f64::from(b.x - a.x), f64::from(b.y - a.y));
    let (acx, acy) = (f64::from(c.x - a.x), f64::from(c.y - a.y));
    abx * acy - aby * acx
}

/// Whether segments `ab` and `cd` cross at interior points.
fn crosses(a: Point, b: Point, c: Point, d: Point) -> bool {
    cross(a, b, c) * cross(a, b, d) < 0.0 && cross(c, d, a) * cross(c, d, b) < 0.0
}

fn bounds_of(pts: &[Point]) -> Rect {
    pts.iter()
        .fold(Rect::new(pts[0].x, pts[0].y, pts[0].x, pts[0].y), |r, p| {
            Rect::new(
                r.left.min(p.x),
                r.top.min(p.y),
                r.right.max(p.x),
                r.bottom.max(p.y),
            )
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
                let w = if verb == Verb::Conic {
                    iter.conic_weight().unwrap_or(1.0)
                } else {
                    1.0
                };
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
    let dd = p
        .windows(3)
        .map(|w| (w[0] - w[1] * 2.0 + w[2]).length())
        .fold(0.0, f32::max);
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

    fn pt(x: f32, y: f32) -> Point {
        Point::new(x, y)
    }

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-2
    }

    fn same(a: Rect, b: Rect) -> bool {
        near(a.left, b.left)
            && near(a.top, b.top)
            && near(a.right, b.right)
            && near(a.bottom, b.bottom)
    }

    const QUAD: [Point; 4] = [
        Point { x: 412.0, y: 180.0 },
        Point { x: 530.0, y: 196.0 },
        Point { x: 526.0, y: 330.0 },
        Point { x: 408.0, y: 322.0 },
    ];

    fn corners(quad: [Point; 4]) -> WarpStage {
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
        assert_eq!(warp.map_point(pt(half + 0.1, 1.0)), None);
        assert!(warp.map_point(pt(half - 0.1, 1.0)).is_some());
        let cuts = warp.cuts();
        assert_eq!(cuts.len(), 2);
        assert!(
            same(cuts[0].1, Rect::new(-20.0, 0.0, -half, 5.0)),
            "{cuts:?}"
        );
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
        assert!(
            same(mapped.compute_tight_bounds(), warp.map_rect(src)),
            "{:?}",
            mapped.compute_tight_bounds()
        );
        let circle = Path::circle((0.0, 0.0), 50.0, None);
        let mapped = warp.map_path(&circle, 0.1);
        let b = mapped.compute_tight_bounds();
        assert!(
            same(
                b,
                Rect::new(
                    -60.0 * (50.0_f32 / 60.0).sin(),
                    -50.0,
                    60.0 * (50.0_f32 / 60.0).sin(),
                    50.0
                )
            ),
            "{b:?}"
        );
        let vertices = mapped.points();
        assert!(vertices.len() > 16, "{}", vertices.len());
        for p in vertices {
            let unbent = 60.0 * (p.x / 60.0).asin();
            let radius = (unbent * unbent + p.y * p.y).sqrt();
            assert!(
                (radius - 50.0).abs() < 0.2,
                "{p:?} unbends to radius {radius}"
            );
        }
    }

    #[test]
    fn corners_map_the_src_corners_onto_the_quad() {
        let src = Rect::new(0.0, 0.0, 40.0, 200.0);
        let warp = Warp::resolve(&[corners(QUAD)], src).unwrap();
        for (from, to) in src.to_quad(None).iter().zip(QUAD) {
            let p = warp.map_point(*from).unwrap();
            assert!(
                near(p.x, to.x) && near(p.y, to.y),
                "{from:?} -> {p:?}, want {to:?}"
            );
        }
        assert!(same(
            warp.map_rect(src),
            Rect::new(408.0, 180.0, 530.0, 330.0)
        ));
        let mapped = warp.map_path(&Path::rect(src, None), 0.1);
        assert!(same(
            mapped.compute_tight_bounds(),
            Rect::new(408.0, 180.0, 530.0, 330.0)
        ));
        assert!(matches!(warp.placer(), Placer::Matrix(_)));
        assert!(warp.cuts().is_empty());
        assert!(warp.max_stretch(src) > 1.0);
    }

    #[test]
    fn a_bow_tie_and_a_collinear_quad_are_rejected_and_a_flipped_one_is_not() {
        let bow_tie = [pt(0.0, 0.0), pt(10.0, 0.0), pt(0.0, 10.0), pt(10.0, 10.0)];
        assert_eq!(
            Corners::try_new(bow_tie).err(),
            Some(CornersError::SelfCrossing)
        );
        let collinear = [pt(0.0, 0.0), pt(5.0, 0.0), pt(10.0, 0.0), pt(0.0, 10.0)];
        assert_eq!(
            Corners::try_new(collinear).err(),
            Some(CornersError::Degenerate)
        );
        let flat = [pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 0.0), pt(0.0, 0.0)];
        assert_eq!(Corners::try_new(flat).err(), Some(CornersError::Degenerate));
        let dart = [pt(0.0, 0.0), pt(10.0, 0.0), pt(5.0, 3.0), pt(0.0, 10.0)];
        assert_eq!(Corners::try_new(dart).err(), Some(CornersError::Concave));
        let nan = [
            pt(f32::NAN, 0.0),
            pt(10.0, 0.0),
            pt(10.0, 10.0),
            pt(0.0, 10.0),
        ];
        assert_eq!(Corners::try_new(nan).err(), Some(CornersError::Degenerate));
        let flipped = [pt(0.0, 0.0), pt(0.0, 10.0), pt(10.0, 10.0), pt(10.0, 0.0)];
        assert!(Corners::try_new(flipped).is_ok());
    }

    #[test]
    fn a_cylinder_then_corners_resolves_the_second_src_to_the_cylinder_box() {
        let src = Rect::new(-20.0, -50.0, 20.0, 50.0);
        let warp = Warp::resolve(&[cylinder(60.0, Axis::Y), corners(QUAD)], src).unwrap();
        let bent = Warp::resolve(&[cylinder(60.0, Axis::Y)], src)
            .unwrap()
            .map_rect(src);
        assert!(
            same(warp.stages()[1].src, bent),
            "{:?}",
            warp.stages()[1].src
        );
        assert_eq!(warp.reference(), Some(src));
        assert!(
            same(warp.map_rect(src), Rect::new(408.0, 180.0, 530.0, 330.0)),
            "{:?}",
            warp.map_rect(src)
        );
        assert!(matches!(warp.placer(), Placer::Grid));
    }

    #[test]
    fn an_empty_warp_is_the_identity() {
        let warp = Warp::default();
        assert!(warp.is_none());
        let r = Rect::new(1.0, 2.0, 3.0, 4.0);
        assert_eq!(warp.map_rect(r), r);
        assert_eq!(warp.visible(r), r);
        assert_eq!(warp.map_point(pt(5.0, 6.0)), Some(pt(5.0, 6.0)));
        assert_eq!(warp.max_stretch(r), 1.0);
        assert!(matches!(warp.placer(), Placer::Matrix(m) if m.is_identity()));
        assert_eq!(warp.reference(), None);
    }

    #[test]
    fn padding_past_the_corners_horizon_is_cut_back_to_a_finite_box() {
        // A strong perspective: the top edge is a fifth of the bottom edge.
        let quad = [
            pt(40.0, 0.0),
            pt(60.0, 0.0),
            pt(100.0, 100.0),
            pt(0.0, 100.0),
        ];
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
