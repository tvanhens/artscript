//! Paint values resolved against a shape's bounds: solid colors, OKLCH-densified gradients and
//! field fills.

use std::rc::Rc;

use artscript_ir::{Paint, Point, Rect, Stop};

use crate::color::{Rgba, densify};
use crate::value::Value;

#[derive(Clone, Debug, PartialEq)]
pub enum GradKind {
    /// Direction in degrees, 0 = right, 90 = down. Fitted to the shape's bounds.
    Linear(f64),
    /// Start and end in the shape's local coordinates.
    Segment { from: (f64, f64), to: (f64, f64) },
    /// `at` is a fraction of the bounds; `r` is in units, defaulting to the farthest corner.
    Radial { at: (f64, f64), r: Option<f64> },
    Conic { at: (f64, f64), start: f64 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct GradSpec {
    pub kind: GradKind,
    pub stops: Vec<(Rgba, Option<f64>)>,
}

pub fn direction_deg(word: &str) -> Option<f64> {
    Some(match word {
        "right" => 0.0,
        "down" => 90.0,
        "left" => 180.0,
        "up" => 270.0,
        _ => return None,
    })
}

fn stops(spec: &[(Rgba, Option<f64>)]) -> Vec<Stop> {
    densify(spec)
        .into_iter()
        .map(|(c, pos)| Stop { pos: pos as f32, color: c.to_ir() })
        .collect()
}

pub fn resolve_gradient(g: &GradSpec, b: Rect) -> Paint {
    let (cx, cy) = (b.center_x() as f64, b.center_y() as f64);
    let (w, h) = (b.width() as f64, b.height() as f64);
    let at = |f: (f64, f64)| (b.left as f64 + f.0 * w, b.top as f64 + f.1 * h);
    let p = |x: f64, y: f64| Point::new(x as f32, y as f32);
    match &g.kind {
        GradKind::Linear(deg) => {
            let (dx, dy) = (deg.to_radians().cos(), deg.to_radians().sin());
            let half = (w * dx.abs() + h * dy.abs()) / 2.0;
            Paint::Linear {
                from: p(cx - dx * half, cy - dy * half),
                to: p(cx + dx * half, cy + dy * half),
                stops: stops(&g.stops),
            }
        }
        GradKind::Segment { from, to } => Paint::Linear {
            from: p(from.0, from.1),
            to: p(to.0, to.1),
            stops: stops(&g.stops),
        },
        GradKind::Radial { at: f, r } => {
            let (x, y) = at(*f);
            let far = [(b.left, b.top), (b.right, b.top), (b.left, b.bottom), (b.right, b.bottom)]
                .iter()
                .map(|(px, py)| (*px as f64 - x).hypot(*py as f64 - y))
                .fold(0.0, f64::max);
            Paint::Radial { center: p(x, y), radius: r.unwrap_or(far).max(1e-3) as f32, stops: stops(&g.stops) }
        }
        GradKind::Conic { at: f, start } => {
            let (x, y) = at(*f);
            Paint::Conic { center: p(x, y), start_deg: *start as f32, stops: stops(&g.stops) }
        }
    }
}

/// Converts a fill or stroke value to IR paint. `None` when the value is not a paint.
pub fn to_paint(v: &Value, bounds: Rect) -> Option<Paint> {
    Some(match v {
        Value::Gradient(g) => resolve_gradient(g, bounds),
        Value::FieldPaint(field, colors) => {
            let spec: Vec<(Rgba, Option<f64>)> = colors.iter().map(|c| (*c, None)).collect();
            Paint::Field { field: (**field).clone(), stops: stops(&spec) }
        }
        Value::Field(field) => {
            let spec = [(Rgba { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }, None), (Rgba { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }, None)];
            Paint::Field { field: (**field).clone(), stops: stops(&spec) }
        }
        other => Paint::Solid(other.color()?.rgba.to_ir()),
    })
}

/// The color effects derive their defaults from: the first solid color or first gradient stop.
pub fn key_color(p: &Paint) -> Rgba {
    let c = match p {
        Paint::Solid(c) => *c,
        Paint::Linear { stops, .. }
        | Paint::Radial { stops, .. }
        | Paint::Conic { stops, .. }
        | Paint::Field { stops, .. } => stops.first().map(|s| s.color).unwrap_or(Rgba::GRAY.to_ir()),
    };
    Rgba::from_ir(c)
}

pub fn gradient(kind: GradKind, stops: Vec<(Rgba, Option<f64>)>) -> Value {
    Value::Gradient(Rc::new(GradSpec { kind, stops }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn down_runs_top_center_to_bottom_center() {
        let g = GradSpec {
            kind: GradKind::Linear(90.0),
            stops: vec![(Rgba::GRAY, None), (Rgba::GRAY.with_alpha(0.5), None)],
        };
        let Paint::Linear { from, to, stops } = resolve_gradient(&g, Rect::from_ltrb(10.0, 20.0, 50.0, 100.0)) else {
            panic!("linear")
        };
        assert_eq!((from, to), (Point::new(30.0, 20.0), Point::new(30.0, 100.0)));
        assert_eq!(stops.len(), 17);
    }

    #[test]
    fn radial_defaults_to_the_farthest_corner() {
        let g = GradSpec { kind: GradKind::Radial { at: (0.0, 0.0), r: None }, stops: vec![(Rgba::GRAY, None)] };
        let Paint::Radial { center, radius, .. } = resolve_gradient(&g, Rect::from_ltrb(0.0, 0.0, 30.0, 40.0)) else {
            panic!("radial")
        };
        assert_eq!((center, radius), (Point::new(0.0, 0.0), 50.0));
    }
}
