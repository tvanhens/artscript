//! Stamp declarations, stamp values with overrides, the `stamp:` key, and the placement of a
//! stamp's block along a node's geometry. A stamp is a block drawn once per placement, in a frame
//! on the spine with x along the direction of travel. Each placement is an isolated group with
//! its own key, so `rand` in the block differs per stamp and `seed:` on the host pins them all.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use artscript_ir::{Matrix, Node, NodeKind, Paint, Point};
use artscript_syntax::ast::{DefItem, Expr, Named, Scene, Span};

use crate::brush::{CHANNEL_KEYS, Spine};
use crate::color::Rgba;
use crate::eval::{Args, Evaluator, Fail, R, Scope, fail, first_fill, in_source_order, new_node, osa_distance};
use crate::fx::FxSite;
use crate::geom::P;
use crate::identity::{self, Key, Lookup, Name, Names, Pending};
use crate::lints::body_idents;
use crate::scope::{Module, Record, RowRecord};
use crate::value::{Col, Value};

/// The names a stamp block sees besides the stamp's parameters, in the order they are bound.
pub const STAMP_NAMES: &[&str] =
    &["i", "s", "u", "len", "width", "tangent", "pressure", "angle", "tilt", "velocity", "paint"];

/// Keys a call may set besides the parameters: the placement keys and the set's `name:`.
const CALL_KEYS: &[&str] = &["spacing", "count", "name"];

/// A `stamp` declaration after `declare_stamp` accepted it. Shared by every value made from it.
pub struct StampDecl {
    pub name: String,
    /// Parameters in head order with their defaults, evaluated at the declaration. A call's
    /// argument must match the default's kind, as a def parameter's does.
    pub params: Vec<(String, Value)>,
    pub body: Vec<DefItem>,
    /// Every identifier and function name the body mentions. The channel-reader check asks
    /// whether `pressure` is among them.
    pub mentions: BTreeSet<String>,
    pub module: String,
    pub span: Span,
}

/// How placements are spread along each contour.
#[derive(Clone)]
pub enum Spacing {
    /// Evaluated at every stamp: the distance to the next one. The first stamp sits at half the
    /// value at `s = 0`. A percent measures the stroke's width at `s`.
    Every(Rc<SpacingExpr>),
    /// Exactly `n` per contour, at `(j + .5) * len / n`.
    Count(u32),
}

/// A `spacing:` expression with the bindings it reads. The declaration's own expression reads
/// the stamp's parameters; a call site's expression reads that site's bindings. Both see the
/// bound stamp names on top.
pub struct SpacingExpr {
    expr: Expr,
    span: Span,
    /// `None` for the declaration's expression, else the call site's scope.
    site: Option<Scope>,
    mentions: BTreeSet<String>,
}

impl SpacingExpr {
    /// Whether the value can differ from one stamp to the next: it reads a bound name or draws
    /// a random number. A constant spacing on a closed contour is spread evenly instead.
    fn varies(&self) -> bool {
        STAMP_NAMES.iter().any(|n| self.mentions.contains(*n))
            || self.mentions.contains("rand")
            || self.mentions.contains("pick")
    }
}

/// A stamp value: a declaration plus bound parameter values, placement, and the set's name.
pub struct StampVal {
    pub decl: Rc<StampDecl>,
    /// Same order as `decl.params`.
    pub params: Vec<Value>,
    pub spacing: Spacing,
    /// `name:` on the call, else the declared name.
    pub name: Name,
}

impl StampVal {
    /// `leaves(spacing:6 tint:p.gold)`: a new value with those keys replaced. `named` holds the
    /// parameter arguments; the placement and `name:` arrive resolved.
    pub fn with(&self, pos: &[Value], named: &[(String, Value)], spacing: Option<Spacing>, name: Option<Name>) -> R<StampVal> {
        let decl_name = &self.decl.name;
        if !pos.is_empty() {
            return Err(self.argument_error(format!("`{decl_name}` takes named arguments only")));
        }
        let mut out = StampVal {
            decl: self.decl.clone(),
            params: self.params.clone(),
            spacing: spacing.unwrap_or_else(|| self.spacing.clone()),
            name: name.unwrap_or_else(|| self.name.clone()),
        };
        for (k, v) in named {
            let Some(i) = self.decl.params.iter().position(|(p, _)| p == k) else {
                let nearest = self
                    .decl
                    .params
                    .iter()
                    .map(|(p, _)| p.as_str())
                    .chain(CALL_KEYS.iter().copied())
                    .map(|p| (osa_distance(k, p), p))
                    .min_by_key(|(d, _)| *d)
                    .filter(|(d, _)| *d <= (k.len() + 1) / 3);
                let f = self.argument_error(format!("`{decl_name}` has no parameter `{k}`"));
                return Err(match nearest {
                    Some((_, p)) => f.fix(format!("did you mean `{p}:`?")),
                    None => f.fix("use one of the listed parameters"),
                });
            };
            let default = &self.decl.params[i].1;
            if !default.accepts(v) {
                return Err(self
                    .argument_error(format!("`{k}` of `{decl_name}` expects {}, got {}", default.kind(), v.kind()))
                    .fix(format!("pass {}", default.kind())));
            }
            out.params[i] = v.clone();
        }
        Ok(out)
    }

    fn argument_error(&self, msg: String) -> Fail {
        self.with_notes(fail("E0366", msg))
    }

    fn with_notes(&self, f: Fail) -> Fail {
        let params: Vec<&str> = self.decl.params.iter().map(|(p, _)| p.as_str()).collect();
        f.note(format!("parameters: {}", params.join(", ")))
            .note(format!("placement: {}", CALL_KEYS.join(", ")))
    }

    /// Whether the block or the spacing reads the channel `key`.
    pub fn reads(&self, key: &str) -> bool {
        self.decl.mentions.contains(key)
            || matches!(&self.spacing, Spacing::Every(e) if e.mentions.contains(key))
    }
}

/// One placement on the spine, in the host's local frame.
struct Site {
    i: u32,
    s: f64,
    len: f64,
    pos: P,
    tangent_deg: f64,
    knot: artscript_ir::Knot,
}

/// What `paint_shape` hands the placer about the host.
pub(crate) struct Host<'a> {
    pub key: &'a Key,
    /// The host's `seed:`.
    pub explicit: Option<u64>,
    /// Canvas from the host's local frame.
    pub world: &'a Matrix,
    /// The `stamp:` key on the host's own line, or `None` when inherited.
    pub own_site: Option<Span>,
    /// The host statement, for the unstable-name report.
    pub span: Span,
    /// The stroke's paint, or the fill's where the host has no stroke, resolved to the host's
    /// bounds. `None` binds `paint` to the default line color.
    pub paint: Option<Paint>,
}

impl Evaluator<'_> {
    /// Declares `stamp NAME`. Parameter defaults evaluate now, in the module's scope. A stamp
    /// that fails is marked lost so its use sites stay quiet.
    pub(crate) fn declare_stamp(&mut self, module: &Rc<Module>, name: &str, params: &[Named], body: &[DefItem], span: Span) -> R<()> {
        let file = module.path.clone();
        match self.build_stamp(module, name, params, body, span) {
            Ok(v) => {
                module.define(name, Value::Stamp(Rc::new(v)));
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

    fn build_stamp(&mut self, module: &Rc<Module>, name: &str, params: &[Named], body: &[DefItem], span: Span) -> Result<StampVal, Vec<(Fail, Span)>> {
        let sc = self.module_scope(module);
        self.begin_seed(&module.path, Key::binding(name));
        let mut errors = Vec::new();
        let mut decl_params = Vec::new();
        let mut spacing: Option<Spacing> = None;
        for p in params {
            if p.key == "spacing" {
                if spacing.is_some() {
                    errors.push((fail("E0365", format!("stamp `{name}` has both `spacing:` and `count:`")).fix("keep one of them"), p.span));
                    continue;
                }
                spacing = Some(Spacing::Every(Rc::new(SpacingExpr {
                    expr: p.value.clone(),
                    span: p.span,
                    site: None,
                    mentions: expr_idents(&p.value),
                })));
                continue;
            }
            let v = match self.expr(&p.value, &sc) {
                Ok(v) => v,
                Err(f) => {
                    errors.push((f, p.span));
                    continue;
                }
            };
            if p.key == "count" {
                if spacing.is_some() {
                    errors.push((fail("E0365", format!("stamp `{name}` has both `spacing:` and `count:`")).fix("keep one of them"), p.span));
                    continue;
                }
                match count_of(name, &v) {
                    Ok(n) => spacing = Some(Spacing::Count(n)),
                    Err(f) => errors.push((f, p.span)),
                }
                continue;
            }
            if STAMP_NAMES.contains(&p.key.as_str()) {
                errors.push((
                    fail("E0365", format!("a stamp parameter cannot be named `{}`: the block reads that name from each placement", p.key))
                        .note(format!("bound names: {}", STAMP_NAMES.join(" ")))
                        .fix("rename the parameter"),
                    p.span,
                ));
                continue;
            }
            if p.key == "name" {
                errors.push((fail("E0365", "`name:` on a stamp declaration is not a parameter").fix("name the set at the call, as in `leaves(name:small)`"), p.span));
                continue;
            }
            decl_params.push((p.key.clone(), v));
        }
        let Some(spacing) = spacing else {
            errors.push((fail("E0365", format!("stamp `{name}` has no `spacing:` or `count:`")).fix("write `spacing:8` or `count:12`"), span));
            return Err(errors);
        };
        for s in body.iter().filter_map(DefItem::scene) {
            if let Some((what, at)) = forbidden_line(s) {
                errors.push((fail("E0365", format!("a stamp block cannot hold {what}")).fix("draw it inside a def and place the def from the block"), at));
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        let mut mentions = body_idents(body);
        if let Spacing::Every(e) = &spacing {
            mentions.extend(e.mentions.iter().cloned());
        }
        Ok(StampVal {
            decl: Rc::new(StampDecl {
                name: name.to_string(),
                params: decl_params,
                body: body.to_vec(),
                mentions,
                module: module.path.clone(),
                span,
            }),
            params: Vec::new(),
            spacing,
            name: Name::parse(name).expect("a declaration name is an identifier"),
        })
        .map(|mut v| {
            v.params = v.decl.params.iter().map(|(_, d)| d.clone()).collect();
            v
        })
    }

    /// `leaves(k:v ...)` in an expression.
    pub(crate) fn call_stamp(&mut self, s: &StampVal, args: &[Expr], named: &[Named], sc: &Scope) -> R<Value> {
        let (a, spacing, name) = self.stamp_args(s, args, named, sc)?;
        Ok(Value::Stamp(Rc::new(s.with(&a.pos, &a.named, spacing, name)?)))
    }

    /// `stamp(k:v)` on a def parameter whose default is a stamp: checked against the default
    /// whatever the instance bound, then applied to the bound stamp. A bound `none` stays `none`.
    pub(crate) fn call_stamp_param(&mut self, default: &StampVal, bound: Option<Value>, args: &[Expr], named: &[Named], sc: &Scope) -> R<Value> {
        let (a, spacing, name) = self.stamp_args(default, args, named, sc)?;
        default.with(&a.pos, &a.named, spacing.clone(), name.clone())?;
        Ok(match bound {
            Some(Value::Stamp(s)) => Value::Stamp(Rc::new(s.with(&a.pos, &a.named, spacing, name)?)),
            _ => Value::None,
        })
    }

    /// The call's arguments: parameters as values, `spacing:` as an expression bound to this
    /// site, `count:` as a number, `name:` as a name.
    fn stamp_args(&mut self, s: &StampVal, args: &[Expr], named: &[Named], sc: &Scope) -> R<(Args, Option<Spacing>, Option<Name>)> {
        let mut a = Args { pos: Vec::with_capacity(args.len()), named: Vec::new() };
        for e in args {
            a.pos.push(self.expr(e, sc)?);
        }
        let mut spacing = None;
        let mut name = None;
        let decl = &s.decl.name;
        for n in named {
            match n.key.as_str() {
                "spacing" | "count" if spacing.is_some() => {
                    return Err(s.argument_error(format!("`{decl}(...)` sets both `spacing:` and `count:`")).fix("keep one of them"));
                }
                "spacing" => {
                    spacing = Some(Spacing::Every(Rc::new(SpacingExpr {
                        expr: n.value.clone(),
                        span: n.span,
                        site: Some(sc.clone()),
                        mentions: expr_idents(&n.value),
                    })));
                }
                "count" => {
                    let v = self.expr(&n.value, sc)?;
                    spacing = Some(Spacing::Count(count_of(decl, &v).map_err(|f| s.with_notes(f))?));
                }
                "name" => {
                    let v = self.expr(&n.value, sc)?;
                    name = Some(v.word().and_then(Name::parse).ok_or_else(|| {
                        s.argument_error(format!("`name` of `{decl}` takes one identifier, got {}", v.show())).fix("write `name:small`")
                    })?);
                }
                _ => a.named.push((n.key.clone(), self.expr(&n.value, sc)?)),
            }
        }
        Ok((a, spacing, name))
    }

    /// The `stamp:` value as an ordered list of sets. `none` is empty. A bare word that is not a
    /// stamp names the nearest stamp in scope.
    pub(crate) fn stamp_stack(&self, v: &Value, sc: &Scope) -> R<Vec<Rc<StampVal>>> {
        let one = |v: &Value| -> R<Rc<StampVal>> {
            match v {
                Value::Stamp(s) => Ok(s.clone()),
                Value::Ident(w) if sc.module.lost(w) => Err(Fail::reported()),
                Value::Ident(w) => {
                    let names = self.stamps_in_scope(sc);
                    let f = fail("E0367", format!("`{w}` is not a stamp"));
                    let f = if names.is_empty() {
                        f.note("no stamps in scope").fix("`use std/stamps { dots }` and write `stamp:dots`")
                    } else {
                        let nearest = names
                            .iter()
                            .map(|n| (osa_distance(w, n), n))
                            .min_by_key(|(d, _)| *d)
                            .filter(|(d, _)| *d <= (w.len() + 1) / 3)
                            .map(|(_, n)| n.clone());
                        let f = f.note(format!("stamps in scope: {}", names.join(", ")));
                        match nearest {
                            Some(n) => f.fix(format!("did you mean `stamp:{n}`?")),
                            None => f.fix("write `stamp:<name>` with one of the listed stamps"),
                        }
                    };
                    Err(f)
                }
                other => Err(fail("E0367", format!("`stamp:` takes a stamp, a list of stamps, or none, got {}", other.kind()))
                    .fix("write `stamp:dots` or `stamp:[dots leaves]`")),
            }
        };
        match v {
            Value::None => Ok(Vec::new()),
            Value::List(items) => {
                if items.is_empty() {
                    return Err(fail("E0367", "`stamp:[]` has no stamps").fix("write `stamp:none` to clear an inherited stamp"));
                }
                items.iter().map(one).collect()
            }
            other => Ok(vec![one(other)?]),
        }
    }

    fn stamps_in_scope(&self, sc: &Scope) -> Vec<String> {
        let mut names: Vec<String> = sc.env.iter().filter(|(_, v)| matches!(v, Value::Stamp(_))).map(|(k, _)| k.clone()).collect();
        for (k, v) in sc.module.globals.borrow().iter() {
            if matches!(v, Value::Stamp(_)) && !names.contains(k) {
                names.push(k.clone());
            }
        }
        names
    }

    /// Places every set along `spine` and draws the block once per site. Returns the stamp
    /// nodes in draw order and one record row per set, for `@host.set.i` references.
    ///
    /// Errors at the `stamp:` key go through `at_key`, which reports them at the key on the
    /// host's own line or notes that the key is inherited.
    pub(crate) fn stamps(
        &mut self,
        sets: &[Rc<StampVal>],
        spine: &Spine<'_>,
        host: &Host<'_>,
        sc: &Scope,
        at_key: &mut dyn FnMut(&mut Self, Fail) -> Fail,
    ) -> R<(Vec<Node>, Vec<RowRecord>)> {
        let mut seen: Vec<&Name> = Vec::new();
        for set in sets {
            if seen.contains(&&set.name) {
                let f = fail("E0333", format!("`{}` is already a stamp set on this node", set.name))
                    .note("two sets with one name would share their paths")
                    .fix(format!("name one of them, as in `{}(name:small)`", set.decl.name));
                return Err(at_key(self, f));
            }
            seen.push(&set.name);
        }
        if matches!(host.paint, Some(Paint::Field { .. })) && sets.iter().any(|s| s.reads("paint")) {
            let f = fail("E0368", "`paint` cannot be read from a field paint")
                .note("only render knows a field's value at a point")
                .fix("paint the node with a color or a gradient, or give the stamp a color parameter");
            return Err(at_key(self, f));
        }
        if spine.contours() == 0 {
            if host.own_site.is_some() {
                let f = fail("E0368", "`stamp:` on a node with nothing to stamp along")
                    .note("every contour of the geometry has zero length")
                    .fix("give the node a path, or remove `stamp:`");
                return Err(at_key(self, f));
            }
            return Ok((Vec::new(), Vec::new()));
        }
        let snap = self.snap_seed();
        let mut nodes = Vec::new();
        let mut rows = Vec::new();
        for set in sets {
            if self.stamping.contains(&Rc::as_ptr(&set.decl)) {
                let f = fail("E0369", format!("stamp `{}` places itself", set.decl.name))
                    .note("a stamp block that stamps its own shapes would never end")
                    .fix("remove `stamp:` from the shapes in the block, or stamp a different stamp");
                self.restore_seed(snap);
                return Err(at_key(self, f));
            }
            self.stamping.push(Rc::as_ptr(&set.decl));
            let placed = self.place_set(set, spine, host, sc);
            self.stamping.pop();
            let records = placed?;
            rows.push(RowRecord { name: set.name.clone(), stamps: records.iter().map(|(_, r)| r.clone()).collect() });
            nodes.extend(records.into_iter().map(|(n, _)| n));
        }
        self.restore_seed(snap);
        Ok((nodes, rows))
    }

    fn place_set(&mut self, set: &Rc<StampVal>, spine: &Spine<'_>, host: &Host<'_>, sc: &Scope) -> R<Vec<(Node, Rc<RefCell<Record>>)>> {
        let decl = set.decl.clone();
        let module = self.modules.get(&decl.module).cloned().ok_or_else(|| fail("E0210", format!("module `{}` is not loaded", decl.module)))?;
        let def_ids = self.ids.get(&decl.module).cloned().unwrap_or_else(|| Rc::new(identity::ModuleIds::blank()));
        let region = def_ids.region(decl.span).unwrap_or_else(|| def_ids.file());
        let root = match host.explicit {
            Some(n) => host.key.seeded(&decl.module, &decl.name, n),
            None => host.key.clone(),
        };
        let set_key = Key::named(&root, &set.name, &[]);
        if !region.is_empty() {
            self.unstable_scope(&set_key, &region, sc.file(), host.span);
        }
        let typed: Vec<(usize, Value)> = decl
            .params
            .iter()
            .enumerate()
            .filter(|(_, (_, d))| matches!(d, Value::Brush(_) | Value::Stamp(_)))
            .map(|(i, (_, d))| (i, d.clone()))
            .collect();
        let style: Vec<(String, Value)> = sc
            .style
            .iter()
            .filter(|(k, _)| k != "stamp" && !CHANNEL_KEYS.iter().any(|(c, _, _)| c == k))
            .cloned()
            .collect();
        let frame = StampFrame {
            set,
            decl: &decl,
            module: &module,
            def_ids: &def_ids,
            region: &region,
            root: &root,
            typed: Rc::new(typed),
            style: Rc::new(style),
            host,
            sc,
        };
        let mut out = Vec::new();
        let mut i = 0u32;
        for c in 0..spine.contours() {
            let len = spine.len(c);
            if len <= 0.0 {
                continue;
            }
            match &set.spacing {
                Spacing::Count(n) => {
                    for j in 0..*n {
                        let s = (f64::from(j) + 0.5) * len / f64::from(*n);
                        if !self.one_stamp(&frame, spine, c, s, &mut i, &mut out)? {
                            return Ok(out);
                        }
                    }
                }
                Spacing::Every(e) => {
                    let closed = spine.closed(c);
                    let first = self.step_at(&frame, e, spine, c, 0.0, i)?;
                    if closed && !e.varies() {
                        let n = (len / first).round().max(1.0);
                        for j in 0..n as u32 {
                            let s = (f64::from(j) + 0.5) * len / n;
                            if !self.one_stamp(&frame, spine, c, s, &mut i, &mut out)? {
                                return Ok(out);
                            }
                        }
                        continue;
                    }
                    let mut s = first / 2.0;
                    while s <= len {
                        let at = i;
                        if !self.one_stamp(&frame, spine, c, s, &mut i, &mut out)? {
                            return Ok(out);
                        }
                        let step = self.step_at(&frame, e, spine, c, s, at)?;
                        let next = s + step;
                        if closed && next >= len - step / 2.0 {
                            break;
                        }
                        s = next;
                    }
                }
            }
        }
        Ok(out)
    }

    /// The spacing at arc length `s`, evaluated in the block's scope with stamp `i` bound. A
    /// percent measures the stroke width there. Not a positive finite number: E0368 at the
    /// spacing expression, with the stamp's path in a note.
    fn step_at(&mut self, frame: &StampFrame<'_>, e: &SpacingExpr, spine: &Spine<'_>, contour: usize, s: f64, i: u32) -> R<f64> {
        let site = site_of(spine, contour, s, i);
        let key = frame.stamp_key(i);
        let path = key.to_string();
        let scope = match &e.site {
            Some(call) => {
                let mut scope = call.clone();
                bind_site(&mut scope.env, &site, frame.paint_at(&site));
                scope
            }
            None => frame.block_scope(&key, &site, Matrix::new_identity()),
        };
        self.begin_seed(scope.file(), key.at_stmt(&[], "spacing"));
        let file = scope.file().to_string();
        let v = self.expr(&e.expr, &scope).map_err(|f| self.report_at(f.note(format!("in the spacing of `{path}`")), &file, e.span, &path))?;
        let step = match &v {
            Value::Num(n) => *n,
            Value::Percent(p) => p * f64::from(site.knot.half_width) * 2.0,
            other => {
                let f = fail("E0368", format!("`spacing:` expects a number or a percent, got {}", other.kind()))
                    .note(format!("at `{path}`"))
                    .fix("write `spacing:8` or `spacing:150%`");
                return Err(self.report_at(f, &file, e.span, &path));
            }
        };
        if !(step.is_finite() && step > 0.0) {
            let mut f = fail("E0368", format!("`spacing:` must be a positive number, got {}", v.show())).note(format!("at `{path}`"));
            if matches!(v, Value::Percent(_)) && site.knot.half_width <= 0.0 {
                f = f.note("a percent spacing measures the stroke width, and this node has no stroke");
            }
            return Err(self.report_at(f.fix("write `spacing:8`"), &file, e.span, &path));
        }
        Ok(step)
    }

    /// Reports `f` at `span` in `file` and hands back the reported marker, so the caller stops
    /// the set with one diagnostic.
    fn report_at(&mut self, f: Fail, file: &str, span: Span, node: &str) -> Fail {
        self.report(f, file, span, node);
        Fail::reported()
    }

    /// Draws the block at `s`. Returns `false` when the block reported an error, so the caller
    /// skips the rest of the set and one typo is one diagnostic.
    fn one_stamp(&mut self, frame: &StampFrame<'_>, spine: &Spine<'_>, contour: usize, s: f64, i: &mut u32, out: &mut Vec<(Node, Rc<RefCell<Record>>)>) -> R<bool> {
        let site = site_of(spine, contour, s, *i);
        *i += 1;
        let key = frame.stamp_key(site.i);
        let path = key.to_string();
        let place = Matrix::translate((site.pos.0 as f32, site.pos.1 as f32)) * Matrix::rotate_deg(site.tangent_deg as f32);
        let mut body = frame.block_scope(&key, &site, place);
        let names = body.names.clone();
        let checks = body.checks.clone();
        let branch_fx = body.collect_branch_fx();
        let before = self.diags.len();
        let children = self.eval_block_nodes(frame.decl.body.iter().filter_map(DefItem::scene), &mut body);
        let origin = format!("{}:{}", frame.decl.module, frame.decl.span.line);
        let mut node = new_node(path.clone(), origin, place, NodeKind::Group { isolate: true, children });
        let fill = first_fill(&node).unwrap_or(Rgba::GRAY);
        let branch_fx = branch_fx.take();
        let lines = in_source_order(frame.decl.body.iter().filter_map(DefItem::fx), &branch_fx, &body);
        let sites: Vec<FxSite> = lines.iter().map(|(line, scope)| FxSite { line, scope }).collect();
        node.effects = self.lower_fx(&sites, &key, fill);
        self.run_checks(&checks);
        let rec = Rc::new(RefCell::new(Record {
            name: String::new(),
            path: path.clone(),
            key,
            parent_world: *frame.host.world,
            node: Some(node.clone()),
            anchor: None,
            at: site.pos,
            home: None,
            decl: None,
            inner: Some(names),
            rows: Rc::default(),
        }));
        self.placed.borrow_mut().insert(path, rec.clone());
        out.push((node, rec));
        Ok(self.diags.len() == before)
    }
}

/// Everything one set's placements share.
struct StampFrame<'a> {
    set: &'a StampVal,
    decl: &'a Rc<StampDecl>,
    module: &'a Rc<Module>,
    def_ids: &'a Rc<identity::ModuleIds>,
    region: &'a Rc<identity::Region>,
    root: &'a Key,
    typed: Rc<Vec<(usize, Value)>>,
    style: Rc<Vec<(String, Value)>>,
    host: &'a Host<'a>,
    sc: &'a Scope,
}

impl StampFrame<'_> {
    /// `host.set.i`, under the host's seed root when it has `seed:`.
    fn stamp_key(&self, i: u32) -> Key {
        Key::named(self.root, &self.set.name, &[i])
    }

    /// The scope the block runs in: the declaration's module, the parameters then the bound
    /// names, the host's enclosing style without `stamp:` and the channels, and the stamp's own
    /// frame on the spine.
    fn block_scope(&self, key: &Key, site: &Site, place: Matrix) -> Scope {
        let names = Rc::new(RefCell::new(Names::new(self.region.clone())));
        // An empty frame, as a def without `size` has: a centered shape with no `at:` sits on
        // the spine, and a percent measures nothing.
        let mut body = self.sc.enter(
            key.clone(),
            names.clone(),
            Lookup::own(names),
            Pending::empty(),
            artscript_ir::Rect::new_empty(),
            *self.host.world * place,
            self.sc.depth + 1,
        );
        let mut env: Vec<(String, Value)> = self.decl.params.iter().map(|(k, _)| k.clone()).zip(self.set.params.iter().cloned()).collect();
        bind_site(&mut env, site, self.paint_at(site));
        body.env = env;
        body.typed_params = self.typed.clone();
        body.module = self.module.clone();
        body.ids = self.def_ids.clone();
        body.inst = None;
        body.host = None;
        body.checks = Rc::new(RefCell::new(Vec::new()));
        body.style = self.style.clone();
        body.palette = self.sc.palette.clone();
        body
    }

    /// The host's paint at the site, as the `paint` name.
    fn paint_at(&self, site: &Site) -> Value {
        let rgba = match &self.host.paint {
            Some(p) => sample(p, site.pos),
            None => Some(crate::color::line(Rgba::GRAY)),
        };
        match rgba {
            Some(c) => Value::Color(Col::plain(c)),
            None => Value::None,
        }
    }
}

fn site_of(spine: &Spine<'_>, contour: usize, s: f64, i: u32) -> Site {
    let (pos, tangent_deg) = spine.pos_tan(contour, s);
    Site { i, s, len: spine.len(contour), pos, tangent_deg, knot: spine.at(contour, s) }
}

/// Appends the bound stamp names, in [`STAMP_NAMES`] order.
fn bind_site(env: &mut Vec<(String, Value)>, site: &Site, paint: Value) {
    let k = site.knot;
    let values = [
        Value::Num(f64::from(site.i)),
        Value::Num(site.s),
        Value::Num(if site.len > 0.0 { site.s / site.len } else { 0.0 }),
        Value::Num(site.len),
        Value::Num(f64::from(k.half_width) * 2.0),
        Value::Num(site.tangent_deg),
        Value::Num(f64::from(k.pressure)),
        Value::Num(f64::from(k.angle)),
        Value::Num(f64::from(k.tilt)),
        Value::Num(f64::from(k.velocity)),
        paint,
    ];
    env.extend(STAMP_NAMES.iter().map(|n| n.to_string()).zip(values));
}

/// A gradient's color at `p`, with the stops interpolated as render does; `None` for a field,
/// whose values only render knows.
pub(crate) fn sample(paint: &Paint, p: P) -> Option<Rgba> {
    let at = |stops: &[artscript_ir::Stop], t: f32| {
        let t = t.clamp(0.0, 1.0);
        let i = stops.partition_point(|s| s.pos <= t);
        let c = if i == 0 {
            stops.first()?.color
        } else if i == stops.len() {
            stops[i - 1].color
        } else {
            let (a, b) = (stops[i - 1], stops[i]);
            let f = ((t - a.pos) / (b.pos - a.pos).max(1e-9)).clamp(0.0, 1.0);
            let mix = |x: f32, y: f32| x + (y - x) * f;
            artscript_ir::Color { r: mix(a.color.r, b.color.r), g: mix(a.color.g, b.color.g), b: mix(a.color.b, b.color.b), a: mix(a.color.a, b.color.a) }
        };
        Some(Rgba::from_ir(c))
    };
    let p = Point::new(p.0 as f32, p.1 as f32);
    match paint {
        Paint::Solid(c) => Some(Rgba::from_ir(*c)),
        Paint::Linear { from, to, stops } => {
            let d = *to - *from;
            let len2 = d.x * d.x + d.y * d.y;
            let t = if len2 > 0.0 { ((p.x - from.x) * d.x + (p.y - from.y) * d.y) / len2 } else { 0.0 };
            at(stops, t)
        }
        Paint::Radial { center, radius, stops } => {
            let d = p - *center;
            at(stops, if *radius > 0.0 { (d.x * d.x + d.y * d.y).sqrt() / radius } else { 0.0 })
        }
        Paint::Conic { center, start_deg, stops } => {
            let d = p - *center;
            let deg = d.y.atan2(d.x).to_degrees() - start_deg;
            at(stops, deg.rem_euclid(360.0) / 360.0)
        }
        Paint::Field { .. } => None,
    }
}

fn count_of(name: &str, v: &Value) -> R<u32> {
    match v.num() {
        Some(n) if n.fract() == 0.0 && n >= 1.0 && n <= f64::from(u32::MAX) => Ok(n as u32),
        _ => Err(fail("E0366", format!("`count` of `{name}` must be a positive whole number, got {}", v.show())).fix("write `count:12`")),
    }
}

/// A `slot` or `anchor` line in a stamp block, with its span.
fn forbidden_line(s: &Scene) -> Option<(&'static str, Span)> {
    use artscript_syntax::ast::SceneKind;
    match &s.kind {
        SceneKind::Slot { .. } => Some(("a `slot`", s.span)),
        SceneKind::Draw(d) if d.verb == "anchor" => Some(("an `anchor`", s.span)),
        SceneKind::If { then, otherwise, .. } => then.iter().chain(otherwise).find_map(forbidden_line),
        SceneKind::Each { body, .. } => body.iter().find_map(forbidden_line),
        _ => None,
    }
}

fn expr_idents(e: &Expr) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    crate::lints::walk_expr(e, &mut |x| match x {
        Expr::Ident(n) => {
            out.insert(n.clone());
        }
        Expr::Call { func, .. } => {
            out.insert(func.clone());
        }
        _ => {}
    });
    out
}
