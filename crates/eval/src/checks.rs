//! Checks: `assert` / `expect` / `rule`, the geometric check functions, measured-value notes and
//! the suggested `fix: set .node at:x,y` nudges. Checks measure the scene and never change it.

use std::cell::RefCell;
use std::rc::Rc;

use ramose_ir::Rect;
use skia_safe::Vector;
use ramose_syntax::ast::{CheckKind, Expr, File, Item, Named, Scene, SceneKind, Span};

use crate::eval::{Evaluator, Fail, R, Scope, fail, measure_rect, unseen};
use crate::scope::{RefVal, RuleDef};
use crate::value::{Value, num};

pub(crate) struct Pending {
    pub stmt: Scene,
    pub scope: Scope,
}

pub(crate) struct CheckState {
    pub tol: f64,
    pub notes: Vec<String>,
    pub fix: Option<String>,
    /// A rule already reported its own failures.
    pub reported: bool,
    /// Canvas x of the enclosing frame's center (`symmetric` default axis).
    pub about_x: f64,
    pub file: String,
    pub span: Span,
}

fn source_of(stmt: &Scene) -> String {
    ramose_syntax::format(&File { directives: vec![], items: vec![Item::Scene(stmt.clone())] }).trim().to_string()
}

fn refs(v: &[Value]) -> Vec<Rc<RefVal>> {
    v.iter()
        .flat_map(|x| match x {
            Value::Ref(r) => vec![r.clone()],
            Value::List(xs) => xs.iter().filter_map(|y| if let Value::Ref(r) = y { Some(r.clone()) } else { None }).collect(),
            _ => vec![],
        })
        .collect()
}

fn bounds(r: &RefVal) -> R<Rect> {
    r.bounds().ok_or_else(|| fail("E0351", format!("@{} has no geometry to measure", r.label)))
}

/// `set .node at:x,y` for moving `r` by a canvas-space delta.
fn nudge(r: &RefVal, dx: f64, dy: f64, owner: &crate::identity::Key) -> Option<String> {
    if r.canvas.is_some() || r.record.anchor.is_some() {
        return None;
    }
    if !r.record.key.is_stable() {
        return Some("add name:".into());
    }
    let inv = r.record.parent_world.flat().invert()?;
    let d = inv.map_vector(Vector::new(dx as f32, dy as f32));
    let (x, y) = (r.record.at.0 + d.x as f64, r.record.at.1 + d.y as f64);
    let addr = r
        .record
        .key
        .address_from(owner)
        .unwrap_or_else(|| format!(".{}", r.record.key));
    Some(format!("set {addr} at:{},{}", num(x), num(y)))
}

impl Evaluator<'_> {
    pub(crate) fn run_checks(&mut self, list: &Rc<RefCell<Vec<Pending>>>) {
        let pending: Vec<Pending> = list.borrow_mut().drain(..).collect();
        for p in pending {
            let before = self.diags.len();
            if let Err(f) = self.run_check(&p.stmt, &p.scope) {
                let file = p.scope.file().to_string();
                self.report(f, &file, p.stmt.span, &p.scope.node_name());
            }
            if let Some(line) = p.scope.site {
                self.mark_site(before, line);
            }
        }
    }

    /// Evaluates one check and reports its failure. Returns whether it passed. A prepass read of
    /// the drawn scene (E0344) is returned instead, so a rule call cannot turn it into `false`.
    fn run_check(&mut self, stmt: &Scene, sc: &Scope) -> R<bool> {
        let SceneKind::Check { kind, expr, named, message } = &stmt.kind else { return Ok(true) };
        let file = sc.file().to_string();
        let tol = match named.iter().find(|n| n.key == "tol").map(|n| self.expr(&n.value, sc)) {
            Some(Err(f)) if f.code == "E0344" => return Err(f),
            Some(Ok(v)) => v.num().unwrap_or(0.5),
            _ => 0.5,
        };
        let center = sc.world.flat().map_point((sc.frame.center_x(), sc.frame.center_y()));
        let saved = self.check.replace(CheckState {
            tol,
            notes: Vec::new(),
            fix: None,
            reported: false,
            about_x: center.x as f64,
            file: file.clone(),
            span: stmt.span,
        });
        let result = self.expr(expr, sc);
        let state = std::mem::replace(&mut self.check, saved).expect("check state");
        match result {
            Err(f) if f.code == "E0344" => Err(f),
            Err(f) => {
                self.report(f, &file, stmt.span, &sc.node_name());
                Ok(false)
            }
            Ok(v) if v.truthy() => Ok(true),
            Ok(_) if state.reported => Ok(false),
            Ok(_) => {
                let code = match kind {
                    CheckKind::Assert => "C0101",
                    CheckKind::Expect => "C0201",
                };
                let mut f = Fail { code, msg: source_of(stmt), notes: Vec::new(), fix: state.fix };
                if let Some(m) = message {
                    f.notes.push(m.clone());
                }
                f.notes.extend(state.notes);
                self.report(f, &file, stmt.span, &sc.node_name());
                Ok(false)
            }
        }
    }

    pub(crate) fn call_rule(&mut self, rule: &Rc<RuleDef>, args: &[Expr], named: &[Named], sc: &Scope) -> R<Value> {
        let module = self.modules.get(&rule.module).cloned().ok_or_else(|| fail("E0210", "rule module not loaded"))?;
        let mut given: Vec<(String, Value)> = Vec::new();
        for (i, e) in args.iter().enumerate() {
            let p = rule.params.get(i).ok_or_else(|| fail("E0321", format!("rule `{}` takes {} argument(s)", rule.name, rule.params.len())))?;
            given.push((p.key.clone(), self.expr(e, sc)?));
        }
        for n in named {
            given.push((n.key.clone(), self.expr(&n.value, sc)?));
        }
        let mut rsc = sc.clone();
        rsc.module = module;
        rsc.env = Vec::new();
        rsc.typed_params = Rc::default();
        for p in &rule.params {
            let v = match given.iter().rev().find(|(k, _)| k == &p.key) {
                Some((_, v)) => v.clone(),
                None => self.expr(&p.value, &rsc)?,
            };
            rsc.env.push((p.key.clone(), v));
        }
        let caller = self.check.as_ref().map(|c| format!("{}:{}", c.file, c.span.line)).unwrap_or_default();
        let before = self.diags.len();
        let mut ok = true;
        for st in &rule.body {
            if matches!(st.kind, SceneKind::Check { .. }) {
                ok &= self.run_check(st, &rsc)?;
            }
        }
        for d in &mut self.diags[before..] {
            d.notes.push(format!("via rule `{}` ({}:{}) applied at {caller}", rule.name, rule.module, rule.span.line));
        }
        if !ok && let Some(c) = &mut self.check {
            c.reported = true;
        }
        Ok(Value::Bool(ok))
    }

    fn tol(&self) -> f64 {
        self.check.as_ref().map(|c| c.tol).unwrap_or(0.5)
    }

    fn set_fix(&mut self, fix: Option<String>) {
        if let (Some(c), Some(f)) = (&mut self.check, fix) {
            c.fix = Some(f);
        }
    }

    /// Check functions over references. `None` when `func` is not one.
    pub(crate) fn check_fn(&mut self, func: &str, a: &[Value], nm: &[(String, Value)], sc: &Scope) -> Option<R<Value>> {
        let known = [
            "level", "aligned", "symmetric", "inside", "overlaps", "touches", "gap", "dist", "centered", "aspect", "on_grid", "same_size",
            "stroke_widths",
        ];
        if !known.contains(&func) {
            return None;
        }
        if self.prepass {
            return Some(Err(unseen(&format!("{func}(...)"))));
        }
        Some(self.check_fn_inner(func, a, nm, sc.owner()))
    }

    fn check_fn_inner(&mut self, func: &str, a: &[Value], nm: &[(String, Value)], owner: &crate::identity::Key) -> R<Value> {
        let get = |k: &str| nm.iter().rev().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        let rs = refs(a);
        let need = |n: usize| -> R<()> {
            if rs.len() < n {
                Err(fail("E0320", format!("`{func}` needs {n} node reference(s)")).fix(format!("write `{func}(@a{})`", if n > 1 { " @b" } else { "" })))
            } else {
                Ok(())
            }
        };
        let tol = self.tol();
        Ok(match func {
            "level" => {
                need(2)?;
                let (x, y) = (bounds(&rs[0])?, bounds(&rs[1])?);
                let (ya, yb) = (x.center_y() as f64, y.center_y() as f64);
                let d = yb - ya;
                let ok = d.abs() <= tol;
                if !ok {
                    self.note(format!(
                        "{}.cy = {}   {}.cy = {}   difference {} > tol {}",
                        rs[0].label,
                        num(ya),
                        rs[1].label,
                        num(yb),
                        num(d.abs()),
                        num(tol)
                    ));
                    self.set_fix(nudge(&rs[1], 0.0, -d, owner));
                }
                Value::Bool(ok)
            }
            "aligned" => {
                need(2)?;
                let edge = get("edge").and_then(|v| v.word().map(str::to_string)).unwrap_or("top".into());
                let (x, y) = (bounds(&rs[0])?, bounds(&rs[1])?);
                let (ma, mb) = match (measure_rect(x, &edge), measure_rect(y, &edge)) {
                    (Some(p), Some(q)) => (p, q),
                    _ => return Err(fail("E0320", format!("unknown edge `{edge}`")).fix("use top, bottom, left, right, cx or cy")),
                };
                let d = mb - ma;
                let ok = d.abs() <= tol;
                if !ok {
                    self.note(format!("{}.{edge} = {}   {}.{edge} = {}   difference {} > tol {}", rs[0].label, num(ma), rs[1].label, num(mb), num(d.abs()), num(tol)));
                    let horizontal = matches!(edge.as_str(), "left" | "right" | "cx");
                    self.set_fix(if horizontal { nudge(&rs[1], -d, 0.0, owner) } else { nudge(&rs[1], 0.0, -d, owner) });
                }
                Value::Bool(ok)
            }
            "symmetric" => {
                need(2)?;
                let about = get("about").and_then(|v| v.num()).unwrap_or(self.check.as_ref().map(|c| c.about_x).unwrap_or(0.0));
                let (x, y) = (bounds(&rs[0])?, bounds(&rs[1])?);
                let ex = x.center_x() as f64 + y.center_x() as f64 - 2.0 * about;
                let ey = y.center_y() as f64 - x.center_y() as f64;
                let ok = ex.abs() <= tol && ey.abs() <= tol;
                if !ok {
                    self.note(format!(
                        "{}.cx = {}   {}.cx = {}   about {}   off by {}",
                        rs[0].label,
                        num(x.center_x() as f64),
                        rs[1].label,
                        num(y.center_x() as f64),
                        num(about),
                        num(ex)
                    ));
                    if ey.abs() > tol {
                        self.note(format!("{}.cy - {}.cy = {}", rs[1].label, rs[0].label, num(ey)));
                    }
                    self.set_fix(nudge(&rs[1], -ex, -ey, owner));
                }
                Value::Bool(ok)
            }
            "inside" => {
                need(2)?;
                let margin = get("margin").and_then(|v| v.num()).unwrap_or(0.0);
                let (x, y) = (bounds(&rs[0])?, bounds(&rs[1])?);
                let over = [
                    ("left", y.left as f64 + margin - x.left as f64),
                    ("right", x.right as f64 - (y.right as f64 - margin)),
                    ("top", y.top as f64 + margin - x.top as f64),
                    ("bottom", x.bottom as f64 - (y.bottom as f64 - margin)),
                ];
                let bad: Vec<&(&str, f64)> = over.iter().filter(|(_, v)| *v > 1e-3).collect();
                for (side, v) in &bad {
                    self.note(format!("{}.{side} overflows {} (margin {}) by {}", rs[0].label, rs[1].label, num(margin), num(*v)));
                }
                let dx = match (over[0].1 > 1e-3, over[1].1 > 1e-3) {
                    (true, false) => over[0].1,
                    (false, true) => -over[1].1,
                    _ => 0.0,
                };
                let dy = match (over[2].1 > 1e-3, over[3].1 > 1e-3) {
                    (true, false) => over[2].1,
                    (false, true) => -over[3].1,
                    _ => 0.0,
                };
                if !bad.is_empty() && (dx != 0.0 || dy != 0.0) {
                    self.set_fix(nudge(&rs[0], dx, dy, owner));
                }
                Value::Bool(bad.is_empty())
            }
            "overlaps" => {
                need(2)?;
                Value::Bool(bounds(&rs[0])?.intersects(bounds(&rs[1])?))
            }
            "touches" | "gap" => {
                need(2)?;
                let (x, y) = (bounds(&rs[0])?, bounds(&rs[1])?);
                let vertical_overlap = x.top < y.bottom && y.top < x.bottom;
                let g = if x.intersects(y) {
                    0.0
                } else if vertical_overlap {
                    (y.left - x.right).max(x.left - y.right).max(0.0) as f64
                } else {
                    (y.top - x.bottom).max(x.top - y.bottom).max(0.0) as f64
                };
                self.note(format!("gap({}, {}) = {}", rs[0].label, rs[1].label, num(g)));
                if func == "gap" { Value::Num(g) } else { Value::Bool(g <= tol) }
            }
            "dist" => {
                need(2)?;
                let (x, y) = (bounds(&rs[0])?, bounds(&rs[1])?);
                let d = ((x.center_x() - y.center_x()) as f64).hypot((x.center_y() - y.center_y()) as f64);
                self.note(format!("dist({}, {}) = {}", rs[0].label, rs[1].label, num(d)));
                Value::Num(d)
            }
            "centered" => {
                need(1)?;
                let container = match get("in") {
                    Some(Value::Ref(r)) => r,
                    _ => rs.get(1).cloned().ok_or_else(|| fail("E0320", "`centered` needs `in:@container`"))?,
                };
                let (x, y) = (bounds(&rs[0])?, bounds(&container)?);
                let (dx, dy) = ((y.center_x() - x.center_x()) as f64, (y.center_y() - x.center_y()) as f64);
                let ok = dx.abs() <= tol && dy.abs() <= tol;
                if !ok {
                    self.note(format!("{} center is off {}'s center by {},{}", rs[0].label, container.label, num(-dx), num(-dy)));
                    self.set_fix(nudge(&rs[0], dx, dy, owner));
                }
                Value::Bool(ok)
            }
            "aspect" => {
                need(1)?;
                let b = bounds(&rs[0])?;
                let v = b.width() as f64 / (b.height() as f64).max(1e-9);
                self.note(format!("aspect({}) = {}", rs[0].label, num(v)));
                Value::Num(v)
            }
            "on_grid" => {
                need(1)?;
                let n = a.iter().find_map(Value::num).unwrap_or(1.0).max(1e-9);
                let b = bounds(&rs[0])?;
                let off = |v: f32| {
                    let r = (v as f64).rem_euclid(n);
                    r.min(n - r)
                };
                let ok = off(b.left) <= tol && off(b.top) <= tol;
                if !ok {
                    self.note(format!("{}.left = {}   .top = {}   not on a {} grid", rs[0].label, num(b.left as f64), num(b.top as f64), num(n)));
                }
                Value::Bool(ok)
            }
            "same_size" => {
                let sizes: Vec<(String, f64, f64)> =
                    rs.iter().map(|r| bounds(r).map(|b| (r.label.clone(), b.width() as f64, b.height() as f64))).collect::<R<_>>()?;
                let Some((_, w0, h0)) = sizes.first().cloned() else { return Ok(Value::Bool(true)) };
                let mut ok = true;
                for (l, w, h) in &sizes {
                    if (w - w0).abs() > tol || (h - h0).abs() > tol {
                        ok = false;
                        self.note(format!("{l} is {}x{}, expected {}x{}", num(*w), num(*h), num(w0), num(h0)));
                    }
                }
                Value::Bool(ok)
            }
            "stroke_widths" => {
                need(1)?;
                let ws = rs[0].stroke_widths();
                self.note(format!("stroke_widths({}) = [{}]", rs[0].label, ws.iter().map(|w| num(*w)).collect::<Vec<_>>().join(", ")));
                Value::List(Rc::new(ws.into_iter().map(Value::Num).collect()))
            }
            _ => unreachable!(),
        })
    }
}
