//! `warp:` values: the `corners` and `cylinder` calls, and the stack a key holds.

use std::rc::Rc;

use ramose_geom::{ToIr, ToSk};
use ramose_ir::{Axis, Corners, CornersError, Cylinder, Matrix, Point, WarpStage};
use ramose_syntax::ast::Named;
use ramose_syntax::format_expr;

use crate::eval::{Fail, R, Scope, fail, ident_of};
use crate::value::{Coord, Value, num};

const ORDER: &str = "write the corners clockwise from the top-left: `corners(tl tr br bl)`";
const CYLINDER: &str = "write `cylinder(60)` or `cylinder(60 axis:x)`";

/// `corners(tl tr br bl)`: four points in canvas units, no keys.
pub(crate) fn corners(args: &[Value], named: &[(String, Value)]) -> R<Value> {
    if let Some((k, _)) = named.first() {
        return Err(fail("E0370", format!("`corners` takes no keys, got `{k}:`")).fix(ORDER));
    }
    if args.len() != 4 {
        return Err(fail(
            "E0370",
            format!("`corners` takes four points, got {}", args.len()),
        )
        .fix(ORDER));
    }
    let mut quad = [Point::default(); 4];
    for (i, v) in args.iter().enumerate() {
        let Some((Coord::Num(x), Coord::Num(y))) = v.pair() else {
            return Err(fail(
                "E0370",
                format!(
                    "`corners` point {} must be a point in canvas units like 84,22, got {}",
                    i + 1,
                    v.show()
                ),
            )
            .fix(ORDER));
        };
        quad[i] = Point::new(x as f32, y as f32);
    }
    let corners = Corners::try_new(quad).map_err(|e| {
        let why = match e {
            CornersError::SelfCrossing => "the corners cross",
            CornersError::Degenerate => "the corners have no area, or three lie on one line",
            CornersError::Concave => "the corners fold inward",
        };
        fail("E0320", why).fix(ORDER)
    })?;
    Ok(Value::Warp(Rc::new(WarpStage::Corners(corners))))
}

/// `cylinder(r axis:y)`. `axis:` is the word as written, so a bound `x` cannot shadow it.
pub(crate) fn cylinder(args: &[Value], named: &[Named]) -> R<Value> {
    let mut axis = Axis::Y;
    for n in named {
        if n.key != "axis" {
            return Err(fail(
                "E0370",
                format!("`cylinder` takes `axis:` only, got `{}:`", n.key),
            )
            .fix(CYLINDER));
        }
        axis = match ident_of(&n.value) {
            Some("x") => Axis::X,
            Some("y") => Axis::Y,
            _ => {
                return Err(fail(
                    "E0370",
                    format!(
                        "`axis:` of `cylinder` is x or y, got `{}`",
                        format_expr(&n.value)
                    ),
                )
                .fix(CYLINDER));
            }
        };
    }
    let r = match args {
        [v] => v.num().ok_or_else(|| {
            fail(
                "E0370",
                format!("`cylinder` radius must be a number, got {}", v.kind()),
            )
            .fix(CYLINDER)
        })?,
        _ => {
            return Err(fail(
                "E0370",
                format!("`cylinder` takes one radius, got {} arguments", args.len()),
            )
            .fix(CYLINDER));
        }
    };
    let c = Cylinder::try_new(r as f32, axis).ok_or_else(|| {
        fail(
            "E0320",
            format!("`cylinder` radius must be positive, got {}", num(r)),
        )
        .fix(CYLINDER)
    })?;
    Ok(Value::Warp(Rc::new(WarpStage::Cylinder(c))))
}

/// `corners` points are written in the parent's frame, and the node's transform applies after
/// the warp, so each quad is pulled back through the inverse transform to the target the stack
/// maps onto. A quad then lands where it is written whatever the node's own `at:`, `rot:`,
/// `scale:`, and `skew:`.
pub(crate) fn pinned_in(stages: &[WarpStage], transform: &Matrix) -> R<Vec<WarpStage>> {
    let inverse = transform.to_sk().invert().ok_or_else(|| {
        fail(
            "E0320",
            "`corners` cannot pin a node whose transform flattens it",
        )
        .fix("check `scale:` for a zero")
    })?;
    stages
        .iter()
        .map(|s| match s {
            WarpStage::Corners(c) => Corners::try_new(c.quad().map(|p| inverse.map_point(p.to_sk()).to_ir()))
                .map(WarpStage::Corners)
                .map_err(|_| {
                    fail("E0320", "the corners fold under this node's transform").fix(ORDER)
                }),
            other => Ok(*other),
        })
        .collect()
}

/// The stages a `warp:` value names, in application order. `none` is no warp.
pub(crate) fn warp_stack(v: &Value, sc: &Scope) -> R<Vec<WarpStage>> {
    const FIX: &str =
        "write `warp:cylinder(60)`, `warp:[cylinder(40) corners(...)]`, or `warp:none`";
    let one = |v: &Value| -> R<WarpStage> {
        match v {
            Value::Warp(w) => Ok(**w),
            Value::Ident(w) if sc.module.lost(w) => Err(Fail::reported()),
            Value::Ident(w) => Err(fail("E0371", format!("`{w}` is not a warp")).fix(FIX)),
            other => Err(fail(
                "E0371",
                format!(
                    "`warp:` takes a warp, a list of warps, or none, got {}",
                    other.kind()
                ),
            )
            .fix(FIX)),
        }
    };
    match v {
        Value::None => Ok(Vec::new()),
        Value::List(items) if items.is_empty() => {
            Err(fail("E0371", "`warp:[]` has no stages").fix("write `warp:none` to clear a warp"))
        }
        Value::List(items) => items.iter().map(one).collect(),
        other => Ok(vec![one(other)?]),
    }
}

/// The stages as the author wrote them: resolved `corners` quads back in the parent's frame.
pub(crate) fn written(
    stages: impl Iterator<Item = WarpStage>,
    transform: &Matrix,
) -> Vec<WarpStage> {
    let transform = transform.to_sk();
    stages
        .map(|s| match s {
            WarpStage::Corners(c) => Corners::try_new(c.quad().map(|p| transform.map_point(p.to_sk()).to_ir()))
                .map_or(s, WarpStage::Corners),
            other => other,
        })
        .collect()
}

/// A stage as the author writes it.
pub(crate) fn stage_text(s: &WarpStage) -> String {
    match s {
        WarpStage::Corners(c) => {
            let points: Vec<String> = c
                .quad()
                .iter()
                .map(|p| format!("{},{}", num(p.x as f64), num(p.y as f64)))
                .collect();
            format!("corners({})", points.join(" "))
        }
        WarpStage::Cylinder(c) => match c.axis() {
            Axis::Y => format!("cylinder({})", num(c.r() as f64)),
            Axis::X => format!("cylinder({} axis:x)", num(c.r() as f64)),
        },
    }
}

/// A stack as the author writes it: one stage bare, more in a list.
pub(crate) fn stack_text(stages: &[WarpStage]) -> String {
    match stages {
        [one] => stage_text(one),
        many => format!(
            "[{}]",
            many.iter().map(stage_text).collect::<Vec<_>>().join(" ")
        ),
    }
}

/// The shape a `key(...)` track keeps while it moves a stage's numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    Corners,
    Cylinder(Axis),
}

fn shape(s: &WarpStage) -> Shape {
    match s {
        WarpStage::Corners(_) => Shape::Corners,
        WarpStage::Cylinder(c) => Shape::Cylinder(c.axis()),
    }
}

fn numbers(s: &WarpStage) -> Vec<f64> {
    match s {
        WarpStage::Corners(c) => c
            .quad()
            .iter()
            .flat_map(|p| [f64::from(p.x), f64::from(p.y)])
            .collect(),
        WarpStage::Cylinder(c) => vec![f64::from(c.r())],
    }
}

fn from_numbers(shape: Shape, v: &[f64]) -> Result<WarpStage, String> {
    match shape {
        Shape::Corners => {
            let p = |i: usize| Point::new(v[2 * i] as f32, v[2 * i + 1] as f32);
            Corners::try_new([p(0), p(1), p(2), p(3)])
                .map(WarpStage::Corners)
                .map_err(|_| "the corners cross or fold between two keys".to_string())
        }
        Shape::Cylinder(axis) => Cylinder::try_new(v[0] as f32, axis)
            .map(WarpStage::Cylinder)
            .ok_or_else(|| "the cylinder radius drops to zero between two keys".to_string()),
    }
}

/// The stages a `key(...)` value holds: a warp, or a list of warps. `None` for anything else.
pub(crate) fn stages_of(v: &Value) -> Option<Vec<WarpStage>> {
    match v {
        Value::Warp(w) => Some(vec![**w]),
        Value::List(items) if !items.is_empty() => items
            .iter()
            .map(|v| match v {
                Value::Warp(w) => Some(**w),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

/// A `key(...)` result in the form of its first key: one stage bare, a list as a list.
pub(crate) fn as_value(stages: Vec<WarpStage>, as_list: bool) -> Value {
    match (stages.as_slice(), as_list) {
        ([one], false) => Value::Warp(Rc::new(*one)),
        _ => Value::List(Rc::new(
            stages
                .into_iter()
                .map(|s| Value::Warp(Rc::new(s)))
                .collect(),
        )),
    }
}

/// `None` when two tracks have the same stages in the same order with the same axes.
pub(crate) fn mix_error(a: &[WarpStage], b: &[WarpStage]) -> Option<String> {
    let same = a.len() == b.len() && a.iter().zip(b).all(|(x, y)| shape(x) == shape(y));
    (!same).then(|| {
        format!(
            "`key` mixes warps of different shapes: `{}` and `{}`",
            stack_text(a),
            stack_text(b)
        )
    })
}

/// `tracks` blended stage by stage: each number of a stage goes through `f` with that number
/// from every track. Every track has the first one's shape.
pub(crate) fn blend(
    tracks: &[&[WarpStage]],
    f: &dyn Fn(&[f64]) -> f64,
) -> Result<Vec<WarpStage>, String> {
    let first = tracks[0];
    for t in tracks {
        if let Some(e) = mix_error(first, t) {
            return Err(e);
        }
    }
    (0..first.len())
        .map(|i| {
            let per: Vec<Vec<f64>> = tracks.iter().map(|t| numbers(&t[i])).collect();
            let mixed: Vec<f64> = (0..per[0].len())
                .map(|j| f(&per.iter().map(|p| p[j]).collect::<Vec<_>>()))
                .collect();
            from_numbers(shape(&first[i]), &mixed)
        })
        .collect()
}
