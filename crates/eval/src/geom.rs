//! Geometry construction: centripetal Catmull-Rom splines, tapered ink outlines, hand-drawn
//! wobble, blobs, regular polygons and stars, and PathOps booleans. The spline, brush and wobble
//! algorithms are ports of chibi `paths.py`.

use std::f64::consts::PI;

use skia_safe::path::Verb;
use skia_safe::{PathBuilder, PathEffect, PathOp, RRect, StrokeRec, Vector};

use artscript_ir::{Path, Point, Rect, Ribbon, taper_ramp};

use crate::seed::Rng;

pub type P = (f64, f64);

fn pt(p: P) -> Point {
    Point::new(p.0 as f32, p.1 as f32)
}

fn sub(a: P, b: P) -> P {
    (a.0 - b.0, a.1 - b.1)
}

fn norm(a: P) -> f64 {
    a.0.hypot(a.1)
}

fn bezier_controls(p0: P, p1: P, p2: P, p3: P, tension: f64) -> (P, P) {
    let d1 = norm(sub(p1, p0)).max(1e-6).sqrt();
    let d2 = norm(sub(p2, p1)).max(1e-6).sqrt();
    let d3 = norm(sub(p3, p2)).max(1e-6).sqrt();
    let b1 = |i: usize| {
        let (a, b, c) = ([p0.0, p0.1][i], [p1.0, p1.1][i], [p2.0, p2.1][i]);
        (d1 * d1 * c - d2 * d2 * a + (2.0 * d1 * d1 + 3.0 * d1 * d2 + d2 * d2) * b) / (3.0 * d1 * (d1 + d2))
    };
    let b2 = |i: usize| {
        let (a, b, c) = ([p1.0, p1.1][i], [p2.0, p2.1][i], [p3.0, p3.1][i]);
        (d3 * d3 * a - d2 * d2 * c + (2.0 * d3 * d3 + 3.0 * d3 * d2 + d2 * d2) * b) / (3.0 * d3 * (d3 + d2))
    };
    let k = tension / 0.5;
    (
        (p1.0 + (b1(0) - p1.0) * k, p1.1 + (b1(1) - p1.1) * k),
        (p2.0 + (b2(0) - p2.0) * k, p2.1 + (b2(1) - p2.1) * k),
    )
}

/// Smooth curve through every point. Needs 2 points open, 3 closed; fewer give a polyline.
pub fn spline(pts: &[P], tension: f64, closed: bool) -> Path {
    let n = pts.len();
    let mut b = PathBuilder::new();
    if n == 0 {
        return b.detach();
    }
    if n < if closed { 3 } else { 2 } {
        b.move_to(pt(pts[0]));
        for p in &pts[1..] {
            b.line_to(pt(*p));
        }
        return b.detach();
    }
    let ext: Vec<P> = if closed {
        let mut e = vec![pts[n - 1]];
        e.extend_from_slice(pts);
        e.extend_from_slice(&pts[..2]);
        e
    } else {
        let mut e = vec![(2.0 * pts[0].0 - pts[1].0, 2.0 * pts[0].1 - pts[1].1)];
        e.extend_from_slice(pts);
        e.push((2.0 * pts[n - 1].0 - pts[n - 2].0, 2.0 * pts[n - 1].1 - pts[n - 2].1));
        e
    };
    b.move_to(pt(pts[0]));
    for i in 0..if closed { n } else { n - 1 } {
        let (c1, c2) = bezier_controls(ext[i], ext[i + 1], ext[i + 2], ext[i + 3], tension);
        b.cubic_to(pt(c1), pt(c2), pt(ext[i + 2]));
    }
    if closed {
        b.close();
    }
    b.detach()
}

/// The on-curve vertices of each contour of `path` that has length, in path order. A closed
/// contour does not repeat its first vertex at its end.
pub fn vertices(path: &Path) -> Vec<Vec<P>> {
    let mut out: Vec<Vec<P>> = Vec::new();
    let mut run: Vec<P> = Vec::new();
    let mut flush = |run: &mut Vec<P>, closed: bool| {
        if closed && run.len() > 1 && run.first() == run.last() {
            run.pop();
        }
        if run.windows(2).any(|w| w[0] != w[1]) || (closed && run.len() > 1) {
            out.push(std::mem::take(run));
        }
        run.clear();
    };
    for (verb, pts) in skia_safe::path::Iter::new(path, false) {
        let p = |q: &Point| (f64::from(q.x), f64::from(q.y));
        match verb {
            Verb::Move => {
                flush(&mut run, false);
                run.push(p(&pts[0]));
            }
            Verb::Line => run.push(p(&pts[1])),
            Verb::Quad | Verb::Conic => run.push(p(&pts[2])),
            Verb::Cubic => run.push(p(&pts[3])),
            Verb::Close => flush(&mut run, true),
            Verb::Done => break,
        }
    }
    flush(&mut run, false);
    out
}

pub struct Contour {
    pub pos: Vec<P>,
    pub tan: Vec<P>,
    pub closed: bool,
}

/// Per contour, positions and unit tangents every `step` units of arc length.
pub fn sample(path: &Path, step: f64) -> Vec<Contour> {
    let mut out = Vec::new();
    for measure in skia_safe::ContourMeasureIter::new(path, false, None) {
        let length = measure.length() as f64;
        if length <= 0.0 {
            continue;
        }
        let n = ((length / step).ceil() as usize).max(2);
        let mut pos = Vec::with_capacity(n + 1);
        let mut tan = Vec::with_capacity(n + 1);
        for i in 0..=n {
            let (p, t) = measure.pos_tan((length * i as f64 / n as f64) as f32).unwrap_or_default();
            pos.push((p.x as f64, p.y as f64));
            tan.push((t.x as f64, t.y as f64));
        }
        out.push(Contour { pos, tan, closed: measure.is_closed() });
    }
    out
}

/// Filled outline of a tapered or width-list stroke, brushed or not: every contour of `path` offset on both sides by the
/// ribbon's tapered half width at each point, with round caps on an open contour and an outer
/// and an inner edge on a closed one. The ribbon's ink fills this outline. `step` is the
/// sample spacing in the path's own units. Callers pass the spacing that lands on 0.75 output
/// pixels after the node's transform and the target scale.
pub fn ribbon_ink(path: &Path, ribbon: &Ribbon, step: f64) -> Path {
    let taper = (f64::from(ribbon.taper[0]), f64::from(ribbon.taper[1]));
    let mut b = PathBuilder::new();
    for (measure, contour) in skia_safe::ContourMeasureIter::new(path, false, None).zip(&ribbon.contours) {
        let length = f64::from(measure.length());
        if length <= 0.0 {
            continue;
        }
        let n = ((length / step.max(1e-6)).ceil() as usize).max(2);
        let (mut pos, mut tan, mut r) = (Vec::with_capacity(n + 1), Vec::with_capacity(n + 1), Vec::with_capacity(n + 1));
        for i in 0..=n {
            let s = length * i as f64 / n as f64;
            let (p, t) = measure.pos_tan(s as f32).unwrap_or_default();
            pos.push((f64::from(p.x), f64::from(p.y)));
            tan.push((f64::from(t.x), f64::from(t.y)));
            let on = (s / length * f64::from(contour.len)) as f32;
            r.push(f64::from(contour.at(on).half_width) * taper_ramp(s / length, taper));
        }
        if !contour.closed {
            b.add_polygon(&capped_ring(&pos, &tan, &r).iter().map(|p| pt(*p)).collect::<Vec<_>>(), true);
            continue;
        }
        let side = |sign: f64| -> Vec<Point> {
            (0..n).map(|i| pt((pos[i].0 - sign * tan[i].1 * r[i], pos[i].1 + sign * tan[i].0 * r[i]))).collect()
        };
        b.add_polygon(&side(1.0), true);
        b.add_polygon(&side(-1.0).into_iter().rev().collect::<Vec<_>>(), true);
    }
    b.detach()
}

/// The ring around the polyline `pos` with unit tangents `tan`, offset by `r[i]` on each side,
/// with round caps at both ends.
fn capped_ring(pos: &[P], tan: &[P], r: &[f64]) -> Vec<P> {
    let n = pos.len();
    let normal = |i: usize| (-tan[i].1, tan[i].0);
    let mut ring: Vec<P> = Vec::with_capacity(2 * n + 24);
    for i in 0..n {
        let (nx, ny) = normal(i);
        ring.push((pos[i].0 + nx * r[i], pos[i].1 + ny * r[i]));
    }
    let thetas: Vec<f64> = (1..13).map(|k| PI * k as f64 / 13.0).collect();
    let (e, ne, te, re) = (pos[n - 1], normal(n - 1), tan[n - 1], r[n - 1]);
    for th in &thetas {
        ring.push((
            e.0 + re * (th.cos() * ne.0 + th.sin() * te.0),
            e.1 + re * (th.cos() * ne.1 + th.sin() * te.1),
        ));
    }
    for i in (0..n).rev() {
        let (nx, ny) = normal(i);
        ring.push((pos[i].0 - nx * r[i], pos[i].1 - ny * r[i]));
    }
    let (s0, n0, t0, r0) = (pos[0], normal(0), tan[0], r[0]);
    for th in &thetas {
        ring.push((
            s0.0 + r0 * (-th.cos() * n0.0 - th.sin() * t0.0),
            s0.1 + r0 * (-th.cos() * n0.1 - th.sin() * t0.1),
        ));
    }
    ring
}

pub fn polygon(pts: &[P]) -> Path {
    let pts: Vec<Point> = pts.iter().map(|p| pt(*p)).collect();
    let mut b = PathBuilder::new();
    b.add_polygon(&pts, true);
    b.detach()
}

/// Displaces the outline along its normal by smooth noise of about `amount` units.
pub fn wobble(path: &Path, amount: f64, rng: &mut Rng, wavelength: f64) -> Path {
    let mut b = PathBuilder::new();
    for c in sample(path, (wavelength / 4.0).max(2.0)) {
        let n = if c.closed { c.pos.len() - 1 } else { c.pos.len() };
        if n < 3 {
            continue;
        }
        let knots = (n / 4).max(3);
        let vals: Vec<f64> = (0..knots).map(|_| rng.uniform(-1.0, 1.0)).collect();
        let pts: Vec<P> = (0..n)
            .map(|i| {
                let t = i as f64 / n as f64 * knots as f64;
                let i0 = t.floor() as usize;
                let f = t - i0 as f64;
                let f = f * f * (3.0 - 2.0 * f);
                let a = vals[i0 % knots];
                let bv = if c.closed { vals[(i0 + 1) % knots] } else { vals[(i0 + 1).min(knots - 1)] };
                let disp = (a + (bv - a) * f) * amount;
                (c.pos[i].0 - c.tan[i].1 * disp, c.pos[i].1 + c.tan[i].0 * disp)
            })
            .collect();
        b.add_path(&spline(&pts, 0.5, c.closed), None);
    }
    b.detach()
}

/// The stretch of `path` from `from` to `to`, fractions 0..1 of the total contour length in path
/// order. A position `p` sits at `seam + p` around the path. A stretch with `from > to` wraps
/// through the seam, and `from == to` is empty. Each contour piece is its own open subpath, but
/// a lone closed contour wraps without a break.
pub fn edge(path: &Path, seam: f64, from: f64, to: f64) -> Path {
    let measures: Vec<_> = skia_safe::ContourMeasureIter::new(path, false, None)
        .filter(|m| m.length() > 0.0)
        .collect();
    let total: f64 = measures.iter().map(|m| f64::from(m.length())).sum();
    let mut b = PathBuilder::new();
    if from == to || total <= 0.0 {
        return b.detach();
    }
    let span = if to > from { to - from } else { to - from + 1.0 };
    let start = (seam + from).rem_euclid(1.0) * total;
    let end = start + span * total;
    let ranges = if end <= total { vec![(start, end)] } else { vec![(start, total), (0.0, end - total)] };
    let lone_loop = measures.len() == 1 && measures[0].is_closed();
    for (i, &(lo, hi)) in ranges.iter().enumerate() {
        let mut offset = 0.0;
        for m in &measures {
            let len = f64::from(m.length());
            let (a, z) = (lo.max(offset) - offset, hi.min(offset + len) - offset);
            offset += len;
            if z > a {
                let _ = m.get_segment(a as f32, z as f32, &mut b, !(lone_loop && i == 1));
            }
        }
    }
    b.detach()
}

/// Where the point of `path` nearest `near` lies, as a fraction 0..1 of the total contour length
/// in path order.
pub fn seam_near(path: &Path, near: P) -> f64 {
    let measures: Vec<_> = skia_safe::ContourMeasureIter::new(path, false, None).collect();
    let total: f64 = measures.iter().map(|m| f64::from(m.length())).sum();
    if total <= 0.0 {
        return 0.0;
    }
    let dist = |m: &skia_safe::ContourMeasure, s: f64| {
        let len = f64::from(m.length());
        let s = if m.is_closed() { s.rem_euclid(len) } else { s.clamp(0.0, len) };
        let (p, _) = m.pos_tan(s as f32).unwrap_or_default();
        (f64::from(p.x) - near.0).hypot(f64::from(p.y) - near.1)
    };
    let (mut best, mut best_at, mut offset) = (f64::INFINITY, 0.0, 0.0);
    for m in &measures {
        let len = f64::from(m.length());
        if len <= 0.0 {
            continue;
        }
        let n = (len / (len / 4096.0).min(0.125)).ceil().clamp(2.0, 65536.0) as usize;
        let step = len / n as f64;
        let (mut lo_d, mut at) = (f64::INFINITY, 0.0);
        for i in 0..=n {
            let d = dist(m, step * i as f64);
            if d < lo_d {
                (lo_d, at) = (d, step * i as f64);
            }
        }
        let (mut lo, mut hi) = (at - step, at + step);
        for _ in 0..40 {
            let (a, c) = (lo + (hi - lo) / 3.0, hi - (hi - lo) / 3.0);
            if dist(m, a) <= dist(m, c) {
                hi = c;
            } else {
                lo = a;
            }
        }
        let refined = (lo + hi) / 2.0;
        let (d, refined) = if dist(m, refined) <= lo_d { (dist(m, refined), refined) } else { (lo_d, at) };
        if d < best {
            let s = if m.is_closed() { refined.rem_euclid(len) } else { refined.clamp(0.0, len) };
            (best, best_at) = (d, offset + s);
        }
        offset += len;
    }
    (best_at / total).rem_euclid(1.0)
}

/// Organic closed shape: `points` radii scaled by `1 + wobble*u`, angles jittered.
pub fn blob(r: f64, points: usize, wobble: f64, rng: &mut Rng) -> Path {
    let n = points.max(3);
    let step = 2.0 * PI / n as f64;
    let pts: Vec<P> = (0..n)
        .map(|i| {
            let rr = r * (1.0 + wobble * rng.uniform(-1.0, 1.0));
            let a = -PI / 2.0 + i as f64 * step + rng.uniform(-1.0, 1.0) * wobble * 0.5 * step;
            (rr * a.cos(), rr * a.sin())
        })
        .collect();
    spline(&pts, 0.5, true)
}

/// Regular polygon, or a star when `inner` is set (fraction of `r`). The first vertex points up.
pub fn regular(n: usize, r: f64, inner: Option<f64>) -> Path {
    let n = n.max(2);
    let count = if inner.is_some() { 2 * n } else { n };
    let pts: Vec<P> = (0..count)
        .map(|i| {
            let a = -PI / 2.0 + i as f64 * 2.0 * PI / count as f64;
            let rr = match inner {
                Some(f) if i % 2 == 1 => r * f,
                _ => r,
            };
            (rr * a.cos(), rr * a.sin())
        })
        .collect();
    polygon(&pts)
}

pub fn round_corners(path: &Path, r: f64) -> Path {
    let Some(effect) = PathEffect::corner_path(r as f32) else {
        return path.clone();
    };
    let cull = path.compute_tight_bounds().with_outset((r as f32 + 1.0, r as f32 + 1.0));
    match effect.filter_path(path, &StrokeRec::new_fill(), cull) {
        Some((mut b, _)) => b.detach(),
        None => path.clone(),
    }
}

/// Centered rect with per-corner radii (top-left, top-right, bottom-right, bottom-left).
pub fn rect(w: f64, h: f64, radii: [f64; 4]) -> Path {
    let r = Rect::from_xywh((-w / 2.0) as f32, (-h / 2.0) as f32, w as f32, h as f32);
    let mut b = PathBuilder::new();
    if radii.iter().all(|v| *v <= 0.0) {
        b.add_rect(r, None, None);
    } else {
        let v = radii.map(|x| Vector::new(x as f32, x as f32));
        b.add_rrect(RRect::new_rect_radii(r, &v), None, None);
    }
    b.detach()
}

pub fn ellipse(rx: f64, ry: f64) -> Path {
    let mut b = PathBuilder::new();
    b.add_oval(Rect::from_ltrb(-rx as f32, -ry as f32, rx as f32, ry as f32), None, None);
    b.detach()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoolOp {
    Subtract,
    Union,
    Intersect,
    Xor,
}

/// Folds the operands left to right: `subtract` is the first minus the rest.
pub fn boolean(op: BoolOp, operands: &[Path]) -> Path {
    let Some((first, rest)) = operands.split_first() else {
        return Path::new();
    };
    let skop = match op {
        BoolOp::Subtract => PathOp::Difference,
        BoolOp::Union => PathOp::Union,
        BoolOp::Intersect => PathOp::Intersect,
        BoolOp::Xor => PathOp::XOR,
    };
    rest.iter().fold(first.clone(), |acc, p| skia_safe::op(&acc, p, skop).unwrap_or(acc))
}

pub fn union_all(paths: &[Path]) -> Path {
    match paths {
        [] => Path::new(),
        [one] => one.clone(),
        _ => boolean(BoolOp::Union, paths),
    }
}

/// Bounds of a path, or an empty rect at the origin for an empty path.
pub fn bounds(path: &Path) -> Rect {
    if path.is_empty() { Rect::new_empty() } else { path.compute_tight_bounds() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contains(path: &Path, p: P) -> bool {
        sample(path, 0.25)
            .iter()
            .flat_map(|c| c.pos.iter())
            .any(|q| (q.0 - p.0).abs() < 0.3 && (q.1 - p.1).abs() < 0.3)
    }

    #[test]
    fn curve_passes_through_its_points() {
        let pts = [(0.0, 0.0), (20.0, -15.0), (40.0, 0.0)];
        let path = spline(&pts, 0.5, false);
        for p in pts {
            assert!(contains(&path, p), "curve misses {p:?}");
        }
        let b = path.compute_tight_bounds();
        assert_eq!((b.left, b.right), (0.0, 40.0));
        assert!((b.top + 15.0).abs() < 0.5, "top {}", b.top);
    }

    #[test]
    fn tapered_stroke_is_full_width_in_the_middle() {
        let line = spline(&[(0.0, 0.0), (100.0, 0.0)], 0.5, false);
        let knots = [vec![(0.0, 0.0), (100.0, 0.0)]];
        let spine = crate::brush::Spine::new(&line, &knots, &[8.0, 8.0], &Default::default());
        let ribbon = crate::brush::ribbon(&spine, (0.3, 0.3), Vec::new());
        let outline = ribbon_ink(&line, &ribbon, 0.75);
        let probe = |y: f32| outline.contains(Point::new(50.0, y));
        assert!(probe(3.9) && probe(-3.9));
        assert!(!probe(4.2) && !probe(-4.2));
        assert!(!outline.contains(Point::new(2.0, 3.0)), "ends taper");
    }

    #[test]
    fn subtract_removes_the_overlap() {
        let a = rect(20.0, 20.0, [0.0; 4]);
        let b = rect(20.0, 20.0, [0.0; 4]).make_offset((10.0, 0.0));
        let out = boolean(BoolOp::Subtract, &[a, b]);
        assert_eq!(bounds(&out), Rect::from_ltrb(-10.0, -10.0, 0.0, 10.0));
    }

    #[test]
    fn star_first_vertex_points_up() {
        let s = regular(4, 20.0, Some(0.5));
        let p = s.points()[0];
        assert!(p.x.abs() < 1e-4 && p.y == -20.0, "{p:?}");
        assert_eq!(s.count_points(), 8);
    }

    #[test]
    fn poly_first_vertex_points_up() {
        let s = regular(3, 30.0, None);
        let p = s.points()[0];
        assert!(p.x.abs() < 1e-4 && p.y == -30.0, "{p:?}");
        assert_eq!(s.count_points(), 3);
    }
}
