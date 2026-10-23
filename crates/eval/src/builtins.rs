//! The pure function library: math, randomness, color helpers, gradients and fields.

use std::rc::Rc;

use ramose_ir::{Field, FieldFn, FieldOp, NoiseKind};
use ramose_syntax::ast::{Expr, Named};
use skia_safe::Point;

use crate::color::{self, DEFAULT_HUE_STEP, Lch, Rgba};
use crate::eval::{Evaluator, Fail, R, Scope, fail, unseen};
use crate::geom;
use crate::paint::{GradKind, direction_deg, gradient};
use crate::place::Place;
use crate::seed::Rng;
use crate::types::Ty;
use crate::value::{Arith, Col, Coord, PathVal, Value, arith};
use crate::warp;

/// What each built-in function returns, for typing a parameter default that calls one. `min`,
/// `max`, `abs`, `clamp` and `lerp` return the type they are given, so the typer reads their
/// arguments instead. A function left out, such as `pick`, has no type a reader can tell from the
/// call, so a default that calls it is E0326.
pub const BUILTIN_RETURNS: &[(&str, Ty)] = &[
    ("sqrt", Ty::Number),
    ("floor", Ty::Number),
    ("round", Ty::Number),
    ("sin", Ty::Number),
    ("cos", Ty::Number),
    ("atan2", Ty::Number),
    ("mod", Ty::Number),
    ("rand", Ty::Number),
    ("polar", Ty::Vector),
    ("shade", Ty::Color),
    ("lit", Ty::Color),
    ("mix", Ty::Color),
    ("fade", Ty::Color),
    ("line", Ty::Color),
    ("oklch", Ty::Color),
    ("ramp", Ty::Color),
    ("grad", Ty::Color),
    ("radial", Ty::Color),
    ("conic", Ty::Color),
    ("simplex", Ty::Field),
    ("perlin", Ty::Field),
    ("worley", Ty::Field),
    ("value", Ty::Field),
    ("fbm", Ty::Field),
    ("ridged", Ty::Field),
    ("corners", Ty::Warp),
    ("cylinder", Ty::Warp),
    ("concat", Ty::List),
    ("path", Ty::Path),
    ("point", Ty::Vector),
    ("tangent", Ty::Vector),
    ("length", Ty::Number),
    ("points", Ty::List),
];

fn wrap_period(v: &Value) -> Option<[f32; 2]> {
    match v.pair()? {
        (Coord::Num(x), Coord::Num(y)) if x > 0.0 && y > 0.0 => Some([x as f32, y as f32]),
        _ => None,
    }
}

fn f32_field(v: &Value) -> Option<Field> {
    match v {
        Value::Field(f) => Some((**f).clone()),
        Value::Num(n) => Some(Field::Const(*n as f32)),
        _ => None,
    }
}

impl Evaluator<'_> {
    pub(crate) fn call(
        &mut self,
        func: &str,
        args: &[Expr],
        named: &[Named],
        sc: &Scope,
    ) -> R<Value> {
        if func == "key" {
            return self.key_call(args, named, sc);
        }
        if matches!(func, "one_of" | "none") && sc.module.global(func).is_none() {
            return Err(fail("E0320", format!("`{func}(...)` is only a parameter default"))
                .fix(format!("write `{func}(...)` after a parameter's `key:` in a def, stamp, finish, rule, or param line")));
        }
        let at = sc.env.iter().rposition(|(k, _)| k == func);
        let bound = at
            .map(|i| sc.env[i].1.clone())
            .or_else(|| sc.module.global(func));
        if let Some((_, ty)) = at.and_then(|i| sc.typed_params.iter().find(|(j, _)| *j == i)) {
            return match ty {
                Ty::Brush(_) => self.call_brush_param(ty, bound, args, named, sc),
                Ty::Stamp(_) => self.call_stamp_param(ty, bound, args, named, sc),
                other => unreachable!("typed parameters are brushes and stamps, not {}", other.kind()),
            };
        }
        match bound {
            Some(Value::Rule(rule)) => return self.call_rule(&rule, args, named, sc),
            Some(Value::Fn(f)) => return self.call_fn(&f, args, named, sc),
            Some(Value::Brush(b)) => return self.call_brush(&b, args, named, sc),
            Some(Value::Stamp(s)) => return self.call_stamp(&s, args, named, sc),
            Some(Value::Unseen) => return Err(unseen(func)),
            _ => {}
        }
        let saved_family = self.accept_family;
        if func == "same_size" {
            self.accept_family = true;
        }
        let parsed = (|| -> R<_> {
            let mut a = Vec::with_capacity(args.len());
            for e in args {
                a.push(self.expr(e, sc)?);
            }
            let mut nm = Vec::with_capacity(named.len());
            for n in named {
                nm.push((n.key.clone(), self.expr(&n.value, sc)?));
            }
            Ok((a, nm))
        })();
        self.accept_family = saved_family;
        let (a, nm) = parsed?;
        if let Some(v) = self.check_fn(func, &a, &nm, sc) {
            return v;
        }
        let get = |k: &str| {
            nm.iter()
                .rev()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.clone())
        };
        let n = |i: usize| -> R<f64> {
            a.get(i).and_then(Value::num).ok_or_else(|| {
                fail(
                    "E0320",
                    format!(
                        "`{func}` argument {} must be a number, got {}",
                        i + 1,
                        a.get(i)
                            .map(|v| v.kind().to_string())
                            .unwrap_or("nothing".into())
                    ),
                )
            })
        };
        let col = |i: usize| -> R<Col> {
            a.get(i).and_then(Value::color).ok_or_else(|| {
                fail(
                    "E0320",
                    format!(
                        "`{func}` argument {} must be a color, got {}",
                        i + 1,
                        a.get(i)
                            .map(|v| v.kind().to_string())
                            .unwrap_or("nothing".into())
                    ),
                )
            })
        };
        // A unary math function applies to a number, or to each component of a vector. A number
        // is matched first: `pair()` reads one as a square size.
        let one = |f: fn(f64) -> f64| -> R<Value> {
            Ok(match a.first() {
                Some(Value::Num(x)) => Value::Num(f(*x)),
                Some(Value::Vec2(Coord::Num(x), Coord::Num(y)) | Value::Size(Coord::Num(x), Coord::Num(y))) => {
                    Value::Vec2(Coord::Num(f(*x)), Coord::Num(f(*y)))
                }
                Some(Value::Vec3(Coord::Num(x), Coord::Num(y), z) | Value::Size3(Coord::Num(x), Coord::Num(y), z)) => {
                    Value::Vec3(Coord::Num(f(*x)), Coord::Num(f(*y)), f(*z))
                }
                _ => Value::Num(f(n(0)?)),
            })
        };
        let path_arg = |i: usize| -> R<Rc<PathVal>> {
            match a.get(i) {
                Some(Value::Path(p)) => Ok(p.clone()),
                other => Err(fail(
                    "E0320",
                    format!(
                        "`{func}` argument {} must be a path, got {}",
                        i + 1,
                        other.map_or("nothing".to_string(), |v| v.kind().to_string())
                    ),
                )
                .fix(format!("write `{func}(path(@body){})`", if matches!(func, "point" | "tangent") { " .5" } else { "" }))),
            }
        };
        Ok(match func {
            "min" | "max" => {
                let items = match a.as_slice() {
                    [Value::List(xs)] => xs.as_ref().clone(),
                    [v @ Value::Range(_)] => v.items().unwrap_or_else(|| a.clone()),
                    _ => a.clone(),
                };
                if items.iter().any(|v| matches!(v, Value::Field(_))) {
                    let op = if func == "min" {
                        FieldOp::Min
                    } else {
                        FieldOp::Max
                    };
                    let fs: Vec<Field> = items.iter().filter_map(f32_field).collect();
                    let first = fs
                        .first()
                        .cloned()
                        .ok_or_else(|| fail("E0320", format!("`{func}` needs arguments")))?;
                    Value::Field(Rc::new(fs.into_iter().skip(1).fold(first, |acc, f| {
                        Field::Binary(op, Box::new(acc), Box::new(f))
                    })))
                } else {
                    let nums: Vec<f64> = items.iter().filter_map(Value::num).collect();
                    if nums.is_empty() {
                        return Err(fail("E0320", format!("`{func}` needs numbers")));
                    }
                    Value::Num(if func == "min" {
                        nums.iter().copied().fold(f64::INFINITY, f64::min)
                    } else {
                        nums.iter().copied().fold(f64::NEG_INFINITY, f64::max)
                    })
                }
            }
            "abs" => match a.first() {
                Some(Value::Field(f)) => {
                    Value::Field(Rc::new(Field::Unary(FieldFn::Abs, Box::new((**f).clone()))))
                }
                _ => one(f64::abs)?,
            },
            "rad" => Value::Num(n(0)?.to_radians()),
            "clamp" => match a.as_slice() {
                [Value::Field(f)] => Value::Field(Rc::new(Field::Unary(
                    FieldFn::Clamp,
                    Box::new((**f).clone()),
                ))),
                _ => Value::Num(n(0)?.clamp(n(1)?, n(2)?.max(n(1)?))),
            },
            "lerp" => {
                let t = n(2)?;
                let (x, y) = (a[0].clone(), a[1].clone());
                if let (Some(cx), Some(cy)) = (x.color(), y.color()) {
                    Value::Color(Col::plain(color::mix(cx.rgba, cy.rgba, t)))
                } else {
                    let d = arith(Arith::Sub, &y, &x).map_err(|m| fail("E0320", m))?;
                    let s = arith(Arith::Mul, &d, &Value::Num(t)).map_err(|m| fail("E0320", m))?;
                    arith(Arith::Add, &x, &s).map_err(|m| fail("E0320", m))?
                }
            }
            "sqrt" => one(|v| v.max(0.0).sqrt())?,
            "floor" => one(f64::floor)?,
            "round" => one(f64::round)?,
            "sin" => one(|v| v.to_radians().sin())?,
            "cos" => one(|v| v.to_radians().cos())?,
            "atan2" => Value::Num(n(0)?.atan2(n(1)?).to_degrees()),
            "mod" => {
                let (a, b) = (n(0)?, n(1)?);
                if b == 0.0 {
                    return Err(fail("E0320", "`mod` divisor must not be 0")
                        .fix("write `mod(a b)` with b not 0"));
                }
                Value::Num(a.rem_euclid(b))
            }
            "polar" => {
                let (r, d) = (n(0)?, n(1)?.to_radians());
                Value::Vec2(Coord::Num(r * d.cos()), Coord::Num(r * d.sin()))
            }
            "rand" => {
                let explicit = get("seed").as_ref().map(Value::seed).transpose()?;
                let mut rng = Rng::new(self.next_seed(explicit));
                let (lo, hi) = match a.len() {
                    0 => (0.0, 1.0),
                    1 => (0.0, n(0)?),
                    _ => (n(0)?, n(1)?),
                };
                Value::Num(rng.uniform(lo, hi))
            }
            "pick" => {
                let items = a.first().and_then(Value::items).ok_or_else(|| {
                    fail("E0320", "`pick` needs a list").fix("write `pick([a b])`")
                })?;
                if items.is_empty() {
                    return Err(fail("E0320", "`pick` needs a non-empty list"));
                }
                let explicit = get("seed").as_ref().map(Value::seed).transpose()?;
                let mut rng = Rng::new(self.next_seed(explicit));
                items[(rng.unit() * items.len() as f64) as usize % items.len()].clone()
            }
            "shade" | "lit" => {
                let c = col(0)?;
                let k = n(1)?;
                Value::Color(match &c.ramp {
                    Some((ramp, i)) => Col {
                        rgba: ramp.at(i + k),
                        ramp: Some((ramp.clone(), i + k)),
                    },
                    None => Col::plain(color::step(c.rgba, k, DEFAULT_HUE_STEP)),
                })
            }
            "mix" => Value::Color(Col::plain(color::mix(
                col(0)?.rgba,
                col(1)?.rgba,
                a.get(2).and_then(Value::num).unwrap_or(0.5),
            ))),
            "fade" => {
                let c = col(0)?;
                Value::Color(Col::plain(c.rgba.with_alpha(c.rgba.a * n(1)?)))
            }
            "line" => Value::Color(Col::plain(color::line(col(0)?.rgba))),
            "oklch" => {
                let l = match a.first() {
                    Some(Value::Percent(p)) => *p,
                    _ => n(0)?,
                };
                let alpha = a.get(3).and_then(Value::num).unwrap_or(1.0);
                Value::Color(Col::plain(color::from_oklch(
                    Lch {
                        l,
                        c: n(1)?,
                        h: n(2)?,
                    },
                    alpha,
                )))
            }
            "ramp" => {
                let hue = get("hue").and_then(|v| v.num()).unwrap_or(DEFAULT_HUE_STEP);
                self.ramp(&a, hue)?
            }
            "grad" => {
                let point = |v: &Value| match v {
                    Value::Vec2(Coord::Num(x), Coord::Num(y)) => Some((*x, *y)),
                    _ => None,
                };
                let (kind, stops) = match a.as_slice() {
                    [start, end, rest @ ..] if let Some(from) = point(start) => {
                        let to = point(end).ok_or_else(|| {
                            fail("E0320", "`grad` with a point start needs a point end")
                                .fix("write `grad(300,240 720,440 c1 c2)`")
                        })?;
                        (GradKind::Segment { from, to }, rest)
                    }
                    [Value::Ident(w), rest @ ..] => (
                        GradKind::Linear(direction_deg(w).ok_or_else(|| {
                            fail("E0320", format!("unknown direction `{w}`"))
                                .fix("use down, up, left, right or an angle")
                        })?),
                        rest,
                    ),
                    [Value::Num(d), rest @ ..] => (GradKind::Linear(*d), rest),
                    _ => (GradKind::Linear(90.0), a.as_slice()),
                };
                gradient(kind, self.stops(stops)?)
            }
            "radial" => {
                let at = get("at").map(|v| v.vec2_for("at")).transpose()?.unwrap_or((0.5, 0.5));
                let r = get("r").and_then(|v| v.num());
                gradient(GradKind::Radial { at, r }, self.stops(&a)?)
            }
            "conic" => {
                let at = get("at").map(|v| v.vec2_for("at")).transpose()?.unwrap_or((0.5, 0.5));
                let start = get("from").and_then(|v| v.num()).unwrap_or(0.0);
                gradient(GradKind::Conic { at, start }, self.stops(&a)?)
            }
            "simplex" | "perlin" | "worley" | "value" => {
                let kind = match func {
                    "simplex" => NoiseKind::Simplex,
                    "perlin" => NoiseKind::Perlin,
                    "worley" => NoiseKind::Worley,
                    _ => NoiseKind::Value,
                };
                let scale = a.first().and_then(Value::num).unwrap_or(32.0) as f32;
                let explicit = get("seed").as_ref().map(Value::seed).transpose()?;
                let wrap = match get("wrap") {
                    None => None,
                    Some(v) => Some(wrap_period(&v).ok_or_else(|| {
                        fail("E0320", format!("`wrap:` on `{func}` is a positive number or a pair, got {}", v.show()))
                            .fix(format!("write `{func}({scale} wrap:64)` or `wrap:64,32`"))
                    })?),
                };
                let base = Field::Noise {
                    kind,
                    scale,
                    seed: self.next_seed(explicit) as u32,
                    wrap,
                };
                match get("oct").and_then(|v| v.num()) {
                    Some(o) if o > 1.0 => Value::Field(Rc::new(Field::Fbm {
                        base: Box::new(base),
                        octaves: o as u32,
                        ridged: false,
                    })),
                    _ => Value::Field(Rc::new(base)),
                }
            }
            "fbm" | "ridged" => {
                let base = a.first().and_then(f32_field).ok_or_else(|| {
                    fail("E0320", format!("`{func}` wraps a field"))
                        .fix(format!("write `{func}(simplex(30))`"))
                })?;
                let octaves = get("oct").and_then(|v| v.num()).unwrap_or(4.0) as u32;
                Value::Field(Rc::new(Field::Fbm {
                    base: Box::new(base),
                    octaves,
                    ridged: func == "ridged",
                }))
            }
            "path" => {
                if let Some(Value::Ref(_)) = a.first() {
                    return self.path_of(&a, sc);
                }
                let Some(items) = a.first().and_then(Value::items) else {
                    let got = a.first().map_or("nothing".to_string(), |v| v.kind().to_string());
                    return Err(fail("E0320", format!("`path` takes a node reference or a list of points, got {got}"))
                        .fix("write `path(@body)` or `path([0,0 10,4 20,0])`"));
                };
                let pts = items
                    .iter()
                    .enumerate()
                    .map(|(i, item)| {
                        item.vec2().ok_or_else(|| {
                            fail("E0320", format!("`path` point {} must be a vector like 4,2, got {}", i + 1, item.kind()))
                                .fix("write `path([0,0 10,4 20,0])`")
                        })
                    })
                    .collect::<R<Vec<_>>>()?;
                if pts.len() < 2 {
                    return Err(fail("E0320", "`path` needs two or more points").fix("write `path([0,0 10,4 20,0])`"));
                }
                let closed = get("closed").is_some_and(|v| v.truthy());
                Value::Path(Rc::new(PathVal {
                    own: geom::polyline(&pts, closed),
                    seam: 0.0,
                    place: Place::identity(),
                    points: pts,
                }))
            }
            "point" | "tangent" => {
                let p = path_arg(0)?;
                let u = match a.get(1).and_then(Value::num).filter(|u| u.is_finite() && (0.0..=1.0).contains(u)) {
                    Some(u) => u,
                    None => {
                        let got = a.get(1).map_or("nothing".to_string(), |v| v.num().map_or_else(|| v.kind().to_string(), crate::value::num));
                        return Err(fail("E0320", format!("`{func}` argument 2 is a fraction from 0 to 1, got {got}"))
                            .fix(format!("write `{func}(p .5)`")));
                    }
                };
                let (pos, tan) = geom::at(&p.own, p.seam, u).ok_or_else(|| {
                    fail("E0320", format!("`{func}` has nothing to sample: the path has no length"))
                        .fix("give the path two different points")
                })?;
                let (pos, tan) = p
                    .place
                    .map_direction(Point::new(pos.0 as f32, pos.1 as f32), Point::new(tan.0 as f32, tan.1 as f32))
                    .ok_or_else(|| {
                        fail("E0320", format!("`{func}` at position {} is hidden by a warp", crate::value::num(u)))
                            .fix("sample a position on the part of the contour the warp shows")
                    })?;
                let v = if func == "point" { pos } else { tan };
                Value::Vec2(Coord::Num(f64::from(v.x)), Coord::Num(f64::from(v.y)))
            }
            "length" => Value::Num(path_arg(0)?.length()),
            "points" => {
                let p = path_arg(0)?;
                let pts = p
                    .points
                    .iter()
                    .enumerate()
                    .map(|(i, &(x, y))| {
                        let q = p.place.map_point(Point::new(x as f32, y as f32)).ok_or_else(|| {
                            fail("E0320", format!("`points` point {} is hidden by a warp", i + 1))
                                .fix("read the points before the warp, or sample with `point`")
                        })?;
                        Ok(Value::Vec2(Coord::Num(f64::from(q.x)), Coord::Num(f64::from(q.y))))
                    })
                    .collect::<R<Vec<_>>>()?;
                Value::List(Rc::new(pts))
            }
            "corners" => warp::corners(&a, &nm)?,
            "cylinder" => warp::cylinder(&a, named)?,
            "concat" => {
                if a.len() < 2 {
                    return Err(fail("E0320", "`concat` joins two or more lists")
                        .fix("write `concat(a b)`"));
                }
                let mut joined = Vec::new();
                for (i, v) in a.iter().enumerate() {
                    match v {
                        Value::List(_) | Value::Range(_) => joined.extend(v.items().unwrap_or_default()),
                        other => {
                            return Err(fail(
                                "E0320",
                                format!("`concat` argument {} must be a list, got {}", i + 1, other.kind()),
                            )
                            .fix("write `concat(a b)` with lists or ranges, and `[v]` for one item"));
                        }
                    }
                }
                Value::List(Rc::new(joined))
            }
            "contrast" | "visible" | "occluded" | "value_range" => {
                return Err(fail(
                    "E0392",
                    format!("`{func}` is a rendered check, not yet implemented in v0.1"),
                )
                .fix("use geometric checks such as inside, gap or level"));
            }
            other if sc.module.lost(other) => return Err(Fail::reported()),
            other => {
                return Err(fail("E0302", format!("unknown function `{other}`"))
                    .note("math: min max abs clamp lerp sqrt floor round sin cos atan2 mod polar rand pick")
                    .note("color: shade lit mix fade line oklch ramp grad radial conic")
                    .note("fields: simplex perlin worley value fbm ridged")
                    .note("warps: corners cylinder")
                    .note("lists: concat")
                    .note("paths: path point tangent length points")
                    .fix("check the spelling"));
            }
        })
    }

    fn stops(&self, vals: &[Value]) -> R<Vec<(Rgba, Option<f64>)>> {
        vals.iter()
            .map(|v| match v {
                Value::Tuple(xs) if xs.len() == 2 && xs[0].color().is_some() => {
                    Ok((xs[0].color().unwrap().rgba, xs[1].num()))
                }
                other => other.color().map(|c| (c.rgba, None)).ok_or_else(|| {
                    fail(
                        "E0320",
                        format!(
                            "a gradient stop is a color or color,position, got {}",
                            other.kind()
                        ),
                    )
                    .fix("write `grad(down p.sky.2 p.sky.5,.8)`")
                }),
            })
            .collect()
    }
}
