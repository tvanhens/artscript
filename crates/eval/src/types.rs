//! Parameter types read from the form of each default, and the table of every file's top-level
//! names that resolves a default's bare names. Nothing here evaluates: the whole project is typed
//! from its parse trees, so `ramose check` knows every parameter's type before any job runs.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use ramose_brush::PLACEMENT_KEYS;
use ramose_syntax::Diagnostic;
use ramose_syntax::ast::{BinOp, DeclKind, Expr, File, Item, Named, Scene, SceneKind, Span, UnOp};

use crate::builtins::BUILTIN_RETURNS;
use crate::eval::{Fail, R, canvas_renamed, fail, measure_rect, osa_distance};
use crate::project::{Project, STD, std_file};
use crate::value::{Kind, Value};

/// A top-level declaration, by the file that holds it and its name.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DeclRef {
    pub file: Arc<str>,
    pub name: Arc<str>,
}

impl DeclRef {
    pub fn new(file: &str, name: &str) -> DeclRef {
        DeclRef { file: file.into(), name: name.into() }
    }
}

/// A parameter's type, read from its default's form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ty {
    Bool,
    Number,
    Vector,
    Vector3,
    Color,
    Palette,
    Field,
    Range,
    List,
    Str,
    Ref,
    Rule,
    Fn,
    Warp,
    Path,
    /// `one_of(a b)` carries its words, the default first. A plain word carries none.
    Word(Option<Arc<[String]>>),
    /// The def or alias target the default names. `None` for `none(component)`.
    Component(Option<DeclRef>),
    Brush(Option<DeclRef>),
    Stamp(Option<DeclRef>),
}

/// The words `none(T)` takes, each with its type.
pub const TYPE_WORDS: &[(&str, Ty)] = &[
    ("bool", Ty::Bool),
    ("number", Ty::Number),
    ("vector", Ty::Vector),
    ("vector3", Ty::Vector3),
    ("color", Ty::Color),
    ("palette", Ty::Palette),
    ("field", Ty::Field),
    ("range", Ty::Range),
    ("list", Ty::List),
    ("string", Ty::Str),
    ("word", Ty::Word(None)),
    ("component", Ty::Component(None)),
    ("ref", Ty::Ref),
    ("rule", Ty::Rule),
    ("fn", Ty::Fn),
    ("brush", Ty::Brush(None)),
    ("stamp", Ty::Stamp(None)),
    ("warp", Ty::Warp),
    ("path", Ty::Path),
];

impl Ty {
    pub fn from_word(w: &str) -> Option<Ty> {
        TYPE_WORDS.iter().find(|(word, _)| *word == w).map(|(_, ty)| ty.clone())
    }

    /// The kind messages name and values are compared by.
    pub fn kind(&self) -> Kind {
        match self {
            Ty::Bool => Kind::Bool,
            Ty::Number => Kind::Number,
            Ty::Vector => Kind::Vector,
            Ty::Vector3 => Kind::Vector3,
            Ty::Color => Kind::Color,
            Ty::Palette => Kind::Palette,
            Ty::Field => Kind::Field,
            Ty::Range => Kind::Range,
            Ty::List => Kind::List,
            Ty::Str => Kind::Str,
            Ty::Ref => Kind::Ref,
            Ty::Rule => Kind::Rule,
            Ty::Fn => Kind::Fn,
            Ty::Warp => Kind::Warp,
            Ty::Path => Kind::Path,
            Ty::Word(_) => Kind::Word,
            Ty::Component(_) => Kind::Component,
            Ty::Brush(_) => Kind::Brush,
            Ty::Stamp(_) => Kind::Stamp,
        }
    }

    /// Whether a parameter of this type takes a value of kind `k`. A component takes a word, a
    /// list and a range take each other, a warp takes a list, a color takes a field, a vector
    /// takes a number, and a palette takes the module a palette file binds as.
    pub fn accepts_kind(&self, k: Kind) -> bool {
        let own = self.kind();
        own == k
            || matches!(
                (own, k),
                (Kind::Component, Kind::Word)
                    | (Kind::List, Kind::Range)
                    | (Kind::Range, Kind::List)
                    | (Kind::Warp, Kind::List)
                    | (Kind::Color, Kind::Field)
                    | (Kind::Vector, Kind::Number)
                    | (Kind::Palette, Kind::Module)
            )
    }

    /// Whether a parameter of this type takes `v` from a call. Every parameter but a `one_of`
    /// takes `none`.
    pub fn accepts(&self, v: &Value) -> bool {
        match (self, v) {
            (Ty::Word(Some(words)), _) => matches!(v, Value::Ident(w) if words.contains(w)),
            (_, Value::None) => true,
            _ => self.accepts_kind(v.kind()),
        }
    }

    /// A value an author could write for this type, for fix hints.
    pub fn example(&self) -> String {
        match self {
            Ty::Bool => "true".into(),
            Ty::Number => "10".into(),
            Ty::Vector => "4,2".into(),
            Ty::Vector3 => "4,2,1".into(),
            Ty::Color => "p.ink".into(),
            Ty::Palette => "p".into(),
            Ty::Field => "simplex(8)".into(),
            Ty::Range => "0..4".into(),
            Ty::List => "[1 2]".into(),
            Ty::Str => "\"text\"".into(),
            Ty::Ref => "@self".into(),
            Ty::Rule | Ty::Fn => "a name declared with `rule` or `fn`".into(),
            Ty::Warp => "cylinder(60)".into(),
            Ty::Path => "path([0,0 10,0])".into(),
            Ty::Word(Some(words)) => words[0].clone(),
            Ty::Word(None) => "a_word".into(),
            Ty::Component(Some(d)) | Ty::Brush(Some(d)) | Ty::Stamp(Some(d)) => d.name.to_string(),
            Ty::Component(None) => "a def name".into(),
            Ty::Brush(None) => "dry".into(),
            Ty::Stamp(None) => "dots".into(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Param {
    pub key: String,
    pub ty: Ty,
}

/// The typed parameters of one declaration, in head order.
#[derive(Clone, Debug)]
pub struct Signature {
    pub name: String,
    pub params: Vec<Param>,
}

impl Signature {
    pub fn get(&self, key: &str) -> Option<&Param> {
        self.params.iter().find(|p| p.key == key)
    }

    pub fn keys(&self) -> Vec<&str> {
        self.params.iter().map(|p| p.key.as_str()).collect()
    }

    /// The notes a call error carries: the parameters and the placement keys the call takes.
    pub fn call_notes(&self, f: Fail, extra: &[&str]) -> Fail {
        f.note(format!("parameters: {}", self.keys().join(", ")))
            .note(format!("placement: {}", extra.join(", ")))
    }

    /// Checks a call's arguments against the parameters. `extra` names the call keys that are
    /// not parameters, such as placement keys; they pass. A positional argument, an unknown key,
    /// or a kind mismatch is `code`, and the notes list what the declaration takes.
    pub fn check_call(&self, pos: &[Value], named: &[(String, Value)], extra: &[&str], code: &'static str) -> R<()> {
        let notes = |f: Fail| self.call_notes(f, extra);
        if !pos.is_empty() {
            return Err(notes(fail(code, format!("`{}` takes named arguments only", self.name))));
        }
        for (k, v) in named {
            if extra.contains(&k.as_str()) {
                continue;
            }
            let Some(p) = self.get(k) else {
                let f = fail(code, format!("`{}` has no parameter `{k}`", self.name));
                let candidates = self.keys().into_iter().chain(extra.iter().copied());
                return Err(notes(match nearest_key(k, candidates) {
                    Some(near) => f.fix(format!("did you mean `{near}:`?")),
                    None => f.fix("use one of the listed parameters"),
                }));
            };
            if !p.ty.accepts(v) {
                let msg = format!("`{k}` of `{}` expects {}, got {}", self.name, p.ty.kind(), v.kind());
                return Err(notes(fail(code, msg).fix(format!("pass {}", p.ty.kind()))));
            }
        }
        Ok(())
    }
}

/// The candidate within a third of `key`'s length in edits, for a "did you mean" fix.
pub fn nearest_key<'a>(key: &str, candidates: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    candidates
        .map(|c| (osa_distance(key, c), c))
        .min_by_key(|(d, _)| *d)
        .filter(|(d, _)| *d <= (key.len() + 1) / 3)
        .map(|(_, c)| c)
}

/// Why a default's type cannot be read from its form.
#[derive(Clone, Debug, PartialEq)]
pub enum Untyped {
    BareNone,
    BadNone,
    /// `one_of` with anything but one or more bare words.
    OneOf,
    FnCall(String),
    UnknownCall(String),
    Branches(Kind, Kind),
    Member,
    Index,
    Arith(Kind, Kind),
    Cycle(String),
    Canvas,
    Let { name: String, why: Box<Untyped> },
    /// A name a failed `use` or a broken alias would have bound. That failure is already
    /// reported, so this one never is.
    Lost,
}

impl Untyped {
    fn clause(&self) -> String {
        match self {
            Untyped::BareNone => "`none` has no type".into(),
            Untyped::BadNone => "`none(T)` takes one type word or the name of a def, brush, or stamp".into(),
            Untyped::OneOf => "`one_of` takes one or more bare words".into(),
            Untyped::FnCall(f) => format!("the fn `{f}` has no result type until it runs"),
            Untyped::UnknownCall(f) => format!("`{f}` is not a function with a known result type"),
            Untyped::Branches(a, b) => format!("the `if` gives {a} or {b}"),
            Untyped::Member => "it reads a member of something that is not a palette, module, or vector".into(),
            Untyped::Index => "a list item has no type until it runs".into(),
            Untyped::Arith(a, b) => format!("arithmetic on {a} and {b} has no type"),
            Untyped::Cycle(n) => format!("`{n}` reads itself"),
            Untyped::Canvas => "`canvas` is now `artboard`".into(),
            Untyped::Let { name, why } => format!("it reads `{name}`, and {}", why.clause()),
            Untyped::Lost => String::new(),
        }
    }

    /// The error at the parameter `key` of `owner`, or `None` for a lost name.
    fn fail(&self, owner: &str, key: &str) -> Option<Fail> {
        let type_words: Vec<&str> = TYPE_WORDS.iter().map(|(w, _)| *w).collect();
        Some(match self {
            Untyped::Lost => return None,
            Untyped::OneOf => fail("E0320", self.clause()).fix(format!("write `{key}:one_of(a b c)`")),
            Untyped::Canvas => canvas_renamed(),
            Untyped::BadNone => fail("E0326", format!("`{key}` of `{owner}` has a default whose type cannot be read: {}", self.clause()))
                .fix(format!("write `{key}:none(color)` or another of {}, or `{key}:none(dry)` for a declaration", type_words.join(" "))),
            _ => fail("E0326", format!("`{key}` of `{owner}` has a default whose type cannot be read: {}", self.clause()))
                .fix(format!("write `{key}:none(number)` with the type word, or a default whose form shows its type, like `{key}:10`"))
                .note(format!("type words: {}", type_words.join(" "))),
        })
    }
}

/// What a top-level name of one file is, from its parse tree alone.
#[derive(Clone, Debug)]
enum Entry {
    Def,
    Alias { target: String, args: Vec<Expr>, named: Vec<Named>, span: Span },
    Brush,
    Stamp,
    Rule,
    Fn,
    Palette { swatches: Vec<String> },
    Let(Expr),
    Param(Expr),
    Import { from: Arc<str>, name: String },
    Module(Arc<str>),
}

impl Entry {
    /// Whether another file's `use` can bind this name: imports and modules are not re-exported.
    fn exported(&self) -> bool {
        !matches!(self, Entry::Import { .. } | Entry::Module(_) | Entry::Param(_))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sort {
    Def,
    Brush,
    Stamp,
    Rule,
    Finish,
    Entry,
}

/// A declaration with typed parameters.
#[derive(Clone, Debug)]
struct Declared {
    sort: Sort,
    name: String,
    params: Vec<Named>,
}

#[derive(Clone, Debug, Default)]
struct Outline {
    names: BTreeMap<String, Entry>,
    /// `use lib` with no item list: every export of `lib` is in scope.
    globs: Vec<Arc<str>>,
    /// `@target ... each k:[..]` axes, typed by their items.
    axes: Vec<(String, Ty)>,
    decls: Vec<Declared>,
}

impl Outline {
    fn new(project: &Project, file: &File) -> Outline {
        let mut out = Outline::default();
        let entry_params: Vec<Named> = file
            .decls()
            .filter_map(|d| match &d.kind {
                DeclKind::Param { params } => Some(params.clone()),
                _ => None,
            })
            .flatten()
            .collect();
        if !entry_params.is_empty() {
            for p in &entry_params {
                out.names.insert(p.key.clone(), Entry::Param(p.value.clone()));
            }
            out.decls.push(Declared { sort: Sort::Entry, name: "entry file".into(), params: entry_params });
        }
        for d in file.directives.iter().filter(|d| d.name == "target") {
            for axis in &d.each {
                let items = match &axis.value {
                    Expr::List(xs) | Expr::Tuple(xs) => xs.as_slice(),
                    one => std::slice::from_ref(one),
                };
                let numeric = items.iter().all(|x| matches!(x, Expr::Num(_) | Expr::Unary(UnOp::Neg, _)));
                out.axes.push((axis.key.clone(), if numeric { Ty::Number } else { Ty::Word(None) }));
            }
        }
        for item in &file.items {
            let d = match item {
                Item::Decl(d) => d,
                Item::Scene(Scene { kind: SceneKind::Let { name, value }, .. }) => {
                    out.names.insert(name.clone(), Entry::Let(value.clone()));
                    continue;
                }
                Item::Scene(_) => continue,
            };
            match &d.kind {
                DeclKind::Param { .. } | DeclKind::Style { .. } | DeclKind::Anim { .. } | DeclKind::Light { .. } => {}
                DeclKind::Use { path, alias, items } => {
                    // A path no file answers keeps its names, so each resolves as lost, not as a word.
                    let rooted = format!("{path}.ramo");
                    let from: Arc<str> = project.resolve_use(path).unwrap_or(rooted).into();
                    match alias {
                        Some(a) => {
                            out.names.insert(a.clone(), Entry::Module(from));
                        }
                        None if items.is_empty() => out.globs.push(from),
                        None => {
                            for name in items {
                                out.names.insert(name.clone(), Entry::Import { from: from.clone(), name: name.clone() });
                            }
                        }
                    }
                }
                DeclKind::Palette { name, body, .. } => {
                    let swatches = body.iter().map(|s| s.name.clone()).collect();
                    out.names.insert(name.clone(), Entry::Palette { swatches });
                }
                DeclKind::Finish { name, params, .. } => {
                    out.decls.push(Declared { sort: Sort::Finish, name: name.clone(), params: params.clone() });
                }
                DeclKind::Def { name, params, .. } => {
                    out.names.insert(name.clone(), Entry::Def);
                    let params = params.iter().filter(|p| p.key != "needs").cloned().collect();
                    out.decls.push(Declared { sort: Sort::Def, name: name.clone(), params });
                }
                DeclKind::Alias { name, target, args, named } => {
                    out.names.insert(
                        name.clone(),
                        Entry::Alias { target: target.clone(), args: args.clone(), named: named.clone(), span: d.span },
                    );
                }
                DeclKind::Rule { name, params, .. } => {
                    out.names.insert(name.clone(), Entry::Rule);
                    out.decls.push(Declared { sort: Sort::Rule, name: name.clone(), params: params.clone() });
                }
                DeclKind::Fn { name, .. } => {
                    out.names.insert(name.clone(), Entry::Fn);
                }
                DeclKind::Brush { name, params, .. } => {
                    out.names.insert(name.clone(), Entry::Brush);
                    let params = params.iter().filter(|p| !PLACEMENT_KEYS.contains(&p.key.as_str())).cloned().collect();
                    out.decls.push(Declared { sort: Sort::Brush, name: name.clone(), params });
                }
                DeclKind::Stamp { name, params, .. } => {
                    out.names.insert(name.clone(), Entry::Stamp);
                    let params = params.iter().filter(|p| p.key != "spacing" && p.key != "count").cloned().collect();
                    out.decls.push(Declared { sort: Sort::Stamp, name: name.clone(), params });
                }
            }
        }
        out
    }
}

/// The outlines of every file, with the types of lets memoized across files.
#[derive(Debug, Default)]
struct Table {
    files: BTreeMap<Arc<str>, Outline>,
    memo: RefCell<BTreeMap<DeclRef, Result<Ty, Untyped>>>,
    visiting: RefCell<Vec<DeclRef>>,
}

/// Resolves the bare names of one file's expressions.
pub struct Resolver<'a> {
    table: &'a Table,
    file: Arc<str>,
}

impl<'a> Resolver<'a> {
    fn in_file(&self, file: &Arc<str>) -> Resolver<'a> {
        Resolver { table: self.table, file: file.clone() }
    }

    fn outline(&self) -> Option<&'a Outline> {
        self.table.files.get(&self.file)
    }

    fn decl(&self, name: &str) -> DeclRef {
        DeclRef { file: self.file.clone(), name: name.into() }
    }

    /// The entry a bare name of this file reaches: its own declaration, else an export of a
    /// file imported whole, the last such `use` winning as it does when the file runs.
    fn entry(&self, name: &str) -> Option<(Resolver<'a>, &'a Entry)> {
        let outline = self.outline()?;
        if let Some(e) = outline.names.get(name) {
            return Some((self.in_file(&self.file), e));
        }
        outline.globs.iter().rev().find_map(|g| self.in_file(g).export(name))
    }

    /// A name another file may import from this one.
    fn export(&self, name: &str) -> Option<(Resolver<'a>, &'a Entry)> {
        if name.starts_with('_') {
            return None;
        }
        let e = self.outline()?.names.get(name).filter(|e| e.exported())?;
        Some((self.in_file(&self.file), e))
    }

    /// A bare name, looked up as a body does: an earlier parameter, an entry parameter,
    /// `target`/`clip`/`t`, a matrix axis, `p`, a top-level name, then the built-in names. A
    /// name that names nothing is a word.
    fn name(&self, name: &str, earlier: &[Param]) -> Result<Ty, Untyped> {
        self.bound(name, earlier).unwrap_or(Ok(Ty::Word(None)))
    }

    /// The type of a name that names something; `None` for a literal word.
    fn bound(&self, name: &str, earlier: &[Param]) -> Option<Result<Ty, Untyped>> {
        if let Some(p) = earlier.iter().rev().find(|p| p.key == name) {
            return Some(Ok(p.ty.clone()));
        }
        match name {
            "target" | "clip" => return Some(Ok(Ty::Word(None))),
            "t" => return Some(Ok(Ty::Number)),
            _ => {}
        }
        if let Some((_, ty)) = self.outline().and_then(|o| o.axes.iter().find(|(k, _)| k == name)) {
            return Some(Ok(ty.clone()));
        }
        if let Some((r, e)) = self.entry(name) {
            return Some(r.entry_ty(name, e));
        }
        Some(Ok(match name {
            "p" => Ty::Palette,
            "scale" => Ty::Number,
            "true" | "false" => Ty::Bool,
            "none" => return Some(Err(Untyped::BareNone)),
            "artboard" => Ty::Ref,
            "canvas" => return Some(Err(Untyped::Canvas)),
            _ => return None,
        }))
    }

    fn entry_ty(&self, name: &str, e: &Entry) -> Result<Ty, Untyped> {
        Ok(match e {
            Entry::Def => Ty::Component(Some(self.decl(name))),
            Entry::Alias { .. } => Ty::Component(Some(self.alias_target(name)?.0)),
            Entry::Brush => Ty::Brush(Some(self.decl(name))),
            Entry::Stamp => Ty::Stamp(Some(self.decl(name))),
            Entry::Rule => Ty::Rule,
            Entry::Fn => Ty::Fn,
            Entry::Palette { .. } | Entry::Module(_) => Ty::Palette,
            Entry::Let(x) | Entry::Param(x) => self.binding(name, x)?,
            Entry::Import { from, name } => match self.in_file(from).export(name) {
                Some((r, e)) => r.entry_ty(name, e)?,
                None => return Err(Untyped::Lost),
            },
        })
    }

    /// The type of a `let` or entry `param`, from its right side, once per binding.
    fn binding(&self, name: &str, value: &Expr) -> Result<Ty, Untyped> {
        let id = self.decl(name);
        if let Some(done) = self.table.memo.borrow().get(&id) {
            return done.clone();
        }
        if self.table.visiting.borrow().contains(&id) {
            return Err(Untyped::Cycle(name.to_string()));
        }
        self.table.visiting.borrow_mut().push(id.clone());
        let ty = ty_of(value, self, &[]).map_err(|why| match why {
            Untyped::Cycle(_) | Untyped::Lost => why,
            why => Untyped::Let { name: name.to_string(), why: Box::new(why) },
        });
        self.table.visiting.borrow_mut().pop();
        self.table.memo.borrow_mut().insert(id, ty.clone());
        ty
    }

    /// The def the alias `name` ends at, with every key the aliases past it fix. The alias's own
    /// arguments are not among them: they are what its caller checks.
    fn alias_target(&self, name: &str) -> Result<(DeclRef, Vec<String>), Untyped> {
        let mut seen = BTreeSet::new();
        let mut r = self.in_file(&self.file);
        let mut name = name.to_string();
        let mut chain: Vec<(usize, Vec<String>)> = Vec::new();
        let def = loop {
            if !seen.insert(r.decl(&name)) {
                return Err(Untyped::Lost);
            }
            let Some((next, e)) = r.entry(&name) else {
                return Err(Untyped::Lost);
            };
            match e {
                Entry::Def => break next.decl(&name),
                Entry::Alias { target, args, named, .. } => {
                    chain.push((args.len(), named.iter().map(|n| n.key.clone()).collect()));
                    name = target.clone();
                    r = next;
                }
                Entry::Import { from, name: imported } => {
                    r = next.in_file(from);
                    name = imported.clone();
                }
                _ => return Err(Untyped::Lost),
            }
        };
        let params: Vec<&str> = r
            .outline()
            .and_then(|o| o.decls.iter().find(|d| d.sort == Sort::Def && *d.name == *def.name))
            .map(|d| d.params.iter().map(|p| p.key.as_str()).collect())
            .unwrap_or_default();
        let mut fixed: Vec<String> = Vec::new();
        for (positional, named) in chain.iter().skip(1).rev() {
            let open: Vec<&str> = params.iter().copied().filter(|k| !fixed.iter().any(|f| f == k)).collect();
            fixed.extend(open.iter().take(*positional).map(|k| k.to_string()));
            fixed.extend(named.iter().cloned());
        }
        Ok((def, fixed))
    }

    /// `m.seg` on a file imported as `m`: an export, else a swatch of one of its palettes.
    fn module_member(&self, seg: &str) -> Result<Ty, Untyped> {
        if let Some((r, e)) = self.export(seg) {
            return r.entry_ty(seg, e);
        }
        let outline = self.outline().ok_or(Untyped::Lost)?;
        let swatch = outline.names.values().any(|e| matches!(e, Entry::Palette { swatches } if swatches.iter().any(|s| s == seg)));
        if swatch { Ok(Ty::Color) } else { Err(Untyped::Member) }
    }
}

/// The words of `one_of(a b)`: one or more bare words and nothing else.
pub fn one_of_words(args: &[Expr], named: &[Named]) -> Result<Arc<[String]>, Untyped> {
    let words: Option<Vec<String>> = args
        .iter()
        .map(|a| match a {
            Expr::Ident(w) => Some(w.clone()),
            _ => None,
        })
        .collect();
    match words {
        Some(w) if !w.is_empty() && named.is_empty() => Ok(w.into()),
        _ => Err(Untyped::OneOf),
    }
}

/// The type of a default from its form. `earlier` are the parameters declared before it.
pub fn ty_of(e: &Expr, r: &Resolver<'_>, earlier: &[Param]) -> Result<Ty, Untyped> {
    Ok(match e {
        Expr::Num(_) | Expr::Percent(_) | Expr::Angle(_) | Expr::Time(_) => Ty::Number,
        Expr::Size { depth: None, .. } => Ty::Vector,
        Expr::Size { depth: Some(_), .. } => Ty::Vector3,
        Expr::Hex(_) => Ty::Color,
        Expr::Str(_) => Ty::Str,
        Expr::Ref(_) => Ty::Ref,
        Expr::Ident(name) => r.name(name, earlier)?,
        Expr::Member(base, seg) => {
            if let Expr::Ident(n) = &**base
                && !earlier.iter().any(|p| p.key == *n)
                && let Some((_, Entry::Module(file))) = r.entry(n)
            {
                return r.in_file(file).module_member(seg);
            }
            match ty_of(base, r, earlier)? {
                Ty::Palette | Ty::Color => Ty::Color,
                Ty::Vector | Ty::Vector3 => Ty::Number,
                Ty::Ref if measure_rect(skia_safe::Rect::default(), seg).is_some() => Ty::Number,
                _ => return Err(Untyped::Member),
            }
        }
        Expr::Index(..) => return Err(Untyped::Index),
        Expr::Tuple(xs) => {
            let tys = xs.iter().map(|x| ty_of(x, r, earlier)).collect::<Result<Vec<_>, _>>()?;
            match tys.as_slice() {
                [Ty::Number, Ty::Number] => Ty::Vector,
                [Ty::Number, Ty::Number, Ty::Number] => Ty::Vector3,
                _ => Ty::List,
            }
        }
        Expr::List(_) | Expr::Each { .. } => Ty::List,
        Expr::Range(..) => Ty::Range,
        Expr::Unary(UnOp::Not, _) => Ty::Bool,
        Expr::Unary(UnOp::Neg, x) => match ty_of(x, r, earlier)? {
            t @ (Ty::Number | Ty::Vector | Ty::Vector3 | Ty::Field | Ty::List) => t,
            t => return Err(Untyped::Arith(t.kind(), Kind::Number)),
        },
        Expr::Binary(op, l, rhs) => match op {
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::Approx | BinOp::And | BinOp::Or | BinOp::In => Ty::Bool,
            BinOp::Map => Ty::Color,
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
                arith_ty(ty_of(l, r, earlier)?.kind(), ty_of(rhs, r, earlier)?.kind())?
            }
        },
        Expr::Call { func, args, named } => match func.as_str() {
            "one_of" => Ty::Word(Some(one_of_words(args, named)?)),
            "none" => match args.as_slice() {
                [Expr::Ident(w)] if named.is_empty() => match Ty::from_word(w) {
                    Some(ty) => ty,
                    None => match r.name(w, earlier)? {
                        ty @ (Ty::Component(Some(_)) | Ty::Brush(Some(_)) | Ty::Stamp(Some(_))) => ty,
                        _ => return Err(Untyped::BadNone),
                    },
                },
                _ => return Err(Untyped::BadNone),
            },
            _ => match r.name(func, earlier)? {
                ty @ (Ty::Brush(_) | Ty::Stamp(_)) => ty,
                Ty::Rule => Ty::Bool,
                Ty::Fn => return Err(Untyped::FnCall(func.clone())),
                Ty::Word(None) if func == "lerp" && args.len() == 3 => {
                    match (ty_of(&args[0], r, earlier)?.kind(), ty_of(&args[1], r, earlier)?.kind()) {
                        (Kind::Color, Kind::Color) => Ty::Color,
                        (a, b) => arith_ty(a, b)?,
                    }
                }
                Ty::Word(None) if matches!(func.as_str(), "min" | "max" | "abs" | "clamp") => {
                    let given = match (func.as_str(), args.as_slice()) {
                        ("min" | "max", [Expr::List(xs)]) => xs.as_slice(),
                        _ => args.as_slice(),
                    };
                    let kinds = given.iter().map(|e| ty_of(e, r, earlier).map(|t| t.kind())).collect::<Result<Vec<_>, _>>()?;
                    match (func.as_str(), kinds.as_slice()) {
                        ("min" | "max", ks) if ks.contains(&Kind::Field) => Ty::Field,
                        ("abs", [Kind::Field, ..]) | ("clamp", [Kind::Field]) => Ty::Field,
                        ("abs", [Kind::Vector, ..]) => Ty::Vector,
                        _ => Ty::Number,
                    }
                }
                Ty::Word(None) => match BUILTIN_RETURNS.iter().find(|(f, _)| f == func) {
                    Some((_, ty)) => ty.clone(),
                    None => return Err(Untyped::UnknownCall(func.clone())),
                },
                _ => return Err(Untyped::UnknownCall(func.clone())),
            },
        },
        Expr::If { then, otherwise, .. } => {
            let (a, b) = (ty_of(then, r, earlier)?, ty_of(otherwise, r, earlier)?);
            match (a, b) {
                (a, b) if a == b => a,
                (Ty::Component(_), Ty::Component(_)) => Ty::Component(None),
                (Ty::Brush(_), Ty::Brush(_)) => Ty::Brush(None),
                (Ty::Stamp(_), Ty::Stamp(_)) => Ty::Stamp(None),
                (a, b) if a.kind() == b.kind() => a,
                (a, b) => return Err(Untyped::Branches(a.kind(), b.kind())),
            }
        }
    })
}

fn arith_ty(a: Kind, b: Kind) -> Result<Ty, Untyped> {
    Ok(match (a, b) {
        (Kind::Number, Kind::Number) => Ty::Number,
        (Kind::Field, Kind::Number | Kind::Field) | (Kind::Number, Kind::Field) => Ty::Field,
        (Kind::Vector, Kind::Number | Kind::Vector) | (Kind::Number, Kind::Vector) => Ty::Vector,
        (Kind::Vector3, Kind::Number | Kind::Vector3) | (Kind::Number, Kind::Vector3) => Ty::Vector3,
        (Kind::List, Kind::Number) | (Kind::Number, Kind::List) => Ty::List,
        (a, b) => return Err(Untyped::Arith(a, b)),
    })
}

/// The value a `one_of(..)` or `none(..)` default stands for, or `None` for an ordinary
/// expression the evaluator computes.
pub fn marker_value(e: &Expr) -> Option<Value> {
    let Expr::Call { func, args, .. } = e else { return None };
    match (func.as_str(), args.first()) {
        ("one_of", Some(Expr::Ident(first))) => Some(Value::Ident(first.clone())),
        ("none", _) => Some(Value::None),
        _ => None,
    }
}

/// Every file's signatures, built once per project from the parse trees.
#[derive(Clone, Debug, Default)]
pub struct Outlines {
    sigs: BTreeMap<DeclRef, Arc<Signature>>,
    finishes: BTreeMap<DeclRef, Arc<Signature>>,
    entries: BTreeMap<Arc<str>, Arc<Signature>>,
    diags: Vec<Diagnostic>,
}

impl Outlines {
    pub fn build(project: &Project) -> Outlines {
        let mut table = Table::default();
        let files = project.files.keys().cloned().chain(STD.iter().map(|(p, _)| std_file(p)));
        for file in files {
            let Some(src) = project.source(&file) else { continue };
            let Ok(parsed) = ramose_syntax::parse(src) else { continue };
            table.files.insert(file.into(), Outline::new(project, &parsed));
        }
        let mut out = Outlines::default();
        for (file, outline) in &table.files {
            let r = Resolver { table: &table, file: file.clone() };
            for d in &outline.decls {
                let sig = out.signature_of(&r, d);
                let id = DeclRef { file: file.clone(), name: d.name.as_str().into() };
                match d.sort {
                    Sort::Entry => out.entries.insert(file.clone(), sig),
                    Sort::Finish => out.finishes.insert(id, sig),
                    _ => out.sigs.insert(id, sig),
                };
            }
        }
        for (file, outline) in &table.files {
            let r = Resolver { table: &table, file: file.clone() };
            for (name, e) in &outline.names {
                if let Entry::Alias { args, named, span, .. } = e {
                    out.check_alias(&r, name, args, named, *span);
                }
            }
        }
        out
    }

    fn report(&mut self, f: Fail, file: &Arc<str>, span: Span) {
        let mut d = Diagnostic::error(f.code, span, f.msg).in_file(file.to_string());
        d.notes = f.notes;
        d.fix = f.fix;
        self.diags.push(d);
    }

    fn signature_of(&mut self, r: &Resolver<'_>, d: &Declared) -> Arc<Signature> {
        let mut params: Vec<Param> = Vec::new();
        for p in &d.params {
            match ty_of(&p.value, r, &params) {
                Ok(ty) => {
                    if d.sort == Sort::Brush && !matches!(ty.kind(), Kind::Number | Kind::Vector | Kind::Color) {
                        let msg = format!("`{}` of `{}` must default to a number, color or vector, got {}", p.key, d.name, ty.kind());
                        self.report(fail("E0363", msg).fix(format!("write `{}:.5`", p.key)), &r.file, p.span);
                    }
                    params.push(Param { key: p.key.clone(), ty });
                }
                Err(why) => {
                    if let Some(f) = why.fail(&d.name, &p.key) {
                        self.report(f, &r.file, p.span);
                    }
                }
            }
        }
        Arc::new(Signature { name: d.name.clone(), params })
    }

    /// Checks each argument an alias fixes against the def's signature, once, at the alias.
    fn check_alias(&mut self, r: &Resolver<'_>, alias: &str, args: &[Expr], named: &[Named], span: Span) {
        let Ok((def, fixed)) = r.alias_target(alias) else { return };
        let Some(sig) = self.sigs.get(&def).cloned() else { return };
        let open: Vec<&Param> = sig.params.iter().filter(|p| !fixed.contains(&p.key)).collect();
        let mut checks: Vec<(&Param, &Expr)> = args.iter().zip(open.iter().copied()).map(|(e, p)| (p, e)).collect();
        checks.extend(named.iter().filter_map(|n| sig.get(&n.key).filter(|_| !fixed.contains(&n.key)).map(|p| (p, &n.value))));
        for (p, e) in checks {
            if matches!(e, Expr::Ident(w) if w == "none") {
                if let Ty::Word(Some(words)) = &p.ty {
                    self.report(one_of_mismatch(&sig.name, &p.key, words, "none"), &r.file, span);
                }
                continue;
            }
            if let Ty::Word(Some(words)) = &p.ty
                && let Expr::Ident(w) = e
                && r.bound(w, &[]).is_none()
            {
                if !words.contains(w) {
                    self.report(one_of_mismatch(&sig.name, &p.key, words, w), &r.file, span);
                }
                continue;
            }
            match ty_of(e, r, &[]) {
                Ok(ty) if p.ty.accepts_kind(ty.kind()) => {}
                Ok(ty) => {
                    let msg = format!("`{}` of `{}` expects {}, got {}", p.key, sig.name, p.ty.kind(), ty.kind());
                    self.report(fail("E0320", msg).fix(param_fix(p)), &r.file, span);
                }
                Err(why) => {
                    if let Some(f) = why.fail(alias, &p.key) {
                        self.report(f, &r.file, span);
                    }
                }
            }
        }
    }

    /// The parameters of a def, brush, stamp, or rule.
    pub fn signature(&self, id: &DeclRef) -> Option<&Arc<Signature>> {
        self.sigs.get(id)
    }

    pub fn finish(&self, id: &DeclRef) -> Option<&Arc<Signature>> {
        self.finishes.get(id)
    }

    /// The `param` declarations of an entry file, in order.
    pub fn entry_params(&self, file: &str) -> Option<&Arc<Signature>> {
        self.entries.get(file)
    }

    /// A file's first parameter type error, by position. Such a file is not evaluated.
    pub fn first_error(&self, file: &str) -> Option<&Diagnostic> {
        self.diags.iter().filter(|d| d.file == file).min_by_key(|d| (d.span.line, d.span.col))
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diags
    }
}

pub fn one_of_mismatch(owner: &str, key: &str, words: &[String], got: &str) -> Fail {
    let list = words.join(", ");
    fail("E0320", format!("`{key}` of `{owner}` is one of {list}, got {got}"))
        .fix(format!("write `{key}:{}` or another of {list}", words[0]))
}

pub fn param_fix(p: &Param) -> String {
    format!("pass {} like `{}:{}`", p.ty.kind(), p.key, p.ty.example())
}

/// E0320 when a call's argument does not fit the parameter.
pub fn check_param(owner: &str, p: &Param, given: &Value) -> R<()> {
    if p.ty.accepts(given) {
        return Ok(());
    }
    Err(match &p.ty {
        Ty::Word(Some(words)) => one_of_mismatch(owner, &p.key, words, &given.show()),
        Ty::Palette if matches!(given, Value::Ident(w) if w == "p") => {
            fail("E0320", format!("`{}` of `{owner}` expects a palette, but no palette is bound to `p`", p.key))
                .fix("set `palette` under `[defaults]` in ramose.toml, or add `palette:` to the @target")
        }
        _ => fail("E0320", format!("`{}` of `{owner}` expects {}, got {}", p.key, p.ty.kind(), given.kind())).fix(param_fix(p)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = "[project]\nout = \"out\"\nlib = [\"lib\"]\n\n[defaults]\npalette = \"palette/swatch\"\n";

    const SWATCH: &str = "palette swatch\n  ramp sky #4f7fe0 5\n  color ink #2b2440\n";

    fn outlines(files: &[(&str, &str)]) -> Outlines {
        let mut all = vec![("lib/palette/swatch.ramo", SWATCH)];
        all.extend_from_slice(files);
        let files = all.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let project = Project::from_sources(MANIFEST, files).unwrap();
        Outlines::build(&project)
    }

    fn param_ty(o: &Outlines, file: &str, decl: &str, key: &str) -> Ty {
        let sig = o.signature(&DeclRef::new(file, decl)).unwrap_or_else(|| panic!("no signature for {decl} in {file}"));
        sig.get(key).unwrap_or_else(|| panic!("no param {key} on {decl}: {:?}", o.diagnostics())).ty.clone()
    }

    fn def_ty(default: &str, libs: &[(&str, &str)]) -> Ty {
        let src = format!("use std/brushes {{ dry }}\nuse std/stamps {{ dots }}\nuse parts {{ eye }}\nuse pal as m\npalette lantern\n  ramp stone #888888 3\n  color amber #ffaa00\nlet stone_line = mix(lantern.stone.line lantern.amber .4)\nlet loop_a = loop_b\nlet loop_b = loop_a\nlet bad_let = pick([1 2])\nfn short(n) = n * 2\nbrush chip strength:.5\n  sksl \"\"\"\n  float4 shade(Mark m) {{ return float4(1); }}\n  \"\"\"\ndef tunic size:20\n  rect size\ndef it width:8 look:0,0 {default}\n  circle 2\n");
        let mut all = vec![
            ("lib/t.ramo", src.as_str()),
            ("lib/parts.ramo", "def eye size:10\n  circle size\nlet tone = #ff0000\n"),
            ("lib/pal.ramo", "palette dusk\n  color paper #f4f1ea\ndef badge size:4\n  circle size\n"),
        ];
        all.extend_from_slice(libs);
        let o = outlines(&all);
        assert!(o.diagnostics().is_empty(), "{default}: {:?}", o.diagnostics());
        param_ty(&o, "lib/t.ramo", "it", "x")
    }

    #[test]
    fn literals_type_by_their_form() {
        for (default, ty) in [
            ("x:10", Ty::Number),
            ("x:50%", Ty::Number),
            ("x:90deg", Ty::Number),
            ("x:200ms", Ty::Number),
            ("x:-1", Ty::Number),
            ("x:72x32", Ty::Vector),
            ("x:0,0", Ty::Vector),
            ("x:-.6,-.8", Ty::Vector),
            ("x:4,2,1", Ty::Vector3),
            ("x:40x20x10", Ty::Vector3),
            ("x:1,2,3,4", Ty::List),
            ("x:p.ink,2", Ty::List),
            ("x:true", Ty::Bool),
            ("x:#fff", Ty::Color),
            ("x:[]", Ty::List),
            ("x:[0,0]", Ty::List),
            ("x:[each k in 0..3 -> k]", Ty::List),
            ("x:0..4", Ty::Range),
            ("x:\"text\"", Ty::Str),
            ("x:@self", Ty::Ref),
            ("x:artboard", Ty::Ref),
            ("x:target", Ty::Word(None)),
            ("x:t", Ty::Number),
            ("x:scale", Ty::Number),
        ] {
            assert_eq!(def_ty(default, &[]), ty, "{default}");
        }
    }

    #[test]
    fn palette_members_calls_and_operators_type_by_their_form() {
        for (default, ty) in [
            ("x:p", Ty::Palette),
            ("x:p.sky", Ty::Color),
            ("x:p.sky.3", Ty::Color),
            ("x:lantern.stone.line", Ty::Color),
            ("x:lantern.amber", Ty::Color),
            ("x:m.paper", Ty::Color),
            ("x:m", Ty::Palette),
            ("x:mix(p.ink p.sky.1 .5)", Ty::Color),
            ("x:grad(down p.ink p.sky)", Ty::Color),
            ("x:value(8 wrap:32 seed:1)", Ty::Field),
            ("x:simplex(8) -> p.sky", Ty::Color),
            ("x:cylinder(60)", Ty::Warp),
            ("x:rand(0 1)", Ty::Number),
            ("x:polar(4 30)", Ty::Vector),
            ("x:abs(width)", Ty::Number),
            ("x:abs(look)", Ty::Vector),
            ("x:abs(simplex(4))", Ty::Field),
            ("x:max(simplex(4) .2)", Ty::Field),
            ("x:min([1 simplex(4)])", Ty::Field),
            ("x:max(width 2)", Ty::Number),
            ("x:clamp(simplex(4))", Ty::Field),
            ("x:clamp(width 0 1)", Ty::Number),
            ("x:lerp(p.ink p.sky .5)", Ty::Color),
            ("x:lerp(look 0,4 .5)", Ty::Vector),
            ("x:lerp(width 4 .5)", Ty::Number),
            ("x:concat([1] [2])", Ty::List),
            ("x:width+2", Ty::Number),
            ("x:width*look", Ty::Vector),
            ("x:-width", Ty::Number),
            ("x:-look", Ty::Vector),
            ("x:(4,2,1)*2", Ty::Vector3),
            ("x:(4,2,1).z", Ty::Number),
            ("x:simplex(8)*2", Ty::Field),
            ("x:width<3", Ty::Bool),
            ("x:not true", Ty::Bool),
            ("x:width in 0..4", Ty::Bool),
            ("x:if width > 2 then 1 else 3", Ty::Number),
            ("x:look.w", Ty::Number),
            ("x:artboard.w/16", Ty::Number),
        ] {
            assert_eq!(def_ty(default, &[]), ty, "{default}");
        }
    }

    #[test]
    fn a_canvas_default_is_the_rename_error() {
        let o = outlines(&[("lib/t.ramo", "def it x:canvas.w\n  circle 2\n")]);
        let d = &o.diagnostics()[0];
        assert_eq!((d.code.as_str(), d.message.as_str(), d.span.line), ("E0320", "`canvas` is now `artboard`", 1), "{d}");
    }

    #[test]
    fn names_type_by_what_they_declare() {
        assert_eq!(def_ty("x:floor_top", &[]), Ty::Word(None));
        assert_eq!(def_ty("x:one_of(a b)", &[]), Ty::Word(Some(vec!["a".to_string(), "b".to_string()].into())));
        assert_eq!(def_ty("x:tunic", &[]), Ty::Component(Some(DeclRef::new("lib/t.ramo", "tunic"))));
        assert_eq!(def_ty("x:eye", &[]), Ty::Component(Some(DeclRef::new("lib/parts.ramo", "eye"))));
        assert_eq!(def_ty("x:m.badge", &[]), Ty::Component(Some(DeclRef::new("lib/pal.ramo", "badge"))));
        assert_eq!(def_ty("x:chip", &[]), Ty::Brush(Some(DeclRef::new("lib/t.ramo", "chip"))));
        assert_eq!(def_ty("x:dry", &[]), Ty::Brush(Some(DeclRef::new("std/brushes.ramo", "dry"))));
        assert_eq!(def_ty("x:dry(strength:.9)", &[]), Ty::Brush(Some(DeclRef::new("std/brushes.ramo", "dry"))));
        assert_eq!(def_ty("x:dots", &[]), Ty::Stamp(Some(DeclRef::new("std/stamps.ramo", "dots"))));
        assert_eq!(def_ty("x:short", &[]), Ty::Fn);
        assert_eq!(def_ty("x:stone_line", &[]), Ty::Color);
        assert_eq!(def_ty("x:width", &[]), Ty::Number);
        assert_eq!(def_ty("x:lantern", &[]), Ty::Palette);
        assert_eq!(def_ty("x:if width > 2 then dry else dry", &[]), Ty::Brush(Some(DeclRef::new("std/brushes.ramo", "dry"))));
        assert_eq!(def_ty("x:if width > 2 then dry else chip", &[]), Ty::Brush(None));
        assert_eq!(def_ty("x:if width > 2 then tunic else eye", &[]), Ty::Component(None));
    }

    #[test]
    fn none_takes_a_type_word_or_a_declaration() {
        assert_eq!(def_ty("x:none(color)", &[]), Ty::Color);
        assert_eq!(def_ty("x:none(component)", &[]), Ty::Component(None));
        assert_eq!(def_ty("x:none(brush)", &[]), Ty::Brush(None));
        assert_eq!(def_ty("x:none(word)", &[]), Ty::Word(None));
        assert_eq!(def_ty("x:none(chip)", &[]), Ty::Brush(Some(DeclRef::new("lib/t.ramo", "chip"))));
        assert_eq!(def_ty("x:none(dots)", &[]), Ty::Stamp(Some(DeclRef::new("std/stamps.ramo", "dots"))));
        assert_eq!(def_ty("x:none(tunic)", &[]), Ty::Component(Some(DeclRef::new("lib/t.ramo", "tunic"))));
    }

    #[test]
    fn a_glob_import_and_an_entry_param_and_an_axis_resolve() {
        let o = outlines(&[
            ("art/t.ramo", "@artboard 8x8\n@target t_{team}.png each team:[red blue] n:[1 2]\nuse parts\nparam width:8 kind:one_of(a b)\nparam height:width\ndef it a:eye b:width c:kind d:team e:n f:height\n  circle 2\n"),
            ("lib/parts.ramo", "def eye size:10\n  circle size\ndef _hidden size:1\n  circle 1\n"),
        ]);
        assert!(o.diagnostics().is_empty(), "{:?}", o.diagnostics());
        assert_eq!(param_ty(&o, "art/t.ramo", "it", "a"), Ty::Component(Some(DeclRef::new("lib/parts.ramo", "eye"))));
        assert_eq!(param_ty(&o, "art/t.ramo", "it", "b"), Ty::Number);
        assert_eq!(param_ty(&o, "art/t.ramo", "it", "c").kind(), Kind::Word);
        assert_eq!(param_ty(&o, "art/t.ramo", "it", "d"), Ty::Word(None));
        assert_eq!(param_ty(&o, "art/t.ramo", "it", "e"), Ty::Number);
        assert_eq!(param_ty(&o, "art/t.ramo", "it", "f"), Ty::Number);
        let entry = o.entry_params("art/t.ramo").unwrap();
        assert_eq!(entry.keys(), ["width", "kind", "height"]);
        assert_eq!(entry.get("height").unwrap().ty, Ty::Number);
    }

    #[test]
    fn a_name_two_glob_imports_export_has_the_type_of_the_last() {
        let files = [("lib/a.ramo", "def eye size:3\n  circle size\n"), ("lib/b.ramo", "let eye = 5\n")];
        for (uses, ty) in [("use a\nuse b", Ty::Number), ("use b\nuse a", Ty::Component(Some(DeclRef::new("lib/a.ramo", "eye"))))] {
            let o = outlines(&[files[0], files[1], ("lib/t.ramo", &format!("{uses}\ndef it x:eye\n  circle 2\n"))]);
            assert_eq!(param_ty(&o, "lib/t.ramo", "it", "x"), ty, "{uses}");
        }
    }

    fn only_error(files: &[(&str, &str)]) -> Diagnostic {
        let ds = outlines(files).diagnostics().to_vec();
        assert_eq!(ds.len(), 1, "{ds:?}");
        ds[0].clone()
    }

    #[test]
    fn an_untypeable_default_is_e0326_at_the_parameter() {
        for (default, clause) in [
            ("none", "`none` has no type"),
            ("none(1)", "`none(T)` takes one type word"),
            ("none(nothing)", "`none(T)` takes one type word"),
            ("short(3)", "the fn `short`"),
            ("pick([1 2])", "`pick` is not a function with a known result type"),
            ("if true then 1 else #fff", "the `if` gives a number or a color"),
            ("pts.{1}", "a list item has no type"),
            ("size.x", "member of something that is not a palette"),
            ("size+#fff", "arithmetic on a number and a color or paint"),
            ("loop_a", "`loop_a` reads itself"),
            ("bad_let", "it reads `bad_let`, and `pick` is not a function"),
        ] {
            let src = format!("let loop_a = loop_b\nlet loop_b = loop_a\nlet bad_let = pick([1 2])\nfn short(n) = n * 2\ndef it width:8 pts:[1 2]\n  circle 2\ndef bad\n  circle 1\ndef it2 size:10 x:{default}\n  circle 2\n");
            let d = only_error(&[("lib/t.ramo", &src)]);
            assert_eq!((d.code.as_str(), d.file.as_str(), d.span.line), ("E0326", "lib/t.ramo", 9), "{default}: {d}");
            assert!(d.message.starts_with("`x` of `it2` has a default whose type cannot be read: "), "{default}: {d}");
            assert!(d.message.contains(clause), "{default}: {d}");
            assert!(d.fix.as_deref().unwrap_or_default().contains("none("), "{default}: {d}");
            assert_eq!(d.span.col, src.lines().nth(8).unwrap().find("x:").unwrap() as u32 + 1, "{default}: {d}");
        }
    }

    #[test]
    fn a_malformed_one_of_stays_e0320() {
        for decl in ["one_of()", "one_of(1 2)", "one_of(a b c:1)"] {
            let d = only_error(&[("lib/t.ramo", &format!("def tile kind:{decl}\n  circle 2\n"))]);
            assert_eq!((d.code.as_str(), d.span.line), ("E0320", 1), "{decl}: {d}");
            assert!(d.message.contains("one_of"), "{d}");
        }
    }

    #[test]
    fn a_brush_key_that_is_not_a_uniform_is_e0363_before_the_body_compiles() {
        let d = only_error(&[("lib/t.ramo", "brush odd strength:.5 tint:one_of(a b)\n  sksl \"\"\"\n  float4 shade(Mark m) { return float4(1); }\n  \"\"\"\n")]);
        assert_eq!((d.code.as_str(), d.span.line, d.span.col), ("E0363", 1, 23), "{d}");
        assert!(d.message.contains("`tint` of `odd` must default to a number, color or vector, got a word"), "{d}");
    }

    #[test]
    fn an_alias_fixed_argument_is_checked_once_at_the_alias() {
        let tile = "def tile kind:one_of(floor_top fill) size:10\n  circle 2\n";
        let d = only_error(&[("lib/t.ramo", &format!("{tile}def wall = tile kind:flor_top\n"))]);
        assert_eq!((d.code.as_str(), d.span.line), ("E0320", 3), "{d}");
        assert!(d.message.contains("`kind` of `tile` is one of floor_top, fill, got flor_top"), "{d}");
        let d = only_error(&[("lib/t.ramo", &format!("{tile}def wall = tile size:#fff\n"))]);
        assert!(d.message.contains("`size` of `tile` expects a number, got a color"), "{d}");
        let d = only_error(&[("lib/t.ramo", &format!("{tile}def wall = tile fill #fff\n"))]);
        assert!(d.message.contains("`size` of `tile` expects a number, got a color"), "{d}");
        let d = only_error(&[("lib/t.ramo", &format!("{tile}fn f(n) = n\ndef wall = tile size:f(1)\n"))]);
        assert_eq!(d.code, "E0326", "{d}");
        let o = outlines(&[("lib/t.ramo", &format!("{tile}def wall = tile fill none\ndef room = wall\n"))]);
        assert!(o.diagnostics().is_empty(), "{:?}", o.diagnostics());
    }

    #[test]
    fn an_alias_fixing_a_one_of_with_a_bound_word_is_checked_at_the_instance() {
        let o = outlines(&[
            ("art/t.ramo", "@artboard 8x8\n@target t_{team}.png each team:[red blue]\nparam side:red\ndef tile kind:one_of(red blue)\n  circle 2\ndef a = tile kind:team\ndef b = tile kind:side\ndef c = tile kind:target\n"),
            ("lib/t.ramo", "let shade_word = blue\ndef tile kind:one_of(red blue)\n  circle 2\ndef wall = tile kind:shade_word\n"),
        ]);
        assert!(o.diagnostics().is_empty(), "{:?}", o.diagnostics());
    }

    #[test]
    fn an_alias_chain_fills_positionals_in_def_order_and_a_cycle_is_quiet() {
        let tile = "def tile kind:one_of(a b) size:10 tone:#fff\n  circle 2\n";
        let o = outlines(&[("lib/t.ramo", &format!("{tile}def wall = tile a\ndef room = wall 4 #000\ndef x = y\ndef y = x\ndef z = nothing\n"))]);
        assert!(o.diagnostics().is_empty(), "{:?}", o.diagnostics());
        let ds = outlines(&[("lib/t.ramo", &format!("{tile}def wall = tile a\ndef room = wall #000\n"))]).diagnostics().to_vec();
        assert_eq!(ds.len(), 1, "{ds:?}");
        assert!(ds[0].message.contains("`size` of `tile` expects a number, got a color"), "{}", ds[0]);
    }

    #[test]
    fn a_lost_import_is_not_reported_again() {
        let o = outlines(&[("lib/t.ramo", "use nowhere { thing }\ndef it x:thing y:3\n  circle 2\n")]);
        assert!(o.diagnostics().is_empty(), "{:?}", o.diagnostics());
        let sig = o.signature(&DeclRef::new("lib/t.ramo", "it")).unwrap();
        assert!(sig.get("x").is_none());
        assert_eq!(sig.get("y").unwrap().ty, Ty::Number);
        assert!(o.first_error("lib/t.ramo").is_none());
    }

    #[test]
    fn a_failed_file_is_the_one_with_the_error() {
        let o = outlines(&[("lib/a.ramo", "def it x:none\n  circle 2\n"), ("lib/b.ramo", "def ok x:1\n  circle 2\n")]);
        assert!(o.first_error("lib/a.ramo").is_some());
        assert!(o.first_error("lib/b.ramo").is_none());
    }

    fn corpus() -> Vec<Project> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut dirs: Vec<_> = std::fs::read_dir(root.join("examples")).unwrap().flatten().map(|e| e.path()).collect();
        dirs.sort();
        dirs.push(root.join("catalog"));
        dirs.push(root.join("assets"));
        dirs.into_iter().filter(|d| d.join("ramose.toml").is_file()).map(|d| crate::load(&d).unwrap()).collect()
    }

    /// Every parameter of the def, brush, stamp, rule, finish, and `param` declarations in `file`.
    fn declared_params(file: &File) -> Vec<(&str, Vec<&Named>)> {
        let mut out: Vec<(&str, Vec<&Named>)> = Vec::new();
        let mut entry = Vec::new();
        for d in file.decls() {
            match &d.kind {
                DeclKind::Param { params } => entry.extend(params),
                DeclKind::Def { name, params, .. } => out.push((name, params.iter().filter(|p| p.key != "needs").collect())),
                DeclKind::Brush { name, params, .. } => out.push((name, params.iter().filter(|p| !PLACEMENT_KEYS.contains(&p.key.as_str())).collect())),
                DeclKind::Stamp { name, params, .. } => out.push((name, params.iter().filter(|p| p.key != "spacing" && p.key != "count").collect())),
                DeclKind::Rule { name, params, .. } | DeclKind::Finish { name, params, .. } => out.push((name, params.iter().collect())),
                _ => {}
            }
        }
        if !entry.is_empty() {
            out.push(("entry file", entry));
        }
        out
    }

    #[test]
    fn the_corpus_types_clean_and_every_parameter_has_a_type_without_evaluation() {
        for project in corpus() {
            let o = project.outlines();
            assert!(o.diagnostics().is_empty(), "{}: {:?}", project.root.display(), o.diagnostics());
            let files = project.files.keys().cloned().chain(STD.iter().map(|(p, _)| std_file(p)));
            for file in files {
                let parsed = ramose_syntax::parse(project.source(&file).unwrap()).unwrap();
                for (name, params) in declared_params(&parsed) {
                    let id = DeclRef::new(&file, name);
                    let sig = o.signature(&id).or_else(|| o.finish(&id)).or_else(|| o.entry_params(&file)).unwrap_or_else(|| panic!("{file}: no signature for `{name}`"));
                    for p in params {
                        assert!(sig.get(&p.key).is_some(), "{file}: `{}` of `{name}` has no type", p.key);
                    }
                }
            }
        }
    }

    #[test]
    fn accepts_keeps_the_loosenesses_and_none() {
        let v = |v: Value| v;
        assert!(Ty::Vector.accepts(&v(Value::Num(1.0))));
        assert!(Ty::Component(None).accepts(&v(Value::Ident("x".into()))));
        assert!(Ty::Color.accepts(&v(Value::None)));
        assert!(!Ty::Number.accepts(&v(Value::Ident("x".into()))));
        let words = Ty::Word(Some(vec!["a".to_string()].into()));
        assert!(words.accepts(&v(Value::Ident("a".into()))));
        assert!(!words.accepts(&v(Value::Ident("b".into()))));
        assert!(!words.accepts(&v(Value::None)));
    }
}
