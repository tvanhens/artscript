//! `fx` lines to `Effect`s for one node: the built-in effects, their default colors from the node's
//! fill, finish declaration and expansion, and the rule that grain is seeded from the file the
//! `fx` line is written in.

use std::rc::Rc;

use ramose_ir::{Effect, EffectInput, EffectOp, Matrix, Point, Rect, Sides};
use ramose_syntax::ast::{Expr, FxLine, MaskLine, Named, Span};

use crate::brush::Slot;
use crate::color::{self, DEFAULT_HUE_STEP, Rgba};
use crate::eval::{Evaluator, Fail, R, Scope, fail, ident_of, unknown_key};
use crate::identity::{self, Key};
use crate::scope::{FinishDef, Module};
use crate::value::{Value, num};

/// Each built-in effect and the keys it reads, beside `of:` and `as:`.
const EFFECTS: &[(&str, &[&str])] = &[
    ("blur", &["blur"]),
    ("shadow", &["blur", "color", "a"]),
    ("inner", &["blur", "color", "a"]),
    ("glow", &["strength", "color", "a"]),
    ("rim", &["dir", "color", "a"]),
    ("adjust", &["hue", "sat", "light"]),
    ("outline", &["sides", "color", "a", "feather", "brush"]),
    ("grain", &["scale", "seed"]),
    ("merge", &["a"]),
];
/// Parsed but not implemented in v0.1.
const PHASE2: &[&str] = &["bevel", "displace", "halftone", "posterize"];

fn effect_keys(op: &str) -> Option<&'static [&'static str]> {
    EFFECTS.iter().find(|(name, _)| *name == op).map(|(_, keys)| *keys)
}

fn is_effect_name(s: &str) -> bool {
    effect_keys(s).is_some() || PHASE2.contains(&s)
}

/// One E0321 for each key on `line` that its effect does not read. `extra` are the keys the
/// line's place allows beside the effect's own.
fn unknown_keys(line: &FxLine, extra: &[&'static str]) -> Vec<Fail> {
    let Some(own) = effect_keys(&line.op) else {
        return Vec::new();
    };
    let keys: Vec<&str> = own.iter().chain(extra).copied().collect();
    let verb = format!("fx {}", line.op);
    line.named.iter().filter(|n| !keys.contains(&n.key.as_str())).map(|n| unknown_key(&verb, &keys, &n.key)).collect()
}

const SIDE_WORDS: [&str; 4] = ["top", "right", "bottom", "left"];

/// `sides:top,left` is read as words, not evaluated, so `top` never looks up a name.
fn sides(named: &[Named]) -> R<Sides> {
    let Some(n) = named.iter().find(|n| n.key == "sides") else {
        return Ok(Sides::ALL);
    };
    let bad = |what: String| {
        fail("E0320", what)
            .note(format!("sides: {}", SIDE_WORDS.join(" ")))
            .fix("write `sides:top,left`, a comma list of side words")
    };
    let words: Vec<&Expr> = match &n.value {
        Expr::Tuple(items) => items.iter().collect(),
        one => vec![one],
    };
    let mut out = Sides { top: false, right: false, bottom: false, left: false };
    for word in words {
        match ident_of(word) {
            Some("top") => out.top = true,
            Some("right") => out.right = true,
            Some("bottom") => out.bottom = true,
            Some("left") => out.left = true,
            Some(other) => return Err(bad(format!("`{other}` is not a side in `sides:`"))),
            None => return Err(bad("`sides:` expects side words".into())),
        }
    }
    Ok(out)
}

fn effects_note() -> String {
    let names: Vec<&str> = EFFECTS.iter().map(|(name, _)| *name).collect();
    format!("effects: {}", names.join(" "))
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
    if effect_keys(op).is_none() {
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

/// One `fx` line as written under a node, with the scope it was written in. `ordinal` is the
/// line's number among every `fx` line its owner has, from `fx_ordinals`. `frame` and `world`
/// are the node's, where a mask under the line evaluates, as the node's own mask does.
pub(crate) struct FxSite<'s> {
    pub line: &'s FxLine,
    pub scope: &'s Scope,
    pub ordinal: u32,
    pub frame: Rect,
    pub world: Matrix,
}

/// Where one `fx` line's seeds come from: the node's key, the line's own key under it, and the
/// node's `seed:`.
struct FxKey<'k> {
    key: &'k Key,
    line: Key,
    seed: Option<u64>,
}

impl Evaluator<'_> {
    /// Every effect of the node at `path`, in source order. A finish line puts its lines in its
    /// place. `fill` is the node's first fill, the source of default colors. `seed` is the node's
    /// `seed:`, which pins an outline's brush stages as it pins the node's own. A failing line is
    /// reported at the span of the line written under the node and skipped.
    pub(crate) fn lower_fx(&mut self, sites: &[FxSite<'_>], key: &Key, fill: Rgba, seed: Option<u64>) -> Vec<Effect> {
        let path = key.to_string();
        let mut out = Vec::new();
        for site in sites {
            let file = site.scope.file().to_string();
            let op = site.line.op.as_str();
            let finish = if is_effect_name(op) { None } else { site.scope.module.finishes.borrow().get(op).cloned() };
            let at = FxKey { key, line: key.fx(site.ordinal), seed };
            match finish {
                Some(def) => self.expand(&def, site, &file, fill, &at, &mut out),
                None => {
                    for f in unknown_keys(site.line, &["of", "as"]) {
                        self.report(f, &file, site.line.span, &path);
                    }
                    let mut effect = match self.builtin(site.line, site.scope, &file, fill, &at) {
                        Ok(e) => e,
                        Err(e) => {
                            self.report(e, &file, site.line.span, &path);
                            continue;
                        }
                    };
                    if let (Some(line), EffectOp::Outline { mask, .. }) = (&site.line.mask, &mut effect.op) {
                        match self.outline_mask(line, site, &file, &key.fx_mask(site.ordinal)) {
                            Ok(m) => *mask = m.map(Box::new),
                            Err(e) => self.report(e, &file, line.span, &path),
                        }
                    }
                    out.push(effect);
                }
            }
        }
        out
    }

    /// The mask under an `fx outline` line, in the node's frame like its node mask, with the
    /// line's `feather:` on it. Its fields seed from the mask's own key, so the ring breaks up
    /// the same way on every build and differently from any other mask.
    fn outline_mask(&mut self, line: &MaskLine, site: &FxSite<'_>, file: &str, mask_key: &Key) -> R<Option<ramose_ir::Mask>> {
        let feather = self.eval_args(&[], &site.line.named, site.scope, 0)?.num("feather").unwrap_or(0.0) as f32;
        self.begin_seed(file, mask_key.clone());
        let origin = format!("{file}:{}:{}", line.span.line, line.span.col);
        self.lower_mask(&line.mask, feather, site.scope, site.frame, site.world, mask_key, &origin)
    }

    /// The finish's lines, in order, evaluated in its module with its params bound. The params'
    /// values evaluate where the `fx` line is written, and grain seeds from that file. Each line
    /// seeds its brushes from its own number under the `fx` line's key.
    fn expand(&mut self, def: &FinishDef, site: &FxSite<'_>, file: &str, fill: Rgba, at: &FxKey<'_>, out: &mut Vec<Effect>) {
        let path = at.key.to_string();
        let fsc = match self.bind_finish(def, site) {
            Ok(s) => s,
            Err(e) => return self.report(e, file, site.line.span, &path),
        };
        for (i, line) in def.lines.iter().enumerate() {
            let at = FxKey { key: at.key, line: at.line.fx(i as u32 + 1), seed: at.seed };
            match self.builtin(line, &fsc, file, fill, &at) {
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
        let env = self.bind_params(&def.name, &def.params, &module, &given, def.span, site.scope)?;
        let mut fsc = self.module_scope(&module);
        fsc.env = env;
        fsc.palette = site.scope.palette.clone();
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
                Ok(()) => {
                    for f in unknown_keys(line, &[]) {
                        self.report(f, &module.path, line.span, "");
                    }
                    lines.push(line.clone());
                }
                Err(f) => self.report(f, &module.path, line.span, ""),
            }
        }
        let def = FinishDef { name: name.to_string(), params: params.to_vec(), lines, module: module.path.clone(), span };
        module.finishes.borrow_mut().insert(name.to_string(), Rc::new(def));
        module.own.borrow_mut().push(format!("finish:{name}"));
        Ok(())
    }

    /// One built-in effect. Arguments evaluate in `sc`. Grain and an outline's brushes seed from
    /// `seed_file`, the file the `fx` line is written in.
    fn builtin(&mut self, line: &FxLine, sc: &Scope, seed_file: &str, f: Rgba, at: &FxKey<'_>) -> R<Effect> {
        let FxLine { op: op_name, args, named, .. } = line;
        let key = at.key;
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
        let plain_named: Vec<Named> = named
            .iter()
            .filter(|n| n.key != "of" && n.key != "as" && !(op_name == "outline" && n.key == "sides"))
            .cloned()
            .collect();
        let a = if op_name == "merge" {
            self.eval_args(&[], &plain_named, sc, 0)?
        } else {
            self.eval_args(args, &plain_named, sc, 0)?
        };
        let alpha = a.num("a").unwrap_or(1.0);
        let color = |default: Rgba| -> R<ramose_ir::Color> {
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
            "glow" => {
                let strength = a.num("strength").unwrap_or(1.0);
                if strength < 0.0 {
                    return Err(fail("E0320", format!("`strength:` must be 0 or more, got {}", num(strength))).fix("write a strength of 0 or more"));
                }
                EffectOp::Glow { radius: n0.unwrap_or(8.0) as f32, color: color(color::step(f, 2.0, DEFAULT_HUE_STEP))?, strength: strength as f32 }
            }
            "rim" => {
                let dir = a.get("dir").map(|v| v.vec2_for("dir")).transpose()?.unwrap_or((1.0, 1.0));
                let rim = color::from_oklch(color::Lch { l: 0.86, c: 0.05, h: 265.0 }, 1.0);
                EffectOp::Rim { width: n0.unwrap_or(3.0) as f32, dir: Point::new(dir.0 as f32, dir.1 as f32), color: color(rim)? }
            }
            "adjust" => EffectOp::Adjust {
                hue: a.num("hue").unwrap_or(0.0) as f32,
                sat: a.num("sat").unwrap_or(0.0) as f32,
                light: a.num("light").unwrap_or(0.0) as f32,
            },
            "outline" => {
                if line.mask.is_none() && a.get("feather").is_some() {
                    return Err(fail("E0320", "`feather:` on `fx outline` softens the edge of its mask, and this line has none")
                        .fix("indent a `mask` line under the `fx outline` line, or remove `feather:`"));
                }
                let brush = match a.get("brush") {
                    None => Vec::new(),
                    Some(bv) => {
                        let stack = self.brush_stack(bv, sc)?;
                        if let Some((i, b)) = stack.iter().enumerate().find(|(_, b)| b.place.on == Some(Slot::Fill)) {
                            let what = if stack.len() == 1 {
                                "`brush:` on `fx outline` is narrowed to `on:fill`, and a ring is a stroke".to_string()
                            } else {
                                format!("stage {} of `brush:` (`{}`) is narrowed to `on:fill`, and a ring is a stroke", i + 1, b.decl.name)
                            };
                            let f = fail("E0362", what).fix("remove `on:fill` from the brush, or the brush from the outline");
                            let span = named.iter().find(|n| n.key == "brush").map_or(line.span, |n| n.span);
                            self.report(f, sc.file(), span, &key.to_string());
                            return Err(Fail::reported());
                        }
                        if let Some((i, b)) = stack.iter().enumerate().find(|(_, b)| b.decl.reads_below()) {
                            let what = if stack.len() == 1 {
                                "`brush:` on `fx outline` reads `m.below`, and a ring is drawn offscreen with no backdrop".to_string()
                            } else {
                                format!("stage {} of `brush:` (`{}`) reads `m.below`, and a ring is drawn offscreen with no backdrop", i + 1, b.decl.name)
                            };
                            let f = fail("E0362", what).fix("use a brush that does not read `m.below` on the outline");
                            let span = named.iter().find(|n| n.key == "brush").map_or(line.span, |n| n.span);
                            self.report(f, sc.file(), span, &key.to_string());
                            return Err(Fail::reported());
                        }
                        self.ribbon_marks(&stack, &at.line, at.seed, seed_file)
                    }
                };
                EffectOp::Outline { width: n0.unwrap_or(2.0) as f32, color: color(color::line(f))?, sides: sides(named)?, mask: None, brush }
            }
            "grain" => {
                let explicit = a.seed()?;
                EffectOp::Grain {
                    strength: n0.unwrap_or(0.05) as f32,
                    scale: a.num("scale").unwrap_or(1.5) as f32,
                    seed: identity::seed(seed_file, &key.grain(), explicit, &mut self.uses) as u32,
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
            other if sc.module.lost(other) => return Err(Fail::reported()),
            other => {
                let mut f = fail("E0391", format!("unknown effect `{other}`")).note(effects_note());
                let finishes: Vec<String> = sc.module.finishes.borrow().keys().cloned().collect();
                if !finishes.is_empty() {
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
