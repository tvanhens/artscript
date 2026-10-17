//! `fx` lines to `Effect`s for one node: the built-in effects, their default colors from the node's
//! fill, finish declaration and expansion, and the rule that grain is seeded from the file the
//! `fx` line is written in.

use std::rc::Rc;

use artscript_ir::{Effect, EffectInput, EffectOp, Point};
use artscript_syntax::ast::{Expr, FxLine, Named, Span};

use crate::color::{self, DEFAULT_HUE_STEP, Rgba};
use crate::eval::{Evaluator, Fail, R, Scope, fail, ident_of};
use crate::identity::{self, Key};
use crate::scope::{FinishDef, Module};
use crate::value::Value;

const EFFECTS: &[&str] = &["blur", "shadow", "inner", "glow", "rim", "adjust", "outline", "grain", "merge"];
/// Parsed but not implemented in v0.1.
const PHASE2: &[&str] = &["bevel", "displace", "halftone", "posterize"];

fn is_effect_name(s: &str) -> bool {
    EFFECTS.contains(&s) || PHASE2.contains(&s)
}

fn effects_note() -> String {
    format!("effects: {}", EFFECTS.join(" "))
}

fn phase2(op: &str) -> Fail {
    fail("E0391", format!("`fx {op}` is a Phase 2 effect, not yet implemented in v0.1"))
        .fix("use blur, shadow, inner, glow, rim, adjust, outline or grain")
}

fn check_finish_line(module: &Module, finish: &str, line: &FxLine) -> R<()> {
    let op = line.op.as_str();
    if PHASE2.contains(&op) {
        return Err(phase2(op));
    }
    if !EFFECTS.contains(&op) {
        let f = fail("E0338", format!("`fx {op}` in a finish must be a built-in effect")).note(effects_note());
        return Err(if module.finishes.borrow().contains_key(op) || op == finish {
            f.note(format!("`{op}` is a finish, and finishes do not nest")).fix(format!("write `fx {op}` under the node beside `fx {finish}`"))
        } else {
            f.fix("pick one of the listed effects")
        });
    }
    if let Some(n) = line.named.iter().find(|n| n.key == "of" || n.key == "as") {
        return Err(fail("E0336", format!("`{}:` is not allowed in a finish", n.key))
            .fix("write the lines that name or pick effect inputs under the node"));
    }
    Ok(())
}

/// One `fx` line as written under a node, with the scope it was written in.
pub(crate) struct FxSite<'s> {
    pub line: &'s FxLine,
    pub scope: &'s Scope,
}

impl Evaluator<'_> {
    /// Every effect of the node at `path`, in source order. A finish line puts its lines in its
    /// place. `fill` is the node's first fill, the source of default colors. A failing line is
    /// reported at the span of the line written under the node and skipped.
    pub(crate) fn lower_fx(&mut self, sites: &[FxSite<'_>], key: &Key, fill: Rgba) -> Vec<Effect> {
        let path = key.to_string();
        let mut out = Vec::new();
        for site in sites {
            let file = site.scope.file().to_string();
            let op = site.line.op.as_str();
            let finish = if is_effect_name(op) { None } else { site.scope.module.finishes.borrow().get(op).cloned() };
            match finish {
                Some(def) => self.expand(&def, site, &file, fill, key, &mut out),
                None => match self.builtin(site.line, site.scope, &file, fill, key) {
                    Ok(e) => out.push(e),
                    Err(e) => self.report(e, &file, site.line.span, &path),
                },
            }
        }
        out
    }

    /// The finish's lines, in order, evaluated in its module with its params bound. The params'
    /// values evaluate where the `fx` line is written, and grain seeds from that file.
    fn expand(&mut self, def: &FinishDef, site: &FxSite<'_>, file: &str, fill: Rgba, key: &Key, out: &mut Vec<Effect>) {
        let path = key.to_string();
        let fsc = match self.bind_finish(def, site) {
            Ok(s) => s,
            Err(e) => return self.report(e, file, site.line.span, &path),
        };
        for line in &def.lines {
            match self.builtin(line, &fsc, file, fill, key) {
                Ok(e) => out.push(e),
                Err(e) => {
                    let e = e.note(format!("in finish `{}` ({}:{})", def.name, def.module, line.span.line));
                    self.report(e, file, site.line.span, &path);
                }
            }
        }
    }

    fn bind_finish(&mut self, def: &FinishDef, site: &FxSite<'_>) -> R<Scope> {
        let names: Vec<&str> = def.params.iter().map(|p| p.key.as_str()).collect();
        let unknown = |what: String| {
            let f = fail("E0321", what);
            if names.is_empty() {
                f.note(format!("`{}` takes no parameters", def.name)).fix(format!("write `fx {}`", def.name))
            } else {
                f.note(format!("parameters: {}", names.join(", "))).fix("pass one of the listed parameters as key:value")
            }
        };
        if !site.line.args.is_empty() {
            return Err(unknown(format!("finish `{}` takes named arguments only", def.name)));
        }
        let mut given = Vec::new();
        for n in &site.line.named {
            if !names.contains(&n.key.as_str()) {
                return Err(unknown(format!("finish `{}` has no parameter `{}`", def.name, n.key)));
            }
            given.push((n.key.clone(), self.expr(&n.value, site.scope)?));
        }
        let module = self.modules.get(&def.module).cloned().ok_or_else(|| fail("E0210", format!("module `{}` is not loaded", def.module)))?;
        let env = self.bind_params(&def.name, &def.params, &module, &given, def.span)?;
        let mut fsc = self.module_scope(&module);
        fsc.env = env;
        Ok(fsc)
    }

    /// `finish name params` + lines. Each line must be a built-in effect without `of:` or `as:`,
    /// so a finish is a plain list with no input graph and no nesting. A bad line is reported and
    /// left out.
    pub(crate) fn declare_finish(&mut self, module: &Rc<Module>, name: &str, params: &[Named], body: &[FxLine], span: Span) -> R<()> {
        if is_effect_name(name) {
            return Err(fail("E0337", format!("a finish cannot take the name of the effect `{name}`")).fix("rename the finish"));
        }
        let mut lines = Vec::new();
        for line in body {
            match check_finish_line(module, name, line) {
                Ok(()) => lines.push(line.clone()),
                Err(f) => self.report(f, &module.path, line.span, ""),
            }
        }
        let def = FinishDef { name: name.to_string(), params: params.to_vec(), lines, module: module.path.clone(), span };
        module.finishes.borrow_mut().insert(name.to_string(), Rc::new(def));
        module.own.borrow_mut().push(format!("finish:{name}"));
        Ok(())
    }

    /// One built-in effect. Arguments evaluate in `sc`. Grain seeds from `seed_file`, the file the
    /// `fx` line is written in.
    fn builtin(&mut self, line: &FxLine, sc: &Scope, seed_file: &str, f: Rgba, key: &Key) -> R<Effect> {
        let FxLine { op: op_name, args, named, .. } = line;
        let op_name = op_name.as_str();
        let input_of = |e: &Expr, ev: &mut Self| -> R<EffectInput> {
            Ok(match e {
                Expr::Ident(s) if s == "self" => EffectInput::Source,
                Expr::Ident(s) => EffectInput::Named(s.clone()),
                Expr::Ref(_) => match ev.expr(e, sc)? {
                    Value::Ref(r) => EffectInput::Node(r.record.path.clone()),
                    _ => EffectInput::Previous,
                },
                _ => return Err(fail("E0320", "an effect input is `self`, a name, or `@node`")),
            })
        };
        let plain_named: Vec<Named> = named.iter().filter(|n| n.key != "of" && n.key != "as").cloned().collect();
        let a = if op_name == "merge" {
            self.eval_args(&[], &plain_named, sc, 0)?
        } else {
            self.eval_args(args, &plain_named, sc, 0)?
        };
        let alpha = a.num("a").unwrap_or(1.0);
        let color = |default: Rgba| -> R<artscript_ir::Color> {
            let c = match a.get("color") {
                Some(v) => v.color().ok_or_else(|| fail("E0320", "`color:` expects a color"))?.rgba,
                None => default,
            };
            Ok(c.with_alpha(c.a * alpha).to_ir())
        };
        let n0 = a.pos.first().and_then(Value::num);
        let offset = a.pos.first().and_then(Value::vec2).filter(|_| !matches!(a.pos.first(), Some(Value::Num(_))));
        let op = match op_name {
            "blur" => EffectOp::Blur { sigma: n0.or(a.num("blur")).unwrap_or(4.0) as f32 },
            "shadow" | "inner" => {
                let (dx, dy) = offset.unwrap_or((0.0, 4.0));
                let blur = a.num("blur").unwrap_or(4.0) as f32;
                let c = color(color::step(f, -2.0, DEFAULT_HUE_STEP).with_alpha(0.35))?;
                if op_name == "shadow" {
                    EffectOp::Shadow { dx: dx as f32, dy: dy as f32, blur, color: c }
                } else {
                    EffectOp::Inner { dx: dx as f32, dy: dy as f32, blur, color: c }
                }
            }
            "glow" => EffectOp::Glow { radius: n0.unwrap_or(8.0) as f32, color: color(color::step(f, 2.0, DEFAULT_HUE_STEP))? },
            "rim" => {
                let dir = a.get("dir").and_then(Value::vec2).unwrap_or((1.0, 1.0));
                let rim = color::from_oklch(color::Lch { l: 0.86, c: 0.05, h: 265.0 }, 1.0);
                EffectOp::Rim { width: n0.unwrap_or(3.0) as f32, dir: Point::new(dir.0 as f32, dir.1 as f32), color: color(rim)? }
            }
            "adjust" => EffectOp::Adjust {
                hue: a.num("hue").unwrap_or(0.0) as f32,
                sat: a.num("sat").unwrap_or(0.0) as f32,
                light: a.num("light").unwrap_or(0.0) as f32,
            },
            "outline" => EffectOp::Outline { width: n0.unwrap_or(2.0) as f32, color: color(color::line(f))? },
            "grain" => {
                let extra = a.num("seed").unwrap_or(0.0) as u64;
                EffectOp::Grain {
                    strength: n0.unwrap_or(0.05) as f32,
                    scale: a.num("scale").unwrap_or(1.5) as f32,
                    seed: identity::seed(seed_file, &key.grain(), extra, &mut self.uses) as u32,
                }
            }
            "merge" => {
                let inputs = args.iter().map(|e| input_of(e, self)).collect::<R<Vec<_>>>()?;
                let alphas = match a.get("a") {
                    Some(v) => v.items().map(|xs| xs.iter().filter_map(Value::num).map(|x| x as f32).collect()).unwrap_or_else(|| vec![v.num().unwrap_or(1.0) as f32]),
                    None => vec![1.0; inputs.len()],
                };
                EffectOp::Merge { inputs, alphas }
            }
            other if PHASE2.contains(&other) => return Err(phase2(other)),
            other => {
                let mut f = fail("E0391", format!("unknown effect `{other}`")).note(effects_note());
                let mut finishes: Vec<String> = sc.module.finishes.borrow().keys().cloned().collect();
                if !finishes.is_empty() {
                    finishes.sort();
                    f = f.note(format!("finishes: {}", finishes.join(", ")));
                }
                if sc.module.styles.borrow().contains_key(other) {
                    f = f.note(format!("`{other}` is a style")).fix(format!("write `style:{other}` on the node"));
                } else {
                    f = f.fix("pick one of the listed effects, or declare `finish name` and write `fx name`");
                }
                return Err(f);
            }
        };
        let input = match named.iter().find(|n| n.key == "of") {
            Some(n) => input_of(&n.value, self)?,
            None => EffectInput::Previous,
        };
        let name = named.iter().find(|n| n.key == "as").and_then(|n| ident_of(&n.value)).map(str::to_string);
        Ok(Effect { op, input, name })
    }
}
