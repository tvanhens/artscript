//! `warp:` values: the `corners` and `cylinder` calls, and the stack a key holds.

use std::rc::Rc;

use ramose_ir::{Axis, Corners, CornersError, Cylinder, Point, WarpStage};
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
