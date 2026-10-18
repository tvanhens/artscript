//! The pure function library: math, randomness, color helpers, gradients and fields.

use std::rc::Rc;

use artscript_ir::{Field, FieldFn, FieldOp, NoiseKind};
use artscript_syntax::ast::{Expr, Named};

use crate::color::{self, DEFAULT_HUE_STEP, Lch, Rgba};
use crate::eval::{Evaluator, R, Scope, fail, unseen};
use crate::paint::{GradKind, direction_deg, gradient};
use crate::seed::Rng;
use crate::value::{Arith, Col, Coord, Value, arith};

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
        match sc.module.global(func) {
            Some(Value::Rule(rule)) => return self.call_rule(&rule, args, named, sc),
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
        let one = |f: fn(f64) -> f64| -> R<Value> { Ok(Value::Num(f(n(0)?))) };
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
                Some(Value::Num(x)) => Value::Num(x.abs()),
                Some(v) if v.vec2().is_some() => {
                    let (x, y) = v.vec2().unwrap();
                    Value::Vec2(Coord::Num(x.abs()), Coord::Num(y.abs()))
                }
                _ => one(f64::abs)?,
            },
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
                let explicit = get("seed").and_then(|v| v.num()).map(|s| s as u64);
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
                let explicit = get("seed").and_then(|v| v.num()).map(|s| s as u64);
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
                let at = get("at").and_then(|v| v.vec2()).unwrap_or((0.5, 0.5));
                let r = get("r").and_then(|v| v.num());
                gradient(GradKind::Radial { at, r }, self.stops(&a)?)
            }
            "conic" => {
                let at = get("at").and_then(|v| v.vec2()).unwrap_or((0.5, 0.5));
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
                let explicit = get("seed").and_then(|v| v.num()).map(|s| s as u64);
                let base = Field::Noise {
                    kind,
                    scale,
                    seed: self.next_seed(explicit) as u32,
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
            "contrast" | "visible" | "occluded" | "value_range" => {
                return Err(fail(
                    "E0392",
                    format!("`{func}` is a rendered check, not yet implemented in v0.1"),
                )
                .fix("use geometric checks such as inside, gap or level"));
            }
            other => {
                return Err(fail("E0302", format!("unknown function `{other}`"))
                    .note("math: min max abs clamp lerp sqrt floor round sin cos atan2 mod polar rand pick")
                    .note("color: shade lit mix fade line oklch ramp grad radial conic")
                    .note("fields: simplex perlin worley value fbm ridged")
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
