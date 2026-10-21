//! `warp:`: a node's non-affine placement, applied to everything it draws before `transform`.
//!
//! Eval resolves each stage against the box it maps once, and the IR stores the resolved stack.
//! `ramose-geom` maps geometry through it.

use crate::{Point, Rect};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
}

/// One stage as the author wrote it. Eval values hold these; resolving binds them to a box.
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

impl Corners {
    pub fn try_new(quad: [Point; 4]) -> Result<Corners, CornersError> {
        if quad.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
            return Err(CornersError::Degenerate);
        }
        let extent = quad
            .iter()
            .flat_map(|p| [p.x.abs(), p.y.abs()])
            .fold(0.0_f32, f32::max)
            .max(1.0);
        let flat = f64::from(extent) * f64::from(extent) * 1e-9;
        let turn = |i: usize| cross(quad[i], quad[(i + 1) % 4], quad[(i + 2) % 4]);
        if (0..4).any(|i| turn(i).abs() <= flat) {
            return Err(CornersError::Degenerate);
        }
        if crosses(quad[0], quad[1], quad[2], quad[3])
            || crosses(quad[1], quad[2], quad[3], quad[0])
        {
            return Err(CornersError::SelfCrossing);
        }
        // A projective map keeps every corner of a rectangle in front of the horizon exactly
        // when the quad turns the same way at all four corners.
        if (1..4).any(|i| (turn(i) > 0.0) != (turn(0) > 0.0)) {
            return Err(CornersError::Concave);
        }
        Ok(Corners { quad })
    }

    pub fn quad(&self) -> [Point; 4] {
        self.quad
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
}

/// A stage bound to the box it maps: `src` is the node's reference box for stage 0 and the
/// previous stage's image of its own `src` after that.
#[derive(Clone, Debug, PartialEq)]
pub struct Stage {
    pub kind: WarpStage,
    pub src: Rect,
}

impl Stage {
    /// Census name as E0209 prints it.
    pub fn feature(&self) -> &'static str {
        match &self.kind {
            WarpStage::Corners(_) => "warp corners",
            WarpStage::Cylinder(_) => "warp cylinder",
        }
    }
}

/// A node's placed warp: resolved stages in application order. Empty is no warp
/// ([`Warp::default`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Warp(Vec<Stage>);

impl Warp {
    /// `stages` in application order, each `src` the previous stage's image of its own `src`.
    pub fn from_stages(stages: Vec<Stage>) -> Warp {
        Warp(stages)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(x: f32, y: f32) -> Point {
        Point::new(x, y)
    }

    #[test]
    fn a_bow_tie_and_a_collinear_quad_are_rejected_and_a_flipped_one_is_not() {
        let bow_tie = [pt(0.0, 0.0), pt(10.0, 0.0), pt(0.0, 10.0), pt(10.0, 10.0)];
        assert_eq!(Corners::try_new(bow_tie).err(), Some(CornersError::SelfCrossing));
        let collinear = [pt(0.0, 0.0), pt(5.0, 0.0), pt(10.0, 0.0), pt(0.0, 10.0)];
        assert_eq!(Corners::try_new(collinear).err(), Some(CornersError::Degenerate));
        let flat = [pt(0.0, 0.0), pt(10.0, 0.0), pt(10.0, 0.0), pt(0.0, 0.0)];
        assert_eq!(Corners::try_new(flat).err(), Some(CornersError::Degenerate));
        let dart = [pt(0.0, 0.0), pt(10.0, 0.0), pt(5.0, 3.0), pt(0.0, 10.0)];
        assert_eq!(Corners::try_new(dart).err(), Some(CornersError::Concave));
        let nan = [pt(f32::NAN, 0.0), pt(10.0, 0.0), pt(10.0, 10.0), pt(0.0, 10.0)];
        assert_eq!(Corners::try_new(nan).err(), Some(CornersError::Degenerate));
        let flipped = [pt(0.0, 0.0), pt(0.0, 10.0), pt(10.0, 10.0), pt(10.0, 0.0)];
        assert!(Corners::try_new(flipped).is_ok());
    }
}
