//! Brush declarations, brush values with overrides, the marks a node's `brush:` key becomes on
//! its fill and its stroke, and the ribbon a brushed stroke's channels become. The SkSL contract
//! itself lives in `artscript_brush`; this module binds `.art` values to it and reports every
//! failure at the author's line.

use std::rc::Rc;
use std::sync::Arc;

use artscript_brush::{
    HELPERS, MARK_FIELDS, MainKind, Origin, PLACEMENT_KEYS, Program, RESERVED_PREFIX,
    UniformKind, UniformValue,
};
use artscript_ir::{AreaMark, Knot, Path, Ribbon, RibbonContour, RibbonMark, Stage};
use artscript_syntax::ast::{Expr, Named, SkslBlock, Span};
use skia_safe::{ContourMeasure, ContourMeasureIter};

use crate::eval::{Args, Evaluator, Fail, R, Scope, fail, osa_distance};
use crate::geom::P;
use crate::identity::{self, Key};
use crate::scope::Module;
use crate::value::{Kind, Value};

/// A `brush` declaration after `check` accepted its programs. Shared by every value made from it.
pub struct BrushDecl {
    pub name: String,
    /// Declared uniforms in head order.
    pub params: Vec<(String, UniformKind)>,
    /// The body under each main, indexed by [`MainKind`]: area first, ribbon second.
    programs: [Arc<Program>; 2],
}

impl BrushDecl {
    pub fn program(&self, kind: MainKind) -> &Arc<Program> {
        &self.programs[match kind {
            MainKind::Area => 0,
            MainKind::Ribbon => 1,
        }]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Fill,
    Stroke,
}

impl Slot {
    pub fn word(self) -> &'static str {
        match self {
            Slot::Fill => "fill",
            Slot::Stroke => "stroke",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Placement {
    pub dir_deg: f64,
    pub reach: f64,
    pub on: Option<Slot>,
}

impl Default for Placement {
    fn default() -> Placement {
        Placement { dir_deg: 0.0, reach: 0.0, on: None }
    }
}

impl Placement {
    /// Applies one placement key. The caller already knows `key` is in [`PLACEMENT_KEYS`].
    fn set(&mut self, brush: &str, key: &str, v: &Value) -> R<()> {
        match key {
            "dir" => self.dir_deg = number(brush, key, v)?,
            "reach" => {
                let r = number(brush, key, v)?;
                if r < 0.0 || !r.is_finite() {
                    return Err(fail("E0361", format!("`reach` of `{brush}` must be 0 or more, got {r}"))
                        .fix("write `reach:3`"));
                }
                self.reach = r;
            }
            "on" => {
                self.on = match v.word() {
                    Some("fill") => Some(Slot::Fill),
                    Some("stroke") => Some(Slot::Stroke),
                    _ => {
                        return Err(fail(
                            "E0361",
                            format!("`on` of `{brush}` takes `fill` or `stroke`, got {}", v.show()),
                        )
                        .fix("write `on:fill`"));
                    }
                }
            }
            _ => unreachable!("placement keys are {PLACEMENT_KEYS:?}"),
        }
        Ok(())
    }
}

fn number(brush: &str, key: &str, v: &Value) -> R<f64> {
    v.num().filter(|n| n.is_finite()).ok_or_else(|| {
        fail("E0361", format!("`{key}` of `{brush}` expects a number, got {}", v.kind()))
            .fix(format!("write `{key}:0`"))
    })
}

/// A brush value: a declaration plus the current uniform values and placement.
pub struct BrushVal {
    pub decl: Rc<BrushDecl>,
    /// Same order as `decl.params`.
    pub uniforms: Vec<UniformValue>,
    pub place: Placement,
}

impl BrushVal {
    /// `dry(strength:.6 dir:90)`: a new brush with those keys replaced. Positional arguments,
    /// unknown keys and kind mismatches are E0361 and the notes list what the brush takes.
    pub fn with(&self, pos: &[Value], named: &[(String, Value)]) -> R<BrushVal> {
        let name = &self.decl.name;
        if !pos.is_empty() {
            return Err(self.argument_error(format!("`{name}` takes named arguments only")));
        }
        let mut out = BrushVal {
            decl: self.decl.clone(),
            uniforms: self.uniforms.clone(),
            place: self.place.clone(),
        };
        for (k, v) in named {
            if PLACEMENT_KEYS.contains(&k.as_str()) {
                out.place.set(name, k, v).map_err(|f| self.with_notes(f))?;
                continue;
            }
            let Some(i) = self.decl.params.iter().position(|(p, _)| p == k) else {
                let nearest = self
                    .decl
                    .params
                    .iter()
                    .map(|(p, _)| p.as_str())
                    .chain(PLACEMENT_KEYS.iter().copied())
                    .map(|p| (osa_distance(k, p), p))
                    .min_by_key(|(d, _)| *d)
                    .filter(|(d, _)| *d <= (k.len() + 1) / 3);
                let f = self.argument_error(format!("`{name}` has no parameter `{k}`"));
                return Err(match nearest {
                    Some((_, p)) => f.fix(format!("did you mean `{p}:`?")),
                    None => f.fix("use one of the listed parameters"),
                });
            };
            let kind = self.decl.params[i].1;
            let Some(u) = uniform_of(v).filter(|u| u.kind() == kind) else {
                return Err(self
                    .argument_error(format!(
                        "`{k}` of `{name}` expects {}, got {}",
                        kind_word(kind),
                        v.kind()
                    ))
                    .fix(format!("pass {}", kind_word(kind))));
            };
            out.uniforms[i] = u;
        }
        Ok(out)
    }

    fn argument_error(&self, msg: String) -> Fail {
        self.with_notes(fail("E0361", msg))
    }

    fn with_notes(&self, f: Fail) -> Fail {
        let params: Vec<&str> = self.decl.params.iter().map(|(p, _)| p.as_str()).collect();
        f.note(format!("parameters: {}", params.join(", ")))
            .note(format!("placement: {}", PLACEMENT_KEYS.join(", ")))
    }
}

/// The uniform a value binds as: a number is `float`, a color `float4` (straight rgba), a
/// vector `float2`.
fn uniform_of(v: &Value) -> Option<UniformValue> {
    match v {
        Value::Num(n) => Some(UniformValue::Float(*n as f32)),
        Value::Color(c) => {
            let c = c.rgba;
            Some(UniformValue::Float4([c.r as f32, c.g as f32, c.b as f32, c.a as f32]))
        }
        _ => match v.kind() {
            Kind::Vector => {
                let (x, y) = v.vec2()?;
                Some(UniformValue::Float2([x as f32, y as f32]))
            }
            _ => None,
        },
    }
}

fn kind_word(kind: UniformKind) -> &'static str {
    match kind {
        UniformKind::Float => "a number",
        UniformKind::Float2 => "a vector like 4,2",
        UniformKind::Float4 => "a color",
    }
}

impl Evaluator<'_> {
    /// A brush that fails is marked lost so its use sites stay quiet.
    pub(crate) fn declare_brush(
        &mut self,
        module: &Rc<Module>,
        name: &str,
        params: &[Named],
        sksl: Option<&SkslBlock>,
        span: Span,
    ) -> R<()> {
        let file = module.path.clone();
        let result = self.build_brush(module, name, params, sksl, span);
        match result {
            Ok(v) => {
                module.define(name, Value::Brush(Rc::new(v)));
                Ok(())
            }
            Err(errors) => {
                for (f, at) in errors {
                    self.report(f, &file, at, "");
                }
                module.lost.borrow_mut().insert(name.to_string());
                Err(Fail::reported())
            }
        }
    }

    fn build_brush(
        &mut self,
        module: &Rc<Module>,
        name: &str,
        params: &[Named],
        sksl: Option<&SkslBlock>,
        span: Span,
    ) -> Result<BrushVal, Vec<(Fail, Span)>> {
        let Some(block) = sksl else {
            return Err(vec![(
                fail("E0363", format!("brush `{name}` has no `sksl` block"))
                    .fix("indent `sksl \"\"\"...\"\"\"` under the brush"),
                span,
            )]);
        };
        let sc = self.module_scope(module);
        self.begin_seed(&module.path, Key::binding(name));
        let mut place = Placement::default();
        let mut decl_params: Vec<(String, UniformKind)> = Vec::new();
        let mut uniforms = Vec::new();
        let mut errors = Vec::new();
        for p in params {
            let v = match self.expr(&p.value, &sc) {
                Ok(v) => v,
                Err(f) => {
                    errors.push((f, p.span));
                    continue;
                }
            };
            if PLACEMENT_KEYS.contains(&p.key.as_str()) {
                if let Err(f) = place.set(name, &p.key, &v) {
                    errors.push((f, p.span));
                }
                continue;
            }
            if let Some(f) = param_name_clash(&p.key) {
                errors.push((f, p.span));
                continue;
            }
            match uniform_of(&v) {
                Some(u) => {
                    decl_params.push((p.key.clone(), u.kind()));
                    uniforms.push(u);
                }
                None => errors.push((
                    fail(
                        "E0363",
                        format!(
                            "`{}` of `{name}` must default to a number, color or vector, got {}",
                            p.key,
                            v.kind()
                        ),
                    )
                    .fix(format!("write `{}:.5`", p.key)),
                    p.span,
                )),
            }
        }
        let at = |origin: Origin| match origin {
            Origin::Body { line, col } => {
                let start = if line == 1 { block.body } else { block.rest };
                Span {
                    line: block.body.line + line - 1,
                    col: start.col + col.unwrap_or(0),
                }
            }
            Origin::Param(i) => params
                .iter()
                .filter(|p| !PLACEMENT_KEYS.contains(&p.key.as_str()))
                .nth(i)
                .map_or(block.span, |p| p.span),
            Origin::Frame => block.span,
        };
        let head_failed = !errors.is_empty();
        for (ident, origin) in artscript_brush::reserved_identifiers(&block.text) {
            errors.push((
                fail("E0363", format!("`{ident}` is reserved: names starting with `{RESERVED_PREFIX}` belong to the runtime"))
                    .fix("rename it"),
                at(origin),
            ));
        }
        if head_failed {
            return Err(errors);
        }
        let area = Program::assemble(MainKind::Area, &decl_params, &block.text);
        // The body is the same text under both mains, so the area check reports its errors once.
        // The ribbon main is checked too, so render never meets a program eval did not.
        let ribbon = Program::assemble(MainKind::Ribbon, &decl_params, &block.text);
        if let Err(errs) = artscript_brush::check(&area) {
            // SkSL stops at the first bad uniform line, so each param is compiled alone to report
            // them all. The body's errors may follow from the bad name, so they wait for a rename.
            let names: Vec<(Fail, Span)> = if errs.iter().any(|e| matches!(e.origin, Origin::Param(_))) {
                decl_params
                    .iter()
                    .enumerate()
                    .filter_map(|(i, (key, kind))| {
                        let why = artscript_brush::rejects_param_name(key, *kind)?;
                        let f = fail("E0363", format!("a brush parameter cannot be named `{key}`: SkSL reserves it"))
                            .note(format!("SkSL: {why}"))
                            .fix("rename the parameter");
                        Some((f, at(Origin::Param(i))))
                    })
                    .collect()
            } else {
                Vec::new()
            };
            if !names.is_empty() {
                errors.extend(names);
                return Err(errors);
            }
            errors.extend(errs.into_iter().map(|e| {
                let f = fail("E0360", format!("SkSL: {}", e.message))
                    .note(format!("in brush `{name}`"))
                    .fix("fix the SkSL; the body defines `float4 shade(Mark m)`");
                (f, at(e.origin))
            }));
        } else if let Err(errs) = artscript_brush::check(&ribbon) {
            errors.extend(errs.into_iter().map(|e| {
                (fail("E0360", format!("SkSL on a stroke: {}", e.message)).note(format!("in brush `{name}`")), at(e.origin))
            }));
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(BrushVal {
            decl: Rc::new(BrushDecl {
                name: name.to_string(),
                params: decl_params,
                programs: [Arc::new(area), Arc::new(ribbon)],
            }),
            uniforms,
            place,
        })
    }

    pub(crate) fn call_brush(
        &mut self,
        b: &BrushVal,
        args: &[Expr],
        named: &[Named],
        sc: &Scope,
    ) -> R<Value> {
        let a = self.brush_args(args, named, sc)?;
        Ok(Value::Brush(Rc::new(b.with(&a.pos, &a.named)?)))
    }

    /// `brush(k:v ...)` on a def parameter whose default is a brush. The arguments are checked
    /// against the default whatever the instance bound, so every instance reports the same
    /// errors, and then applied to the bound brush. A bound `none` stays `none`.
    pub(crate) fn call_brush_param(
        &mut self,
        default: &BrushVal,
        bound: Option<Value>,
        args: &[Expr],
        named: &[Named],
        sc: &Scope,
    ) -> R<Value> {
        let a = self.brush_args(args, named, sc)?;
        default.with(&a.pos, &a.named)?;
        Ok(match bound {
            Some(Value::Brush(b)) => Value::Brush(Rc::new(b.with(&a.pos, &a.named)?)),
            _ => Value::None,
        })
    }

    fn brush_args(&mut self, args: &[Expr], named: &[Named], sc: &Scope) -> R<Args> {
        let mut a = Args { pos: Vec::with_capacity(args.len()), named: Vec::with_capacity(named.len()) };
        for e in args {
            a.pos.push(self.expr(e, sc)?);
        }
        for n in named {
            a.named.push((n.key.clone(), self.expr(&n.value, sc)?));
        }
        Ok(a)
    }

    /// The `brush:` value as an ordered stack. A bare word that is not a brush names the nearest
    /// brush in scope. `none` is an empty stack.
    pub(crate) fn brush_stack(&self, v: &Value, sc: &Scope) -> R<Vec<Rc<BrushVal>>> {
        let one = |v: &Value| -> R<Rc<BrushVal>> {
            match v {
                Value::Brush(b) => Ok(b.clone()),
                Value::Ident(w) if sc.module.lost(w) => Err(Fail::reported()),
                Value::Ident(w) => {
                    let names = sc.names_where(|v| matches!(v, Value::Brush(_)));
                    let f = fail("E0364", format!("`{w}` is not a brush"));
                    let f = if names.is_empty() {
                        f.note("no brushes in scope").fix("`use std/brushes { dry chip }` and write `brush:dry`")
                    } else {
                        let nearest = names
                            .iter()
                            .map(|n| (osa_distance(w, n), n))
                            .min_by_key(|(d, _)| *d)
                            .filter(|(d, _)| *d <= (w.len() + 1) / 3)
                            .map(|(_, n)| n.clone());
                        let f = f.note(format!("brushes in scope: {}", names.join(", ")));
                        match nearest {
                            Some(n) => f.fix(format!("did you mean `brush:{n}`?")),
                            None => f.fix("write `brush:<name>` with one of the listed brushes"),
                        }
                    };
                    Err(f)
                }
                other => Err(fail(
                    "E0364",
                    format!("`brush:` takes a brush, a list of brushes, or none, got {}", other.kind()),
                )
                .fix("write `brush:dry` or `brush:[chip dry]`")),
            }
        };
        match v {
            Value::None => Ok(Vec::new()),
            Value::List(items) => {
                if items.is_empty() {
                    return Err(fail("E0364", "`brush:[]` has no brushes").fix("write `brush:none` to clear an inherited brush"));
                }
                items.iter().map(one).collect()
            }
            other => Ok(vec![one(other)?]),
        }
    }

    /// One stage per brush of `stack` on `slot`, each seeded from this node's key, the slot, and
    /// its index in the stack, through `identity::seed` so `seed:` pins it and L0104 sees it.
    fn stages<'b>(
        &mut self,
        stack: &'b [Rc<BrushVal>],
        slot: Slot,
        kind: MainKind,
        key: &Key,
        explicit: Option<u64>,
        file: &str,
    ) -> Vec<(Stage, &'b Placement)> {
        stack
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let seed = identity::seed(file, &key.brush(slot.word(), i as u32), explicit, &mut self.uses);
                let stage = Stage {
                    program: b.decl.program(kind).clone(),
                    uniforms: b
                        .decl
                        .params
                        .iter()
                        .zip(&b.uniforms)
                        .map(|((n, _), u)| (n.clone(), *u))
                        .collect(),
                    seed: (seed ^ (seed >> 32)) as u32,
                };
                (stage, &b.place)
            })
            .collect()
    }

    pub(crate) fn area_marks(
        &mut self,
        stack: &[Rc<BrushVal>],
        key: &Key,
        explicit: Option<u64>,
        file: &str,
    ) -> Vec<AreaMark> {
        self.stages(stack, Slot::Fill, MainKind::Area, key, explicit, file)
            .into_iter()
            .map(|(stage, place)| {
                let rad = place.dir_deg.to_radians();
                AreaMark {
                    stage,
                    dir_deg: place.dir_deg as f32,
                    cos_sin: [rad.cos() as f32, rad.sin() as f32],
                    reach: place.reach as f32,
                }
            })
            .collect()
    }

    pub(crate) fn ribbon_marks(
        &mut self,
        stack: &[Rc<BrushVal>],
        key: &Key,
        explicit: Option<u64>,
        file: &str,
    ) -> Vec<RibbonMark> {
        self.stages(stack, Slot::Stroke, MainKind::Ribbon, key, explicit, file)
            .into_iter()
            .map(|(stage, place)| RibbonMark { stage, reach: place.reach as f32 })
            .collect()
    }
}

/// The per-point channels of a brushed stroke, as authored. `None` is the channel's default.
#[derive(Default)]
pub struct Channels {
    pub pressure: Option<Vec<f64>>,
    pub angle: Option<Vec<f64>>,
    pub tilt: Option<Vec<f64>>,
}

/// The key, default, and spelling of each channel, in the order the Mark lists them.
pub const CHANNEL_KEYS: &[(&str, f64, &str)] =
    &[("pressure", 1.0, "pressure:[.3 1 .5]"), ("angle", 0.0, "angle:[0 45 90]"), ("tilt", 0.0, "tilt:[0 .5 1]")];

impl Channels {
    pub fn get(&self, key: &str) -> Option<&[f64]> {
        match key {
            "pressure" => self.pressure.as_deref(),
            "angle" => self.angle.as_deref(),
            "tilt" => self.tilt.as_deref(),
            _ => unreachable!("channels are {CHANNEL_KEYS:?}"),
        }
    }

    pub fn set(&mut self, key: &str, values: Vec<f64>) {
        match key {
            "pressure" => self.pressure = Some(values),
            "angle" => self.angle = Some(values),
            "tilt" => self.tilt = Some(values),
            _ => unreachable!("channels are {CHANNEL_KEYS:?}"),
        }
    }
}

/// One contour of the geometry as the ribbon places values on it: its arc length, whether it
/// closes, and the authored points it passes through, in order. A closed contour's last knot
/// joins back to its first.
struct Run<'a> {
    start: f64,
    len: f64,
    closed: bool,
    knots: &'a [P],
}

impl Run<'_> {
    /// The chord from each knot to the next, the closing chord included on a closed contour.
    fn chords(&self) -> Vec<f64> {
        let k = self.knots;
        let mut out: Vec<f64> = k.windows(2).map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1)).collect();
        if self.closed && k.len() >= 2 {
            let (a, b) = (k[k.len() - 1], k[0]);
            out.push((b.0 - a.0).hypot(b.1 - a.1));
        }
        out
    }

    /// Local arc length of each knot by chord fraction, as the tapered outline places widths. A
    /// closed contour adds its first knot again at `len`.
    fn knot_at(&self) -> Vec<f64> {
        let mut acc = vec![0.0];
        for c in self.chords() {
            acc.push(acc.last().unwrap() + c);
        }
        let total = acc.last().copied().unwrap().max(1e-9);
        acc.iter().map(|a| a / total * self.len).collect()
    }
}

/// A piecewise-linear channel on one contour: rising local arc lengths and their values.
struct Line {
    at: Vec<f64>,
    values: Vec<f64>,
}

impl Line {
    fn at(&self, s: f64) -> f64 {
        let i = self.at.partition_point(|a| *a <= s);
        if i == 0 {
            return self.values[0];
        }
        if i == self.at.len() {
            return self.values[i - 1];
        }
        let t = ((s - self.at[i - 1]) / (self.at[i] - self.at[i - 1]).max(1e-9)).clamp(0.0, 1.0);
        self.values[i - 1] + (self.values[i] - self.values[i - 1]) * t
    }
}

/// One [`Line`] per run for a list of values. A list with one value per knot puts value `i` on
/// knot `i`, and a closed contour's list wraps back to its first value. Any other list spreads
/// evenly by arc length over the whole path. An empty list is `default` everywhere.
fn lines(values: &[f64], default: f64, spans: &[Run<'_>]) -> Vec<Line> {
    let constant = |v: f64| spans.iter().map(|_| Line { at: vec![0.0], values: vec![v] }).collect();
    match values {
        [] => return constant(default),
        [v] => return constant(*v),
        _ => {}
    }
    let knots: usize = spans.iter().map(|sp| sp.knots.len()).sum();
    if values.len() == knots {
        let mut next = 0;
        return spans
            .iter()
            .map(|sp| {
                let own = &values[next..next + sp.knots.len()];
                next += sp.knots.len();
                if own.is_empty() {
                    return Line { at: vec![0.0], values: vec![default] };
                }
                let mut vals = own.to_vec();
                if sp.closed {
                    vals.push(own[0]);
                }
                Line { at: sp.knot_at(), values: vals }
            })
            .collect();
    }
    let len: f64 = spans.iter().map(|sp| sp.len).sum();
    let whole = Line {
        at: (0..values.len()).map(|i| i as f64 / (values.len() - 1) as f64 * len).collect(),
        values: values.to_vec(),
    };
    spans
        .iter()
        .map(|sp| {
            let end = sp.start + sp.len;
            let mut at = vec![0.0];
            let mut vals = vec![whole.at(sp.start)];
            for (a, v) in whole.at.iter().zip(&whole.values) {
                if *a > sp.start && *a < end {
                    at.push(a - sp.start);
                    vals.push(*v);
                }
            }
            at.push(sp.len);
            vals.push(whole.at(end));
            Line { at, values: vals }
        })
        .collect()
}

/// Velocity from knot spacing: knots are evenly timed, so a long chord is a fast segment and
/// the mean chord is 1. A knot moves at the mean of the chords on either side of it, the closing
/// chord included on a closed contour. A contour without knots moves at 1.
fn velocity(spans: &[Run<'_>]) -> Vec<Line> {
    let chords: Vec<Vec<f64>> = spans.iter().map(Run::chords).collect();
    let all: Vec<f64> = chords.iter().flatten().copied().collect();
    let mean = (all.iter().sum::<f64>() / all.len().max(1) as f64).max(1e-9);
    spans
        .iter()
        .zip(&chords)
        .map(|(sp, chords)| {
            if chords.is_empty() {
                return Line { at: vec![0.0], values: vec![1.0] };
            }
            let n = chords.len();
            let mut v: Vec<f64> = (0..sp.knots.len())
                .map(|i| {
                    let before = if sp.closed { chords[(i + n - 1) % n] } else { chords[i.saturating_sub(1).min(n - 1)] };
                    let after = chords[i.min(n - 1)];
                    (before + after) / 2.0 / mean
                })
                .collect();
            if sp.closed {
                v.push(v[0]);
            }
            Line { at: sp.knot_at(), values: v }
        })
        .collect()
}

/// The spine of a stamped or brushed stroke: the geometry's contours with their measured lengths
/// and the channel lines placed on them. `knots` holds the authored points of each contour in
/// path order: a curve's points, or a path's on-curve vertices. `widths` and each channel list
/// sit on the knots when they have one value per knot and spread evenly otherwise. Built once
/// per node; the ribbon and the stamp placer both read it, so channel placement has one owner.
pub struct Spine<'a> {
    measures: Vec<ContourMeasure>,
    runs: Vec<Run<'a>>,
    width: Vec<Line>,
    pressure: Vec<Line>,
    angle: Vec<Line>,
    tilt: Vec<Line>,
    velocity: Vec<Line>,
}

impl<'a> Spine<'a> {
    pub fn new(path: &Path, knots: &'a [Vec<P>], widths: &[f64], ch: &Channels) -> Spine<'a> {
        let measures: Vec<ContourMeasure> = ContourMeasureIter::new(path, false, None).collect();
        let aligned = knots.len() == measures.len();
        let mut start = 0.0;
        let runs: Vec<Run<'a>> = measures
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let len = f64::from(m.length());
                let sp = Run { start, len, closed: m.is_closed(), knots: if aligned { &knots[i] } else { &[] } };
                start += len;
                sp
            })
            .collect();
        let channel = |key: &str, default: f64| lines(ch.get(key).unwrap_or(&[]), default, &runs);
        let (pressure, angle, tilt) = (channel("pressure", 1.0), channel("angle", 0.0), channel("tilt", 0.0));
        Spine {
            width: lines(widths, 1.0, &runs),
            pressure,
            angle,
            tilt,
            velocity: velocity(&runs),
            measures,
            runs,
        }
    }

    /// Contours with a length. Skia drops a zero-length contour, so a dot has none.
    pub fn contours(&self) -> usize {
        self.runs.len()
    }

    pub fn len(&self, contour: usize) -> f64 {
        self.runs[contour].len
    }

    pub fn closed(&self, contour: usize) -> bool {
        self.runs[contour].closed
    }

    /// The channels at arc length `s` of `contour`. `half_width` is before the taper.
    pub fn at(&self, contour: usize, s: f64) -> Knot {
        let i = contour;
        Knot {
            s: s as f32,
            half_width: (self.width[i].at(s) / 2.0) as f32,
            pressure: self.pressure[i].at(s) as f32,
            angle: self.angle[i].at(s) as f32,
            tilt: self.tilt[i].at(s) as f32,
            velocity: self.velocity[i].at(s) as f32,
        }
    }

    /// The point at arc length `s` of `contour` and the tangent there, in degrees of the
    /// geometry's frame (y down, 0 along +x).
    pub fn pos_tan(&self, contour: usize, s: f64) -> (P, f64) {
        let s = s.clamp(0.0, self.runs[contour].len) as f32;
        let (p, t) = self.measures[contour].pos_tan(s).unwrap_or_default();
        ((f64::from(p.x), f64::from(p.y)), f64::from(t.y).atan2(f64::from(t.x)).to_degrees())
    }

    /// Every arc length at which a channel line of `contour` bends, with 0 and `len`, rising
    /// and without near duplicates.
    fn bends(&self, contour: usize) -> Vec<f64> {
        let i = contour;
        let all = [&self.width[i], &self.pressure[i], &self.angle[i], &self.tilt[i], &self.velocity[i]];
        let len = self.runs[i].len;
        let mut at: Vec<f64> = all.iter().flat_map(|c| c.at.iter().copied()).chain([0.0, len]).collect();
        at.sort_by(f64::total_cmp);
        at.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
        at.iter().map(|&s| s.clamp(0.0, len)).collect()
    }
}

/// The ribbon a brushed stroke along `spine` becomes: one contour of knots per contour of the
/// spine, with a knot wherever a channel line bends.
pub fn ribbon(spine: &Spine<'_>, taper: (f64, f64), marks: Vec<RibbonMark>) -> Ribbon {
    let contours = (0..spine.contours())
        .map(|i| RibbonContour {
            len: spine.len(i) as f32,
            closed: spine.closed(i),
            knots: spine.bends(i).into_iter().map(|s| spine.at(i, s)).collect(),
        })
        .collect();
    Ribbon { marks, contours, taper: [taper.0 as f32, taper.1 as f32], ink: None }
}

/// A param cannot take the name of a `Mark` field, a helper, a placement key, or a runtime
/// uniform: the body would then read the param where it meant the field.
fn param_name_clash(key: &str) -> Option<Fail> {
    let what = if MARK_FIELDS.contains(&key) {
        "a `Mark` field"
    } else if HELPERS.contains(&key) {
        "a prelude helper"
    } else if key.starts_with(RESERVED_PREFIX) {
        "reserved for the runtime"
    } else {
        return None;
    };
    Some(
        fail("E0363", format!("a brush parameter cannot be named `{key}`: it is {what}"))
            .fix("rename the parameter"),
    )
}
