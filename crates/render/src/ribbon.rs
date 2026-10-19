//! The `(s, d)` field of one ribbon contour over a device-pixel grid, as two F16 images a brush
//! shader samples. The spine is flattened in local space to a device tolerance, each segment is
//! binned by the cells within the ribbon's reach, and every device pixel takes the nearest
//! segment: its arc length, its signed offset, the half width there and the channels there. CPU
//! only, so both backends read the same texels. Cost follows the pixels within reach, not the
//! pixels times the segments.

use std::collections::HashMap;

use artscript_brush::S_CHUNK;
use artscript_ir::{Knot, RibbonContour, taper_ramp};
use skia_safe::{AlphaType, ColorType, ContourMeasure, Data, IRect, ISize, Image, ImageInfo, Matrix, Point, RoundOut, images};

use crate::RenderError;
use crate::sdf::f16_bits;

/// Device pixels of flattening tolerance and of the margin kept past the ribbon's reach.
const TOLERANCE_PX: f32 = 0.05;
const MARGIN_PX: f32 = 1.0;
/// Device pixels per bin cell.
const CELL_PX: f32 = 16.0;
/// Device rows per strip the field is built and drawn in, so a long diagonal ribbon costs its
/// own pixels and not its bounding box.
pub(crate) const STRIP_ROWS: i32 = 64;

/// What one contour's field depends on. Different contours of one path share a path and matrix.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) struct FieldKey {
    path: Vec<u8>,
    matrix: [u32; 9],
    rect: (i32, i32, i32, i32),
    contour: usize,
    data: Vec<u32>,
}

impl FieldKey {
    pub(crate) fn new(path: &skia_safe::Path, ltd: &Matrix, rect: IRect, spec: &Spec<'_>) -> FieldKey {
        let m = [
            ltd.scale_x(), ltd.skew_x(), ltd.translate_x(),
            ltd.skew_y(), ltd.scale_y(), ltd.translate_y(),
            ltd.persp_x(), ltd.persp_y(), ltd[8],
        ];
        let mut data: Vec<u32> = vec![spec.taper[0], spec.taper[1], spec.reach].into_iter().map(f32::to_bits).collect();
        for k in &spec.contour.knots {
            data.extend([k.s, k.half_width, k.pressure, k.angle, k.tilt, k.velocity].map(f32::to_bits));
        }
        FieldKey {
            path: path.serialize().as_bytes().to_vec(),
            matrix: m.map(f32::to_bits),
            rect: (rect.left, rect.top, rect.right, rect.bottom),
            contour: spec.index,
            data,
        }
    }
}

/// One contour of a ribbon as the field needs it.
pub(crate) struct Spec<'a> {
    /// Index of the contour in the path, as `ContourMeasureIter` yields them.
    pub index: usize,
    pub contour: &'a RibbonContour,
    pub taper: [f32; 2],
    /// Local units the stages may paint past the ribbon's edge.
    pub reach: f32,
}

impl Spec<'_> {
    /// The widest the ribbon gets, in local units.
    pub fn max_half_width(&self) -> f32 {
        self.contour.knots.iter().map(|k| k.half_width).fold(0.0, f32::max)
    }
}

/// The field over one device rect: texel `(i, j)` describes device pixel `(rect.left + i,
/// rect.top + j)`. `field` holds `s` in whole chunks of [`S_CHUNK`] device pixels, the rest of
/// `s`, the signed offset `d` and the half width, all in device pixels; a negative rest marks a
/// pixel past the ribbon's reach. `chan` holds pressure, angle, tilt and velocity.
#[derive(Clone)]
pub(crate) struct Field {
    pub field: Image,
    pub chan: Image,
}

#[derive(Default)]
pub(crate) struct FieldMemo {
    fields: HashMap<FieldKey, Field>,
}

impl FieldMemo {
    pub(crate) fn get(&mut self, spine: &Spine, rect: IRect, spec: &Spec<'_>) -> Result<Field, RenderError> {
        let key = FieldKey::new(&spine.path, &spine.ltd, rect, spec);
        if let Some(f) = self.fields.get(&key) {
            return Ok(f.clone());
        }
        let f = build(spine, rect, spec)?;
        self.fields.insert(key, f.clone());
        Ok(f)
    }
}

/// A segment of the flattened spine in local units. `s0..s1` is its arc length in the contour's
/// own measure, as the knots count it.
struct Seg {
    p0: Point,
    dir: Point,
    len: f32,
    s0: f32,
    s1: f32,
}

/// One contour of a path flattened in local space to [`TOLERANCE_PX`] device pixels, with the
/// segments binned by the cells they can reach, ready to be sampled over any device rect.
pub(crate) struct Spine {
    path: skia_safe::Path,
    ltd: Matrix,
    dtl: Matrix,
    /// Local units per device pixel.
    px: f32,
    segs: Vec<Seg>,
    closed: bool,
    /// Local units past the spine a pixel may lie and still take a segment.
    radius: f32,
    cell: f32,
    grid_origin: Point,
    grid_w: i32,
    grid_h: i32,
    bins: Vec<Vec<u32>>,
}

impl Spine {
    /// Flattens contour `spec.index` of `path`. `None` when the path has no such contour or it
    /// has no length.
    pub(crate) fn new(path: &skia_safe::Path, ltd: &Matrix, px: f32, spec: &Spec<'_>, device: IRect) -> Option<Spine> {
        let dtl = ltd.invert()?;
        let measure: ContourMeasure =
            skia_safe::ContourMeasureIter::new(path, false, Some(10.0 / px)).nth(spec.index)?;
        let measured = measure.length();
        if measured <= 0.0 {
            return None;
        }
        let scale = spec.contour.len / measured;
        let step = 0.5 * px;
        let n = ((measured / step).ceil() as usize).max(1);
        let mut pts: Vec<(Point, f32)> = Vec::with_capacity(n + 1);
        for i in 0..=n {
            let s = (measured * i as f32 / n as f32).min(measured);
            let (p, _) = measure.pos_tan(s)?;
            pts.push((p, s * scale));
        }
        let segs = simplify(&pts, TOLERANCE_PX * px);
        let radius = spec.max_half_width() + spec.reach + MARGIN_PX * px;
        let cell = CELL_PX * px;
        let local = dtl.map_rect(skia_safe::Rect::from_irect(device)).0;
        let grid_origin = Point::new(local.left, local.top);
        let grid_w = ((local.width() / cell).ceil() as i32).max(1);
        let grid_h = ((local.height() / cell).ceil() as i32).max(1);
        let mut bins = vec![Vec::new(); (grid_w * grid_h) as usize];
        for (i, seg) in segs.iter().enumerate() {
            let p1 = seg.p0 + seg.dir * seg.len;
            let (x0, x1) = (seg.p0.x.min(p1.x) - radius, seg.p0.x.max(p1.x) + radius);
            let (y0, y1) = (seg.p0.y.min(p1.y) - radius, seg.p0.y.max(p1.y) + radius);
            let cx0 = (((x0 - grid_origin.x) / cell).floor() as i32).clamp(0, grid_w - 1);
            let cx1 = (((x1 - grid_origin.x) / cell).floor() as i32).clamp(0, grid_w - 1);
            let cy0 = (((y0 - grid_origin.y) / cell).floor() as i32).clamp(0, grid_h - 1);
            let cy1 = (((y1 - grid_origin.y) / cell).floor() as i32).clamp(0, grid_h - 1);
            if x1 < grid_origin.x || y1 < grid_origin.y || x0 > local.right || y0 > local.bottom {
                continue;
            }
            for cy in cy0..=cy1 {
                for cx in cx0..=cx1 {
                    bins[(cy * grid_w + cx) as usize].push(i as u32);
                }
            }
        }
        Some(Spine {
            path: path.clone(),
            ltd: *ltd,
            dtl,
            px,
            segs,
            closed: spec.contour.closed,
            radius,
            cell,
            grid_origin,
            grid_w,
            grid_h,
            bins,
        })
    }

    /// The part of the device rect `r` that some segment reaches: the device bounds of its live
    /// cells, clipped to `r`. `None` when no segment reaches it.
    pub(crate) fn live_rect(&self, r: IRect) -> Option<IRect> {
        let local = self.dtl.map_rect(skia_safe::Rect::from_irect(r)).0;
        let cell_at = |v: f32, origin: f32, n: i32| (((v - origin) / self.cell).floor() as i32).clamp(0, n - 1);
        let (cx0, cx1) = (cell_at(local.left, self.grid_origin.x, self.grid_w), cell_at(local.right, self.grid_origin.x, self.grid_w));
        let (cy0, cy1) = (cell_at(local.top, self.grid_origin.y, self.grid_h), cell_at(local.bottom, self.grid_origin.y, self.grid_h));
        let mut live: Option<skia_safe::Rect> = None;
        for cy in cy0..=cy1 {
            for cx in cx0..=cx1 {
                if self.bins[(cy * self.grid_w + cx) as usize].is_empty() {
                    continue;
                }
                let cell = skia_safe::Rect::from_xywh(
                    self.grid_origin.x + cx as f32 * self.cell,
                    self.grid_origin.y + cy as f32 * self.cell,
                    self.cell,
                    self.cell,
                );
                let dev = self.ltd.map_rect(cell).0;
                live = Some(live.map_or(dev, |l| skia_safe::Rect::join2(l, dev)));
            }
        }
        let live: IRect = live?.round_out();
        IRect::intersect(&live, &r).filter(|r| !r.is_empty())
    }

    /// The nearest point of the spine to local `q`: arc length (unclamped past an open end),
    /// signed offset (positive on the right of travel on a y-down canvas), and the distance that
    /// chose the segment. `None` past the reach.
    fn nearest(&self, q: Point) -> Option<(f32, f32)> {
        let cx = ((q.x - self.grid_origin.x) / self.cell).floor() as i32;
        let cy = ((q.y - self.grid_origin.y) / self.cell).floor() as i32;
        if cx < 0 || cy < 0 || cx >= self.grid_w || cy >= self.grid_h {
            return None;
        }
        let mut best: Option<(f32, f32, f32)> = None;
        let last = self.segs.len() - 1;
        for &i in &self.bins[(cy * self.grid_w + cx) as usize] {
            let seg = &self.segs[i as usize];
            let r = q - seg.p0;
            let along = r.x * seg.dir.x + r.y * seg.dir.y;
            let across = seg.dir.x * r.y - seg.dir.y * r.x;
            let t = along.clamp(0.0, seg.len);
            let over = along - t;
            let dist = (over * over + across * across).sqrt();
            if best.is_some_and(|(d, _, _)| dist >= d) {
                continue;
            }
            let i = i as usize;
            let free = !self.closed && ((i == 0 && along < 0.0) || (i == last && along > seg.len));
            let (s, d) = if free {
                (seg.s0 + along / seg.len * (seg.s1 - seg.s0), across)
            } else {
                (seg.s0 + t / seg.len * (seg.s1 - seg.s0), dist.copysign(across))
            };
            best = Some((dist, s, d));
        }
        best.filter(|(dist, _, _)| *dist <= self.radius).map(|(_, s, d)| (s, d))
    }
}

/// The polyline through `pts` with every vertex that lies within `tol` of the chord between its
/// kept neighbours dropped. Each point carries its arc length, which the segments keep.
fn simplify(pts: &[(Point, f32)], tol: f32) -> Vec<Seg> {
    const MAX_RUN: usize = 128;
    let mut segs = Vec::new();
    let mut a = 0;
    while a + 1 < pts.len() {
        let mut b = a + 1;
        while b + 1 < pts.len() && b - a < MAX_RUN {
            let (pa, pb) = (pts[a].0, pts[b + 1].0);
            let chord = pb - pa;
            let len = chord.length();
            let fits = len > 0.0
                && pts[a + 1..=b].iter().all(|(p, _)| {
                    let r = *p - pa;
                    ((chord.x * r.y - chord.y * r.x) / len).abs() <= tol
                        && (r.x * chord.x + r.y * chord.y) >= 0.0
                });
            if !fits {
                break;
            }
            b += 1;
        }
        let (p0, s0) = pts[a];
        let (p1, s1) = pts[b];
        let chord = p1 - p0;
        let len = chord.length();
        if len > 0.0 {
            segs.push(Seg { p0, dir: chord * (1.0 / len), len, s0, s1 });
        }
        a = b;
    }
    if segs.is_empty() {
        let (p0, s0) = pts[0];
        segs.push(Seg { p0, dir: Point::new(1.0, 0.0), len: 1e-6, s0, s1: s0 });
    }
    segs
}

/// Encodes `s` in device pixels as whole chunks and a rest, both exact in a half float.
pub(crate) fn split_s(s_px: f32) -> (f32, f32) {
    let hi = (s_px / S_CHUNK).floor();
    (hi, s_px - hi * S_CHUNK)
}

fn build(spine: &Spine, rect: IRect, spec: &Spec<'_>) -> Result<Field, RenderError> {
    let (w, h) = (rect.width().max(1) as usize, rect.height().max(1) as usize);
    let mut field = Vec::with_capacity(w * h * 8);
    let mut chan = Vec::with_capacity(w * h * 8);
    let contour = spec.contour;
    let px = spine.px;
    for j in 0..h {
        for i in 0..w {
            let dev = Point::new(rect.left as f32 + i as f32 + 0.5, rect.top as f32 + j as f32 + 0.5);
            let q = spine.dtl.map_point(dev);
            let Some((s, d)) = spine.nearest(q) else {
                field.extend([0.0, -1.0, 0.0, 0.0].map(|v| f16_bits(v).to_ne_bytes()).into_iter().flatten());
                chan.extend([0u8; 8]);
                continue;
            };
            let on = s.clamp(0.0, contour.len);
            let k: Knot = contour.at(on);
            let ramp = taper_ramp(f64::from(on / contour.len.max(1e-9)), (f64::from(spec.taper[0]), f64::from(spec.taper[1])));
            let hw = k.half_width * ramp as f32;
            let (hi, lo) = split_s(s / px);
            field.extend([hi, lo, d / px, hw / px].map(|v| f16_bits(v).to_ne_bytes()).into_iter().flatten());
            chan.extend([k.pressure, k.angle, k.tilt, k.velocity].map(|v| f16_bits(v).to_ne_bytes()).into_iter().flatten());
        }
    }
    let info = ImageInfo::new(ISize::new(w as i32, h as i32), ColorType::RGBAF16, AlphaType::Unpremul, None);
    let image = |bytes: &[u8]| {
        images::raster_from_data(&info, Data::new_copy(bytes), w * 8).ok_or_else(|| RenderError::Skia("ribbon field image".into()))
    };
    Ok(Field { field: image(&field)?, chan: image(&chan)? })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sdf::half_to_f32;
    use skia_safe::PathBuilder;

    fn contour(len: f32, closed: bool, hw: f32) -> RibbonContour {
        let knot = |s: f32| Knot { s, half_width: hw, pressure: 1.0, angle: 0.0, tilt: 0.0, velocity: 1.0 };
        RibbonContour { len, closed, knots: vec![knot(0.0), knot(len)] }
    }

    struct Texels {
        w: usize,
        field: Vec<u8>,
        chan: Vec<u8>,
    }

    impl Texels {
        fn read(f: &Field, rect: IRect) -> Texels {
            let (w, h) = (rect.width() as usize, rect.height() as usize);
            let info = ImageInfo::new(ISize::new(w as i32, h as i32), ColorType::RGBAF16, AlphaType::Unpremul, None);
            let mut field = vec![0u8; w * h * 8];
            let mut chan = vec![0u8; w * h * 8];
            assert!(f.field.read_pixels(&info, &mut field, w * 8, (0, 0), skia_safe::image::CachingHint::Disallow));
            assert!(f.chan.read_pixels(&info, &mut chan, w * 8, (0, 0), skia_safe::image::CachingHint::Disallow));
            Texels { w, field, chan }
        }

        fn at(&self, buf: &[u8], x: usize, y: usize) -> [f32; 4] {
            let i = (y * self.w + x) * 8;
            [0, 2, 4, 6].map(|c| half_to_f32(u16::from_ne_bytes([buf[i + c], buf[i + c + 1]])))
        }

        /// `(s, d, hw)` in device pixels at texel `(x, y)`, or `None` past the reach.
        fn sd(&self, x: usize, y: usize) -> Option<(f32, f32, f32)> {
            let [hi, lo, d, hw] = self.at(&self.field, x, y);
            (lo >= 0.0).then_some((hi * S_CHUNK + lo, d, hw))
        }

        /// Pressure, angle, tilt and velocity at texel `(x, y)`.
        fn channels(&self, x: usize, y: usize) -> [f32; 4] {
            self.at(&self.chan, x, y)
        }
    }

    fn field_of(path: &skia_safe::Path, ltd: Matrix, contour: &RibbonContour, reach: f32, rect: IRect) -> Field {
        let spec = Spec { index: 0, contour, taper: [0.0, 0.0], reach };
        let px = 1.0 / ltd.scale_x();
        let spine = Spine::new(path, &ltd, px, &spec, rect).expect("spine");
        build(&spine, rect, &spec).unwrap()
    }

    #[test]
    fn s_is_exact_to_a_twentieth_of_a_pixel_along_a_4000_px_line() {
        let mut b = PathBuilder::new();
        b.move_to((5.0, 8.0));
        b.line_to((4005.0, 8.0));
        let path = b.detach();
        let rect = IRect::from_ltrb(0, 0, 4010, 16);
        let f = field_of(&path, Matrix::new_identity(), &contour(4000.0, false, 3.0), 0.0, rect);
        let t = Texels::read(&f, rect);
        let mut worst = 0.0f32;
        for x in 5..4005 {
            let (s, d, hw) = t.sd(x, 8).expect("within reach");
            let expect = x as f32 + 0.5 - 5.0;
            worst = worst.max((s - expect).abs());
            assert_eq!(d, 0.5, "x={x}: half a pixel right of the center line");
            assert_eq!(hw, 3.0);
        }
        assert!(worst < 0.05, "worst s error {worst} px");
        assert_eq!(t.channels(2000, 8), [1.0, 0.0, 0.0, 1.0], "the knots' channels at the texel");
        assert_eq!(t.channels(0, 8), [0.0; 4], "nothing past the reach");
        let (s, _, _) = t.sd(2, 8).unwrap();
        assert!((s - -2.5).abs() < 0.05, "past the start s keeps running: {s}");
        assert!(t.sd(0, 8).is_none(), "4.5 px before the start is past reach 3 + 1");
        assert!(t.sd(2000, 12).is_none(), "4.5 px off the line is past reach 3 + 1");
        assert!(t.sd(2000, 11).is_some(), "3.5 px off the line is within the 1 px margin");
    }

    #[test]
    fn the_nearest_segment_wins_where_the_spine_crosses_itself() {
        let mut b = PathBuilder::new();
        b.move_to((0.0, 50.0));
        b.line_to((100.0, 50.0));
        b.line_to((100.0, 100.0));
        b.line_to((50.0, 100.0));
        b.line_to((50.0, 0.0));
        let path = b.detach();
        let rect = IRect::from_ltrb(0, 0, 110, 110);
        let len = 100.0 + 50.0 + 50.0 + 100.0;
        let f = field_of(&path, Matrix::new_identity(), &contour(len, false, 4.0), 0.0, rect);
        let t = Texels::read(&f, rect);
        let (s, d, _) = t.sd(45, 48).unwrap();
        assert!((s - 45.5).abs() < 0.05 && (d - -1.5).abs() < 1e-3, "1.5 px above the first leg, 4.5 from the last: the first wins: s {s} d {d}");
        let (s, d, _) = t.sd(52, 44).unwrap();
        assert!((s - 255.5).abs() < 0.05, "5.5 px above the first leg, 2.5 right of the last: the last wins, counting upward: s {s}");
        assert!((d - 2.5).abs() < 1e-3, "screen-right of an upward leg is its left-hand side: d {d}");
        let (s, d, _) = t.sd(48, 20).unwrap();
        assert!((s - (len - 20.5)).abs() < 0.05 && (d - -1.5).abs() < 1e-3, "s {s} d {d}");
        let (s, _, _) = t.sd(51, 48).unwrap();
        assert!((s - 51.5).abs() < 0.05, "1.5 px from both legs: the earlier segment wins the tie: s {s}");
        let (s, d, _) = t.sd(103, 103).unwrap();
        assert!((s - 150.0).abs() < 0.05 && (d.abs() - 3.5f32.hypot(3.5)).abs() < 0.01, "outside a corner: s at the corner, |d| the distance to it: s {s} d {d}");
    }

    #[test]
    fn a_closed_contour_wraps_and_a_scaled_matrix_keeps_device_precision() {
        let path = skia_safe::Path::rect(skia_safe::Rect::from_xywh(10.0, 10.0, 40.0, 20.0), None);
        let ltd = Matrix::scale((3.0, 3.0));
        let rect = IRect::from_ltrb(0, 0, 180, 120);
        let f = field_of(&path, ltd, &contour(120.0, true, 2.0), 1.0, rect);
        let t = Texels::read(&f, rect);
        let (s, d, hw) = t.sd(30, 30).unwrap();
        assert!((s - 0.5).abs() < 0.05, "a sixth of a unit in from the top-left corner, along the top edge: s {s}");
        assert!((d - 0.5).abs() < 1e-3, "{d}");
        assert_eq!(hw, 6.0, "2 local units at 3x is 6 px");
        let (s, d, _) = t.sd(28, 60).unwrap();
        assert!((s - 329.5).abs() < 0.05, "half a unit left of the left edge, 9.83 units up it: s {s}");
        assert!((d - -1.5).abs() < 1e-3, "left on screen of an upward edge is its right-hand side: d {d}");
        assert!(t.sd(90, 60).is_none(), "the rect's middle is past reach");
    }
}
