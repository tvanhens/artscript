//! Brush declarations, brush values with overrides, and the fill marks a node's `brush:` key
//! becomes. The SkSL contract itself lives in `artscript_brush`; this module binds `.art` values
//! to it and reports every failure at the author's line.

use std::rc::Rc;
use std::sync::Arc;

use artscript_brush::{
    HELPERS, MARK_FIELDS, MainKind, Origin, PLACEMENT_KEYS, Program, RESERVED_PREFIX,
    UniformKind, UniformValue,
};
use artscript_ir::{AreaMark, Stage};
use artscript_syntax::ast::{Expr, Named, SkslBlock, Span};

use crate::eval::{Evaluator, Fail, R, Scope, fail, osa_distance};
use crate::identity::{self, Key};
use crate::scope::Module;
use crate::value::{Kind, Value};

/// A `brush` declaration after `check` accepted its program. Shared by every value made from it.
pub struct BrushDecl {
    pub name: String,
    /// `file:line` of the declaration.
    pub origin: String,
    /// Declared uniforms in head order.
    pub params: Vec<(String, UniformKind)>,
    pub program: Arc<Program>,
}

/// The paint slot a brush is narrowed to with `on:`.
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

/// Placement settings. Every field has a default, so a brush with none set is usable.
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
    /// `DeclKind::Brush`. Defaults are evaluated in module scope and seeded like a library
    /// `let`. Head keys split into placement and uniforms, the body is assembled and checked,
    /// and every failure is reported at its own span: a param at its key, a body error at its
    /// line and column in the `sksl` block, a frame error at the `sksl` keyword. A brush that
    /// fails is marked lost so its use sites stay quiet.
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
        for ident in artscript_brush::reserved_identifiers(&block.text) {
            errors.push((
                fail("E0363", format!("`{ident}` is reserved: names starting with `{RESERVED_PREFIX}` belong to the runtime"))
                    .fix("rename it"),
                block.span,
            ));
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        let program = Program::assemble(MainKind::Area, &decl_params, &block.text);
        if let Err(errs) = artscript_brush::check(&program) {
            let at = |origin: Origin| match origin {
                Origin::Body { line, col } => Span {
                    line: block.body.line + line - 1,
                    col: block.body.col + col.unwrap_or(0),
                },
                Origin::Param(i) => params
                    .iter()
                    .filter(|p| !PLACEMENT_KEYS.contains(&p.key.as_str()))
                    .nth(i)
                    .map_or(block.span, |p| p.span),
                Origin::Frame => block.span,
            };
            return Err(errs
                .into_iter()
                .map(|e| {
                    let f = fail("E0360", format!("SkSL: {}", e.message))
                        .note(format!("in brush `{name}`"))
                        .fix("fix the SkSL; the body defines `float4 shade(Mark m)`");
                    (f, at(e.origin))
                })
                .collect());
        }
        Ok(BrushVal {
            decl: Rc::new(BrushDecl {
                name: name.to_string(),
                origin: format!("{}:{}", module.path, span.line),
                params: decl_params,
                program: Arc::new(program),
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
        let (pos, nm) = self.brush_args(args, named, sc)?;
        Ok(Value::Brush(Rc::new(b.with(&pos, &nm)?)))
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
        let (pos, nm) = self.brush_args(args, named, sc)?;
        default.with(&pos, &nm)?;
        Ok(match bound {
            Some(Value::Brush(b)) => Value::Brush(Rc::new(b.with(&pos, &nm)?)),
            _ => Value::None,
        })
    }

    fn brush_args(&mut self, args: &[Expr], named: &[Named], sc: &Scope) -> R<(Vec<Value>, Vec<(String, Value)>)> {
        let mut pos = Vec::with_capacity(args.len());
        for e in args {
            pos.push(self.expr(e, sc)?);
        }
        let mut nm = Vec::with_capacity(named.len());
        for n in named {
            nm.push((n.key.clone(), self.expr(&n.value, sc)?));
        }
        Ok((pos, nm))
    }

    /// The `brush:` value as an ordered stack. A bare word that is not a brush names the nearest
    /// brush in scope. `none` is an empty stack.
    pub(crate) fn brush_stack(&self, v: &Value, sc: &Scope) -> R<Vec<Rc<BrushVal>>> {
        let one = |v: &Value| -> R<Rc<BrushVal>> {
            match v {
                Value::Brush(b) => Ok(b.clone()),
                Value::Ident(w) if sc.module.lost(w) => Err(Fail::reported()),
                Value::Ident(w) => {
                    let names = self.brushes_in_scope(sc);
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

    fn brushes_in_scope(&self, sc: &Scope) -> Vec<String> {
        let mut names: Vec<String> = sc
            .env
            .iter()
            .filter(|(_, v)| matches!(v, Value::Brush(_)))
            .map(|(k, _)| k.clone())
            .collect();
        for (k, v) in sc.module.globals.borrow().iter() {
            if matches!(v, Value::Brush(_)) && !names.contains(k) {
                names.push(k.clone());
            }
        }
        names
    }

    /// The fill's marks: one stage per brush, each seeded from this node's key, the slot, and
    /// its index in the stack, through `identity::seed` so `seed:` pins it and L0104 sees it.
    pub(crate) fn area_marks(
        &mut self,
        stack: &[Rc<BrushVal>],
        key: &Key,
        explicit: Option<u64>,
        file: &str,
    ) -> Vec<AreaMark> {
        stack
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let seed = identity::seed(file, &key.brush(Slot::Fill.word(), i as u32), explicit, &mut self.uses);
                let rad = b.place.dir_deg.to_radians();
                AreaMark {
                    stage: Stage {
                        program: b.decl.program.clone(),
                        uniforms: b
                            .decl
                            .params
                            .iter()
                            .zip(&b.uniforms)
                            .map(|((n, _), u)| (n.clone(), *u))
                            .collect(),
                        seed: (seed ^ (seed >> 32)) as u32,
                        origin: b.decl.origin.clone(),
                    },
                    dir_deg: b.place.dir_deg as f32,
                    cos_sin: [rad.cos() as f32, rad.sin() as f32],
                    reach: b.place.reach as f32,
                }
            })
            .collect()
    }
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
