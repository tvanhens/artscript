//! The evaluator: modules and declarations, the drawing statements, components with slots and
//! patches, and expressions. Produces the scene IR described in `artscript-ir`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use artscript_ir::{
    Blend, Cap, Clip, Join, Mask, MaskSource, Matrix, Node, NodeKind, Paint, Path,
    Point, Rect, Shape, Stroke,
};
use artscript_syntax::{format_expr, Diagnostic};
use artscript_syntax::ast::{
    self, BinOp, DeclKind, DefItem, Draw, DrawBody, DrawItem, Expr, File, Item, Named, Scene, SceneKind, Span, Swatch, SwatchKind,
    UnOp,
};

use crate::checks::{CheckState, Pending};
use crate::fx::FxSite;
use crate::color::{self, DEFAULT_HUE_STEP, Ramp, Rgba};
use crate::geom::{self, BoolOp, P};
use crate::paint::{key_color, to_paint};
use crate::project::Project;
use crate::scope::{
    Component, Module, Palette, Record, RefVal, RuleDef, StyleDef, Table, new_table, table_get, table_names,
};
use crate::seed::{self, Rng};
use crate::targets::{MatrixValue, TargetJob, is_entry};
use crate::range::Range;
use crate::value::{Arith, Col, Coord, Kind, Value, arith, equal, num};

/// An evaluation failure, turned into a [`Diagnostic`] at the statement that caused it.
#[derive(Debug)]
pub struct Fail {
    pub code: &'static str,
    pub msg: String,
    pub notes: Vec<String>,
    pub fix: Option<String>,
}

pub type R<T> = Result<T, Fail>;

pub fn fail(code: &'static str, msg: impl Into<String>) -> Fail {
    Fail { code, msg: msg.into(), notes: Vec::new(), fix: None }
}

impl Fail {
    pub fn fix(mut self, f: impl Into<String>) -> Fail {
        self.fix = Some(f.into());
        self
    }

    pub fn note(mut self, n: impl Into<String>) -> Fail {
        self.notes.push(n.into());
        self
    }

    /// Already reported (for example, syntax errors in an imported file).
    fn reported() -> Fail {
        fail("", "")
    }
}

const MEASURES: &[&str] = &["x", "y", "w", "h", "cx", "cy", "left", "right", "top", "bottom", "area"];
const NODE_KEYS: &[&str] = &["at", "rot", "scale", "skew", "pivot", "a", "blend", "z", "name", "style", "feather"];
const CENTERED: &[&str] = &["rect", "circle", "ellipse", "poly", "star", "blob"];

pub(crate) struct Instance {
    pub def_name: String,
    fills: Vec<Fill>,
    pub table: Table,
    used_fills: RefCell<Vec<String>>,
    record: RefCell<Option<Rc<RefCell<Record>>>>,
}

struct Fill {
    name: String,
    body: Vec<Scene>,
    scope: Scope,
    span: Span,
}

pub(crate) struct Host {
    def_name: String,
    table: Table,
}

/// Everything a block of statements is evaluated against.
#[derive(Clone)]
pub(crate) struct Scope {
    pub env: Vec<(String, Value)>,
    pub module: Rc<Module>,
    /// Path of the enclosing node; children are `path.<name>`.
    pub path: String,
    /// `#i` suffixes from enclosing `each`/`repeat`/`grid` copies, appended to new node names.
    pub suffix: String,
    pub frame: Rect,
    /// Canvas from local coordinates.
    pub world: Matrix,
    pub style: Rc<Vec<(String, Value)>>,
    pub register: Table,
    pub lookup: Rc<Vec<Table>>,
    pub inst: Option<Rc<Instance>>,
    host: Option<Rc<Host>>,
    pub checks: Rc<RefCell<Vec<Pending>>>,
    top: bool,
}

impl Scope {
    fn child(&self, path: &str, frame: Rect, world: Matrix) -> Scope {
        Scope { path: path.to_string(), suffix: String::new(), frame, world, top: false, ..self.clone() }
    }

    pub(crate) fn file(&self) -> &str {
        &self.module.path
    }
}

struct PatchRec {
    target: String,
    named: Vec<(String, Value)>,
    off: bool,
    owner: String,
    depth: usize,
    rel: String,
    span: Span,
    file: String,
    matched: Cell<bool>,
}

#[derive(Default)]
struct Siblings {
    items: Vec<(Node, f64)>,
    counts: HashMap<String, usize>,
}

impl Siblings {
    fn name(&mut self, base: &str, suffix: &str) -> String {
        let key = format!("{base}{suffix}");
        let n = self.counts.entry(key).or_insert(0);
        *n += 1;
        if *n == 1 { format!("{base}{suffix}") } else { format!("{base}{n}{suffix}") }
    }

    fn finish(mut self) -> Vec<Node> {
        self.items.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        self.items.into_iter().map(|(n, _)| n).collect()
    }
}

/// Evaluated arguments of one statement. Later named values win.
pub(crate) struct Args {
    pub pos: Vec<Value>,
    pub named: Vec<(String, Value)>,
}

impl Args {
    pub fn get(&self, k: &str) -> Option<&Value> {
        self.named.iter().rev().find(|(n, _)| n == k).map(|(_, v)| v)
    }

    pub fn num(&self, k: &str) -> Option<f64> {
        self.get(k).and_then(Value::num)
    }

    fn word(&self, k: &str) -> Option<String> {
        self.get(k).and_then(|v| v.word().map(str::to_string))
    }
}

pub(crate) struct Evaluator<'a> {
    pub project: &'a Project,
    pub job: &'a TargetJob,
    pub modules: HashMap<String, Rc<Module>>,
    pub parsed: HashMap<String, Rc<File>>,
    loading: Vec<String>,
    pub diags: Vec<Diagnostic>,
    pub canvas: Rect,
    patches: Vec<PatchRec>,
    pub anchors: HashMap<String, Point>,
    pub background: Option<Rgba>,
    pub check: Option<CheckState>,
    seed_base: String,
    seed_count: u64,
}

pub(crate) fn ident_of(e: &Expr) -> Option<&str> {
    match e {
        Expr::Ident(s) => Some(s),
        _ => None,
    }
}

fn mat_trs(at: (f64, f64), pivot: (f64, f64), rot: f64, skew: (f64, f64), scale: (f64, f64)) -> Matrix {
    let f = |v: f64| v as f32;
    Matrix::translate((f(at.0), f(at.1)))
        * Matrix::translate((f(pivot.0), f(pivot.1)))
        * Matrix::rotate_deg(f(rot))
        * Matrix::skew((f(skew.0.to_radians().tan()), f(skew.1.to_radians().tan())))
        * Matrix::scale((f(scale.0), f(scale.1)))
        * Matrix::translate((f(-pivot.0), f(-pivot.1)))
}

fn new_node(path: String, origin: String, transform: Matrix, kind: NodeKind) -> Node {
    Node { path, origin, transform, opacity: 1.0, blend: Blend::Normal, clip: None, mask: None, effects: Vec::new(), kind }
}

fn inverse(m: &Matrix) -> Matrix {
    m.invert().unwrap_or_default()
}

/// A percent of an empty frame would resolve to 0 and hide a missing box.
fn check_percent(coord: Coord, frame: Rect, sc: &Scope) -> R<()> {
    let Coord::Pct(p) = coord else {
        return Ok(());
    };
    if !frame.is_empty() {
        return Ok(());
    }
    let pct = Value::Percent(p).show();
    Err(match sc.inst.as_ref() {
        Some(inst) => fail("E0322", format!("`{}` has no size, so {pct} has nothing to measure", inst.def_name))
            .fix(format!("declare the box with `size:`, as in `def {} size:72x32`", inst.def_name)),
        None => fail("E0322", format!("{pct} has nothing to measure")).fix("use a number, or measure against a frame that has a size"),
    })
}

fn size_box(w: f64, h: f64) -> Rect {
    Rect::from_xywh((-w / 2.0) as f32, (-h / 2.0) as f32, w as f32, h as f32)
}

/// The body sees `size` in its default's kind, so `size:72x32` always reads `size.w`.
fn resolve_size(v: &Value, pair_default: bool, frame: Rect, sc: &Scope, def_name: &str) -> R<(Value, Rect)> {
    let positive = |n: f64| n.is_finite() && n > 0.0;
    let reject = |got: &str| fail("E0320", format!("`size` of `{def_name}` must be positive, got {got}")).fix("write `size:40` or `size:72x32`");
    let square = |side: f64| {
        let value = if pair_default { Value::Size(Coord::Num(side), Coord::Num(side)) } else { Value::Num(side) };
        (value, size_box(side, side))
    };
    let (fw, fh) = (frame.width() as f64, frame.height() as f64);
    match v {
        Value::Num(n) => {
            if !positive(*n) {
                return Err(reject(&v.show()));
            }
            Ok(square(*n))
        }
        Value::Percent(p) => {
            check_percent(Coord::Pct(*p), frame, sc)?;
            let side = p * fw.min(fh);
            if !positive(side) {
                return Err(reject(&v.show()));
            }
            Ok(square(side))
        }
        other => {
            let Some((x, y)) = other.pair() else {
                return Err(fail("E0320", format!("`size` of `{def_name}` expects a number or a size, got {}", v.kind()))
                    .fix("write `size:40` or `size:72x32`"));
            };
            check_percent(x, frame, sc)?;
            check_percent(y, frame, sc)?;
            let (w, h) = (x.len(fw), y.len(fh));
            if !positive(w) || !positive(h) {
                return Err(reject(&v.show()));
            }
            Ok((Value::Size(Coord::Num(w), Coord::Num(h)), size_box(w, h)))
        }
    }
}

/// Parent units per local unit, along the more-stretched axis.
fn axis_scale(m: &Matrix) -> f64 {
    let sx = f64::from(m.scale_x()).hypot(f64::from(m.skew_y()));
    let sy = f64::from(m.scale_y()).hypot(f64::from(m.skew_x()));
    sx.max(sy)
}

fn each_items(it: Value) -> R<Vec<Value>> {
    if let Value::Range(r) = &it {
        let prefix: Vec<Value> = r.steps().take(100_001).map(Value::Num).collect();
        if prefix.len() > 100_000 {
            return Err(fail("E0306", "`each` is limited to 100000 iterations"));
        }
        return Ok(prefix);
    }
    let items = it.items().ok_or_else(|| {
        fail("E0306", format!("`each` needs a list or range, got {}", it.kind())).fix("write `each x in [a b]` or `each i in 0..8`")
    })?;
    if items.len() > 100_000 {
        return Err(fail("E0306", "`each` is limited to 100000 iterations"));
    }
    Ok(items)
}

fn stroke_expr(named: &[Named], style: Option<&str>, sc: &Scope) -> Option<Expr> {
    if let Some(n) = named.iter().rev().find(|n| n.key == "stroke") {
        return Some(n.value.clone());
    }
    let sname = style?;
    let styles = sc.module.styles.borrow();
    styles.get(sname).and_then(|s| s.named.iter().rev().find(|n| n.key == "stroke").map(|n| n.value.clone()))
}

fn range_width(stroke: Option<&Expr>) -> Fail {
    let rewrite = match stroke {
        Some(Expr::Range(a, b)) => format!("stroke:[{} {}]", format_expr(a), format_expr(b)),
        Some(Expr::Tuple(xs)) if xs.len() == 2 => match &xs[1] {
            Expr::Range(a, b) => format!("stroke:{},[{} {}]", format_expr(&xs[0]), format_expr(a), format_expr(b)),
            _ => "stroke:paint,[2 6]".into(),
        },
        // An inherited style has already been evaluated, so the source spelling is gone.
        _ => "stroke:paint,[2 6]".into(),
    };
    fail("E0320", "a range is not a stroke width").fix(format!("write `{rewrite}`"))
}

fn blend_of(w: &str) -> Option<Blend> {
    Some(match w {
        "normal" => Blend::Normal,
        "multiply" => Blend::Multiply,
        "screen" => Blend::Screen,
        "overlay" => Blend::Overlay,
        "darken" => Blend::Darken,
        "lighten" => Blend::Lighten,
        "color_dodge" | "dodge" => Blend::ColorDodge,
        "color_burn" | "burn" => Blend::ColorBurn,
        "hard_light" => Blend::HardLight,
        "soft_light" => Blend::SoftLight,
        "difference" => Blend::Difference,
        "exclusion" => Blend::Exclusion,
        "hue" => Blend::Hue,
        "saturation" => Blend::Saturation,
        "color" => Blend::Color,
        "luminosity" => Blend::Luminosity,
        "add" | "plus" => Blend::Plus,
        "erase" => Blend::Erase,
        _ => return None,
    })
}

/// The first shape color under a node, used for effect defaults.
fn first_fill(n: &Node) -> Option<Rgba> {
    match &n.kind {
        NodeKind::Shape(s) => s.fill.as_ref().or(s.stroke.as_ref().map(|st| &st.paint)).map(key_color),
        NodeKind::Group { children, .. } => children.iter().find_map(first_fill),
    }
}

fn rename(node: &mut Node, from: &str, to: &str) {
    if let Some(rest) = node.path.strip_prefix(from) {
        node.path = format!("{to}{rest}");
    }
    match &mut node.kind {
        NodeKind::Shape(s) => s.children.iter_mut().for_each(|c| rename(c, from, to)),
        NodeKind::Group { children, .. } => children.iter_mut().for_each(|c| rename(c, from, to)),
    }
}

impl<'a> Evaluator<'a> {
    pub fn new(project: &'a Project, job: &'a TargetJob) -> Self {
        Evaluator {
            project,
            job,
            modules: HashMap::new(),
            parsed: HashMap::new(),
            loading: Vec::new(),
            diags: Vec::new(),
            canvas: Rect::from_wh(job.size.0, job.size.1),
            patches: Vec::new(),
            anchors: HashMap::new(),
            background: None,
            check: None,
            seed_base: String::new(),
            seed_count: 0,
        }
    }

    pub fn report(&mut self, f: Fail, file: &str, span: Span, node: &str) {
        if f.code.is_empty() {
            return;
        }
        let mut d = if f.code.starts_with('L') || f.code == "C0201" {
            Diagnostic::warning(f.code, span, f.msg)
        } else {
            Diagnostic::error(f.code, span, f.msg)
        };
        d.notes = f.notes;
        d.fix = f.fix;
        self.diags.push(d.in_file(file).at_node(node));
    }

    fn seed_at(&mut self, base: String) {
        self.seed_base = base;
        self.seed_count = 0;
    }

    /// Seed for `rand`/`pick`: the statement's file and path, plus how many draws came before.
    pub fn next_seed(&mut self, extra: u64) -> u64 {
        self.seed_count += 1;
        seed::node_seed(&self.seed_base, &self.seed_count.to_string(), extra)
    }

    // ---------------------------------------------------------------- modules

    pub fn parse(&mut self, file: &str) -> R<Rc<File>> {
        if let Some(f) = self.parsed.get(file) {
            return Ok(f.clone());
        }
        let src = self.project.files.get(file).ok_or_else(|| fail("E0210", format!("no file `{file}`")))?;
        match artscript_syntax::parse(src) {
            Ok(f) => {
                let f = Rc::new(f);
                self.parsed.insert(file.to_string(), f.clone());
                Ok(f)
            }
            Err(errs) => {
                self.diags.extend(errs.into_iter().map(|d| d.in_file(file)));
                Err(Fail::reported())
            }
        }
    }

    pub fn load_module(&mut self, file: &str, entry: bool) -> R<Rc<Module>> {
        if let Some(m) = self.modules.get(file) {
            return Ok(m.clone());
        }
        if self.loading.iter().any(|f| f == file) {
            return Err(fail("E0211", format!("import cycle through `{file}`"))
                .note(format!("cycle: {} -> {file}", self.loading.join(" -> ")))
                .fix("move the shared declarations into a third library both can import"));
        }
        let parsed = self.parse(file)?;
        self.loading.push(file.to_string());
        let module = Rc::new(Module::new(file));
        if let Some(pal) = self.project.manifest.palette.clone()
            && let Some(pfile) = self.project.resolve_use(&pal)
            && pfile != file
        {
            match self.load_module(&pfile, false) {
                Ok(m) => {
                    module.globals.borrow_mut().insert("p".into(), Value::Module(m));
                }
                Err(f) => self.report(f, file, Span { line: 1, col: 1 }, ""),
            }
        }
        for item in &parsed.items {
            if let Err(f) = self.declare(&module, item, entry) {
                self.report(f, file, item.span(), "");
            }
        }
        self.loading.pop();
        self.modules.insert(file.to_string(), module.clone());
        Ok(module)
    }

    fn declare(&mut self, module: &Rc<Module>, item: &Item, entry: bool) -> R<()> {
        let decl = match item {
            Item::Decl(d) => d,
            Item::Scene(Scene { kind: SceneKind::Let { name, value }, .. }) if !entry => {
                let sc = self.module_scope(module);
                self.seed_at(format!("{}\0{name}", module.path));
                let v = self.expr(value, &sc)?;
                module.define(name, v);
                return Ok(());
            }
            Item::Scene(_) if entry => return Ok(()),
            Item::Scene(_) => {
                return Err(fail("E0204", "a library may only hold declarations at its top level")
                    .note("allowed: use, palette, style, finish, def, rule, let")
                    .fix("wrap the drawing in a `def` and place it from an entry file"));
            }
        };
        match &decl.kind {
            DeclKind::Use { path, alias, items } => self.declare_use(module, path, alias, items),
            DeclKind::Palette { name, from, body } => {
                let pal = self.palette(module, name, from.as_deref(), body)?;
                module.palettes.borrow_mut().push(pal.clone());
                module.define(name, Value::Palette(pal));
                Ok(())
            }
            DeclKind::Style { name, named } => {
                module
                    .styles
                    .borrow_mut()
                    .insert(name.clone(), Rc::new(StyleDef { module: module.path.clone(), named: named.clone() }));
                module.own.borrow_mut().push(format!("style:{name}"));
                Ok(())
            }
            DeclKind::Finish { name, params, body } => self.declare_finish(module, name, params, body, decl.span),
            DeclKind::Def { name, params, body } => {
                let mut needs = Vec::new();
                let mut ps = Vec::new();
                for p in params {
                    if p.key == "needs" {
                        let refs = match &p.value {
                            Expr::Tuple(xs) | Expr::List(xs) => xs.clone(),
                            other => vec![other.clone()],
                        };
                        for r in refs {
                            if let Expr::Ref(segs) = r {
                                needs.push(segs.join("."));
                            }
                        }
                    } else {
                        ps.push(p.clone());
                    }
                }
                let comp = Component {
                    name: name.clone(),
                    params: ps,
                    needs,
                    body: body.clone(),
                    module: module.path.clone(),
                    span: decl.span,
                };
                module.define(name, Value::Component(Rc::new(comp)));
                Ok(())
            }
            DeclKind::Rule { name, params, body } => {
                let rule = RuleDef {
                    name: name.clone(),
                    params: params.clone(),
                    body: body.clone(),
                    module: module.path.clone(),
                    span: decl.span,
                };
                module.define(name, Value::Rule(Rc::new(rule)));
                Ok(())
            }
            DeclKind::Shader { .. } | DeclKind::Light { .. } | DeclKind::Brush { .. } => {
                let what = match &decl.kind {
                    DeclKind::Shader { .. } => "shader (Phase 2)",
                    DeclKind::Light { .. } => "light (Phase 2)",
                    _ => "brush (Phase 2)",
                };
                Err(fail("E0390", format!("{what} is not yet implemented in v0.1"))
                    .fix("remove the declaration, or use built-in fields and fx lines"))
            }
        }
    }

    fn declare_use(&mut self, module: &Rc<Module>, path: &str, alias: &Option<String>, items: &[String]) -> R<()> {
        let rooted = format!("{path}.art");
        let target = match self.project.resolve_use(path) {
            Some(f) => f,
            None if self.project.files.contains_key(&rooted) => rooted,
            None => {
                return Err(fail("E0210", format!("no library `{path}`"))
                    .note(format!("libraries: {}", self.project.library_names().join(", ")))
                    .fix(format!("create `{}/{path}.art`, or fix the path", self.project.manifest.lib[0])));
            }
        };
        let parsed = self.parse(&target)?;
        if is_entry(&parsed) {
            return Err(fail("E0201", format!("`{target}` is an entry file and cannot be imported"))
                .fix("move the shared code into a library under a lib root and `use` that instead"));
        }
        let m = self.load_module(&target, false)?;
        if let Some(a) = alias {
            module.globals.borrow_mut().insert(a.clone(), Value::Module(m));
            return Ok(());
        }
        let exports = m.exports();
        let styles = m.styles.borrow().clone();
        let finishes = m.finishes.borrow().clone();
        if items.is_empty() {
            for (k, v) in exports {
                module.globals.borrow_mut().insert(k, v);
            }
            for (k, s) in styles.into_iter().filter(|(k, _)| !k.starts_with('_')) {
                module.styles.borrow_mut().insert(k, s);
            }
            for (k, f) in finishes.into_iter().filter(|(k, _)| !k.starts_with('_')) {
                module.finishes.borrow_mut().insert(k, f);
            }
            return Ok(());
        }
        for item in items {
            let mut found = false;
            if let Some((_, v)) = exports.iter().find(|(k, _)| k == item) {
                module.globals.borrow_mut().insert(item.clone(), v.clone());
                found = true;
            }
            if !item.starts_with('_')
                && let Some(s) = styles.get(item)
            {
                module.styles.borrow_mut().insert(item.clone(), s.clone());
                found = true;
            }
            if !item.starts_with('_')
                && let Some(f) = finishes.get(item)
            {
                module.finishes.borrow_mut().insert(item.clone(), f.clone());
                found = true;
            }
            if !found {
                let names: Vec<String> = exports.iter().map(|(k, _)| k.clone()).collect();
                return Err(fail("E0212", format!("`{path}` does not export `{item}`"))
                    .note(format!("exports: {}", names.join(", ")))
                    .fix("import one of the exported names (names starting with `_` are private)"));
            }
        }
        Ok(())
    }

    pub(crate) fn module_scope(&self, module: &Rc<Module>) -> Scope {
        let t = new_table();
        Scope {
            env: Vec::new(),
            module: module.clone(),
            path: String::new(),
            suffix: String::new(),
            frame: Rect::new_empty(),
            world: Matrix::new_identity(),
            style: Rc::new(Vec::new()),
            register: t.clone(),
            lookup: Rc::new(vec![t]),
            inst: None,
            host: None,
            checks: Rc::new(RefCell::new(Vec::new())),
            top: false,
        }
    }

    pub fn top_scope(&self, module: &Rc<Module>) -> Scope {
        Scope { frame: self.canvas, top: true, ..self.module_scope(module) }
    }

    fn palette(&mut self, module: &Rc<Module>, name: &str, from: Option<&str>, body: &[Swatch]) -> R<Rc<Palette>> {
        let mut entries = Vec::new();
        if let Some(parent) = from {
            match module.global(parent) {
                Some(Value::Palette(p)) => entries.extend(p.entries.iter().cloned()),
                _ => {
                    return Err(fail("E0304", format!("no palette `{parent}` to inherit from"))
                        .fix("define or import the parent palette first"));
                }
            }
        }
        let sc = self.module_scope(module);
        for sw in body {
            match self.swatch(&sw.kind, &sc) {
                Ok(v) => entries.push((sw.name.clone(), v)),
                Err(f) => self.report(f, &module.path, sw.span, &format!("{name}.{}", sw.name)),
            }
        }
        Ok(Rc::new(Palette { name: name.to_string(), entries }))
    }

    fn swatch(&mut self, kind: &SwatchKind, sc: &Scope) -> R<Value> {
        match kind {
            SwatchKind::Ramp { pos, hue } => {
                let vals: Vec<Value> = pos.exprs().into_iter().map(|e| self.expr(e, sc)).collect::<R<_>>()?;
                let hue = match hue {
                    Some(e) => self.expr(e, sc)?.num().unwrap_or(DEFAULT_HUE_STEP),
                    None => DEFAULT_HUE_STEP,
                };
                self.ramp(&vals, hue)
            }
            SwatchKind::Color(e) => {
                let v = self.expr(e, sc)?;
                match v.kind() {
                    Kind::Color => Ok(v),
                    k => Err(fail("E0305", format!("`color` takes a color, got {k}"))
                        .fix("write `color name #rrggbb`, or `ramp name #rrggbb 5` for a ramp")),
                }
            }
        }
    }

    pub fn ramp(&mut self, vals: &[Value], hue: f64) -> R<Value> {
        let colors: Vec<Rgba> = vals.iter().filter_map(|v| v.color().map(|c| c.rgba)).collect();
        let n = vals.iter().rev().find_map(Value::num).unwrap_or(5.0).max(1.0) as usize;
        let ramp = match colors.as_slice() {
            [c] => Ramp::single(*c, n, hue),
            [a, b] => Ramp::two(*a, *b, n),
            _ => return Err(fail("E0305", "`ramp` takes one or two colors and a count").fix("write `ramp #f3a2a6 5`")),
        };
        Ok(Value::Ramp(Rc::new(ramp)))
    }

    // ---------------------------------------------------------------- statements

    pub fn eval_block_nodes<'s>(&mut self, stmts: impl IntoIterator<Item = &'s Scene>, sc: &mut Scope) -> Vec<Node> {
        let mut sib = Siblings::default();
        self.eval_block(stmts, sc, &mut sib);
        sib.finish()
    }

    fn eval_block<'s>(&mut self, stmts: impl IntoIterator<Item = &'s Scene>, sc: &mut Scope, out: &mut Siblings) {
        for st in stmts {
            if let Err(f) = self.eval_stmt(st, sc, out) {
                let node = sc.path.clone();
                let file = sc.file().to_string();
                self.report(f, &file, st.span, &node);
            }
        }
    }

    fn eval_stmt(&mut self, st: &Scene, sc: &mut Scope, out: &mut Siblings) -> R<()> {
        match &st.kind {
            SceneKind::Let { name, value } => {
                self.seed_at(format!("{}\0{}{}\0{}", sc.file(), sc.path, sc.suffix, st.span.line));
                let v = self.expr(value, sc);
                let v = match v {
                    Ok(v) => v,
                    Err(f) => {
                        sc.env.push((name.clone(), Value::None));
                        return Err(f);
                    }
                };
                if sc.top {
                    sc.module.globals.borrow_mut().insert(name.clone(), v.clone());
                }
                sc.env.push((name.clone(), v));
                Ok(())
            }
            SceneKind::If { cond, then, otherwise } => {
                self.seed_at(format!("{}\0{}{}\0{}", sc.file(), sc.path, sc.suffix, st.span.line));
                let c = self.expr(cond, sc)?;
                let mut inner = sc.clone();
                self.eval_block(if c.truthy() { then } else { otherwise }, &mut inner, out);
                Ok(())
            }
            SceneKind::Each { var, iter, body } => {
                self.seed_at(format!("{}\0{}{}\0{}", sc.file(), sc.path, sc.suffix, st.span.line));
                let items = each_items(self.expr(iter, sc)?)?;
                for (i, v) in items.into_iter().enumerate() {
                    let mut inner = sc.clone();
                    inner.env.push((var.clone(), v));
                    inner.suffix = format!("{}#{i}", sc.suffix);
                    self.eval_block(body, &mut inner, out);
                }
                Ok(())
            }
            SceneKind::Check { .. } => {
                sc.checks.borrow_mut().push(Pending { stmt: st.clone(), scope: sc.clone() });
                Ok(())
            }
            SceneKind::Slot { name, named, fallback } => self.slot(st, name, named, fallback, sc, out),
            SceneKind::Draw(draw) => self.eval_node(st, draw, sc, out),
        }
    }

    pub(crate) fn eval_args(&mut self, args: &[Expr], named: &[Named], sc: &Scope, skip_pos: usize) -> R<Args> {
        let mut out = Args { pos: Vec::new(), named: Vec::new() };
        for e in args.iter().skip(skip_pos) {
            out.pos.push(self.expr(e, sc)?);
        }
        for n in named {
            if n.key == "name" {
                continue;
            }
            let v = self.expr(&n.value, sc)?;
            out.named.push((n.key.clone(), v));
        }
        Ok(out)
    }

    fn style_named(&mut self, name: &str, sc: &Scope) -> R<Vec<(String, Value)>> {
        let style = sc.module.styles.borrow().get(name).cloned().ok_or_else(|| {
            let names: Vec<String> = sc.module.styles.borrow().keys().cloned().collect();
            let f = fail("E0307", format!("no style `{name}`")).note(format!("styles: {}", names.join(", ")));
            if sc.module.finishes.borrow().contains_key(name) {
                f.note(format!("`{name}` is a finish")).fix(format!("write `fx {name}` under the node"))
            } else {
                f.fix("declare `style name key:value` or import it")
            }
        })?;
        let module = self.modules.get(&style.module).cloned().unwrap_or(sc.module.clone());
        let msc = self.module_scope(&module);
        style.named.iter().map(|n| Ok((n.key.clone(), self.expr(&n.value, &msc)?))).collect()
    }

    fn transform(&mut self, a: &Args, default_at: (f64, f64), frame: Rect, sc: &Scope, with_rot: bool) -> R<(Matrix, (f64, f64))> {
        let (fx, fy, fw, fh) = (frame.left as f64, frame.top as f64, frame.width() as f64, frame.height() as f64);
        let at = match a.get("at") {
            Some(v) => {
                let (x, y) = v.pair().ok_or_else(|| fail("E0320", format!("`at:` expects a vector like 4,2, got {}", v.kind())))?;
                check_percent(x, frame, sc)?;
                check_percent(y, frame, sc)?;
                (x.pos(fx, fw), y.pos(fy, fh))
            }
            None => default_at,
        };
        let pivot = match a.get("pivot").and_then(Value::pair) {
            Some((x, y)) => {
                check_percent(x, frame, sc)?;
                check_percent(y, frame, sc)?;
                (x.len(fw), y.len(fh))
            }
            None => (0.0, 0.0),
        };
        let rot = if with_rot { a.num("rot").unwrap_or(0.0) } else { 0.0 };
        let scale = a.get("scale").and_then(Value::vec2).unwrap_or((1.0, 1.0));
        let skew = match a.get("skew") {
            Some(Value::Num(k)) => (*k, 0.0),
            Some(v) => v.vec2().unwrap_or((0.0, 0.0)),
            None => (0.0, 0.0),
        };
        Ok((mat_trs(at, pivot, rot, skew, scale), at))
    }

    /// Opacity, blend and z order shared by every node.
    fn finish_node(&mut self, node: &mut Node, a: &Args) -> R<f64> {
        if let Some(op) = a.num("a") {
            node.opacity = op.clamp(0.0, 1.0) as f32;
        }
        if let Some(b) = a.word("blend") {
            node.blend = blend_of(&b).ok_or_else(|| {
                fail("E0320", format!("unknown blend `{b}`")).fix("use normal, multiply, screen, overlay, soft_light, add, erase, …")
            })?;
        }
        Ok(match a.get("z") {
            Some(Value::Ident(w)) if w == "behind" => f64::NEG_INFINITY,
            Some(Value::Ident(w)) if w == "front" => f64::INFINITY,
            Some(Value::Num(n)) => *n,
            Some(other) => return Err(fail("E0320", format!("`z:` expects behind, front or a number, got {}", other.show()))),
            None => 0.0,
        })
    }

    fn take_patches(&self, path: &str) -> (Vec<(String, Value)>, bool) {
        let mut hits: Vec<(usize, usize)> =
            self.patches.iter().enumerate().filter(|(_, p)| p.target == path).map(|(i, p)| (p.depth, i)).collect();
        hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        let mut named = Vec::new();
        let mut off = false;
        for (_, i) in hits {
            let p = &self.patches[i];
            p.matched.set(true);
            off |= p.off;
            named.extend(p.named.iter().cloned());
        }
        (named, off)
    }

    fn register(&self, sc: &Scope, name: &str, record: Record) -> Rc<RefCell<Record>> {
        let r = Rc::new(RefCell::new(record));
        sc.register.borrow_mut().push((name.to_string(), r.clone()));
        r
    }

    fn eval_node(&mut self, st: &Scene, draw: &Draw, sc: &mut Scope, out: &mut Siblings) -> R<()> {
        let Draw { verb, args, named, body } = draw;
        let verb = verb.as_str();
        match verb {
            "anchor" => return self.anchor(args, sc),
            "canvas" if sc.top => {
                let a = self.eval_args(args, named, sc, 0)?;
                if let Some(v) = a.get("fill") {
                    self.background = v.color().map(|c| c.rgba);
                }
                return Ok(());
            }
            _ => {}
        }
        let comp = self.component_for(verb, sc);
        if matches!(comp, Some(None)) {
            return Ok(());
        }
        let known = matches!(
            verb,
            "rect" | "circle" | "ellipse" | "poly" | "star" | "curve" | "path" | "blob" | "copy" | "subtract" | "union"
                | "intersect" | "xor" | "layer" | "group" | "inside" | "outside" | "mirror" | "repeat" | "grid"
        );
        if comp.is_none() && !known {
            let mut f = fail("E0301", format!("unknown verb `{verb}`"))
                .note("shapes: rect circle ellipse poly star curve path blob copy subtract union intersect xor")
                .note("structure: layer group inside outside mirror repeat grid anchor slot");
            let comps: Vec<String> = sc
                .module
                .globals
                .borrow()
                .iter()
                .filter(|(_, v)| matches!(v, Value::Component(_)))
                .map(|(k, _)| k.clone())
                .collect();
            if !comps.is_empty() {
                f = f.note(format!("components in scope: {}", comps.join(", ")));
            }
            return Err(f.fix(format!("check the spelling, or `use` the library that defines `{verb}`")));
        }
        if let Some(n) = named.iter().find(|n| n.key == "finish") {
            let name = ident_of(&n.value).unwrap_or("<name>");
            return Err(fail("E0339", "`finish:` is not a node key").fix(format!("write `fx {name}` under the node")));
        }
        let explicit = named.iter().find(|n| n.key == "name").and_then(|n| ident_of(&n.value)).map(str::to_string);
        let positional_name = matches!(verb, "layer" | "group").then(|| args.first().and_then(ident_of).map(str::to_string)).flatten();
        let comp = comp.flatten();
        let base = explicit
            .clone()
            .or(positional_name.clone())
            .or_else(|| comp.as_ref().map(|c| c.name.clone()))
            .unwrap_or_else(|| verb.to_string());
        let registered = explicit.is_some() || positional_name.is_some() || comp.is_some();
        let leaf = out.name(&base, &sc.suffix);
        let path = if sc.path.is_empty() { leaf.clone() } else { format!("{}.{leaf}", sc.path) };
        let (patch_named, off) = self.take_patches(&path);
        let patch_stroke = patch_named.iter().any(|(k, _)| k == "stroke");
        if off {
            return Ok(());
        }
        self.seed_at(format!("{}\0{path}", sc.file()));
        let skip = usize::from(positional_name.is_some());
        let raw_flags: Vec<String> = args.iter().filter_map(ident_of).map(str::to_string).collect();
        let flag = |f: &str| raw_flags.iter().any(|x| x == f);
        let pos_exprs: Vec<Expr> = args
            .iter()
            .skip(skip)
            .filter(|e| !(matches!(ident_of(e), Some("closed")) || (verb == "mirror" && matches!(ident_of(e), Some("x" | "y")))))
            .cloned()
            .collect();
        let mut a = self.eval_args(&pos_exprs, named, sc, 0)?;
        let expanded_style = a.word("style");
        if let Some(sname) = &expanded_style {
            let mut s = self.style_named(sname, sc)?;
            s.append(&mut a.named);
            a.named = s;
        }
        a.named.extend(patch_named);
        let origin = format!("{}:{}", sc.file(), st.span.line);
        let name = registered.then(|| explicit.clone().or(positional_name.clone()).unwrap_or(base.clone()));

        if let Some(comp) = comp {
            return self.instance(&comp, a, body, sc, out, path, origin, name);
        }
        for item in body.items() {
            let err = match item {
                DrawItem::SlotFill(fill) => (
                    fill.span,
                    fail("E0331", format!("slot fill `{}:` is only valid under a component call", fill.name))
                        .fix("indent it under the component that exposes the slot"),
                ),
                DrawItem::Patch(p) => (
                    p.span,
                    fail("E0332", format!("patch `.{}` is only valid under a component call", p.path.join(".")))
                        .fix("indent it under the component instance it overrides"),
                ),
                _ => continue,
            };
            let file = sc.file().to_string();
            self.report(err.1, &file, err.0, &path);
        }
        let child_style = match a.word("style") {
            Some(_) => {
                let mut s = (*sc.style).clone();
                s.extend(a.named.iter().filter(|(k, _)| is_paint_key(k)).cloned());
                Rc::new(s)
            }
            None => sc.style.clone(),
        };

        let centered = CENTERED.contains(&verb);
        let group_like = matches!(verb, "layer" | "group" | "inside" | "outside" | "mirror" | "repeat" | "grid");
        let frame = sc.frame;
        let default_at = if centered { (frame.center_x() as f64, frame.center_y() as f64) } else { (0.0, 0.0) };
        let (m, at) = self.transform(&a, default_at, frame, sc, verb != "repeat")?;
        let world = sc.world * m;
        let mut node;
        let mut record = None;
        if group_like {
            let local_frame = inverse(&m).map_rect(frame).0;
            let mut inner = sc.child(&path, local_frame, world);
            inner.style = child_style;
            let mut clip = None;
            if matches!(verb, "inside" | "outside") {
                let r = match a.pos.first() {
                    Some(Value::Ref(r)) => r.clone(),
                    _ => return Err(fail("E0320", format!("`{verb}` needs a node reference")).fix(format!("write `{verb} @torso`"))),
                };
                let to_local = inverse(&world);
                let sil = r.silhouette().make_transform(&to_local);
                inner.frame = geom::bounds(&sil);
                clip = Some(Clip { path: sil, feather: a.num("feather").unwrap_or(0.0) as f32, invert: verb == "outside" });
            }
            let placeholder = new_node(path.clone(), origin.clone(), m, NodeKind::Group { isolate: verb == "layer", children: vec![] });
            if let Some(n) = &name {
                record = Some(self.register(sc, n, Record { name: n.clone(), path: path.clone(), parent_world: sc.world, node: Some(placeholder), anchor: None, at }));
            }
            let children = match verb {
                "mirror" => self.mirror(body, &mut inner, if flag("y") { (1.0, -1.0) } else { (-1.0, 1.0) }),
                "repeat" => self.repeat(body, &mut inner, &a)?,
                "grid" => self.grid(body, &mut inner, &a)?,
                _ => self.eval_block_nodes(body.scenes(), &mut inner),
            };
            node = new_node(path.clone(), origin, m, NodeKind::Group { isolate: verb == "layer", children });
            node.clip = clip;
        } else {
            let inner_world = world;
            let (geometry, knots, closed) = match verb {
                "subtract" | "union" | "intersect" | "xor" => {
                    let op = match verb {
                        "subtract" => BoolOp::Subtract,
                        "union" => BoolOp::Union,
                        "intersect" => BoolOp::Intersect,
                        _ => BoolOp::Xor,
                    };
                    let local_frame = inverse(&m).map_rect(frame).0;
                    let mut inner = sc.child(&path, local_frame, inner_world);
                    let operands = self.eval_block_nodes(body.scenes(), &mut inner);
                    let sils: Vec<Path> = operands.iter().map(|n| crate::scope::silhouette(n, &Matrix::new_identity())).collect();
                    (geom::boolean(op, &sils), None, true)
                }
                "copy" => {
                    let r = match a.pos.first() {
                        Some(Value::Ref(r)) => r.clone(),
                        _ => return Err(fail("E0320", "`copy` needs a node reference").fix("write `copy @body`")),
                    };
                    (r.silhouette().make_transform(&inverse(&sc.world)), None, true)
                }
                _ => self.primitive(verb, &a, frame, sc, flag("closed"))?,
            };
            let mut geometry = geometry;
            if let Some(spec) = a.get("jitter") {
                let (amount, wavelength) = match spec {
                    Value::Num(amp) => {
                        return Err(fail("E0320", "`jitter:` takes amplitude and wavelength").fix(format!("write `jitter:{},60`", num(*amp))));
                    }
                    other => other.vec2().ok_or_else(|| {
                        fail("E0320", format!("`jitter:` takes amplitude,wavelength, got {}", other.kind())).fix("write `jitter:1.5,60`")
                    })?,
                };
                if amount != 0.0 {
                    if wavelength <= 0.0 {
                        return Err(fail("E0320", "`jitter:` wavelength must be positive").fix("write `jitter:1.5,60`"));
                    }
                    let extra = a.num("seed").unwrap_or(0.0) as u64;
                    let mut rng = Rng::new(seed::node_seed(sc.file(), &format!("{path}#jitter"), extra));
                    geometry = geom::wobble(&geometry, amount, &mut rng, wavelength);
                }
            }
            // A patch is stored as a value. A stroke it supplies has no spelling, and a style it
            // renames is not the style that was expanded above.
            let stroke_src = if patch_stroke { None } else { stroke_expr(named, expanded_style.as_deref(), sc) };
            let shape = self.paint_shape(&a, geometry, knots.as_deref(), closed, &sc.style, &world, stroke_src.as_ref())?;
            node = new_node(path.clone(), origin, m, NodeKind::Shape(Box::new(shape)));
            if let Some(n) = &name {
                record = Some(self.register(sc, n, Record { name: n.clone(), path: path.clone(), parent_world: sc.world, node: Some(node.clone()), anchor: None, at }));
            }
            if !matches!(verb, "subtract" | "union" | "intersect" | "xor") && body.scenes().next().is_some() {
                let gb = match &node.kind {
                    NodeKind::Shape(s) => geom::bounds(&s.geometry),
                    _ => Rect::new_empty(),
                };
                let mut inner = sc.child(&path, gb, inner_world);
                inner.style = child_style;
                let kids = self.eval_block_nodes(body.scenes(), &mut inner);
                if let NodeKind::Shape(s) = &mut node.kind {
                    s.children = kids;
                }
            }
        }
        let fill_key = first_fill(&node).unwrap_or(Rgba::GRAY);
        let fx_scope = sc.child(&path, sc.frame, world);
        let sites: Vec<FxSite> = body.fx().map(|line| FxSite { line, scope: &fx_scope }).collect();
        node.effects = self.lower_fx(&sites, &path, fill_key);
        self.mask(&mut node, &a, body.mask(), sc, world)?;
        let z = self.finish_node(&mut node, &a)?;
        if let Some(r) = record {
            r.borrow_mut().node = Some(node.clone());
        }
        out.items.push((node, z));
        Ok(())
    }

    fn component_for(&self, verb: &str, sc: &Scope) -> Option<Option<Rc<Component>>> {
        let v = sc.env.iter().rev().find(|(k, _)| k == verb).map(|(_, v)| v.clone()).or_else(|| sc.module.global(verb))?;
        match v {
            Value::Component(c) => Some(Some(c)),
            Value::None => Some(None),
            _ => None,
        }
    }

    fn mask(&mut self, node: &mut Node, a: &Args, mask: Option<&ast::Mask>, sc: &Scope, world: Matrix) -> R<()> {
        let feather = a.num("feather").unwrap_or(0.0) as f32;
        let source = match mask {
            None => None,
            Some(ast::Mask::Block(body)) => {
                let mut inner = sc.child(&format!("{}.mask", node.path), sc.frame, world);
                Some(MaskSource::Nodes(self.eval_block_nodes(body, &mut inner)))
            }
            Some(ast::Mask::Value(e)) => match self.expr(e, sc)? {
                Value::None => None,
                Value::Ref(r) => {
                    let geometry = r.silhouette().make_transform(&inverse(&world));
                    let shape = Shape { geometry, fill: Some(Paint::Solid(Rgba { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }.to_ir())), stroke: None, blur: 0.0, children: vec![] };
                    let n = new_node(format!("{}.mask", node.path), node.origin.clone(), Matrix::new_identity(), NodeKind::Shape(Box::new(shape)));
                    Some(MaskSource::Nodes(vec![n]))
                }
                Value::FieldMask(f, t) => Some(MaskSource::Field { field: (*f).clone(), threshold: Some(t as f32) }),
                Value::Field(f) => Some(MaskSource::Field { field: (*f).clone(), threshold: None }),
                other => {
                    return Err(fail("E0320", format!("`mask` expects a node reference or a field, got {}", other.kind()))
                        .fix("write `mask @body` or `mask simplex(30) > .5`"));
                }
            },
        };
        if let Some(source) = source {
            node.mask = Some(Box::new(Mask { source, luminance: false, feather, invert: false }));
        }
        Ok(())
    }

    fn anchor(&mut self, args: &[Expr], sc: &Scope) -> R<()> {
        let name = args.first().and_then(ident_of).ok_or_else(|| fail("E0320", "`anchor` needs a name").fix("write `anchor neck 0,-4`"))?;
        let at = match args.get(1) {
            Some(e) => self.expr(e, sc)?.vec2().unwrap_or((0.0, 0.0)),
            None => (0.0, 0.0),
        };
        let p = sc.world.map_point(Point::new(at.0 as f32, at.1 as f32));
        let path = if sc.path.is_empty() { name.to_string() } else { format!("{}.{name}", sc.path) };
        self.anchors.insert(path.clone(), p);
        self.register(sc, name, Record { name: name.to_string(), path, parent_world: sc.world, node: None, anchor: Some(p), at });
        Ok(())
    }

    fn mirror(&mut self, body: &DrawBody, sc: &mut Scope, flip: (f32, f32)) -> Vec<Node> {
        let originals = self.eval_block_nodes(body.scenes(), sc);
        let mut out = originals.clone();
        for n in originals {
            let mut m = n.clone();
            let from = n.path.clone();
            let to = format!("{from}_m");
            rename(&mut m, &from, &to);
            m.transform = Matrix::scale(flip) * n.transform;
            let name = to.rsplit('.').next().unwrap_or(&to).to_string();
            if table_get(&sc.register, from.rsplit('.').next().unwrap_or(&from)).is_some() {
                self.register(sc, &name, Record { name: name.clone(), path: to.clone(), parent_world: sc.world, node: Some(m.clone()), anchor: None, at: (0.0, 0.0) });
            }
            out.push(m);
        }
        out
    }

    fn repeat(&mut self, body: &DrawBody, sc: &mut Scope, a: &Args) -> R<Vec<Node>> {
        let n = a.pos.first().and_then(Value::num).ok_or_else(|| fail("E0320", "`repeat` needs a count").fix("write `repeat 8 rot:45`"))?;
        let rot = a.num("rot").unwrap_or(0.0);
        let step = a.get("step").and_then(Value::vec2).unwrap_or((0.0, 0.0));
        let mut out = Vec::new();
        for i in 0..(n.max(0.0) as usize).min(10_000) {
            let fi = i as f64;
            let copy_m = Matrix::translate(((step.0 * fi) as f32, (step.1 * fi) as f32)) * Matrix::rotate_deg((rot * fi) as f32);
            let mut inner = sc.clone();
            inner.env.push(("i".into(), Value::Num(fi)));
            inner.suffix = format!("{}#{i}", sc.suffix);
            inner.world = sc.world * copy_m;
            for mut node in self.eval_block_nodes(body.scenes(), &mut inner) {
                node.transform = copy_m * node.transform;
                out.push(node);
            }
        }
        Ok(out)
    }

    fn grid(&mut self, body: &DrawBody, sc: &mut Scope, a: &Args) -> R<Vec<Node>> {
        let (cols, rows) = a.pos.first().and_then(Value::vec2).ok_or_else(|| fail("E0320", "`grid` needs columns x rows").fix("write `grid 4x3 gap:8`"))?;
        let (cols, rows) = (cols.max(1.0) as usize, rows.max(1.0) as usize);
        let gap = a.num("gap").unwrap_or(0.0);
        let mut copies = Vec::new();
        for row in 0..rows {
            for col in 0..cols {
                let i = row * cols + col;
                let mut inner = sc.clone();
                inner.env.push(("i".into(), Value::Num(i as f64)));
                inner.env.push(("row".into(), Value::Num(row as f64)));
                inner.env.push(("col".into(), Value::Num(col as f64)));
                inner.suffix = format!("{}#{i}", sc.suffix);
                copies.push(((row, col), self.eval_block_nodes(body.scenes(), &mut inner)));
            }
        }
        let (cw, ch) = match a.get("cell").and_then(Value::vec2) {
            Some(c) => c,
            None => {
                let b = copies.first().and_then(|(_, ns)| artscript_ir::union_bounds(ns)).unwrap_or(Rect::new_empty());
                (b.width() as f64, b.height() as f64)
            }
        };
        let (sx, sy) = (cw + gap, ch + gap);
        let (ox, oy) = ((cols - 1) as f64 * sx / 2.0, (rows - 1) as f64 * sy / 2.0);
        let mut out = Vec::new();
        for ((row, col), nodes) in copies {
            let t = Matrix::translate(((col as f64 * sx - ox) as f32, (row as f64 * sy - oy) as f32));
            for mut n in nodes {
                n.transform = t * n.transform;
                out.push(n);
            }
        }
        Ok(out)
    }

    fn primitive(&mut self, verb: &str, a: &Args, frame: Rect, sc: &Scope, closed_flag: bool) -> R<(Path, Option<Vec<P>>, bool)> {
        let (fx, fy, fw, fh) = (frame.left as f64, frame.top as f64, frame.width() as f64, frame.height() as f64);
        let first = a.pos.first();
        let need = |what: &str, example: &str| fail("E0320", format!("`{verb}` needs {what}")).fix(format!("write `{example}`"));
        Ok(match verb {
            "rect" => {
                let (w, h) = first.and_then(Value::pair).ok_or_else(|| need("a size", "rect 40x20"))?;
                check_percent(w, frame, sc)?;
                check_percent(h, frame, sc)?;
                let radii = match a.get("r") {
                    Some(Value::Num(r)) => [*r; 4],
                    Some(v) => match v.items() {
                        Some(xs) if xs.len() == 4 => {
                            let r: Vec<f64> = xs.iter().map(|x| x.num().unwrap_or(0.0)).collect();
                            [r[0], r[1], r[2], r[3]]
                        }
                        _ => [0.0; 4],
                    },
                    None => [0.0; 4],
                };
                (geom::rect(w.len(fw), h.len(fh), radii), None, true)
            }
            "circle" => {
                let r = first.and_then(Value::coord).ok_or_else(|| need("a radius", "circle 12"))?;
                check_percent(r, frame, sc)?;
                let r = r.len(fw.min(fh));
                (geom::ellipse(r, r), None, true)
            }
            "ellipse" => {
                let (rx, ry) = first.and_then(Value::pair).ok_or_else(|| need("two radii", "ellipse 20,12"))?;
                check_percent(rx, frame, sc)?;
                check_percent(ry, frame, sc)?;
                (geom::ellipse(rx.len(fw), ry.len(fh)), None, true)
            }
            "poly" | "star" => {
                let n = first.and_then(Value::num).ok_or_else(|| need("a point count", "star 5 r:20"))?;
                let r = a.num("r").unwrap_or(10.0);
                let inner = (verb == "star").then(|| a.num("inner").unwrap_or(0.5));
                let mut p = geom::regular(n as usize, r, inner);
                if let Some(round) = a.num("round").filter(|v| *v > 0.0) {
                    p = geom::round_corners(&p, round);
                }
                (p, None, true)
            }
            "curve" => {
                let values: Vec<Value> = match a.pos.as_slice() {
                    [Value::List(xs)] => xs.clone(),
                    _ if a.pos.iter().any(|v| matches!(v, Value::List(_))) => {
                        return Err(fail("E0320", "`curve` takes points, or one list of points")
                            .fix("write `curve pts` or `curve [each k in 0..n -> x,y]`"));
                    }
                    _ => a.pos.clone(),
                };
                let mut pts = Vec::with_capacity(values.len());
                for v in &values {
                    let (x, y) = v.pair().ok_or_else(|| need("points like 0,0 20,-15", "curve 0,0 20,-15 40,0"))?;
                    check_percent(x, frame, sc)?;
                    check_percent(y, frame, sc)?;
                    pts.push((x.pos(fx, fw), y.pos(fy, fh)));
                }
                if pts.len() < 2 {
                    return Err(need("at least two points", "curve 0,0 20,-15 40,0"));
                }
                let closed = closed_flag && pts.len() >= 3;
                let tension = a.num("tension").unwrap_or(0.5);
                let path = if closed && tension == 0.0 && let Some(round) = a.num("round").filter(|v| *v > 0.0) {
                    // A tension-0 spline is cubics. Skia's corner effect only rounds line joins,
                    // so the straight sides are a polygon before the same effect `poly` uses.
                    geom::round_corners(&geom::polygon(&pts), round)
                } else {
                    geom::spline(&pts, tension, closed)
                };
                (path, Some(pts), closed)
            }
            "path" => {
                let d = first.and_then(|v| match v {
                    Value::Str(s) => Some(s.clone()),
                    _ => None,
                });
                let d = d.ok_or_else(|| need("SVG path data", "path \"M0 0 L10 0 Z\""))?;
                let p = skia_safe::utils::parse_path::from_svg(&d).ok_or_else(|| fail("E0320", "invalid SVG path data").fix("check the path string"))?;
                (p, None, true)
            }
            "blob" => {
                let r = first.and_then(Value::coord).ok_or_else(|| need("a radius", "blob 30 seed:4 wobble:.2"))?;
                check_percent(r, frame, sc)?;
                let r = r.len(fw.min(fh));
                let extra = a.num("seed").unwrap_or(0.0) as u64;
                let mut rng = Rng::new(seed::node_seed(&self.seed_base, "blob", extra));
                let points = a.num("points").unwrap_or(7.0) as usize;
                (geom::blob(r, points, a.num("wobble").unwrap_or(0.2), &mut rng), None, true)
            }
            _ => unreachable!("primitive {verb}"),
        })
    }

    fn paint_shape(&mut self, a: &Args, geometry: Path, knots: Option<&[P]>, closed: bool, style: &[(String, Value)], world: &Matrix, stroke_src: Option<&Expr>) -> R<Shape> {
        let pick = |k: &str| a.get(k).cloned().or_else(|| style.iter().rev().find(|(n, _)| n == k).map(|(_, v)| v.clone()));
        let bounds = geom::bounds(&geometry);
        let fill_v = pick("fill");
        let fill = match &fill_v {
            Some(Value::None) => None,
            Some(v) => Some(to_paint(v, bounds).ok_or_else(|| {
                fail("E0320", format!("`fill:` expects a color, gradient or field, got {}", v.kind())).fix("write `fill:p.skin` or `fill:grad(down a b)`")
            })?),
            None if closed => Some(Paint::Solid(Rgba::GRAY.to_ir())),
            None => None,
        };
        let line_of = |fill: &Option<Paint>| color::line(fill.as_ref().map(key_color).unwrap_or(Rgba::GRAY));
        enum W {
            Const(f64),
            List(Vec<f64>),
        }
        let stroke_v = pick("stroke");
        let (paint_v, width) = match stroke_v {
            None if !closed && fill.is_none() => (Value::Color(Col::plain(color::line(Rgba::GRAY))), Some(W::Const(2.0))),
            None | Some(Value::None) => (Value::None, None),
            Some(Value::Num(w)) => (Value::Color(Col::plain(line_of(&fill))), Some(W::Const(w))),
            Some(Value::Range(_)) => return Err(range_width(stroke_src)),
            Some(Value::List(ws)) => (Value::Color(Col::plain(line_of(&fill))), Some(W::List(ws.iter().filter_map(Value::num).collect()))),
            Some(Value::Tuple(xs)) if xs.len() == 2 => {
                let w = match &xs[1] {
                    Value::Num(w) => W::Const(*w),
                    Value::Range(_) => return Err(range_width(stroke_src)),
                    Value::List(ws) => W::List(ws.iter().filter_map(Value::num).collect()),
                    other => return Err(fail("E0320", format!("stroke width must be a number or list, got {}", other.kind())).fix("write `stroke:p.ink,2`")),
                };
                (xs[0].clone(), Some(w))
            }
            Some(v) if v.kind() == Kind::Color => (v, Some(W::Const(1.0))),
            Some(v) => return Err(fail("E0320", format!("`stroke:` expects color,width, got {}", v.kind())).fix("write `stroke:p.ink,2`")),
        };
        let taper = match pick("taper") {
            Some(Value::Num(t)) => Some((t, t)),
            Some(v) => v.vec2(),
            None => None,
        };
        let blur = pick("blur").and_then(|v| v.num()).unwrap_or(0.0) as f32;
        let Some(width) = width else {
            return Ok(Shape { geometry, fill, stroke: None, blur, children: vec![] });
        };
        let tapered = matches!(width, W::List(_)) || taper.is_some();
        if tapered {
            let widths = match width {
                W::Const(w) => vec![w],
                W::List(ws) if ws.is_empty() => vec![1.0],
                W::List(ws) => ws,
            };
            let knots = knots.filter(|k| k.len() == widths.len()).unwrap_or(&[]);
            // 0.75 output pixels. The local step divides by the accumulated transform
            // scale and the target scale, so a unit-space stroke matches one drawn in pixels.
            let px = (axis_scale(world) * f64::from(self.job.bake_scale)).max(1e-6);
            let outline = geom::brush(&geometry, knots, &widths, taper.unwrap_or((0.0, 0.0)), 0.75 / px);
            let ob = geom::bounds(&outline);
            let paint = to_paint(&paint_v, ob).ok_or_else(|| fail("E0320", "stroke color must be a color or gradient"))?;
            return Ok(Shape { geometry: outline, fill: Some(paint), stroke: None, blur, children: vec![] });
        }
        let W::Const(w) = width else { unreachable!() };
        let paint = to_paint(&paint_v, bounds).ok_or_else(|| fail("E0320", "stroke color must be a color or gradient"))?;
        let cap = match pick("cap").as_ref().and_then(Value::word) {
            Some("butt") => Cap::Butt,
            Some("square") => Cap::Square,
            _ => Cap::Round,
        };
        let join = match pick("join").as_ref().and_then(Value::word) {
            Some("miter") => Join::Miter,
            Some("bevel") => Join::Bevel,
            _ => Join::Round,
        };
        let dash = pick("dash").and_then(|v| v.items()).map(|xs| xs.iter().filter_map(Value::num).map(|v| v as f32).collect()).unwrap_or_default();
        Ok(Shape { geometry, fill, stroke: Some(Stroke { paint, width: w as f32, cap, join, dash }), blur, children: vec![] })
    }

    pub(crate) fn bind_params(
        &mut self,
        owner: &str,
        params: &[Named],
        module: &Rc<Module>,
        given: &[(String, Value)],
        span: Span,
    ) -> R<Vec<(String, Value)>> {
        let def_scope = self.module_scope(module);
        let mut env: Vec<(String, Value)> = Vec::new();
        for p in params {
            let mut dsc = def_scope.clone();
            dsc.env = env.clone();
            let default = self.expr(&p.value, &dsc);
            let value = match given.iter().rev().find(|(k, _)| k == &p.key) {
                Some((_, v)) => {
                    if let Ok(d) = &default
                        && !d.accepts(v)
                    {
                        return Err(fail("E0320", format!("`{}` of `{owner}` expects {}, got {}", p.key, d.kind(), v.kind()))
                            .fix(format!("pass {} like `{}:{}`", d.kind(), p.key, d.show())));
                    }
                    v.clone()
                }
                None => default.map_err(|f| f.note(format!("in the default of `{}` ({}:{})", p.key, module.path, span.line)))?,
            };
            env.push((p.key.clone(), value));
        }
        Ok(env)
    }

    // ---------------------------------------------------------------- components

    #[allow(clippy::too_many_arguments)]
    fn instance(
        &mut self,
        comp: &Rc<Component>,
        a: Args,
        block: &DrawBody,
        sc: &mut Scope,
        out: &mut Siblings,
        path: String,
        origin: String,
        name: Option<String>,
    ) -> R<()> {
        let module = self.modules.get(&comp.module).cloned().ok_or_else(|| fail("E0210", format!("module `{}` is not loaded", comp.module)))?;
        let param_names: Vec<&str> = comp.params.iter().map(|p| p.key.as_str()).collect();
        let mut given: Vec<(String, Value)> = Vec::new();
        for (i, v) in a.pos.iter().enumerate() {
            let p = comp.params.get(i).ok_or_else(|| {
                fail("E0321", format!("`{}` takes {} positional argument(s)", comp.name, comp.params.len().min(1)))
                    .fix("pass the rest as key:value")
            })?;
            given.push((p.key.clone(), v.clone()));
        }
        let mut node_args = Args { pos: Vec::new(), named: Vec::new() };
        for (k, v) in &a.named {
            if param_names.contains(&k.as_str()) {
                given.push((k.clone(), v.clone()));
            } else if NODE_KEYS.contains(&k.as_str()) {
                node_args.named.push((k.clone(), v.clone()));
            } else {
                return Err(fail("E0321", format!("`{}` has no parameter `{k}`", comp.name))
                    .note(format!("parameters: {}", param_names.join(", ")))
                    .fix("use one of the listed parameters"));
            }
        }
        let (m, at) = self.transform(&node_args, (0.0, 0.0), sc.frame, sc, true)?;
        let caller_local = inverse(&m).map_rect(sc.frame).0;
        let mut box_frame = Rect::new_empty();
        let def_scope = self.module_scope(&module);
        let mut env: Vec<(String, Value)> = Vec::new();
        for p in &comp.params {
            let mut dsc = def_scope.clone();
            dsc.env = env.clone();
            let default = self.expr(&p.value, &dsc);
            let pair_default = matches!(&default, Ok(d) if d.kind() == Kind::Vector);
            let value = match given.iter().rev().find(|(k, _)| k == &p.key) {
                Some((_, v)) => {
                    if let Ok(d) = &default
                        && !d.accepts(v)
                    {
                        return Err(fail("E0320", format!("`{}` of `{}` expects {}, got {}", p.key, comp.name, d.kind(), v.kind()))
                            .fix(format!("pass {} like `{}:{}`", d.kind(), p.key, d.show())));
                    }
                    v.clone()
                }
                None => default.map_err(|f| f.note(format!("in the default of `{}` ({}:{})", p.key, comp.module, comp.span.line)))?,
            };
            let value = if p.key == "size" {
                let (value, rect) = resolve_size(&value, pair_default, caller_local, sc, &comp.name)?;
                box_frame = rect;
                value
            } else {
                value
            };
            env.push((p.key.clone(), value));
        }
        if let Some(host) = &sc.host {
            for need in &comp.needs {
                if table_get(&host.table, need).is_none() {
                    let parts = table_names(&host.table);
                    let mut f = fail("E0311", format!("{} needs @{need}, but the host `{}` has no part named {need}", comp.name, host.def_name))
                        .note(format!("parts available: {}", parts.join(", ")));
                    f = match parts.first() {
                        Some(first) => f.fix(format!("rename {}'s `{first}` to `{need}`, or fill a slot on a host that has `{need}`", host.def_name)),
                        None => f.fix(format!("give `{}` a part named `{need}`", host.def_name)),
                    };
                    return Err(f);
                }
            }
        }
        let world = sc.world * m;
        let table = new_table();
        let mut fills = Vec::new();
        let depth = path.matches('.').count();
        for b in block.items() {
            match b {
                DrawItem::SlotFill(fill) => {
                    fills.push(Fill { name: fill.name.clone(), body: fill.body.clone(), scope: sc.clone(), span: fill.span })
                }
                DrawItem::Patch(ast::Patch { path: segs, named, off, span }) => {
                    let mut vals = Vec::new();
                    for n in named {
                        vals.push((n.key.clone(), self.expr(&n.value, sc)?));
                    }
                    let rel = segs.join(".");
                    self.patches.push(PatchRec {
                        target: format!("{path}.{rel}"),
                        named: vals,
                        off: *off,
                        owner: path.clone(),
                        depth,
                        rel,
                        span: *span,
                        file: sc.file().to_string(),
                        matched: Cell::new(false),
                    });
                }
                DrawItem::Fx(_) | DrawItem::Mask { .. } => {}
                DrawItem::Scene(st @ Scene { kind: SceneKind::Check { .. }, .. }) => {
                    sc.checks.borrow_mut().push(Pending { stmt: st.clone(), scope: sc.clone() })
                }
                DrawItem::Scene(st) => {
                    let file = sc.file().to_string();
                    self.report(
                        fail("E0330", format!("only slot fills, patches, checks, fx and one mask may go under an instance of `{}`", comp.name))
                            .fix("wrap the content in a slot fill such as `head:`, or dedent it"),
                        &file,
                        st.span,
                        &path,
                    );
                }
            }
        }
        let inst = Rc::new(Instance {
            def_name: comp.name.clone(),
            fills,
            table: table.clone(),
            used_fills: RefCell::new(Vec::new()),
            record: RefCell::new(None),
        });
        let mut lookup = vec![table.clone()];
        lookup.extend(sc.lookup.iter().cloned());
        let checks = Rc::new(RefCell::new(Vec::new()));
        let mut body_scope = Scope {
            env,
            module,
            path: path.clone(),
            suffix: String::new(),
            frame: box_frame,
            world,
            style: sc.style.clone(),
            register: table,
            lookup: Rc::new(lookup),
            inst: Some(inst.clone()),
            host: None,
            checks: checks.clone(),
            top: false,
        };
        let children = self.eval_block_nodes(comp.body.iter().filter_map(DefItem::scene), &mut body_scope);
        let mut node = new_node(path.clone(), origin, m, NodeKind::Group { isolate: false, children });
        let f = first_fill(&node).unwrap_or(Rgba::GRAY);
        let fx_scope = body_scope.child(&path, body_scope.frame, world);
        let sites: Vec<FxSite> = comp
            .body
            .iter()
            .filter_map(DefItem::fx)
            .map(|line| FxSite { line, scope: &fx_scope })
            .chain(block.fx().map(|line| FxSite { line, scope: &*sc }))
            .collect();
        node.effects = self.lower_fx(&sites, &path, f);
        self.mask(&mut node, &node_args, block.mask(), sc, world)?;
        let z = self.finish_node(&mut node, &node_args)?;
        let reg_name = name.unwrap_or(comp.name.clone());
        let rec = self.register(sc, &reg_name, Record { name: reg_name.clone(), path: path.clone(), parent_world: sc.world, node: Some(node.clone()), anchor: None, at });
        *inst.record.borrow_mut() = Some(rec);
        self.run_checks(&checks);
        for fill in &inst.fills {
            if !inst.used_fills.borrow().contains(&fill.name) {
                let file = fill.scope.file().to_string();
                self.report(
                    fail("E0335", format!("`{}` has no slot `{}`", comp.name, fill.name)).fix("fill one of the slots the def declares with `slot <name>`"),
                    &file,
                    fill.span,
                    &path,
                );
            }
        }
        let unmatched: Vec<(String, Span, String)> = self
            .patches
            .iter()
            .filter(|p| p.owner == path && !p.matched.get())
            .map(|p| (p.rel.clone(), p.span, p.file.clone()))
            .collect();
        if !unmatched.is_empty() {
            let parts = part_names(&node);
            for (rel, span, file) in unmatched {
                self.report(
                    fail("E0340", format!("patch `.{rel}` matches no part of `{}`", comp.name))
                        .note(format!("parts: {}", parts.join(", ")))
                        .fix("use one of the listed part paths"),
                    &file,
                    span,
                    &path,
                );
            }
        }
        out.items.push((node, z));
        Ok(())
    }

    fn slot(&mut self, st: &Scene, name: &str, named: &[Named], fallback: &[Scene], sc: &mut Scope, out: &mut Siblings) -> R<()> {
        let inst = sc.inst.clone().ok_or_else(|| fail("E0334", "`slot` is only valid inside a `def`").fix("move it into a def body"))?;
        let leaf = out.name(name, &sc.suffix);
        let path = format!("{}.{leaf}", sc.path);
        let (patch_named, off) = self.take_patches(&path);
        if off {
            if inst.fills.iter().any(|f| f.name == name) {
                inst.used_fills.borrow_mut().push(name.to_string());
            }
            return Ok(());
        }
        let mut a = self.eval_args(&[], named, sc, 0)?;
        a.named.extend(patch_named);
        let (m, _) = self.transform(&a, (0.0, 0.0), sc.frame, sc, true)?;
        let world = sc.world * m;
        let mapped = inverse(&m).map_rect(sc.frame).0;
        // Centered on the slot origin, because content with no `at:` defaults to the frame center.
        let local_frame = size_box(mapped.width() as f64, mapped.height() as f64);
        let children = match inst.fills.iter().find(|f| f.name == name) {
            Some(fill) => {
                inst.used_fills.borrow_mut().push(name.to_string());
                let mut fsc = fill.scope.child(&path, local_frame, world);
                let mut lookup = vec![inst.table.clone()];
                lookup.extend(fill.scope.lookup.iter().cloned());
                fsc.lookup = Rc::new(lookup);
                fsc.host = Some(Rc::new(Host { def_name: inst.def_name.clone(), table: inst.table.clone() }));
                self.eval_block_nodes(&fill.body, &mut fsc)
            }
            None => {
                let mut inner = sc.child(&path, local_frame, world);
                self.eval_block_nodes(fallback, &mut inner)
            }
        };
        let mut node = new_node(path, format!("{}:{}", sc.file(), st.span.line), m, NodeKind::Group { isolate: false, children });
        let z = self.finish_node(&mut node, &a)?;
        out.items.push((node, z));
        Ok(())
    }

    // ---------------------------------------------------------------- expressions

    pub fn lookup_ident(&self, name: &str, sc: &Scope) -> Option<Value> {
        if let Some((_, v)) = sc.env.iter().rev().find(|(k, _)| k == name) {
            return Some(v.clone());
        }
        if name == "target" {
            return Some(self.job.label.clone().map(Value::Ident).unwrap_or(Value::None));
        }
        if let Some((_, v)) = self.job.matrix.iter().find(|(k, _)| k == name) {
            return Some(match v {
                MatrixValue::Word(w) => Value::Ident(w.clone()),
                MatrixValue::Num(n) => Value::Num(*n),
            });
        }
        if let Some(v) = sc.module.global(name) {
            return Some(v);
        }
        match name {
            "true" => Some(Value::Bool(true)),
            "false" => Some(Value::Bool(false)),
            "none" => Some(Value::None),
            "canvas" => Some(Value::Ref(Rc::new(RefVal {
                label: "canvas".into(),
                record: Record { name: "canvas".into(), path: "canvas".into(), parent_world: Matrix::new_identity(), node: None, anchor: None, at: (0.0, 0.0) },
                canvas: Some(self.canvas),
            }))),
            _ => None,
        }
    }

    pub fn expr(&mut self, e: &Expr, sc: &Scope) -> R<Value> {
        Ok(match e {
            Expr::Num(n) => Value::Num(*n),
            Expr::Percent(p) => Value::Percent(*p),
            Expr::Angle(d) => Value::Num(*d),
            Expr::Size(w, h) => {
                let (w, h) = (self.expr(w, sc)?, self.expr(h, sc)?);
                match (w.coord(), h.coord()) {
                    (Some(a), Some(b)) => Value::Size(a, b),
                    _ => return Err(fail("E0320", "a size is two numbers or percents")),
                }
            }
            Expr::Hex(h) => Value::Color(Col::plain(Rgba::hex(h).ok_or_else(|| fail("E0320", format!("bad color #{h}")))?)),
            Expr::Str(s) => Value::Str(s.clone()),
            Expr::Ident(name) => self.lookup_ident(name, sc).unwrap_or_else(|| Value::Ident(name.clone())),
            Expr::Ref(segs) => self.reference(segs, sc)?,
            Expr::Member(base, seg) => {
                let b = self.expr(base, sc)?;
                self.member(&b, seg, sc)?
            }
            Expr::Tuple(xs) => Value::tuple(xs.iter().map(|x| self.expr(x, sc)).collect::<R<_>>()?),
            Expr::List(xs) => Value::List(xs.iter().map(|x| self.expr(x, sc)).collect::<R<_>>()?),
            Expr::Each { var, iter, body } => {
                let items = each_items(self.expr(iter, sc)?)?;
                let mut out = Vec::with_capacity(items.len());
                for v in items {
                    let mut inner = sc.clone();
                    inner.env.push((var.clone(), v));
                    out.push(self.expr(body, &inner)?);
                }
                Value::List(out)
            }
            Expr::Range(a, b) => {
                let (a, b) = (self.expr(a, sc)?, self.expr(b, sc)?);
                match (a.num(), b.num()) {
                    (Some(a), Some(b)) => Value::Range(Range::new(a, b)),
                    _ => return Err(fail("E0320", "a range needs numbers on both ends")),
                }
            }
            Expr::Unary(UnOp::Neg, x) => {
                let v = self.expr(x, sc)?;
                match v {
                    Value::Field(f) => Value::Field(Rc::new(artscript_ir::Field::Unary(artscript_ir::FieldFn::Neg, Box::new((*f).clone())))),
                    other => arith(Arith::Mul, &other, &Value::Num(-1.0)).map_err(|m| fail("E0320", m))?,
                }
            }
            Expr::Unary(UnOp::Not, x) => Value::Bool(!self.expr(x, sc)?.truthy()),
            Expr::Binary(op, l, r) => self.binary(*op, l, r, sc)?,
            Expr::Call { func, args, named } => self.call(func, args, named, sc)?,
            Expr::If { cond, then, otherwise } => {
                if self.expr(cond, sc)?.truthy() {
                    self.expr(then, sc)?
                } else {
                    self.expr(otherwise, sc)?
                }
            }
        })
    }

    fn binary(&mut self, op: BinOp, l: &Expr, r: &Expr, sc: &Scope) -> R<Value> {
        if op == BinOp::And {
            return Ok(Value::Bool(self.expr(l, sc)?.truthy() && self.expr(r, sc)?.truthy()));
        }
        if op == BinOp::Or {
            return Ok(Value::Bool(self.expr(l, sc)?.truthy() || self.expr(r, sc)?.truthy()));
        }
        let (a, b) = (self.expr(l, sc)?, self.expr(r, sc)?);
        let ar = |o| arith(o, &a, &b).map_err(|m| fail("E0320", m));
        Ok(match op {
            BinOp::Add => ar(Arith::Add)?,
            BinOp::Sub => ar(Arith::Sub)?,
            BinOp::Mul | BinOp::Div => match (&a, b.num()) {
                (Value::Range(r), Some(k)) => {
                    let arith_op = if op == BinOp::Mul { Arith::Mul } else { Arith::Div };
                    Value::Range(r.scale(arith_op, k))
                }
                _ => ar(if op == BinOp::Mul { Arith::Mul } else { Arith::Div })?,
            },
            BinOp::Eq => Value::Bool(equal(&a, &b)),
            BinOp::Ne => Value::Bool(!equal(&a, &b)),
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                if let Value::Field(f) = &a
                    && let Some(t) = b.num()
                    && matches!(op, BinOp::Gt | BinOp::Ge)
                {
                    return Ok(Value::FieldMask(f.clone(), t));
                }
                let (x, y) = match (a.num(), b.num()) {
                    (Some(x), Some(y)) => (x, y),
                    _ => return Err(fail("E0320", format!("cannot compare {} and {}", a.kind(), b.kind()))),
                };
                Value::Bool(match op {
                    BinOp::Lt => x < y,
                    BinOp::Le => x <= y,
                    BinOp::Gt => x > y,
                    _ => x >= y,
                })
            }
            BinOp::Approx => {
                let tol = self.check.as_ref().map(|c| c.tol).unwrap_or(0.5);
                let ok = match (a.num(), b.num(), a.vec2(), b.vec2()) {
                    (Some(x), Some(y), _, _) => (x - y).abs() <= tol,
                    (_, _, Some(p), Some(q)) => (p.0 - q.0).abs() <= tol && (p.1 - q.1).abs() <= tol,
                    _ => return Err(fail("E0320", format!("`~=` compares numbers or vectors, got {} and {}", a.kind(), b.kind()))),
                };
                if !ok {
                    self.note(format!("{} ~= {} is off by more than tol {}", a.show(), b.show(), num(tol)));
                }
                Value::Bool(ok)
            }
            BinOp::In => match (&a, &b) {
                (Value::Num(x), Value::Range(r)) => {
                    let ok = r.contains(*x);
                    if !ok {
                        self.note(r.miss_note(*x));
                    }
                    Value::Bool(ok)
                }
                (_, Value::List(xs)) => Value::Bool(xs.iter().any(|x| equal(x, &a))),
                _ => return Err(fail("E0320", format!("`in` needs a range or list on the right, got {}", b.kind()))),
            },
            BinOp::Map => {
                let field = match &a {
                    Value::Field(f) => f.clone(),
                    _ => return Err(fail("E0320", "`->` maps a field through colors").fix("write `fill:fbm(simplex(30)) -> p.stone`")),
                };
                let colors: Vec<Rgba> = match &b {
                    Value::Ramp(r) => r.stops.clone(),
                    other => other.items().unwrap_or_default().iter().filter_map(|v| v.color().map(|c| c.rgba)).collect(),
                };
                if colors.is_empty() {
                    return Err(fail("E0320", "`->` needs a ramp or a list of colors"));
                }
                Value::FieldPaint(field, colors)
            }
            BinOp::And | BinOp::Or => unreachable!(),
        })
    }

    pub fn note(&mut self, n: String) {
        if let Some(c) = &mut self.check
            && !c.notes.contains(&n)
        {
            c.notes.push(n);
        }
    }

    fn member(&mut self, base: &Value, seg: &str, sc: &Scope) -> R<Value> {
        let seg: String = match seg.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
            Some(var) => self.lookup_ident(var, sc).and_then(|v| v.word().map(str::to_string).or(v.num().map(num))).unwrap_or(var.to_string()),
            None => seg.to_string(),
        };
        let seg = seg.as_str();
        let missing = |what: &str, names: Vec<String>| {
            fail("E0350", format!("{what} has no member `{seg}`")).note(format!("members: {}", names.join(", "))).fix("use one of the listed members")
        };
        Ok(match base {
            Value::Palette(p) => p.get(seg).ok_or_else(|| missing(&format!("palette `{}`", p.name), p.entries.iter().map(|e| e.0.clone()).collect()))?,
            Value::Module(m) => m.member(seg).ok_or_else(|| {
                let mut names: Vec<String> = m.exports().into_iter().map(|e| e.0).collect();
                names.extend(m.palettes.borrow().iter().flat_map(|p| p.entries.iter().map(|e| e.0.clone())));
                missing(&format!("`{}`", m.path), names)
            })?,
            Value::Ramp(r) => match seg {
                "line" => Value::Color(Col::plain(r.line())),
                "base" => base.clone(),
                _ => {
                    let i: f64 = seg.parse().map_err(|_| missing("a ramp", vec!["0..n".into(), "line".into(), "base".into()]))?;
                    Value::Color(Col { rgba: r.at(i), ramp: Some((r.clone(), i)) })
                }
            },
            Value::Color(c) if seg == "line" => Value::Color(Col::plain(color::line(c.rgba))),
            Value::Vec2(x, y) | Value::Size(x, y) => {
                let c = match seg {
                    "x" | "w" => *x,
                    "y" | "h" => *y,
                    _ => return Err(missing("a vector", vec!["x".into(), "y".into()])),
                };
                match c {
                    Coord::Num(v) => Value::Num(v),
                    Coord::Pct(p) => Value::Percent(p),
                }
            }
            Value::Ref(r) => self.measure(r, seg)?,
            Value::Tuple(xs) | Value::List(xs) => {
                let i: usize = seg.parse().map_err(|_| missing("a list", vec!["0..n".into()]))?;
                xs.get(i).cloned().ok_or_else(|| fail("E0350", format!("index {i} is out of range")))?
            }
            Value::Color(_) => {
                return Err(fail("E0350", format!("a plain color has no step `.{seg}`; only ramps are indexed"))
                    .fix("declare the palette entry as `ramp name <color> 5` to index it, or drop the index"));
            }
            other => return Err(fail("E0350", format!("{} has no member `.{seg}`", other.kind()))),
        })
    }

    pub fn measure(&mut self, r: &RefVal, m: &str) -> R<Value> {
        let b = r.bounds().ok_or_else(|| fail("E0351", format!("@{} has no geometry", r.label)))?;
        let v = measure_rect(b, m).ok_or_else(|| fail("E0350", format!("unknown measurement `.{m}`")).note(format!("measurements: {}", MEASURES.join(" "))))?;
        self.note(format!("{}.{m} = {}", r.label, num(v)));
        Ok(Value::Num(v))
    }

    fn reference(&mut self, segs: &[String], sc: &Scope) -> R<Value> {
        let first = &segs[0];
        let mut cur = if first == "self" {
            let inst = sc.inst.clone().ok_or_else(|| fail("E0350", "`@self` is only valid inside a def"))?;
            let rec = inst.record.borrow().clone().ok_or_else(|| fail("E0350", "`@self` is only available in checks"))?;
            let rec = rec.borrow().clone();
            RefVal { label: "self".into(), record: rec, canvas: None }
        } else if first == "canvas" {
            match self.lookup_ident("canvas", sc) {
                Some(Value::Ref(r)) => (*r).clone(),
                _ => unreachable!(),
            }
        } else {
            let rec = sc.lookup.iter().find_map(|t| table_get(t, first)).ok_or_else(|| {
                let mut names: Vec<String> = Vec::new();
                for t in sc.lookup.iter() {
                    for n in table_names(t) {
                        if !names.contains(&n) {
                            names.push(n);
                        }
                    }
                }
                fail("E0350", format!("no part named `{first}` here"))
                    .note(format!("named so far: {}", if names.is_empty() { "(none)".into() } else { names.join(", ") }))
                    .fix("name the node with `name:` before this line, or fix the spelling")
            })?;
            let rec = rec.borrow().clone();
            RefVal { label: first.clone(), record: rec, canvas: None }
        };
        let mut i = 1;
        while i < segs.len() {
            let seg = segs[i].as_str();
            let last = i == segs.len() - 1;
            if seg == "*" {
                return Ok(Value::List(cur.children().into_iter().map(|c| Value::Ref(Rc::new(c))).collect()));
            }
            if seg == "geom" && i + 1 < segs.len() {
                let m = segs[i + 1].as_str();
                let b = cur.geom_bounds().ok_or_else(|| fail("E0351", format!("@{} has no geometry", cur.label)))?;
                let v = measure_rect(b, m).ok_or_else(|| fail("E0350", format!("unknown measurement `.geom.{m}`")))?;
                self.note(format!("{}.geom.{m} = {}", cur.label, num(v)));
                return Ok(Value::Num(v));
            }
            if last && MEASURES.contains(&seg) && !cur.children().iter().any(|c| c.record.name == seg) {
                return self.measure(&cur, seg);
            }
            let child_path = format!("{}.{seg}", cur.record.path);
            if let Some(p) = self.anchors.get(&child_path) {
                cur = RefVal {
                    label: format!("{}.{seg}", cur.label),
                    record: Record { name: seg.into(), path: child_path, parent_world: Matrix::new_identity(), node: None, anchor: Some(*p), at: (0.0, 0.0) },
                    canvas: None,
                };
            } else {
                let kids = cur.children();
                let names: Vec<String> = kids.iter().map(|c| c.record.name.clone()).collect();
                cur = kids.into_iter().find(|c| c.record.name == seg).ok_or_else(|| {
                    fail("E0350", format!("@{} has no part `{seg}`", cur.label)).note(format!("parts: {}", names.join(", "))).fix("use one of the listed parts")
                })?;
            }
            i += 1;
        }
        Ok(Value::Ref(Rc::new(cur)))
    }
}

fn is_paint_key(k: &str) -> bool {
    matches!(k, "fill" | "stroke" | "cap" | "join" | "dash" | "taper" | "blur")
}

pub fn measure_rect(b: Rect, m: &str) -> Option<f64> {
    let (l, t, r, bt) = (b.left as f64, b.top as f64, b.right as f64, b.bottom as f64);
    Some(match m {
        "x" | "left" => l,
        "y" | "top" => t,
        "w" => r - l,
        "h" => bt - t,
        "cx" => (l + r) / 2.0,
        "cy" => (t + bt) / 2.0,
        "right" => r,
        "bottom" => bt,
        "area" => (r - l) * (bt - t),
        _ => return None,
    })
}

fn part_names(node: &Node) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(n: &Node, root: &str, out: &mut Vec<String>) {
        for c in n.children() {
            if let Some(rel) = c.path.strip_prefix(root) {
                out.push(rel.trim_start_matches('.').to_string());
            }
            walk(c, root, out);
        }
    }
    walk(node, &node.path, &mut out);
    out
}
