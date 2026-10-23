//! AST to IR in one walk: scopes, type inference, helper instantiation, constants, loops, seeds,
//! reads, and diagnostics. The only file that knows `Expr`.

use std::collections::BTreeSet;
use std::rc::Rc;

use ramose_syntax::ast::{BinOp, BodyLineKind, DeclKind, Expr, Named, Span, UnOp};

use crate::ir::{self, Arith, Block, Cover, Local, Logic, LoopVar, NoiseSeed, Op, Out, Stmt, Swizzle};
use crate::lang::{self, Builtin, Field, MAX_TRIPS, Noise, Reduce, Ty};
use crate::{BrushSource, Checker, Diag, Global, Library, PLACEMENT_KEYS, Param, Reads, Seeds, Shader, Site, TopFn};

/// A fn as the memo keys it: its file, the brush whose body declares it (none for a top-level
/// fn), and its name.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct FnKey {
    file: String,
    scope: Option<String>,
    name: String,
}

/// A value under construction. `konst` is Some only for a literal or arithmetic on literals: a
/// `let`, a parameter, a loop variable, and a field of `m` drop it.
#[derive(Clone, Copy)]
struct Val {
    id: Local,
    ty: Ty,
    konst: Option<f64>,
}

/// The fault was recorded already; callers stop without a second message.
#[derive(Clone, Copy)]
struct Poisoned;

type Lowered = Result<Val, Poisoned>;

/// What a bare name may resolve to besides the bindings in scope. A body line and a helper see
/// the brush parameters and `m`; a top-level fn sees neither, and looks names up in `module`.
#[derive(Clone)]
enum Kind {
    Body,
    Helper,
    TopFn { module: String },
}

#[derive(Clone)]
struct Env {
    kind: Kind,
    /// Bindings in scope, innermost last: the lets so far, a fn's parameters, loop variables.
    names: Vec<(String, Lowered)>,
}

/// A `fn` line of the body.
#[derive(Clone, Copy)]
struct Helper<'a> {
    name: &'a str,
    params: &'a [String],
    body: &'a Expr,
    span: Span,
}

enum Callee<'a> {
    Helper(Helper<'a>),
    Top(Rc<TopFn>),
}

/// An active fn instantiation.
struct Frame {
    key: FnKey,
    name: String,
    /// The fn line, where a fault that does not depend on the argument types lands.
    site: Site,
    /// The call, where a type fault lands with a note at the fn.
    call: Site,
}

pub(crate) struct Lower<'c> {
    ck: &'c mut Checker,
    lib: &'c dyn Library,
    src: &'c BrushSource<'c>,
    helpers: Vec<Helper<'c>>,
    /// The `let` names, so a helper that reads one gets a fix.
    lets: Vec<String>,
    /// Innermost block last; `emit` pushes into it.
    blocks: Vec<Vec<Stmt>>,
    next: u32,
    loops: u32,
    /// The product of the enclosing reductions' trip counts.
    trips: u32,
    salts: BTreeSet<u32>,
    reads: Reads,
    diags: Vec<Diag>,
    stack: Vec<Frame>,
    /// Where a fault is reported: the current line, a named argument, or the fn being lowered.
    site: Site,
}

impl<'c> Lower<'c> {
    pub(crate) fn new(ck: &'c mut Checker, lib: &'c dyn Library, src: &'c BrushSource<'c>) -> Lower<'c> {
        Lower {
            ck,
            lib,
            src,
            helpers: Vec::new(),
            lets: Vec::new(),
            blocks: vec![Vec::new()],
            next: 0,
            loops: 0,
            trips: 1,
            salts: BTreeSet::new(),
            reads: Reads::default(),
            diags: Vec::new(),
            stack: Vec::new(),
            site: Site { file: src.file.to_string(), span: src.span },
        }
    }

    fn at(&self, span: Span) -> Site {
        Site { file: self.src.file.to_string(), span }
    }

    pub(crate) fn run(mut self) -> Result<Shader, Vec<Diag>> {
        for p in self.src.params {
            if let Some(what) = lang::taken(&p.name) {
                let at = self.at(p.span);
                self.report_at("E0363", at, format!("a brush parameter cannot be named `{}`: it is {what}", p.name), Some("rename the parameter".into()));
            }
        }
        for line in self.src.lines {
            let BodyLineKind::Fn { name, params, body } = &line.kind else { continue };
            self.site = self.at(line.span);
            if let Some(what) = lang::taken(name) {
                self.report(format!("a helper cannot be named `{name}`: it is {what}"), Some("rename the helper".into()));
                continue;
            }
            if self.helpers.iter().any(|h| h.name == name) {
                self.report(format!("helper `{name}` is declared twice"), Some("give each helper its own name".into()));
                continue;
            }
            if self.src.params.iter().any(|p| p.name == *name) {
                self.report(format!("`{name}` is already a parameter"), Some("rename the helper".into()));
                continue;
            }
            for p in params {
                if let Some(what) = lang::taken(p) {
                    self.report(format!("a helper parameter cannot be named `{p}`: it is {what}"), Some("rename the parameter".into()));
                }
            }
            self.helpers.push(Helper { name, params, body, span: line.span });
        }
        let mut lets: Vec<(String, Lowered)> = Vec::new();
        let mut out: Option<(Lowered, Result<Cover, Poisoned>)> = None;
        let mut saw_sksl = false;
        for line in self.src.lines {
            self.site = self.at(line.span);
            if out.is_some() {
                self.report("`out` is the body's last line".into(), Some("move this line above `out`".into()));
                continue;
            }
            match &line.kind {
                BodyLineKind::Fn { .. } => {}
                BodyLineKind::Sksl { .. } => {
                    saw_sksl = true;
                    self.report(
                        "a brush body is `let`, `fn`, and `out` lines, not SkSL".into(),
                        Some("write the body as ramose lines ending in `out RGB cover:NUMBER`; see `ramose docs brushes`".into()),
                    );
                }
                BodyLineKind::Let { name, value } => {
                    let clash = if let Some(what) = lang::taken(name) {
                        Some(format!("a `let` cannot be named `{name}`: it is {what}"))
                    } else if lets.iter().any(|(n, _)| n == name) {
                        Some(format!("`{name}` is already bound by an earlier `let`"))
                    } else if self.src.params.iter().any(|p| p.name == *name) {
                        Some(format!("`{name}` is already a parameter"))
                    } else if self.helpers.iter().any(|h| h.name == name) {
                        Some(format!("`{name}` is already a helper"))
                    } else {
                        None
                    };
                    let v = match clash {
                        Some(msg) => Err(self.report(msg, Some("rename the `let`".into()))),
                        None => {
                            let env = Env { kind: Kind::Body, names: lets.clone() };
                            self.expr(value, &env)
                        }
                    };
                    self.lets.push(name.clone());
                    lets.push((name.clone(), v));
                }
                BodyLineKind::Out { rgb, cover } => {
                    let env = Env { kind: Kind::Body, names: lets.clone() };
                    let rgb = self.expr(rgb, &env).and_then(|v| match v.ty {
                        Ty::Vec3 => Ok(v),
                        Ty::Color => Err(self.mismatch("`out` takes a vec3 of straight rgb, got a color".into(), Some("write `.rgb`".into()))),
                        other => Err(self.mismatch(format!("`out` takes a vec3 of straight rgb, got {}", other.word()), None)),
                    });
                    let cover = match cover {
                        None => Ok(Cover::Surface),
                        Some(n) => {
                            self.site = self.at(n.span);
                            self.expr(&n.value, &env).and_then(|v| match v.ty {
                                Ty::Num => Ok(Cover::Value(v.id)),
                                other => Err(self.mismatch(format!("`cover:` takes a number in 0..1, got {}", other.word()), None)),
                            })
                        }
                    };
                    out = Some((rgb, cover));
                }
            }
        }
        if out.is_none() && !saw_sksl {
            self.site = self.at(self.src.span);
            self.report(
                format!("brush `{}` has no `out` line", self.src.name),
                Some("end the body with `out RGB cover:NUMBER`, such as `out m.paint.rgb cover:cover(m.edge)`".into()),
            );
        }
        if !self.diags.is_empty() {
            let own = self.src.file.to_string();
            self.diags.sort_by_key(|d| (d.at.file != own, d.at.file.clone(), d.at.span));
            return Err(self.diags);
        }
        let (Some((Ok(rgb), Ok(cover))), Some(stmts)) = (out, self.blocks.pop()) else {
            unreachable!("a body with no diags has a checked `out`");
        };
        let body = ir::Body {
            params: self.src.params.iter().map(|p| (p.name.clone(), p.ty)).collect(),
            salts: self.salts.into_iter().collect(),
            stmts,
            out: Out { rgb: rgb.id, cover },
        };
        Ok(Shader::new(body, self.reads))
    }

    fn report_at(&mut self, code: &'static str, at: Site, message: String, fix: Option<String>) -> Poisoned {
        self.diags.push(Diag { code, at, message, notes: Vec::new(), fix });
        Poisoned
    }

    /// A fault that does not depend on argument types: at the current line, or at the fn.
    fn report(&mut self, message: String, fix: Option<String>) -> Poisoned {
        let at = self.site.clone();
        self.report_at("E0360", at, message, fix)
    }

    /// A type fault. Inside a fn it lands at the call, with a note at the fn, since another call
    /// with other types may be sound.
    fn mismatch(&mut self, message: String, fix: Option<String>) -> Poisoned {
        match self.stack.last() {
            Some(fr) => {
                let note = format!("in fn `{}` ({}:{})", fr.name, fr.site.file, fr.site.span.line);
                self.diags.push(Diag { code: "E0360", at: fr.call.clone(), message, notes: vec![note], fix });
                Poisoned
            }
            None => self.report(message, fix),
        }
    }

    fn emit(&mut self, ty: Ty, op: Op) -> Val {
        let id = Local(self.next);
        self.next += 1;
        self.blocks.last_mut().expect("a block is open").push(Stmt { dst: id, ty, op });
        Val { id, ty, konst: None }
    }

    fn block<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> (T, Vec<Stmt>) {
        self.blocks.push(Vec::new());
        let r = f(self);
        (r, self.blocks.pop().expect("the block just pushed"))
    }

    fn expr(&mut self, e: &Expr, env: &Env) -> Lowered {
        match e {
            Expr::Num(n) | Expr::Angle(n) => {
                let mut v = self.emit(Ty::Num, Op::Num(*n));
                v.konst = Some(*n);
                Ok(v)
            }
            Expr::Percent(_) => Err(self.report("a percent has no frame in a body".into(), Some("write the number".into()))),
            Expr::Time(_) => Err(self.report("a time has no meaning in a body".into(), None)),
            Expr::Size { .. } => Err(self.report("a size has no meaning in a body".into(), Some("write a vector `w,h`".into()))),
            Expr::Hex(h) => Err(self.report(
                format!("a color literal `#{h}` cannot be written in a body"),
                Some(format!("declare it in the head, as `tint:#{h}`, and read `tint.rgb`")),
            )),
            Expr::Str(_) => Err(self.report("a string has no meaning in a body".into(), None)),
            Expr::Ref(segs) => Err(self.report(format!("a body cannot read `@{}`", segs.join(".")), Some("pass the measurement in as a parameter".into()))),
            Expr::Index(..) => Err(self.report("a body has no lists to index".into(), None)),
            Expr::Range(..) => Err(self.report("a range is only the bounds of a comprehension in `min`, `max`, or `sum`".into(), None)),
            Expr::List(_) | Expr::Each { .. } => Err(self.report(
                "a list is only the argument of `min`, `max`, or `sum`".into(),
                Some("write `max([each i in 0..4 -> ...])`".into()),
            )),
            Expr::Ident(name) => self.name(name, env),
            Expr::Member(base, seg) => self.member(base, seg, env),
            Expr::Tuple(xs) => self.tuple(xs, env),
            Expr::Unary(UnOp::Neg, a) => {
                let v = self.expr(a, env)?;
                if !v.ty.is_numeric() {
                    return Err(self.mismatch(format!("cannot negate {}", v.ty.word()), None));
                }
                let mut out = self.emit(v.ty, Op::Neg(v.id));
                out.konst = v.konst.map(|k| -k);
                Ok(out)
            }
            Expr::Unary(UnOp::Not, a) => {
                let v = self.expr(a, env)?;
                if v.ty != Ty::Bool {
                    return Err(self.mismatch(format!("`not` takes a bool, got {}", v.ty.word()), None));
                }
                Ok(self.emit(Ty::Bool, Op::Not(v.id)))
            }
            Expr::Binary(op, a, b) => self.binary(*op, a, b, env),
            Expr::Call { func, args, named } => self.call(func, args, named, env),
            Expr::If { cond, then, otherwise } => {
                let c = self.expr(cond, env)?;
                if c.ty != Ty::Bool {
                    return Err(self.mismatch(format!("`if` takes a bool, got {}", c.ty.word()), None));
                }
                let (t, then_stmts) = self.block(|s| s.expr(then, env));
                let (o, else_stmts) = self.block(|s| s.expr(otherwise, env));
                let (t, o) = (t?, o?);
                if t.ty != o.ty {
                    return Err(self.mismatch(format!("`then` gives {} and `else` gives {}", t.ty.word(), o.ty.word()), None));
                }
                let op = Op::If {
                    cond: c.id,
                    then: Block { stmts: then_stmts, value: t.id },
                    otherwise: Block { stmts: else_stmts, value: o.id },
                };
                Ok(self.emit(t.ty, op))
            }
        }
    }

    fn name(&mut self, name: &str, env: &Env) -> Lowered {
        if let Some((_, v)) = env.names.iter().rev().find(|(n, _)| n == name) {
            return *v;
        }
        match name {
            "true" => return Ok(self.emit(Ty::Bool, Op::Bool(true))),
            "false" => return Ok(self.emit(Ty::Bool, Op::Bool(false))),
            _ => {}
        }
        let module = match &env.kind {
            Kind::TopFn { module } => module.clone(),
            Kind::Body | Kind::Helper => {
                if let Some(i) = self.src.params.iter().position(|p| p.name == name) {
                    return Ok(self.emit(self.src.params[i].ty, Op::Param(i as u16)));
                }
                self.src.file.to_string()
            }
        };
        if name == "m" {
            return Err(self.report("`m` is the sample; read one of its fields".into(), Some("write `m.edge`, `m.uv`, `m.paint`, ...".into())));
        }
        if Field::parse(name).is_some() {
            return Err(self.report(format!("`{name}` is a field of `m`"), Some(format!("write `m.{name}`"))));
        }
        if Builtin::parse(name).is_some() || Noise::parse(name).is_some() || Reduce::parse(name).is_some() || self.helpers.iter().any(|h| h.name == name) {
            return Err(self.report(format!("`{name}` is a function"), Some(format!("call it: `{name}(...)`"))));
        }
        if matches!(env.kind, Kind::Helper) && self.lets.iter().any(|l| l == name) {
            return Err(self.report(
                format!("a helper does not see the body's `let {name}`"),
                Some(format!("pass `{name}` in as an argument")),
            ));
        }
        if matches!(env.kind, Kind::TopFn { .. }) && (self.src.params.iter().any(|p| p.name == name) || self.lets.iter().any(|l| l == name)) {
            return Err(self.report(
                format!("a top-level fn does not see the brush's `{name}`"),
                Some(format!("pass `{name}` in as an argument")),
            ));
        }
        match self.lib.lookup(&module, name) {
            Some(Global::Fn(_)) => Err(self.report(format!("`{name}` is a function"), Some(format!("call it: `{name}(...)`")))),
            Some(Global::Other(what)) => Err(self.report(
                format!("`{name}` is {what}, which a body cannot read"),
                Some(format!("pass it in as a parameter, as in `{name}:{name}` in the brush head")),
            )),
            None => {
                let fix = lang::values_only(name).map(str::to_string);
                Err(self.report(format!("unknown name `{name}`"), fix))
            }
        }
    }

    fn member(&mut self, base: &Expr, seg: &str, env: &Env) -> Lowered {
        if let Expr::Ident(b) = base
            && b == "m"
            && !env.names.iter().any(|(n, _)| n == "m")
        {
            if matches!(env.kind, Kind::TopFn { .. }) {
                return Err(self.report("a top-level fn does not see `m`".into(), Some(format!("pass the field in as an argument, as in `f(m.{seg})`"))));
            }
            return self.field(seg);
        }
        let v = self.expr(base, env)?;
        match Swizzle::parse(v.ty, seg) {
            Some(sw) => Ok(self.emit(sw.ty(), Op::Swizzle(v.id, sw))),
            None => {
                let members = match v.ty {
                    Ty::Vec2 => Some("members: x, y"),
                    Ty::Vec3 => Some("members: x, y, z, xy"),
                    Ty::Color => Some("members: r, g, b, a, rgb"),
                    Ty::Num | Ty::Bool => None,
                };
                Err(self.mismatch(format!("{} has no member `{seg}`", v.ty.word()), members.map(str::to_string)))
            }
        }
    }

    fn field(&mut self, seg: &str) -> Lowered {
        match Field::parse(seg) {
            Some(Field::Light) => Err(self.report("`m.light` lands with #256".into(), None)),
            Some(f) => {
                let site = self.site.clone();
                self.reads.record(f, &site);
                Ok(self.emit(f.ty().expect("every field but light has a type"), Op::Field(f)))
            }
            None => {
                let fix = match seg {
                    "dev" => Some("shade in local units; `m.px` is local units per device pixel".to_string()),
                    other => lang::renamed(other).map(|r| format!("write `{r}`")),
                };
                let fields: Vec<&str> = Field::ALL.iter().map(|f| f.name()).collect();
                self.diags.push(Diag {
                    code: "E0360",
                    at: self.site.clone(),
                    message: format!("`m` has no field `{seg}`"),
                    notes: vec![format!("fields: {}", fields.join(" "))],
                    fix,
                });
                Err(Poisoned)
            }
        }
    }

    fn tuple(&mut self, xs: &[Expr], env: &Env) -> Lowered {
        let mut vals = Vec::with_capacity(xs.len());
        for x in xs {
            vals.push(self.expr(x, env)?);
        }
        let mut lanes = 0u8;
        for v in &vals {
            if !v.ty.is_numeric() {
                return Err(self.mismatch(format!("a tuple joins numbers and vectors, got {}", v.ty.word()), None));
            }
            lanes = lanes.saturating_add(v.ty.lanes());
        }
        let Some(ty) = Ty::of_lanes(lanes).filter(|t| t.is_vector()) else {
            return Err(self.mismatch(format!("a tuple has two or three components, this one has {lanes}"), None));
        };
        Ok(self.emit(ty, Op::Pack(vals.iter().map(|v| v.id).collect())))
    }

    /// The type of `a op b` for arithmetic and the broadcasting builtins: both numeric, and a
    /// number applies to every component of the other side.
    fn broadcast(&mut self, a: Ty, b: Ty, what: &str) -> Result<Ty, Poisoned> {
        if a == Ty::Color || b == Ty::Color {
            return Err(self.mismatch(format!("a color takes no {what}"), Some("read `.rgb`".into())));
        }
        if !a.is_numeric() || !b.is_numeric() {
            return Err(self.mismatch(format!("cannot apply {what} to {} and {}", a.word(), b.word()), None));
        }
        match (a, b) {
            _ if a == b => Ok(a),
            (Ty::Num, _) => Ok(b),
            (_, Ty::Num) => Ok(a),
            _ => Err(self.mismatch("cannot combine a vec2 and a vec3".into(), Some("write `(v,z)` to lift the vec2".into()))),
        }
    }

    fn binary(&mut self, op: BinOp, a: &Expr, b: &Expr, env: &Env) -> Lowered {
        let arith = match op {
            BinOp::Add => Some(Arith::Add),
            BinOp::Sub => Some(Arith::Sub),
            BinOp::Mul => Some(Arith::Mul),
            BinOp::Div => Some(Arith::Div),
            _ => None,
        };
        let logic = match op {
            BinOp::Eq => Some(Logic::Eq),
            BinOp::Ne => Some(Logic::Ne),
            BinOp::Lt => Some(Logic::Lt),
            BinOp::Le => Some(Logic::Le),
            BinOp::Gt => Some(Logic::Gt),
            BinOp::Ge => Some(Logic::Ge),
            BinOp::And => Some(Logic::And),
            BinOp::Or => Some(Logic::Or),
            _ => None,
        };
        match op {
            BinOp::Approx => return Err(self.report("`~=` belongs to checks; a body has no tolerance".into(), Some("write `abs(a - b) < tol`".into()))),
            BinOp::In => return Err(self.report("`in` tests a range; a body has none".into(), Some("write `x >= lo and x < hi`".into()))),
            BinOp::Map => return Err(self.report("`->` maps a field through colors; a body shades one sample".into(), None)),
            _ => {}
        }
        let x = self.expr(a, env)?;
        let y = self.expr(b, env)?;
        if let Some(arith) = arith {
            let ty = self.broadcast(x.ty, y.ty, &format!("`{}`", arith.symbol()))?;
            let mut v = self.emit(ty, Op::Arith(arith, x.id, y.id));
            v.konst = match (x.konst, y.konst) {
                (Some(p), Some(q)) => Some(match arith {
                    Arith::Add => p + q,
                    Arith::Sub => p - q,
                    Arith::Mul => p * q,
                    Arith::Div => p / q,
                }),
                _ => None,
            };
            return Ok(v);
        }
        let logic = logic.expect("every other operator is a comparison or a connective");
        let sym = match logic {
            Logic::Eq => "==",
            Logic::Ne => "!=",
            Logic::Lt => "<",
            Logic::Le => "<=",
            Logic::Gt => ">",
            Logic::Ge => ">=",
            Logic::And => "and",
            Logic::Or => "or",
        };
        let ok = match logic {
            Logic::Eq | Logic::Ne => x.ty == y.ty && matches!(x.ty, Ty::Num | Ty::Bool),
            Logic::Lt | Logic::Le | Logic::Gt | Logic::Ge => x.ty == Ty::Num && y.ty == Ty::Num,
            Logic::And | Logic::Or => x.ty == Ty::Bool && y.ty == Ty::Bool,
        };
        if !ok {
            return Err(self.mismatch(format!("cannot apply `{sym}` to {} and {}", x.ty.word(), y.ty.word()), None));
        }
        Ok(self.emit(Ty::Bool, Op::Logic(logic, x.id, y.id)))
    }

    fn no_named(&mut self, func: &str, named: &[Named]) -> Result<(), Poisoned> {
        match named.first() {
            None => Ok(()),
            Some(n) => {
                let at = self.at(n.span);
                Err(self.report_at("E0360", at, format!("`{func}` takes no `{}:`", n.key), Some("remove the named argument".into())))
            }
        }
    }

    fn call(&mut self, func: &str, args: &[Expr], named: &[Named], env: &Env) -> Lowered {
        if let Some(r) = Reduce::parse(func)
            && (func == "sum" || (args.len() == 1 && matches!(args[0], Expr::Each { .. } | Expr::List(_))))
        {
            self.no_named(func, named)?;
            if args.len() != 1 {
                return Err(self.report(format!("`{func}` takes one list"), Some(format!("write `{func}([each i in 0..4 -> ...])`"))));
            }
            return self.reduce(r, &args[0], env);
        }
        if let Some(n) = Noise::parse(func) {
            return self.noise(n, args, named, env);
        }
        if let Some(b) = Builtin::parse(func) {
            self.no_named(func, named)?;
            return self.builtin(b, args, env);
        }
        self.no_named(func, named)?;
        if !matches!(env.kind, Kind::TopFn { .. })
            && let Some(h) = self.helpers.iter().find(|h| h.name == func).copied()
        {
            return self.instantiate(Callee::Helper(h), args, env);
        }
        let module = match &env.kind {
            Kind::TopFn { module } => module.clone(),
            _ => self.src.file.to_string(),
        };
        match self.lib.lookup(&module, func) {
            Some(Global::Fn(f)) => self.instantiate(Callee::Top(f), args, env),
            Some(Global::Other(what)) => Err(self.report(format!("`{func}` is {what}, not a function"), None)),
            None => match lang::values_only(func) {
                Some(hint) => Err(self.report(format!("`{func}` is not available in a body"), Some(hint.into()))),
                None => Err(self.report(format!("unknown function `{func}`"), None)),
            },
        }
    }

    /// Lowers the callee's expression inline, with its parameters bound to the arguments. Each
    /// call lowers its own statements, and the memo keeps every failing instantiation to one
    /// report.
    fn instantiate(&mut self, callee: Callee<'c>, args: &[Expr], env: &Env) -> Lowered {
        let mut vals = Vec::with_capacity(args.len());
        for a in args {
            vals.push(self.expr(a, env)?);
        }
        let (key, name, site, params, body, kind) = match &callee {
            Callee::Helper(h) => (
                FnKey { file: self.src.file.to_string(), scope: Some(self.src.name.to_string()), name: h.name.to_string() },
                h.name.to_string(),
                self.at(h.span),
                h.params,
                h.body,
                Kind::Helper,
            ),
            Callee::Top(f) => (
                FnKey { file: f.module.clone(), scope: None, name: f.name.clone() },
                f.name.clone(),
                Site { file: f.module.clone(), span: f.span },
                f.params.as_slice(),
                &f.body,
                Kind::TopFn { module: f.module.clone() },
            ),
        };
        if vals.len() != params.len() {
            let s = if params.len() == 1 { "" } else { "s" };
            return Err(self.report(
                format!("`{name}` takes {} argument{s}, got {}", params.len(), vals.len()),
                Some(format!("write `{name}({})`", params.join(" "))),
            ));
        }
        if let Some(at) = self.stack.iter().position(|f| f.key == key) {
            let shown: Vec<&str> = self.stack[at..].iter().map(|f| f.name.as_str()).chain([name.as_str()]).collect();
            self.diags.push(Diag {
                code: "E0360",
                at: self.site.clone(),
                message: format!("fn `{name}` calls itself"),
                notes: vec![format!("through {}", shown.join(" -> "))],
                fix: Some("a fn cannot recurse; write the repetition as a comprehension".into()),
            });
            return Err(Poisoned);
        }
        let tys: Vec<Ty> = vals.iter().map(|v| v.ty).collect();
        if let Some(None) = self.ck.types.get(&(key.clone(), tys.clone())) {
            return Err(Poisoned);
        }
        let frame_env = Env {
            kind,
            names: params.iter().cloned().zip(vals.iter().map(|v| Ok(Val { konst: None, ..*v }))).collect(),
        };
        let before = self.diags.len();
        self.stack.push(Frame { key: key.clone(), name, site: site.clone(), call: self.site.clone() });
        let caller_site = std::mem::replace(&mut self.site, site.clone());
        let r = self.expr(body, &frame_env);
        self.site = caller_site;
        self.stack.pop();
        let mut i = before;
        while i < self.diags.len() {
            if self.diags[i].at == site && !self.ck.reported.insert((key.clone(), self.diags[i].message.clone())) {
                self.diags.remove(i);
            } else {
                i += 1;
            }
        }
        self.ck.types.insert((key, tys), r.ok().map(|v| v.ty));
        r
    }

    fn reduce(&mut self, op: Reduce, arg: &Expr, env: &Env) -> Lowered {
        let name = op.name();
        match arg {
            Expr::List(items) => {
                if items.is_empty() {
                    return Err(self.report(format!("`{name}` of an empty list"), None));
                }
                let mut ids = Vec::with_capacity(items.len());
                for item in items {
                    let v = self.expr(item, env)?;
                    if v.ty != Ty::Num {
                        return Err(self.mismatch(format!("`{name}` reduces numbers, got {}", v.ty.word()), None));
                    }
                    ids.push(v.id);
                }
                Ok(self.emit(Ty::Num, Op::Fold { op, items: ids }))
            }
            Expr::Each { index, var, iter, body } => {
                if index.is_some() {
                    return Err(self.report("a comprehension in a body has no index name".into(), Some("write `[each x in lo..hi -> ...]`".into())));
                }
                let Expr::Range(lo, hi) = &**iter else {
                    return Err(self.report(
                        "a comprehension in a body runs over a constant range".into(),
                        Some("write `lo..hi` with number literals".into()),
                    ));
                };
                let (Some(lo), Some(hi)) = (const_num(lo), const_num(hi)) else {
                    self.diags.push(Diag {
                        code: "E0360",
                        at: self.site.clone(),
                        message: "the bounds of a range in a body are constants".into(),
                        notes: vec!["a parameter is a uniform, so it cannot bound a loop".into()],
                        fix: Some("write number literals, such as `-1..2`".into()),
                    });
                    return Err(Poisoned);
                };
                if lo.fract() != 0.0 || hi.fract() != 0.0 || lo.abs() > 1e6 || hi.abs() > 1e6 {
                    return Err(self.report("range bounds in a body are whole numbers within a million".into(), None));
                }
                if lo >= hi {
                    return Err(self.report(format!("a range in a body holds at least one item; `{lo}..{hi}` is empty"), None));
                }
                let n = (hi - lo) as u32;
                if n.saturating_mul(self.trips) > MAX_TRIPS {
                    self.diags.push(Diag {
                        code: "E0360",
                        at: self.site.clone(),
                        message: format!("a comprehension runs at most {MAX_TRIPS} items, this one runs {}", n as u64 * self.trips as u64),
                        notes: vec!["nested comprehensions multiply their item counts".into()],
                        fix: Some("narrow the range".into()),
                    });
                    return Err(Poisoned);
                }
                if let Some(what) = lang::taken(var) {
                    return Err(self.report(format!("a loop variable cannot be named `{var}`: it is {what}"), Some("rename it".into())));
                }
                let var_id = LoopVar(self.loops);
                self.loops += 1;
                let outer = self.trips;
                self.trips *= n;
                let (item, stmts) = self.block(|s| {
                    let lv = s.emit(Ty::Num, Op::Loop(var_id));
                    let mut inner = env.clone();
                    inner.names.push((var.clone(), Ok(lv)));
                    s.expr(body, &inner)
                });
                self.trips = outer;
                let item = item?;
                if item.ty != Ty::Num {
                    return Err(self.mismatch(format!("`{name}` reduces numbers, got {}", item.ty.word()), None));
                }
                let op = Op::Reduce { op, var: var_id, lo: lo as i32, hi: hi as i32, item: Block { stmts, value: item.id } };
                Ok(self.emit(Ty::Num, op))
            }
            _ => Err(self.report(format!("`{name}` takes one list"), Some(format!("write `{name}([each i in 0..4 -> ...])`")))),
        }
    }

    fn noise(&mut self, f: Noise, args: &[Expr], named: &[Named], env: &Env) -> Lowered {
        let name = f.name();
        if args.len() != 1 {
            return Err(self.report(format!("`{name}` takes one point"), Some(format!("write `{name}(m.uv / 20)`"))));
        }
        let p = self.expr(&args[0], env)?;
        match p.ty {
            Ty::Vec2 => {}
            Ty::Num => return Err(self.mismatch(format!("`{name}` takes a vec2 point, got a number"), Some("write `(x,0)`".into()))),
            Ty::Vec3 => return Err(self.mismatch(format!("`{name}` takes a vec2 point; a vec3 point waits for #309"), Some("write `.xy`".into()))),
            other => return Err(self.mismatch(format!("`{name}` takes a vec2 point, got {}", other.word()), None)),
        }
        let mut seed = None;
        let mut oct = 4u8;
        for n in named {
            let at = self.at(n.span);
            match n.key.as_str() {
                "salt" | "seed" => {
                    if seed.is_some() {
                        return Err(self.report_at("E0360", at, "a noise call takes `salt:` or `seed:`, not both".into(), None));
                    }
                    let k = self.whole(&n.value, &at, 0.0, u32::MAX as f64, &format!("`{}:`", n.key))? as u32;
                    seed = Some(if n.key == "salt" {
                        self.salts.insert(k);
                        NoiseSeed::Salt(k)
                    } else {
                        let o = Seeds::from(k).offset;
                        NoiseSeed::Fixed([o[0] as u8, o[1] as u8])
                    });
                }
                "oct" if f == Noise::Fbm => {
                    oct = self.whole(&n.value, &at, 1.0, 8.0, "`oct:`")? as u8;
                }
                other => {
                    let fix = if f == Noise::Fbm { "`fbm` takes `oct:`, `salt:`, and `seed:`" } else { "a noise call takes `salt:` or `seed:`" };
                    return Err(self.report_at("E0360", at, format!("`{name}` takes no `{other}:`"), Some(fix.into())));
                }
            }
        }
        Ok(self.emit(f.ty(), Op::Noise { f, at: p.id, seed: seed.unwrap_or(NoiseSeed::Stage), oct }))
    }

    /// A whole-number constant in `lo..=hi`, reported at `at`.
    fn whole(&mut self, e: &Expr, at: &Site, lo: f64, hi: f64, what: &str) -> Result<f64, Poisoned> {
        let Some(v) = const_num(e) else {
            return Err(self.report_at(
                "E0360",
                at.clone(),
                format!("{what} takes a constant"),
                Some("write a number literal; a parameter is a uniform".into()),
            ));
        };
        if v.fract() != 0.0 || v < lo || v > hi {
            return Err(self.report_at("E0360", at.clone(), format!("{what} must be a whole number from {lo} to {hi}, got {v}"), None));
        }
        Ok(v)
    }

    fn builtin(&mut self, f: Builtin, args: &[Expr], env: &Env) -> Lowered {
        let name = f.name();
        if args.len() != f.arity() {
            let s = if f.arity() == 1 { "" } else { "s" };
            return Err(self.report(format!("`{name}` takes {} argument{s}, got {}", f.arity(), args.len()), None));
        }
        let mut vals = Vec::with_capacity(args.len());
        for a in args {
            vals.push(self.expr(a, env)?);
        }
        let tys: Vec<Ty> = vals.iter().map(|v| v.ty).collect();
        let ty = match f {
            Builtin::Abs | Builtin::Floor | Builtin::Round | Builtin::Fract | Builtin::Sqrt | Builtin::Sin | Builtin::Cos | Builtin::Exp => {
                if !tys[0].is_numeric() {
                    return Err(self.mismatch(format!("`{name}` takes a number or a vector, got {}", tys[0].word()), None));
                }
                tys[0]
            }
            Builtin::Min | Builtin::Max | Builtin::Clamp | Builtin::Lerp | Builtin::Step | Builtin::Smoothstep | Builtin::Pow | Builtin::Mod => {
                if f == Builtin::Lerp && tys[0] == Ty::Color && tys[1] == Ty::Color {
                    return Err(self.mismatch(
                        "`lerp` on two colors is not available in a body".into(),
                        Some("lerp on `.rgb`: `lerp(a.rgb b.rgb k)`".into()),
                    ));
                }
                let mut ty = tys[0];
                for t in &tys[1..] {
                    ty = self.broadcast(ty, *t, &format!("`{name}`"))?;
                }
                ty
            }
            Builtin::Atan2 => {
                if tys != [Ty::Num, Ty::Num] {
                    return Err(self.mismatch(format!("`atan2(y x)` takes two numbers, got {} and {}", tys[0].word(), tys[1].word()), None));
                }
                Ty::Num
            }
            Builtin::Rad | Builtin::Cover => {
                if tys[0] != Ty::Num {
                    return Err(self.mismatch(format!("`{name}` takes a number, got {}", tys[0].word()), None));
                }
                Ty::Num
            }
            Builtin::Dot => {
                if !tys[0].is_vector() || tys[0] != tys[1] {
                    return Err(self.mismatch(format!("`dot` takes two vectors of one length, got {} and {}", tys[0].word(), tys[1].word()), None));
                }
                Ty::Num
            }
            Builtin::Length | Builtin::Normalize => {
                if !tys[0].is_vector() {
                    return Err(self.mismatch(format!("`{name}` takes a vector, got {}", tys[0].word()), None));
                }
                if f == Builtin::Length { Ty::Num } else { tys[0] }
            }
            Builtin::Rot => {
                if tys != [Ty::Vec2, Ty::Num] {
                    return Err(self.mismatch(format!("`rot(p deg)` takes a vec2 and a number, got {} and {}", tys[0].word(), tys[1].word()), None));
                }
                Ty::Vec2
            }
        };
        Ok(self.emit(ty, Op::Call(f, vals.iter().map(|v| v.id).collect())))
    }
}

/// A constant: a number literal, or arithmetic on number literals.
fn const_num(e: &Expr) -> Option<f64> {
    match e {
        Expr::Num(n) | Expr::Angle(n) => Some(*n),
        Expr::Unary(UnOp::Neg, a) => Some(-const_num(a)?),
        Expr::Binary(op, a, b) => {
            let (x, y) = (const_num(a)?, const_num(b)?);
            Some(match op {
                BinOp::Add => x + y,
                BinOp::Sub => x - y,
                BinOp::Mul => x * y,
                BinOp::Div => x / y,
                _ => return None,
            })
        }
        _ => None,
    }
}

/// The type of a head default written as a literal, for [`Shader::from_text`].
fn literal_ty(e: &Expr) -> Option<Ty> {
    match e {
        Expr::Num(_) | Expr::Angle(_) | Expr::Unary(UnOp::Neg, _) => const_num(e).map(|_| Ty::Num),
        Expr::Hex(_) => Some(Ty::Color),
        Expr::Tuple(xs) if xs.iter().all(|x| const_num(x).is_some()) => Ty::of_lanes(xs.len() as u8).filter(|t| t.is_vector()),
        _ => None,
    }
}

struct NoLibrary;

impl Library for NoLibrary {
    fn lookup(&self, _file: &str, _name: &str) -> Option<Global> {
        None
    }
}

pub(crate) fn from_text(src: &str) -> Result<Shader, Vec<Diag>> {
    const FILE: &str = "text";
    let at = |span: Span| Site { file: FILE.to_string(), span };
    let file = ramose_syntax::parse(src).map_err(|errs| {
        errs.into_iter()
            .map(|e| Diag { code: "E0101", at: at(e.span), message: e.message, notes: e.notes, fix: e.fix })
            .collect::<Vec<_>>()
    })?;
    let Some((name, params, lines, span)) = file.decls().find_map(|d| match &d.kind {
        DeclKind::Brush { name, params, body } => Some((name, params, body, d.span)),
        _ => None,
    }) else {
        return Err(vec![Diag { code: "E0363", at: at(Span::default()), message: "no `brush` declaration".into(), notes: Vec::new(), fix: None }]);
    };
    let mut ps = Vec::new();
    for p in params {
        if PLACEMENT_KEYS.contains(&p.key.as_str()) {
            continue;
        }
        let Some(ty) = literal_ty(&p.value) else {
            return Err(vec![Diag {
                code: "E0363",
                at: at(p.span),
                message: format!("`{}` must default to a number, color or vector literal", p.key),
                notes: Vec::new(),
                fix: None,
            }]);
        };
        ps.push(Param { name: p.key.clone(), ty, span: p.span });
    }
    let source = BrushSource { file: FILE, name, span, params: &ps, lines };
    Checker::default().check(&source, &NoLibrary)
}
