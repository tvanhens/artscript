//! The evaluator: modules and declarations, the drawing statements, components with slots and
//! patches, and expressions. Produces the scene IR described in `ramose-ir`.

use std::cell::{OnceCell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::rc::Rc;
use std::sync::Arc;

use ramose_ir::{
    Blend, Cap, Clip, Fill as IrFill, Join, Mask, MaskSource, Node, NodeKind, Paint, Shape, Stroke,
    Warp,
};
use ramose_geom::{NodeGeom, ToIr, ToSk, WarpError, WarpGeom, union_bounds};
use skia_safe::{Matrix, Path, Point, Rect};
use ramose_syntax::ast::{
    self, BinOp, Decl, DeclKind, DefItem, Draw, DrawBody, DrawItem, Expr, File, Item, Named, Scene,
    SceneKind, Span, Swatch, SwatchKind, UnOp,
};
use ramose_syntax::{Diagnostic, format_expr};

use crate::brush::{self, BrushVal, CHANNEL_KEYS, Channels, Slot};
use crate::checks::{CheckState, Pending as CheckPending};
use crate::color::{self, DEFAULT_HUE_STEP, Ramp, Rgba};
use crate::fx::FxSite;
use crate::geom::{self, BoolOp, P};
use crate::identity::{
    self, Arrival, ChannelPatch, DeclId, Entry, Key, Lookup, ModuleIds, Names, Order, Pending,
    PendingPatch, ReportKey, Resolved, Site,
};
use crate::key::Ease;
use crate::motion::{absolute_channel, offset_channel};
use crate::order;
use crate::paint::{key_color, to_paint};
use crate::place::{Place, axis_scale, inverse};
use crate::project::Project;
use crate::range::Range;
use crate::scope::{
    AliasDef, AnimDef, Component, Extent, Fixed, FnDef, Module, Palette, Record, RefVal, RowRecord, RuleDef,
    StyleDef, ref_from,
};
use crate::seed::Rng;
use crate::targets::{MatrixValue, TargetJob, is_entry};
use crate::types::{DeclRef, Signature, Ty, check_param, marker_value};
use crate::value::{Arith, Col, Coord, Kind, PathVal, Value, arith, equal, num};
use crate::warp;

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
    Fail {
        code,
        msg: msg.into(),
        notes: Vec::new(),
        fix: None,
    }
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
    pub(crate) fn reported() -> Fail {
        fail("", "")
    }
}

const MEASURES: &[&str] = &[
    "x", "y", "w", "h", "cx", "cy", "left", "right", "top", "bottom", "area",
];
const NODE_KEYS: &[&str] = &[
    "at", "rot", "scale", "skew", "pivot", "warp", "a", "blend", "order", "name", "style", "feather",
];
/// Keys the language renamed, each with the key that replaced it.
const RENAMED: &[(&str, &str)] = &[("z", "order")];

fn renamed(key: &str) -> Option<&'static str> {
    RENAMED.iter().find(|(old, _)| *old == key).map(|(_, new)| *new)
}

/// E0321 for a key that `target`, an instance of `def`, does not take. A def draws several
/// shapes, so `fill:` and `blur:` have no single shape to land on; the fix names what recolors or
/// blurs the whole instance.
fn no_parameter(target: &str, def: &str, key: &str, params: &[impl AsRef<str>]) -> Fail {
    let params: Vec<&str> = params.iter().map(AsRef::as_ref).collect();
    let mut f = fail("E0321", format!("`{target}` has no parameter `{key}`"));
    if !params.is_empty() {
        f = f.note(format!("parameters: {}", params.join(", ")));
    }
    if let Some(new) = renamed(key) {
        return f.fix(format!("`{key}:` is now `{new}:`"));
    }
    match key {
        "fill" | "stroke" => f.fix(format!(
            "recolor the instance with `palette:`, or give `{def}` a color parameter"
        )),
        "blur" => f.fix("blur the instance with an `fx blur N` line under it"),
        _ => f.fix("use one of the listed parameters"),
    }
}

const CENTERED: &[&str] = &["rect", "circle", "ellipse", "poly", "star", "blob"];

const PAINT_KEYS: &[&str] = &[
    "fill", "stroke", "cap", "join", "dash", "taper", "blur", "brush", "pressure", "angle", "tilt", "stamp",
];
/// Keys a boolean operand cannot use, because the boolean reads only its geometry.
const OPERAND_IGNORES: &[&str] = &[
    "fill", "stroke", "cap", "join", "dash", "taper", "blur", "brush", "pressure", "angle", "tilt", "stamp", "a", "blend", "warp",
];

/// A shape's geometry, the authored points of each contour, whether it is closed, and the point
/// that contour position 0 is nearest when the geometry does not start there.
type Outline = (Path, Vec<Vec<P>>, bool, Option<P>);

struct Builtin {
    takes: usize,
    keys: &'static [&'static str],
}

impl Builtin {
    fn of(verb: &str) -> Option<Builtin> {
        let (takes, keys): (usize, &[&str]) = match verb {
            "rect" => (1, &["r", "jitter", "seed"]),
            "circle" | "ellipse" | "path" | "copy" => (1, &["jitter", "seed"]),
            "poly" => (1, &["r", "round", "jitter", "seed"]),
            "star" => (1, &["r", "inner", "round", "jitter", "seed"]),
            "blob" => (1, &["points", "wobble", "jitter", "seed"]),
            "curve" => (usize::MAX, &["tension", "round", "jitter", "seed"]),
            "edge" => (1, &["from", "to", "jitter", "seed"]),
            "subtract" | "union" | "intersect" | "xor" => (0, &["jitter", "seed"]),
            "repeat" => (1, &["step"]),
            "grid" => (1, &["gap", "cell"]),
            "inside" | "outside" => (1, &[]),
            "layer" | "group" | "mirror" => (0, &[]),
            _ => return None,
        };
        Some(Builtin { takes, keys })
    }

    fn all_keys(&self) -> impl Iterator<Item = &'static str> {
        [NODE_KEYS, PAINT_KEYS, self.keys].into_iter().flatten().copied()
    }
}

pub(crate) struct Instance {
    pub def_name: String,
    fills: Vec<Fill>,
    names: Rc<RefCell<Names>>,
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
    names: Rc<RefCell<Names>>,
    copies: Vec<u32>,
    loops: Vec<Span>,
}

/// Everything a block of statements is evaluated against.
#[derive(Clone)]
pub(crate) struct Scope {
    pub env: Vec<(String, Value)>,
    /// The names in `env` whose value follows an anim patch, with that patch.
    pub timed: BTreeMap<String, Rc<Timed>>,
    /// Def and stamp parameters typed as a brush or a stamp, by their index in `env`. A call on
    /// one is checked against that declaration whatever the instance bound.
    pub typed_params: Rc<Vec<(usize, Ty)>>,
    pub module: Rc<Module>,
    owner: Key,
    parent: Key,
    scope_copies: Vec<u32>,
    parent_copies: Vec<u32>,
    scope_loops: Vec<Span>,
    pub(crate) names: Rc<RefCell<Names>>,
    pub(crate) lookup: Lookup,
    pending: Rc<Pending>,
    pub(crate) ids: Rc<ModuleIds>,
    pub(crate) depth: u32,
    pub frame: Rect,
    /// Canvas from local coordinates.
    pub world: Place,
    pub style: Rc<Vec<(String, Value)>>,
    pub palette: Option<Value>,
    pub inst: Option<Rc<Instance>>,
    pub(crate) host: Option<Rc<Host>>,
    pub checks: Rc<RefCell<Vec<CheckPending>>>,
    /// Where an `fx` line under a taken `if` goes: the draw or def that owns this block.
    branch_fx: Option<Rc<RefCell<Vec<BranchFx>>>>,
    /// Line of the entry-file statement being evaluated, so a check deferred from a library knows it.
    pub site: Option<u32>,
    provisional: Option<Rc<Provisional>>,
    /// The boolean whose operands this block draws. An instance body is not an operand.
    operand_of: Option<Rc<OperandOf>>,
    top: bool,
    publish: bool,
}

struct OperandOf {
    verb: String,
    name: Option<String>,
}

/// The first copy of a `grid` without `cell:`, which moves once the grid has measured it.
struct Provisional {
    names: Rc<RefCell<Names>>,
    /// Records that existed before the copy.
    before: BTreeSet<usize>,
}

impl Scope {
    /// Names bound here or in the module whose value passes `is`, the local ones first. A
    /// misspelled `brush:` or `stamp:` word lists them.
    pub(crate) fn names_where(&self, is: impl Fn(&Value) -> bool) -> Vec<String> {
        let mut names: Vec<String> = self.env.iter().filter(|(_, v)| is(v)).map(|(k, _)| k.clone()).collect();
        for (k, v) in self.module.globals.borrow().iter() {
            if is(v) && !names.contains(k) {
                names.push(k.clone());
            }
        }
        names
    }

    fn child(&self, parent: Key, frame: Rect, world: Place) -> Scope {
        Scope {
            parent,
            parent_copies: Vec::new(),
            frame,
            world,
            branch_fx: None,
            top: false,
            publish: false,
            ..self.clone()
        }
    }

    fn bind_loop(&mut self, index: Option<&str>, var: &str, i: usize, v: Value) {
        if let Some(index) = index {
            self.env.push((index.to_string(), Value::Num(i as f64)));
            self.timed.remove(index);
        }
        self.env.push((var.to_string(), v));
        self.timed.remove(var);
    }

    fn iteration(&self, span: Span, i: u32) -> Scope {
        let mut inner = self.clone();
        inner.scope_copies.push(i);
        inner.parent_copies.push(i);
        inner.scope_loops.push(span);
        inner
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn enter(
        &self,
        key: Key,
        names: Rc<RefCell<Names>>,
        lookup: Lookup,
        pending: Pending,
        frame: Rect,
        world: Place,
        depth: u32,
    ) -> Scope {
        Scope {
            owner: key.clone(),
            parent: key,
            scope_copies: Vec::new(),
            parent_copies: Vec::new(),
            scope_loops: Vec::new(),
            names,
            lookup,
            pending: Rc::new(pending),
            frame,
            world,
            depth,
            operand_of: None,
            branch_fx: None,
            top: false,
            publish: false,
            ..self.clone()
        }
    }

    pub(crate) fn file(&self) -> &str {
        &self.module.path
    }

    /// Gives this block's `fx` lines under a taken `if` to a fresh list the owner lowers.
    pub(crate) fn collect_branch_fx(&mut self) -> Rc<RefCell<Vec<BranchFx>>> {
        let fx = Rc::new(RefCell::new(Vec::new()));
        self.branch_fx = Some(fx.clone());
        fx
    }

    fn branch_fx(&self, line: &ast::Fx) -> R<()> {
        let fx = self.branch_fx.as_ref().ok_or_else(|| {
            fail("E0101", "`fx` under this `if` has no draw or def to apply to")
                .fix("indent the `if` directly under the shape, group, def, or component call")
        })?;
        fx.borrow_mut().push(BranchFx {
            line: line.clone(),
            env: self.env.clone(),
        });
        Ok(())
    }

    pub(crate) fn owner(&self) -> &Key {
        &self.owner
    }

    /// The key a diagnostic on a statement in this block is attached to.
    pub(crate) fn node_name(&self) -> String {
        if self.parent.is_root() {
            String::new()
        } else {
            self.parent.to_string()
        }
    }

    fn node_key(&self, span: Span) -> Key {
        let region = self.names.borrow().region();
        if let Some(id) = region.decl_at(span) {
            return self.named_key(&region, id);
        }
        if let Some(anon) = self.ids.anon(span) {
            Key::anon(&self.parent, anon, &self.parent_copies)
        } else {
            self.parent.clone()
        }
    }

    /// A named node's key is the path an author writes, such as `thorax.1.sheen.0`.
    fn named_key(&self, region: &identity::Region, id: DeclId) -> Key {
        let mut key = self.owner.clone();
        let mut taken = 0;
        for id in region.chain(id) {
            let decl = region.decl(id);
            let own = self
                .scope_copies
                .get(taken..decl.loops.len())
                .unwrap_or(&[]);
            key = Key::named(&key, &decl.name, own);
            taken = decl.loops.len();
        }
        key
    }

    fn decl_at(&self, span: Span) -> Option<DeclId> {
        self.names.borrow().region().decl_at(span)
    }
}

#[derive(Default)]
struct Siblings {
    items: Vec<(Node, f64)>,
}

impl Siblings {
    fn finish(mut self) -> Vec<Node> {
        self.items
            .sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        self.items.into_iter().map(|(n, _)| n).collect()
    }
}

/// Evaluated arguments of one statement. Later named values win.
#[derive(Default)]
pub(crate) struct Args {
    pub pos: Vec<Value>,
    pub named: Vec<(String, Value)>,
    /// The anim patch a positional argument follows, by index.
    pub timed_pos: Vec<(usize, Rc<Timed>)>,
    /// The anim patch a named argument follows, by key.
    pub timed: Vec<(String, Rc<Timed>)>,
}

impl Args {
    pub fn get(&self, k: &str) -> Option<&Value> {
        self.named
            .iter()
            .rev()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v)
    }

    pub fn num(&self, k: &str) -> Option<f64> {
        self.get(k).and_then(Value::num)
    }

    fn rot(&self) -> R<f64> {
        let Some(v) = self.get("rot") else { return Ok(0.0) };
        v.num().ok_or_else(|| {
            fail("E0320", format!("`rot:` takes one angle, got {}", v.kind()))
                .fix("write `rot:30`; a 3D rotation will take an axis and an angle, not a vector")
        })
    }

    pub fn seed(&self) -> R<Option<u64>> {
        if let Some(by) = self.timed_key("seed") {
            return Err(by.sink("`seed:`"));
        }
        self.get("seed").map(Value::seed).transpose()
    }

    fn timed_at(&self, i: usize) -> Option<&Rc<Timed>> {
        self.timed_pos.iter().find(|(j, _)| *j == i).map(|(_, by)| by)
    }

    pub(crate) fn timed_key(&self, k: &str) -> Option<&Rc<Timed>> {
        self.timed.iter().find(|(n, _)| n == k).map(|(_, by)| by)
    }

    fn word(&self, k: &str) -> Option<String> {
        self.get(k).and_then(|v| v.word().map(str::to_string))
    }
}

/// What a component verb instances: the def with a body, and the alias the verb named, if any.
struct Callee {
    comp: Rc<Component>,
    alias: Option<Rc<AliasDef>>,
}

pub(crate) struct Evaluator<'a> {
    pub project: &'a Project,
    pub job: &'a TargetJob,
    pub modules: HashMap<String, Rc<Module>>,
    pub parsed: HashMap<String, Rc<File>>,
    loading: Vec<String>,
    pub diags: Vec<Diagnostic>,
    pub artboard: Rect,
    pub background: Option<Rgba>,
    pub check: Option<CheckState>,
    pub(crate) ids: HashMap<String, Rc<ModuleIds>>,
    incoming: Vec<PendingPatch>,
    reported: BTreeSet<ReportKey>,
    pub(crate) uses: identity::PositionalUses,
    seed_file: String,
    seed_key: Key,
    seed_count: u64,
    style_hits: Vec<StyleHit>,
    pub(crate) accept_family: bool,
    pub(crate) active_fns: Vec<Rc<FnDef>>,
    patch_order: u32,
    unstable_calls: BTreeSet<(String, u32, u32)>,
    pub time: f64,
    anim_clock: Option<AnimClock>,
    /// The anim patch a value read since the enclosing `timed_by` follows, if any.
    timed: Option<Rc<Timed>>,
    pub plays: Vec<PlayMark>,
    /// What the prepass decided at each `if` and `each` that holds an entry-file play, keyed by
    /// position: the branch or item count, or `None` when it could not evaluate the guard.
    guards: HashMap<Vec<u32>, Option<usize>>,
    /// True while `collect_entry` runs, before anything is drawn.
    pub(crate) prepass: bool,
    silence: bool,
    /// True once a `--set` binding of the entry file was rejected. The entry is not evaluated,
    /// because its drawing would read a value the author did not ask for.
    pub(crate) rejected_parameters: bool,
    /// Every placed record by node path, for `.outer` to find the nodes an `of:@x` effect reads.
    pub(crate) placed: RefCell<HashMap<String, Rc<RefCell<Record>>>>,
    /// The stamp declarations whose blocks are being placed, outermost first. A set whose
    /// declaration is already here would place itself forever.
    pub(crate) stamping: Vec<*const crate::stamp::StampDecl>,
    blocked: Vec<order::Blocker>,
    /// Names of the statements that failed while blocked. A read of one stays quiet, because
    /// the cycle was reported at that statement.
    broken: BTreeSet<Site>,
    /// Libraries whose parameter types failed and whose importer this job already told.
    told_failed: BTreeSet<String>,
}

struct StyleHit {
    key: String,
    word: String,
    file: String,
    span: Span,
    anim: String,
}

#[derive(Clone, Debug)]
pub struct PlayMark {
    pub end: f64,
    pub loops: bool,
}

struct AnimClock {
    local: f64,
    dur: f64,
    patch: Option<Rc<Timed>>,
}

/// The anim patch a value follows: the key it set, and where.
pub(crate) struct Timed {
    key: String,
    anim: String,
    file: String,
    line: u32,
}

impl Timed {
    /// E0345 at a position that decides the scene's structure.
    fn sink(&self, what: &str) -> Fail {
        fail("E0345", format!("{what} follows an anim patch"))
            .note(format!(
                "`{}:` is set by anim `{}` at {}:{}",
                self.key, self.anim, self.file, self.line
            ))
            .fix("a scene's structure does not change over time; draw every variant always and animate its `a:`")
    }
}

fn secs(v: Value) -> R<f64> {
    match v {
        Value::Num(n) if n.is_finite() => Ok(n),
        _ => Err(fail("E0320", format!("expected a time, got {}", v.kind()))
            .fix("write `1s`, `200ms`, or a number of seconds")),
    }
}

fn outside_anim(what: &str) -> Fail {
    fail("E0345", format!("{what} is read outside an anim patch value")).fix(
        "move it into an `anim` patch and `play` it; to show a part for only some of a clip, draw it always and animate its `a:`",
    )
}

pub(crate) struct SeedSnap {
    file: String,
    key: Key,
    count: u64,
}

pub(crate) fn canvas_renamed() -> Fail {
    fail("E0320", "`canvas` is now `artboard`").fix("write `artboard`")
}

/// A prepass read of the drawn scene. The `let`s and guards that hit it are checked again while
/// drawing, so only a top-level play's own inputs report it.
pub(crate) fn unseen(what: &str) -> Fail {
    fail("E0344", format!("`{what}` reads the drawn scene"))
        .note("top-level plays are scheduled before anything is drawn")
        .fix("compute it from `let`s or `clip`, or move the `play` under the instance it drives")
}

/// A statement's place in evaluation order: each enclosing loop's line and copy index, then its
/// own line. Every statement is one line, so the line is unique in its file.
fn position(sc: &Scope, span: Span) -> Vec<u32> {
    let mut at: Vec<u32> = sc
        .scope_loops
        .iter()
        .zip(&sc.scope_copies)
        .flat_map(|(l, &i)| [l.line, i])
        .collect();
    at.push(span.line);
    at
}

fn is_copy_index(seg: &str) -> bool {
    !seg.is_empty() && seg.chars().all(|c| c.is_ascii_digit())
}

/// An `fx` line under a taken `if`, with the bindings in effect where the branch reached it.
pub(crate) struct BranchFx {
    line: ast::Fx,
    env: Vec<(String, Value)>,
}

/// A block's direct `fx` lines and those under its taken `if`s, merged by line, each with the
/// scope it evaluates in. One block is one file.
pub(crate) fn in_source_order<'a>(
    direct: impl Iterator<Item = &'a ast::Fx>,
    branch: &'a [BranchFx],
    scope: &Scope,
) -> Vec<(&'a ast::Fx, Scope)> {
    let mut all: Vec<(&ast::Fx, Scope)> = direct
        .map(|line| (line, scope.clone()))
        .chain(branch.iter().map(|b| {
            let mut sc = scope.clone();
            sc.env = b.env.clone();
            (&b.line, sc)
        }))
        .collect();
    all.sort_by_key(|(line, _)| line.span);
    all
}

/// The number of every `fx` line a draw, def, or call block owns, by the line's span: its
/// direct lines and those under its `if` arms, taken or not, counted from 1 in source order.
/// A mask under the line keys on this number, so a branch skipped this frame moves no other
/// line's mask.
pub(crate) fn fx_ordinals<'a>(
    direct: impl Iterator<Item = &'a ast::Fx>,
    scenes: impl Iterator<Item = &'a Scene>,
) -> BTreeMap<Span, u32> {
    fn under_ifs<'a>(scenes: impl Iterator<Item = &'a Scene>, out: &mut Vec<Span>) {
        for s in scenes {
            match &s.kind {
                SceneKind::If {
                    then, otherwise, ..
                } => {
                    under_ifs(then.iter(), out);
                    under_ifs(otherwise.iter(), out);
                }
                SceneKind::Fx(f) => out.push(f.span),
                _ => {}
            }
        }
    }
    let mut spans: Vec<Span> = direct.map(|f| f.span).collect();
    under_ifs(scenes, &mut spans);
    spans.sort();
    spans
        .into_iter()
        .enumerate()
        .map(|(i, span)| (span, i as u32 + 1))
        .collect()
}

fn holds_play(scenes: &[Scene]) -> bool {
    scenes.iter().any(|st| match &st.kind {
        SceneKind::Play { name, .. } => name.is_some(),
        SceneKind::If {
            then, otherwise, ..
        } => holds_play(then) || holds_play(otherwise),
        SceneKind::Each { body, .. } => holds_play(body),
        SceneKind::Let { .. }
        | SceneKind::Check { .. }
        | SceneKind::Slot { .. }
        | SceneKind::Draw(_)
        | SceneKind::Fx(_) => false,
    })
}

/// A percent rest and a numeric offset are different units. Resolve only the percent
/// axes the offset measures in artboard units, and leave a percent offset on a percent rest.
fn ground_percent_rest(rest: &Value, offset: &Value, frame: Rect, sc: &Scope) -> R<Value> {
    let (Some((rx, ry)), Some((ox, oy))) = (rest.pair(), offset.pair()) else {
        return Ok(rest.clone());
    };
    let (fx, fy, fw, fh) = (
        frame.left as f64,
        frame.top as f64,
        frame.width() as f64,
        frame.height() as f64,
    );
    let axis = |rest: Coord, offset: Coord, min: f64, size: f64| -> R<Coord> {
        if matches!((rest, offset), (Coord::Pct(_), Coord::Num(_))) {
            check_percent(rest, frame, sc)?;
            Ok(Coord::Num(rest.pos(min, size)))
        } else {
            Ok(rest)
        }
    };
    let x = axis(rx, ox, fx, fw)?;
    let y = axis(ry, oy, fy, fh)?;
    Ok(match rest {
        Value::Size(..) => Value::Size(x, y),
        _ => Value::Vec2(x, y),
    })
}

fn blend_arg(
    a: &mut Args,
    key: &str,
    absolute: bool,
    value: &Value,
    default_at: (f64, f64),
    frame: Rect,
    sc: &Scope,
) -> R<()> {
    if absolute || !offset_channel(key) {
        a.named.push((key.to_string(), value.clone()));
        return Ok(());
    }
    match key {
        "rot" => {
            let base = a.rot()?;
            let d = value.num().ok_or_else(|| {
                fail(
                    "E0320",
                    format!("`rot:` offset expects a number, got {}", value.kind()),
                )
            })?;
            a.named.push(("rot".into(), Value::Num(base + d)));
        }
        "at" => {
            let base = a.get("at").cloned().unwrap_or(Value::Vec2(
                Coord::Num(default_at.0),
                Coord::Num(default_at.1),
            ));
            // A rest `at:` that is still a reference names a node, and `transform` reports it.
            if matches!(base, Value::Ref(_)) {
                return Ok(());
            }
            let base = ground_percent_rest(&base, value, frame, sc)?;
            let sum = arith(Arith::Add, &base, value).map_err(|m| fail("E0320", m))?;
            a.named.push(("at".into(), sum));
        }
        "scale" => {
            let base = a
                .get("scale")
                .cloned()
                .unwrap_or(Value::Vec2(Coord::Num(1.0), Coord::Num(1.0)));
            let factor = match value {
                Value::Num(n) => Value::Vec2(Coord::Num(*n), Coord::Num(*n)),
                other => other.clone(),
            };
            let prod = arith(Arith::Mul, &base, &factor).map_err(|m| fail("E0320", m))?;
            a.named.push(("scale".into(), prod));
        }
        _ => a.named.push((key.to_string(), value.clone())),
    }
    Ok(())
}

pub(crate) fn ident_of(e: &Expr) -> Option<&str> {
    match e {
        Expr::Ident(s) => Some(s),
        _ => None,
    }
}

fn is_name(e: &Expr) -> bool {
    ident_of(e).is_some_and(|s| identity::Name::parse(s).is_some())
}

fn mat_trs(
    at: (f64, f64),
    pivot: (f64, f64),
    rot: f64,
    skew: (f64, f64),
    scale: (f64, f64),
) -> Matrix {
    let f = |v: f64| v as f32;
    Matrix::translate((f(at.0), f(at.1)))
        * Matrix::translate((f(pivot.0), f(pivot.1)))
        * Matrix::rotate_deg(f(rot))
        * Matrix::skew((f(skew.0.to_radians().tan()), f(skew.1.to_radians().tan())))
        * Matrix::scale((f(scale.0), f(scale.1)))
        * Matrix::translate((f(-pivot.0), f(-pivot.1)))
}

pub(crate) fn new_node(path: String, origin: String, transform: Matrix, kind: NodeKind) -> Node {
    Node {
        path,
        origin,
        transform: transform.to_ir(),
        warp: Warp::default(),
        opacity: 1.0,
        blend: Blend::Normal,
        clip: None,
        mask: None,
        effects: Vec::new(),
        kind,
    }
}

/// The frame a node's content draws in: `m` under the parent and, when the line sets `warp:`,
/// a warp link that `finish_node` fills once the body closes.
fn content_place(parent: &Place, a: &Args, m: Matrix) -> (Place, Option<Rc<OnceCell<Warp>>>) {
    let place = parent.inner(m);
    if a.get("warp").is_none() {
        return (place, None);
    }
    let (place, cell) = place.inner_warp();
    (place, Some(cell))
}

/// A grid cell's offset is a translation, so it changes every measurement but a size.
fn moves(m: &str) -> bool {
    !matches!(m, "w" | "h" | "area")
}

fn record_ids(names: &Rc<RefCell<Names>>) -> BTreeSet<usize> {
    names
        .borrow()
        .records()
        .iter()
        .map(|r| Rc::as_ptr(r) as usize)
        .collect()
}

fn records_after(names: &Rc<RefCell<Names>>, seen: &BTreeSet<usize>) -> Vec<Rc<RefCell<Record>>> {
    names
        .borrow()
        .records()
        .into_iter()
        .filter(|r| !seen.contains(&(Rc::as_ptr(r) as usize)))
        .collect()
}

/// `roots` and the records in every scope they open.
fn placed_tree(roots: &[Rc<RefCell<Record>>]) -> Vec<Rc<RefCell<Record>>> {
    let mut stack: Vec<Rc<RefCell<Record>>> = roots.to_vec();
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    while let Some(rec) = stack.pop() {
        if !seen.insert(Rc::as_ptr(&rec) as usize) {
            continue;
        }
        if let Some(inner) = rec.borrow().inner.clone() {
            stack.extend(inner.borrow().records());
        }
        out.push(rec);
    }
    out
}

/// Without `cell:`, `grid` learns its pitch from its first copy, then moves that copy's nodes.
/// Records were placed before that move, so the same translation has to land on them.
fn shift_placed(roots: &[Rc<RefCell<Record>>], base: &Place, t: &Matrix) {
    for rec in placed_tree(roots) {
        rec.borrow_mut().parent_world.shift(base, t);
    }
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
        Some(inst) => fail(
            "E0322",
            format!(
                "`{}` has no size, so {pct} has nothing to measure",
                inst.def_name
            ),
        )
        .fix(format!(
            "declare the box with `size:`, as in `def {} size:72x32`",
            inst.def_name
        )),
        None => fail("E0322", format!("{pct} has nothing to measure"))
            .fix("use a number, or measure against a frame that has a size"),
    })
}

fn size_box(w: f64, h: f64) -> Rect {
    Rect::from_xywh((-w / 2.0) as f32, (-h / 2.0) as f32, w as f32, h as f32)
}

/// The body sees `size` in its default's type, so `size:72x32` always reads `size.w`.
fn resolve_size(
    v: &Value,
    pair_default: bool,
    frame: Rect,
    sc: &Scope,
    def_name: &str,
) -> R<(Value, Rect)> {
    let positive = |n: f64| n.is_finite() && n > 0.0;
    let reject = |got: &str| {
        fail(
            "E0320",
            format!("`size` of `{def_name}` must be positive, got {got}"),
        )
        .fix("write `size:40` or `size:72x32`")
    };
    let square = |side: f64| {
        let value = if pair_default {
            Value::Size(Coord::Num(side), Coord::Num(side))
        } else {
            Value::Num(side)
        };
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
                return Err(fail(
                    "E0320",
                    format!(
                        "`size` of `{def_name}` expects a number or a size, got {}",
                        v.kind()
                    ),
                )
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

fn each_items(it: Value) -> R<Vec<Value>> {
    if let Value::Range(r) = &it {
        let prefix: Vec<Value> = r.steps().take(100_001).map(Value::Num).collect();
        if prefix.len() > 100_000 {
            return Err(fail("E0306", "`each` is limited to 100000 iterations"));
        }
        return Ok(prefix);
    }
    let items = it.items().ok_or_else(|| {
        fail(
            "E0306",
            format!("`each` needs a list or range, got {}", it.kind()),
        )
        .fix("write `each x in [a b]` or `each i in 0..8`")
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
    styles.get(sname).and_then(|s| {
        s.named
            .iter()
            .rev()
            .find(|n| n.key == "stroke")
            .map(|n| n.value.clone())
    })
}

fn range_width(stroke: Option<&Expr>) -> Fail {
    let rewrite = match stroke {
        Some(Expr::Range(a, b)) => format!("stroke:[{} {}]", format_expr(a), format_expr(b)),
        Some(Expr::Tuple(xs)) if xs.len() == 2 => match &xs[1] {
            Expr::Range(a, b) => format!(
                "stroke:{},[{} {}]",
                format_expr(&xs[0]),
                format_expr(a),
                format_expr(b)
            ),
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

/// The first shape color under a node, used for effect defaults. A shape with no paint of its
/// own, such as a dotted line, takes its stamps' color.
pub(crate) fn first_fill(n: &Node) -> Option<Rgba> {
    match &n.kind {
        NodeKind::Shape(s) => s
            .fill
            .as_ref()
            .map(|f| &f.paint)
            .or(s.stroke.as_ref().map(|st| &st.paint))
            .map(key_color)
            .or_else(|| s.stamps.iter().find_map(first_fill)),
        NodeKind::Group { children, .. } => children.iter().find_map(first_fill),
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
            artboard: job.artboard.rect().to_sk(),
            background: None,
            check: None,
            ids: HashMap::new(),
            incoming: Vec::new(),
            reported: BTreeSet::new(),
            uses: identity::PositionalUses::default(),
            seed_file: String::new(),
            seed_key: Key::root(),
            seed_count: 0,
            style_hits: Vec::new(),
            accept_family: false,
            active_fns: Vec::new(),
            patch_order: 0,
            unstable_calls: BTreeSet::new(),
            time: job.frame_t,
            anim_clock: None,
            timed: None,
            plays: Vec::new(),
            guards: HashMap::new(),
            prepass: false,
            silence: false,
            rejected_parameters: false,
            placed: RefCell::new(HashMap::new()),
            stamping: Vec::new(),
            blocked: Vec::new(),
            broken: BTreeSet::new(),
            told_failed: BTreeSet::new(),
        }
    }

    pub fn report(&mut self, f: Fail, file: &str, span: Span, node: &str) {
        if self.silence || f.code.is_empty() {
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

    pub(crate) fn begin_seed(&mut self, file: &str, key: Key) {
        self.seed_file = file.to_string();
        self.seed_key = key;
        self.seed_count = 0;
    }

    fn seed_stmt(&mut self, sc: &Scope, span: Span) {
        let tag = sc.ids.tag(span).unwrap_or_default();
        let key = sc.parent.at_stmt(&sc.parent_copies, tag);
        self.begin_seed(sc.file(), key);
    }

    fn seed_node(&mut self, file: &str, key: &Key) {
        self.begin_seed(file, key.clone());
    }

    /// Draw-site `rand`, `pick`, and noise. The counter is how many of those this statement drew.
    pub fn next_seed(&mut self, explicit: Option<u64>) -> u64 {
        self.seed_count += 1;
        let key = self.seed_key.draw(self.seed_count);
        identity::seed(&self.seed_file, &key, explicit, &mut self.uses)
    }

    pub(crate) fn seeded(&self) -> &BTreeSet<String> {
        self.uses.seeded()
    }

    pub(crate) fn positional(&self) -> &identity::PositionalUses {
        &self.uses
    }

    pub(crate) fn snap_seed(&self) -> SeedSnap {
        SeedSnap {
            file: self.seed_file.clone(),
            key: self.seed_key.clone(),
            count: self.seed_count,
        }
    }

    pub(crate) fn restore_seed(&mut self, snap: SeedSnap) {
        self.seed_file = snap.file;
        self.seed_key = snap.key;
        self.seed_count = snap.count;
    }

    fn next_order(&mut self) -> u32 {
        let order = self.patch_order;
        self.patch_order += 1;
        order
    }

    pub(crate) fn unstable_scope(&mut self, key: &Key, region: &identity::Region, file: &str, span: Span) {
        if key.is_stable() || region.is_empty() {
            return;
        }
        if !self
            .unstable_calls
            .insert((file.to_string(), span.line, span.col))
        {
            return;
        }
        self.report(
            fail(
                "E0312",
                "this call has no stable name, so a name inside it has nothing stable to hang from",
            )
            .fix("add name:"),
            file,
            span,
            &key.to_string(),
        );
    }

    // ---------------------------------------------------------------- modules

    pub fn parse(&mut self, file: &str) -> R<Rc<File>> {
        if let Some(f) = self.parsed.get(file) {
            return Ok(f.clone());
        }
        let src = self
            .project
            .source(file)
            .ok_or_else(|| fail("E0210", format!("no file `{file}`")))?;
        match ramose_syntax::parse(src) {
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
        // `plan` reports the file's own errors; an importer hears once per job that it is gone.
        if let Some(first) = self.project.outlines().first_error(file) {
            if file == self.job.source || !self.told_failed.insert(file.to_string()) {
                return Err(Fail::reported());
            }
            return Err(fail("E0214", format!("`{file}` has a parameter type error, so nothing it declares can be used"))
                .note(format!("first error: {}:{}:{} {} {}", first.file, first.span.line, first.span.col, first.code, first.message))
                .fix(format!("fix the error at {}:{}", first.file, first.span.line)));
        }
        let parsed = self.parse(file)?;
        let mut parameter_names = BTreeSet::new();
        for decl in parsed.decls() {
            if let DeclKind::Param { params } = &decl.kind {
                if !entry {
                    self.report(fail("E0320", "param declarations require an entry file with @target"), file, decl.span, "");
                }
                for p in params {
                    let reserved = ["target", "t", "clip", "scale", "artboard", "p", "true", "false", "none"];
                    let collision = reserved.contains(&p.key.as_str())
                        || (file == self.job.source && self.job.matrix.iter().any(|(k, _)| k == &p.key))
                        || parsed.scenes().any(|s| matches!(&s.kind, SceneKind::Let { name, .. } if name == &p.key));
                    if !parameter_names.insert(p.key.clone()) || collision {
                        self.report(fail("E0320", format!("parameter `{}` collides with another binding", p.key)), file, p.span, "");
                    }
                }
            }
        }
        if file == self.job.source {
            for name in self.job.parameters.keys() {
                if !parameter_names.contains(name) {
                    self.rejected_parameters = true;
                    self.report(fail("E0320", format!("unknown entry parameter `{name}` in --set"))
                        .fix("declare it with `param name:default`, or remove --set"), file, Span::default(), "");
                }
            }
        }
        self.loading.push(file.to_string());
        let mut module = Module::new(file);
        module.bundled = self.project.is_bundled(file);
        let module = Rc::new(module);
        if let Some(pal) = self.project.manifest.palette.clone()
            && let Some(pfile) = self.project.resolve_use(&pal)
            && pfile != file
        {
            match self.load_module(&pfile, false) {
                Ok(m) => {
                    module
                        .globals
                        .borrow_mut()
                        .insert("p".into(), Value::Module(m));
                }
                Err(f) => {
                    module.lost.borrow_mut().insert("p".into());
                    self.report(f, file, Span { line: 1, col: 1 }, "")
                }
            }
        }
        for item in &parsed.items {
            if let Err(f) = self.declare(&module, item, entry) {
                self.report(f, file, item.span(), "");
            }
        }
        self.declare_aliases(&module, &parsed);
        for name in &parameter_names {
            if (module.own.borrow().iter().any(|own| own == name || own == &format!("style:{name}") || own == &format!("finish:{name}")) || module.anims.borrow().contains_key(name)) && !self.diags.iter().any(|d| d.file == file && d.message == format!("parameter `{name}` collides with another binding")) {
                let span = parsed.decls().find_map(|d| match &d.kind {
                    DeclKind::Param { params } => params.iter().find(|p| &p.key == name).map(|p| p.span),
                    _ => None,
                }).unwrap_or_default();
                self.report(fail("E0320", format!("parameter `{name}` collides with another binding")), file, span, "");
            }
        }
        self.check_fn_cycles(&module, &parsed);
        let components = component_names(&module);
        let mut id_diags = Vec::new();
        let aliases: BTreeMap<String, String> = module
            .globals
            .borrow()
            .iter()
            .filter_map(|(name, v)| match v {
                Value::Alias(a) => Some((name.clone(), a.comp.name.clone())),
                _ => None,
            })
            .collect();
        let ids = ModuleIds::build(file, &parsed, &components, &aliases, &mut id_diags);
        self.diags.extend(id_diags);
        self.ids.insert(file.to_string(), Rc::new(ids));
        self.loading.pop();
        self.modules.insert(file.to_string(), module.clone());
        Ok(module)
    }

    fn declare(&mut self, module: &Rc<Module>, item: &Item, entry: bool) -> R<()> {
        let decl = match item {
            Item::Decl(d) => d,
            Item::Scene(Scene {
                kind: SceneKind::Let { name, value },
                ..
            }) if !entry => {
                let sc = self.module_scope(module);
                self.begin_seed(&module.path, Key::binding(name));
                let v = self.expr(value, &sc).inspect_err(|_| {
                    module.lost.borrow_mut().insert(name.clone());
                })?;
                module.define(name, v);
                return Ok(());
            }
            Item::Scene(Scene {
                kind: SceneKind::Let { name, .. },
                ..
            }) if entry && module.path != self.job.source => {
                module.skipped.borrow_mut().insert(name.clone());
                return Ok(());
            }
            Item::Scene(_) if entry => return Ok(()),
            Item::Scene(_) => {
                return Err(fail(
                    "E0204",
                    "a library may only hold declarations at its top level",
                )
                .note("allowed: use, palette, style, finish, def, rule, fn, let, anim")
                .fix("wrap the drawing in a `def` and place it from an entry file"));
            }
        };
        match &decl.kind {
            DeclKind::Param { params } => {
                for p in params {
                    if (module.global(&p.key).is_some() && !module.parameters.borrow().contains(&p.key))
                        || module.styles.borrow().contains_key(&p.key)
                        || module.finishes.borrow().contains_key(&p.key)
                        || module.anims.borrow().contains_key(&p.key)
                    {
                        self.report(fail("E0320", format!("parameter `{}` collides with another binding", p.key)), &module.path, p.span, "");
                        continue;
                    }
                    self.begin_seed(&module.path, Key::binding(&p.key));
                    let scope = self.module_scope(module);
                    let overridden = module.path == self.job.source && self.job.parameters.contains_key(&p.key);
                    let bound = match self.bind_entry_param(p, module, &scope) {
                        Ok(bound) => bound,
                        Err(error) => {
                            self.rejected_parameters |= overridden;
                            self.report(error, &module.path, p.span, "");
                            module.lost.borrow_mut().insert(p.key.clone());
                            continue;
                        }
                    };
                    for (key, value) in bound {
                        module.parameters.borrow_mut().insert(key.clone());
                        module.bind(&key, value);
                    }
                }
                Ok(())
            }
            DeclKind::Use { path, alias, items } => {
                let declared = self.declare_use(module, path, alias, items);
                if declared.is_err() {
                    match alias {
                        Some(a) => {
                            module.lost.borrow_mut().insert(a.clone());
                        }
                        None if items.is_empty() => module.lost_all.set(true),
                        None => module.lost.borrow_mut().extend(items.iter().cloned()),
                    }
                }
                declared
            }
            DeclKind::Palette { name, from, body } => {
                let pal = self.palette(module, name, from.as_deref(), body)?;
                module.palettes.borrow_mut().push(pal.clone());
                module.define(name, Value::Palette(pal));
                Ok(())
            }
            DeclKind::Style { name, named } => {
                module.styles.borrow_mut().insert(
                    name.clone(),
                    Rc::new(StyleDef {
                        module: module.path.clone(),
                        named: named.clone(),
                    }),
                );
                module.own.borrow_mut().push(format!("style:{name}"));
                Ok(())
            }
            DeclKind::Finish { name, params, body } => {
                self.declare_finish(module, name, params, body, decl.span)
            }
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
                    sig: self.signature_of(&module.path, name),
                    needs,
                    body: body.clone(),
                    module: module.path.clone(),
                    span: decl.span,
                };
                module.define(name, Value::Component(Rc::new(comp)));
                Ok(())
            }
            DeclKind::Alias { name, target, .. } => {
                if module.global(target).is_none() {
                    return Ok(());
                }
                let mut done = BTreeSet::new();
                self.bind_alias(module, &[], decl, &mut Vec::new(), &mut done)
                    .inspect_err(|_| {
                        module.lost.borrow_mut().insert(name.clone());
                    })
            }
            DeclKind::Rule { name, params, body } => {
                let rule = RuleDef {
                    name: name.clone(),
                    params: params.clone(),
                    sig: self.signature_of(&module.path, name),
                    body: body.clone(),
                    module: module.path.clone(),
                    span: decl.span,
                };
                module.define(name, Value::Rule(Rc::new(rule)));
                Ok(())
            }
            DeclKind::Fn { name, params, body } => self
                .declare_fn(module, name, params, body, decl.span)
                .inspect_err(|_| {
                    module.lost.borrow_mut().insert(name.clone());
                }),
            DeclKind::Anim {
                name,
                dur,
                loop_,
                body,
            } => {
                let anim = Rc::new(AnimDef {
                    name: name.clone(),
                    dur: dur.clone(),
                    loop_: *loop_,
                    body: body.clone(),
                    module: module.path.clone(),
                    span: decl.span,
                });
                module.anims.borrow_mut().insert(name.clone(), anim);
                Ok(())
            }
            DeclKind::Brush { name, params, sksl } => {
                self.declare_brush(module, name, params, sksl.as_ref(), decl.span)
            }
            DeclKind::Stamp { name, params, body } => self.declare_stamp(module, name, params, body, decl.span),
            DeclKind::Light { .. } => Err(
                fail("E0390", "light (Phase 2) is not yet implemented in v0.1")
                    .fix("remove the declaration, or use built-in fields and fx lines"),
            ),
        }
    }

    fn declare_use(
        &mut self,
        module: &Rc<Module>,
        path: &str,
        alias: &Option<String>,
        items: &[String],
    ) -> R<()> {
        let rooted = format!("{path}.ramo");
        let target = match self.project.resolve_use(path) {
            Some(f) => f,
            None if self.project.files.contains_key(&rooted) => rooted,
            None => {
                let f = fail("E0210", format!("no library `{path}`")).note(format!(
                    "libraries: {}",
                    self.project.library_names().join(", ")
                ));
                return Err(if path.starts_with("std/") {
                    f.fix("`std/` names only the bundled libraries; `ramose std` lists them")
                } else {
                    f.fix(format!(
                        "create `{}/{path}.ramo`, or fix the path",
                        self.project.manifest.lib[0]
                    ))
                });
            }
        };
        let parsed = self.parse(&target)?;
        let m = self.load_module(&target, is_entry(&parsed))?;
        if let Some(a) = alias {
            if module.parameters.borrow().contains(a) {
                return Err(fail("E0320", format!("import `{a}` collides with an entry parameter")));
            }
            module.bind(a, Value::Module(m));
            return Ok(());
        }
        let exports = m.exports();
        let styles = m.styles.borrow().clone();
        let finishes = m.finishes.borrow().clone();
        let anims = m.anims.borrow().clone();
        for name in module.parameters.borrow().iter() {
            let exported = exports.iter().any(|(key, _)| key == name)
                || (!name.starts_with('_') && (styles.contains_key(name) || finishes.contains_key(name) || anims.contains_key(name)));
            if (items.is_empty() && exported) || items.contains(name) {
                return Err(fail("E0320", format!("import `{name}` collides with an entry parameter")));
            }
        }
        // A declaration the library already reported as failed stays quiet at its use sites here.
        let lost = m.lost.borrow().clone();
        if items.is_empty() {
            for (k, v) in exports {
                module.bind(&k, v);
            }
            module.lost.borrow_mut().extend(lost);
            for (k, s) in styles.into_iter().filter(|(k, _)| !k.starts_with('_')) {
                module.styles.borrow_mut().insert(k, s);
            }
            for (k, f) in finishes.into_iter().filter(|(k, _)| !k.starts_with('_')) {
                module.finishes.borrow_mut().insert(k, f);
            }
            for (k, a) in anims.into_iter().filter(|(k, _)| !k.starts_with('_')) {
                module.anims.borrow_mut().insert(k, a);
            }
            return Ok(());
        }
        for item in items {
            let mut found = false;
            if let Some((_, v)) = exports.iter().find(|(k, _)| k == item) {
                module.bind(item, v.clone());
                found = true;
            }
            if lost.contains(item) {
                module.lost.borrow_mut().insert(item.clone());
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
            if !item.starts_with('_')
                && let Some(a) = anims.get(item)
            {
                module.anims.borrow_mut().insert(item.clone(), a.clone());
                found = true;
            }
            if !found {
                let names: Vec<String> = exports.iter().map(|(k, _)| k.clone()).collect();
                return Err(fail("E0212", format!("`{path}` does not export `{item}`"))
                    .note(format!("exports: {}", names.join(", ")))
                    .fix(
                        "import one of the exported names (names starting with `_` are private)",
                    ));
            }
        }
        Ok(())
    }

    /// The typed parameters of a def, brush, stamp, or rule this project declares.
    pub(crate) fn signature_of(&self, file: &str, name: &str) -> Arc<Signature> {
        self.project
            .outlines()
            .signature(&DeclRef::new(file, name))
            .cloned()
            .expect("the outline holds every declaration of a parsed file")
    }

    fn scope_shell(&self, module: &Rc<Module>, ids: Rc<ModuleIds>) -> Scope {
        let names = Rc::new(RefCell::new(Names::new(ids.file())));
        Scope {
            env: Vec::new(),
            timed: BTreeMap::new(),
            typed_params: Rc::default(),
            module: module.clone(),
            owner: Key::root(),
            parent: Key::root(),
            scope_copies: Vec::new(),
            parent_copies: Vec::new(),
            scope_loops: Vec::new(),
            lookup: Lookup::own(names.clone()),
            names,
            pending: Rc::new(Pending::empty()),
            ids,
            depth: 0,
            frame: Rect::new_empty(),
            world: Place::identity(),
            style: Rc::new(Vec::new()),
            palette: None,
            inst: None,
            host: None,
            checks: Rc::new(RefCell::new(Vec::new())),
            branch_fx: None,
            site: None,
            provisional: None,
            operand_of: None,
            top: false,
            publish: false,
        }
    }

    pub(crate) fn module_scope(&self, module: &Rc<Module>) -> Scope {
        self.scope_shell(module, Rc::new(ModuleIds::blank()))
    }

    pub fn top_scope(&self, module: &Rc<Module>) -> Scope {
        let ids = self
            .ids
            .get(&module.path)
            .cloned()
            .unwrap_or_else(|| Rc::new(ModuleIds::blank()));
        Scope {
            frame: self.artboard,
            top: true,
            publish: true,
            ..self.scope_shell(module, ids)
        }
    }

    fn palette(
        &mut self,
        module: &Rc<Module>,
        name: &str,
        from: Option<&str>,
        body: &[Swatch],
    ) -> R<Rc<Palette>> {
        let mut entries = Vec::new();
        if let Some(parent) = from {
            match module.global(parent) {
                Some(Value::Palette(p)) => entries.extend(p.entries.iter().cloned()),
                _ => {
                    return Err(
                        fail("E0304", format!("no palette `{parent}` to inherit from"))
                            .fix("define or import the parent palette first"),
                    );
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
        Ok(Rc::new(Palette {
            name: name.to_string(),
            entries,
        }))
    }

    fn swatch(&mut self, kind: &SwatchKind, sc: &Scope) -> R<Value> {
        match kind {
            SwatchKind::Ramp { pos, hue } => {
                let vals: Vec<Value> = pos
                    .exprs()
                    .into_iter()
                    .map(|e| self.expr(e, sc))
                    .collect::<R<_>>()?;
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
        let colors: Vec<Rgba> = vals
            .iter()
            .filter_map(|v| v.color().map(|c| c.rgba))
            .collect();
        let n = vals
            .iter()
            .rev()
            .find_map(Value::num)
            .unwrap_or(5.0)
            .max(1.0) as usize;
        let ramp = match colors.as_slice() {
            [c] => Ramp::single(*c, n, hue),
            [a, b] => Ramp::two(*a, *b, n),
            _ => {
                return Err(fail("E0305", "`ramp` takes one or two colors and a count")
                    .fix("write `ramp #f3a2a6 5`"));
            }
        };
        Ok(Value::Ramp(Rc::new(ramp)))
    }

    // ---------------------------------------------------------------- statements

    pub fn eval_block_nodes<'s>(
        &mut self,
        stmts: impl IntoIterator<Item = &'s Scene>,
        sc: &mut Scope,
    ) -> Vec<Node> {
        let mut sib = Siblings::default();
        self.eval_block(stmts, sc, &mut sib);
        sib.finish()
    }

    fn quiet_expr(&mut self, e: &Expr, sc: &Scope) -> R<Value> {
        let silence = self.silence;
        self.silence = true;
        let v = self.expr(e, sc);
        self.silence = silence;
        v
    }

    /// Runs `f` and returns the anim patch its reads followed, leaving the enclosing read as it was.
    pub(crate) fn timed_by<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> (T, Option<Rc<Timed>>) {
        let outer = self.timed.take();
        let v = f(self);
        (v, std::mem::replace(&mut self.timed, outer))
    }

    /// Evaluates an expression that decides the scene's structure, where a value that follows an
    /// anim patch is E0345.
    pub(crate) fn untimed(&mut self, what: &str, e: &Expr, sc: &Scope) -> R<Value> {
        let (v, by) = self.timed_by(|ev| ev.expr(e, sc));
        let v = v?;
        match by {
            Some(by) => Err(by.sink(what)),
            None => Ok(v),
        }
    }

    fn bind_let(&mut self, sc: &mut Scope, name: &str, value: &Expr) -> R<Value> {
        let (v, by) = self.timed_by(|ev| ev.expr(value, sc));
        let v = v?;
        match by {
            Some(by) => sc.timed.insert(name.to_string(), by),
            None => sc.timed.remove(name),
        };
        sc.env.push((name.to_string(), v.clone()));
        Ok(v)
    }

    fn eval_block<'s>(
        &mut self,
        stmts: impl IntoIterator<Item = &'s Scene>,
        sc: &mut Scope,
        out: &mut Siblings,
    ) {
        let stmts: Vec<&Scene> = stmts.into_iter().collect();
        let mut slots: Vec<Siblings> = stmts.iter().map(|_| Siblings::default()).collect();
        self.run_block(&stmts, sc, |ev, st, sc, i| ev.eval_stmt(st, sc, &mut slots[i]));
        out.items.extend(slots.into_iter().flat_map(|s| s.items));
    }

    /// Runs a block's statements in the order `order::plan` gives. Each one sees the `let`s
    /// written above it, however early or late it runs.
    fn run_block(
        &mut self,
        stmts: &[&Scene],
        sc: &mut Scope,
        mut run: impl FnMut(&mut Self, &Scene, &mut Scope, usize) -> R<()>,
    ) {
        let region = sc.names.borrow().region();
        let entry = sc.file() == self.job.source;
        let base = sc.env.len();
        let mut bound: Vec<Vec<(String, Value)>> = vec![Vec::new(); stmts.len()];
        let mut in_order = true;
        let mut prev = None;
        for step in order::plan(stmts, &region) {
            let i = step.stmt;
            in_order &= prev.is_none_or(|p| p < i);
            prev = Some(i);
            if !in_order {
                sc.env.truncate(base);
                sc.env.extend(bound[..i].iter().flatten().cloned());
            }
            let mark = sc.env.len();
            let st = stmts[i];
            if entry {
                sc.site = Some(st.span.line);
            }
            let before = self.diags.len();
            let outer = self.blocked.len();
            let forced = !step.blocked.is_empty();
            self.blocked.extend(step.blocked);
            let result = run(self, st, sc, i);
            self.blocked.truncate(outer);
            if let Err(f) = result {
                if forced {
                    let sites = order::declared_in(st, &region)
                        .into_iter()
                        .map(|d| region.decl(d).site.clone());
                    self.broken.extend(sites);
                }
                let node = sc.node_name();
                let file = sc.file().to_string();
                self.report(f, &file, st.span, &node);
            }
            if entry {
                self.mark_site(before, st.span.line);
            }
            bound[i] = sc.env[mark..].to_vec();
        }
        if !in_order {
            sc.env.truncate(base);
            sc.env.extend(bound.into_iter().flatten());
        }
    }

    /// Notes the entry-file line on each diagnostic since `before` that a library raised.
    pub(crate) fn mark_site(&mut self, before: usize, line: u32) {
        let entry = &self.job.source;
        for d in self.diags.iter_mut().skip(before) {
            if d.file != *entry && !d.notes.iter().any(|n| crate::is_entry_site(n)) {
                d.notes.push(format!("in {entry}:{line}"));
            }
        }
    }

    fn eval_stmt(&mut self, st: &Scene, sc: &mut Scope, out: &mut Siblings) -> R<()> {
        match &st.kind {
            SceneKind::Let { name, value } => {
                self.seed_stmt(sc, st.span);
                let v = match self.bind_let(sc, name, value) {
                    Ok(v) => v,
                    Err(f) => {
                        sc.env.push((name.clone(), Value::None));
                        return Err(f);
                    }
                };
                if sc.top && sc.publish {
                    sc.module.bind(name, v);
                }
                Ok(())
            }
            SceneKind::If {
                cond,
                then,
                otherwise,
            } => {
                self.seed_stmt(sc, st.span);
                let c = self.untimed("this `if`", cond, sc)?;
                self.confirm_guard(st, "if", usize::from(c.truthy()), sc);
                let mut inner = sc.clone();
                inner.publish = false;
                self.eval_block(if c.truthy() { then } else { otherwise }, &mut inner, out);
                Ok(())
            }
            SceneKind::Each {
                index,
                var,
                iter,
                body,
            } => {
                self.seed_stmt(sc, st.span);
                let items = each_items(self.untimed("this `each`", iter, sc)?)?;
                self.confirm_guard(st, "each", items.len(), sc);
                for (i, v) in items.into_iter().enumerate() {
                    let mut inner = sc.iteration(st.span, i as u32);
                    inner.publish = false;
                    inner.bind_loop(index.as_deref(), var, i, v);
                    self.eval_block(body, &mut inner, out);
                }
                Ok(())
            }
            SceneKind::Check { .. } => {
                sc.checks.borrow_mut().push(CheckPending {
                    stmt: st.clone(),
                    scope: sc.clone(),
                });
                Ok(())
            }
            SceneKind::Slot {
                name,
                named,
                fallback,
            } => self.slot(st, name, named, fallback, sc, out),
            SceneKind::Play { .. } if sc.top => Ok(()),
            SceneKind::Play { .. } => Err(fail(
                "E0342",
                "`play` is only valid at the top level of an entry file or under an instance",
            )
            .fix("move `play` out of this block, or indent it under the instance it drives")),
            SceneKind::Draw(draw) => self.eval_node(st, draw, sc, out),
            SceneKind::Fx(line) => sc.branch_fx(line),
        }
    }

    pub(crate) fn eval_args(
        &mut self,
        args: &[Expr],
        named: &[Named],
        sc: &Scope,
        skip_pos: usize,
    ) -> R<Args> {
        let mut out = Args::default();
        for e in args.iter().skip(skip_pos) {
            let (v, by) = self.timed_by(|ev| ev.expr(e, sc));
            if let Some(by) = by {
                out.timed_pos.push((out.pos.len(), by));
            }
            out.pos.push(v?);
        }
        for n in named {
            if n.key == "name" {
                continue;
            }
            let (v, by) = self.timed_by(|ev| ev.expr(&n.value, sc));
            if let Some(by) = by {
                out.timed.push((n.key.clone(), by));
            }
            out.named.push((n.key.clone(), v?));
        }
        Ok(out)
    }

    fn style_named(
        &mut self,
        name: &str,
        home: &Rc<Module>,
        sc: &Scope,
    ) -> R<Vec<(String, Value)>> {
        if home.lost(name) && !home.styles.borrow().contains_key(name) {
            return Err(Fail::reported());
        }
        let style = home.styles.borrow().get(name).cloned().ok_or_else(|| {
            let names: Vec<String> = home.styles.borrow().keys().cloned().collect();
            let f = fail("E0307", format!("no style `{name}`"))
                .note(format!("styles: {}", names.join(", ")));
            if home.finishes.borrow().contains_key(name) {
                f.note(format!("`{name}` is a finish"))
                    .fix(format!("write `fx {name}` under the node"))
            } else {
                f.fix("declare `style name key:value` or import it")
            }
        })?;
        let module = self
            .modules
            .get(&style.module)
            .cloned()
            .unwrap_or_else(|| home.clone());
        let mut msc = self.module_scope(&module);
        msc.palette = sc.palette.clone();
        style
            .named
            .iter()
            .map(|n| Ok((n.key.clone(), self.expr(&n.value, &msc)?)))
            .collect()
    }

    /// The last `style:` word, after patches and motion, becomes paint keys. Earlier keys stay
    /// behind those paint keys so an explicit fill still wins. A word written by an anim is
    /// resolved in that anim's module.
    fn expand_style(&mut self, a: &mut Args, sc: &Scope, path: &str) -> R<()> {
        let Some(sname) = a.word("style") else {
            return Ok(());
        };
        let site = self.style_site(path, &sname);
        let home = site
            .as_ref()
            .and_then(|(file, _, _)| self.modules.get(file).cloned())
            .unwrap_or_else(|| sc.module.clone());
        let mut styled = match (self.style_named(&sname, &home, sc), site) {
            (Ok(styled), _) => styled,
            (Err(f), Some((file, span, anim))) => {
                self.report(f.note(format!("in anim `{anim}`")), &file, span, path);
                return Ok(());
            }
            (Err(f), None) => return Err(f),
        };
        styled.append(&mut a.named);
        a.named = styled;
        Ok(())
    }

    /// The anim patch that wrote this `style:` word, as its file, key span, and anim name.
    fn style_site(&self, path: &str, word: &str) -> Option<(String, Span, String)> {
        self.style_hits
            .iter()
            .rev()
            .find(|h| h.key == path && h.word == word)
            .map(|h| (h.file.clone(), h.span, h.anim.clone()))
    }

    fn transform(
        &mut self,
        a: &Args,
        default_at: (f64, f64),
        frame: Rect,
        sc: &Scope,
        with_rot: bool,
    ) -> R<(Matrix, (f64, f64))> {
        let (fx, fy, fw, fh) = (
            frame.left as f64,
            frame.top as f64,
            frame.width() as f64,
            frame.height() as f64,
        );
        let at = match a.get("at") {
            Some(Value::Ref(r)) => self.anchor_point(r, sc)?,
            Some(v) => {
                let (x, y) = v.pair_for("at")?;
                check_percent(x, frame, sc)?;
                check_percent(y, frame, sc)?;
                (x.pos(fx, fw), y.pos(fy, fh))
            }
            None => default_at,
        };
        let pivot = match a.get("pivot").map(|v| v.pair_for("pivot")).transpose()? {
            Some((x, y)) => {
                check_percent(x, frame, sc)?;
                check_percent(y, frame, sc)?;
                (x.len(fw), y.len(fh))
            }
            None => (0.0, 0.0),
        };
        let rot = if with_rot {
            a.rot()?
        } else {
            0.0
        };
        let vector = |key: &str, default: (f64, f64)| match a.get(key) {
            None => Ok(default),
            Some(v) => v.vec2_for(key),
        };
        let scale = vector("scale", (1.0, 1.0))?;
        let skew = match a.get("skew") {
            Some(Value::Num(k)) => (*k, 0.0),
            _ => vector("skew", (0.0, 0.0))?,
        };
        if skew.0.abs() >= 90.0 || skew.1.abs() >= 90.0 {
            return Err(fail("E0320", "`skew:` must be between -90 and 90 degrees")
                .fix("a skew of 90 degrees flattens the shape to a line"));
        }
        let m = mat_trs(at, pivot, rot, skew, scale);
        if !m.is_finite() {
            return Err(fail("E0320", "this node's transform is not finite")
                .fix("check `at:`, `rot:`, `scale:` and `pivot:` for a division by zero"));
        }
        Ok((m, at))
    }

    /// The warp, opacity, blend and z order shared by every node. The warp binds to the node's
    /// content box, known only now that the body has closed, and fills `pending` so the records
    /// placed inside read through it from here on.
    fn finish_node(
        &mut self,
        node: &mut Node,
        a: &Args,
        sc: &Scope,
        pending: Option<Rc<OnceCell<Warp>>>,
    ) -> R<f64> {
        if let Some(v) = a.get("warp") {
            let stages = warp::warp_stack(v, sc)?;
            let warp = if stages.is_empty() {
                Warp::default()
            } else {
                let reference = node.reference_box().ok_or_else(|| {
                    fail("E0320", "`warp:` has nothing to warp: the node draws nothing")
                        .fix("give the node geometry or children, or remove `warp:`")
                })?;
                let pinned = warp::pinned_in(&stages, &node.transform)?;
                Warp::resolve(&pinned, reference).map_err(|WarpError::Empty(i)| {
                    fail(
                        "E0320",
                        format!(
                            "`{}` has nothing left to map: the stage before it hides all of the content",
                            warp::stage_text(&stages[i])
                        ),
                    )
                    .fix("use a larger cylinder radius, or narrower content")
                })?
            };
            if let Some(cell) = pending {
                cell.set(warp.clone()).expect("a node's warp is resolved once");
            }
            node.warp = warp;
        }
        if let Some(op) = a.num("a") {
            node.opacity = op.clamp(0.0, 1.0) as f32;
        }
        if let Some(b) = a.word("blend") {
            node.blend = blend_of(&b).ok_or_else(|| {
                fail("E0320", format!("unknown blend `{b}`"))
                    .fix("use normal, multiply, screen, overlay, soft_light, add, erase, …")
            })?;
        }
        Ok(match a.get("order") {
            Some(Value::Ident(w)) if w == "behind" => f64::NEG_INFINITY,
            Some(Value::Ident(w)) if w == "front" => f64::INFINITY,
            Some(Value::Num(n)) => *n,
            Some(other) => {
                return Err(fail(
                    "E0320",
                    format!(
                        "`order:` expects behind, front or a number, got {}",
                        other.show()
                    ),
                ));
            }
            None => 0.0,
        })
    }

    fn register(
        &self,
        sc: &Scope,
        decl: DeclId,
        key: &Key,
        mut record: Record,
    ) -> Rc<RefCell<Record>> {
        record.path = key.to_string();
        record.key = key.clone();
        record.copies = sc.scope_copies.clone();
        record.home = Some(Rc::downgrade(&sc.names));
        record.decl = Some(decl);
        let rec = Rc::new(RefCell::new(record));
        self.placed
            .borrow_mut()
            .insert(rec.borrow().path.clone(), rec.clone());
        let name = sc.names.borrow().region().decl(decl).name.clone();
        sc.names.borrow_mut().register(
            decl,
            &sc.scope_copies,
            Entry {
                name,
                record: rec.clone(),
            },
        );
        rec
    }

    #[allow(clippy::too_many_arguments)]
    fn place(
        &self,
        sc: &Scope,
        decl: DeclId,
        key: &Key,
        parent_world: Place,
        node: Option<Node>,
        anchor: Option<Point>,
        at: (f64, f64),
        inner: Option<Rc<RefCell<Names>>>,
    ) -> Rc<RefCell<Record>> {
        self.register(
            sc,
            decl,
            key,
            Record {
                path: String::new(),
                key: key.clone(),
                copies: Vec::new(),
                parent_world,
                node,
                anchor,
                at,
                home: None,
                decl: None,
                inner,
                rows: Rc::default(),
                seam: 0.0,
            },
        )
    }

    /// Returns `None` when a live `off` hides the node, else how many named args the anims appended.
    fn land(
        &mut self,
        a: &mut Args,
        own: &[PendingPatch],
        default_at: (f64, f64),
        frame: Rect,
        sc: &Scope,
        path: &str,
    ) -> Option<usize> {
        let win = |a: &&PendingPatch, b: &&PendingPatch| {
            b.origin_depth
                .cmp(&a.origin_depth)
                .then(a.order.cmp(&b.order))
        };
        let mut statics: Vec<&PendingPatch> = own.iter().filter(|p| p.channel.is_none()).collect();
        statics.sort_by(win);
        for p in statics {
            a.timed.retain(|(k, _)| !p.named.iter().any(|(n, _)| n == k));
            a.named.extend(p.named.iter().cloned());
            a.timed.extend(p.timed.iter().cloned());
        }
        if let Some(Value::Ref(r)) = a.get("at").cloned()
            && let Ok((x, y)) = self.anchor_point(&r, sc)
        {
            a.named.push(("at".into(), Value::Vec2(Coord::Num(x), Coord::Num(y))));
        }
        let rest = a.named.len();
        let mut chans: Vec<&PendingPatch> = own.iter().filter(|p| p.channel.is_some()).collect();
        chans.sort_by(win);
        let mut hide = false;
        for p in chans {
            let Some(ch) = &p.channel else { continue };
            if !ch.key.is_empty() {
                a.timed.push((
                    ch.key.clone(),
                    Rc::new(Timed {
                        key: ch.key.clone(),
                        anim: ch.anim.clone(),
                        file: ch.patch_file.clone(),
                        line: ch.patch_span.line,
                    }),
                ));
            }
            if !ch.live {
                continue;
            }
            if p.off {
                hide = true;
                continue;
            }
            if ch.key.is_empty() {
                continue;
            }
            if ch.key == "style" && let Some(word) = ch.value.word() {
                self.style_hits.push(StyleHit {
                    key: path.to_string(),
                    word: word.to_string(),
                    file: ch.patch_file.clone(),
                    span: ch.patch_span,
                    anim: ch.anim.clone(),
                });
            }
            if let Err(f) = blend_arg(a, &ch.key, ch.absolute, &ch.value, default_at, frame, sc) {
                self.report(
                    f.note(format!("in anim `{}`", ch.anim)),
                    &ch.patch_file,
                    ch.patch_span,
                    path,
                );
            }
        }
        (!hide).then_some(a.named.len() - rest)
    }

    /// A container with `at:` in its rest pose centers its children's frame on its origin.
    /// An anim offset moves the container and leaves that frame alone.
    fn child_frame(
        &mut self,
        a: &Args,
        animated: usize,
        default_at: (f64, f64),
        frame: Rect,
        sc: &Scope,
        with_rot: bool,
    ) -> R<Rect> {
        let rest = Args {
            pos: a.pos.clone(),
            named: a.named[..a.named.len() - animated].to_vec(),
            ..Args::default()
        };
        let (m, _) = self.transform(&rest, default_at, frame, sc, with_rot)?;
        let local = inverse(&m).map_rect(frame).0;
        Ok(if rest.get("at").is_some() {
            size_box(local.width() as f64, local.height() as f64)
        } else {
            local
        })
    }

    fn eval_node(&mut self, st: &Scene, draw: &Draw, sc: &mut Scope, out: &mut Siblings) -> R<()> {
        let Draw {
            verb,
            args,
            named,
            body,
        } = draw;
        let verb = verb.as_str();
        match verb {
            "anchor" => return self.anchor(st.span, args, sc),
            "artboard" if sc.top => {
                let (file, node) = (sc.file().to_string(), sc.node_name());
                for n in named.iter().filter(|n| n.key != "fill") {
                    self.report(unknown_key(verb, &["fill"], &n.key), &file, st.span, &node);
                }
                let a = self.eval_args(args, named, sc, 0)?;
                if let Some(v) = a.get("fill") {
                    self.background = v.color().map(|c| c.rgba);
                }
                return Ok(());
            }
            _ => {}
        }
        if let Some(by) = sc.timed.get(verb) {
            return Err(by.sink(&format!("the verb `{verb}`")));
        }
        let comp = self.component_for(verb, sc);
        if matches!(comp, Some(None)) {
            return Ok(());
        }
        let builtin = Builtin::of(verb);
        if comp.is_none() && builtin.is_none() {
            if sc.module.lost(verb) {
                return Err(Fail::reported());
            }
            if verb == "canvas" {
                return Err(canvas_renamed());
            }
            let mut f = fail("E0301", format!("unknown verb `{verb}`"))
                .note("shapes: rect circle ellipse poly star curve path blob copy edge subtract union intersect xor")
                .note("structure: layer group inside outside mirror repeat grid anchor slot");
            let comps: Vec<String> = component_names(&sc.module).into_iter().collect();
            if !comps.is_empty() {
                f = f.note(format!("components in scope: {}", comps.join(", ")));
            }
            return Err(f.fix(format!(
                "check the spelling, or `use` the library that defines `{verb}`"
            )));
        }
        if let Some(n) = named.iter().find(|n| n.key == "finish") {
            let name = ident_of(&n.value).unwrap_or("<name>");
            return Err(fail("E0339", "`finish:` is not a node key")
                .fix(format!("write `fx {name}` under the node")));
        }
        let positional_name = matches!(verb, "layer" | "group")
            .then(|| args.first().and_then(ident_of).map(str::to_string))
            .flatten();
        let callee = comp.flatten();
        let comp = callee.as_ref().map(|c| c.comp.clone());
        let skip = usize::from(positional_name.is_some());
        let pos_exprs: Vec<Expr> = args
            .iter()
            .skip(skip)
            .filter(|e| {
                !(matches!(ident_of(e), Some("closed"))
                    || (verb == "mirror" && matches!(ident_of(e), Some("x" | "y"))))
            })
            .cloned()
            .collect();
        if comp.is_none()
            && let Some(b) = &builtin
        {
            if pos_exprs.len() > b.takes {
                return Err(self.extra_positional(verb, b.takes, &pos_exprs, sc));
            }
            let (file, node) = (sc.file().to_string(), sc.node_name());
            let keys: Vec<&str> = b.all_keys().collect();
            for n in named.iter().filter(|n| !keys.contains(&n.key.as_str())) {
                self.report(unknown_key(verb, &keys, &n.key), &file, st.span, &node);
            }
        }
        if let Some(of) = sc.operand_of.clone() {
            self.report_operand_paint(&of, named, body, comp.as_deref(), sc);
        }
        let key = sc.node_key(st.span);
        let path = key.to_string();
        let decl = sc.decl_at(st.span);
        let region = sc.names.borrow().region();
        let arrival = match decl {
            Some(id) => sc.pending.arrive(id, &region, &sc.scope_copies),
            None => Arrival {
                own: Vec::new(),
                forward: Vec::new(),
            },
        };
        if arrival
            .own
            .iter()
            .any(|p| p.channel.is_none() && p.off)
        {
            if let Some(comp) = &comp {
                self.admit_forward(comp, &arrival.forward, &path, body, &sc.ids);
            }
            return Ok(());
        }
        let patch_stroke = arrival.own.iter().filter(|p| p.channel.is_none()).any(|p| {
            p.named.iter().any(|(k, _)| k == "stroke")
        });
        self.seed_node(sc.file(), &key);
        let raw_flags: Vec<String> = args.iter().filter_map(ident_of).map(str::to_string).collect();
        let flag = |f: &str| raw_flags.iter().any(|x| x == f);
        let mut a = self.eval_args(&pos_exprs, named, sc, 0)?;
        if let Some(alias) = callee.as_ref().and_then(|c| c.alias.as_ref()) {
            self.fix_args(alias, &mut a, sc)?;
        }
        let centered = CENTERED.contains(&verb);
        let group_like = matches!(
            verb,
            "layer" | "group" | "inside" | "outside" | "mirror" | "repeat" | "grid"
        );
        let frame = sc.frame;
        let default_at = if comp.is_some() {
            (0.0, 0.0)
        } else if centered {
            (frame.center_x() as f64, frame.center_y() as f64)
        } else {
            (0.0, 0.0)
        };
        let Some(animated) = self.land(&mut a, &arrival.own, default_at, frame, sc, &path) else {
            if let Some(comp) = &comp {
                self.admit_forward(comp, &arrival.forward, &path, body, &sc.ids);
            }
            return Ok(());
        };
        self.expand_style(&mut a, sc, &path)?;
        let origin = format!("{}:{}:{}", sc.file(), st.span.line, st.span.col);
        if let Some(comp) = comp {
            return self.instance(
                &comp,
                a,
                body,
                sc,
                out,
                key,
                origin,
                decl,
                arrival.forward,
                st.span,
                named.iter().any(|n| n.key == "name" && !is_name(&n.value)),
            );
        }
        for item in body.items() {
            let err = match item {
                DrawItem::SlotFill(fill) => (
                    fill.span,
                    fail(
                        "E0331",
                        format!(
                            "slot fill `{}:` is only valid under a component call",
                            fill.name
                        ),
                    )
                    .fix("indent it under the component that exposes the slot"),
                ),
                DrawItem::Patch(p) => (
                    p.span,
                    fail(
                        "E0332",
                        format!(
                            "patch `.{}` is only valid under a component call",
                            p.path.join(".")
                        ),
                    )
                    .fix("indent it under the component instance it overrides"),
                ),
                _ => continue,
            };
            let file = sc.file().to_string();
            self.report(err.1, &file, err.0, &path);
        }
        let expanded_style = a.word("style");
        let child_style = if group_like || expanded_style.is_some() {
            let mut s = (*sc.style).clone();
            s.extend(a.named.iter().filter(|(k, _)| is_paint_key(k)).cloned());
            Rc::new(s)
        } else {
            sc.style.clone()
        };
        let (m, at) = self.transform(&a, default_at, frame, sc, verb != "repeat")?;
        let (world, warp_cell) = content_place(&sc.world, &a, m);
        let mut node;
        let mut record = None;
        let body_frame;
        let mut branch_fx = None;
        if group_like {
            let local_frame = self.child_frame(&a, animated, default_at, frame, sc, verb != "repeat")?;
            let mut inner = sc.child(key.clone(), local_frame, world.clone());
            inner.style = child_style;
            branch_fx = Some(inner.collect_branch_fx());
            let mut clip = None;
            if matches!(verb, "inside" | "outside") {
                let r = match a.pos.first() {
                    Some(Value::Ref(r)) => r.clone(),
                    _ => {
                        return Err(fail("E0320", format!("`{verb}` needs a node reference"))
                            .fix(format!("write `{verb} @torso`")));
                    }
                };
                self.outside_copy(&r, sc)?;
                let sil = world.unmap_path(&r.silhouette());
                inner.frame = geom::bounds(&sil);
                clip = Some(Clip {
                    path: sil.to_ir(),
                    feather: a.num("feather").unwrap_or(0.0) as f32,
                    invert: verb == "outside",
                });
            }
            let placeholder = new_node(
                path.clone(),
                origin.clone(),
                m,
                NodeKind::Group {
                    isolate: verb == "layer",
                    children: vec![],
                },
            );
            if let Some(id) = decl {
                record = Some(self.place(
                    sc,
                    id,
                    &key,
                    sc.world.clone(),
                    Some(placeholder),
                    None,
                    at,
                    None,
                ));
            }
            let children = match verb {
                "mirror" => self.mirror(
                    body,
                    &mut inner,
                    if flag("y") { (1.0, -1.0) } else { (-1.0, 1.0) },
                ),
                "repeat" => self.repeat(body, &mut inner, &a, st.span)?,
                "grid" => self.grid(body, &mut inner, &a, st.span)?,
                _ => self.eval_block_nodes(body.scenes(), &mut inner),
            };
            node = new_node(
                path.clone(),
                origin,
                m,
                NodeKind::Group {
                    isolate: verb == "layer",
                    children,
                },
            );
            node.clip = clip;
            body_frame = inner.frame;
        } else {
            let mut operand_frame = None;
            let explicit = a.seed()?;
            let mut seam = 0.0;
            let (geometry, knots, closed, anchor) = match verb {
                "subtract" | "union" | "intersect" | "xor" => {
                    let op = match verb {
                        "subtract" => BoolOp::Subtract,
                        "union" => BoolOp::Union,
                        "intersect" => BoolOp::Intersect,
                        _ => BoolOp::Xor,
                    };
                    let local_frame = self.child_frame(&a, animated, default_at, frame, sc, true)?;
                    operand_frame = Some(local_frame);
                    let mut inner = sc.child(key.clone(), local_frame, world.clone());
                    branch_fx = Some(inner.collect_branch_fx());
                    inner.operand_of = Some(Rc::new(OperandOf {
                        verb: verb.to_string(),
                        name: named
                            .iter()
                            .find(|n| n.key == "name")
                            .and_then(|n| ident_of(&n.value))
                            .map(str::to_string),
                    }));
                    let operands = self.eval_block_nodes(body.scenes(), &mut inner);
                    let sils: Vec<Path> = operands
                        .iter()
                        .map(|n| crate::scope::silhouette(n, &Place::identity()))
                        .collect();
                    (geom::boolean(op, &sils), Vec::new(), true, None)
                }
                "copy" => {
                    let r = match a.pos.first() {
                        Some(Value::Ref(r)) => r.clone(),
                        _ => {
                            return Err(fail("E0320", "`copy` needs a node reference")
                                .fix("write `copy @body`"));
                        }
                    };
                    self.outside_copy(&r, sc)?;
                    seam = r.record.seam;
                    (
                        sc.world.unmap_path(&r.silhouette()),
                        Vec::new(),
                        true,
                        None,
                    )
                }
                "edge" => (self.edge(&a, named, st.span, sc)?, Vec::new(), false, None),
                _ => self.primitive(verb, &a, frame, sc, flag("closed"))?,
            };
            if let Some(near) = anchor.filter(|_| decl.is_some()) {
                seam = geom::seam_near(&geometry, near);
            }
            let mut geometry = geometry;
            if let Some(spec) = a.get("jitter") {
                let (amount, wavelength) = match spec {
                    Value::Num(amp) => {
                        return Err(fail("E0320", "`jitter:` takes amplitude and wavelength")
                            .fix(format!("write `jitter:{},60`", num(*amp))));
                    }
                    other => other.vec2().ok_or_else(|| {
                        fail(
                            "E0320",
                            format!("`jitter:` takes amplitude,wavelength, got {}", other.kind()),
                        )
                        .fix("write `jitter:1.5,60`")
                    })?,
                };
                if amount != 0.0 {
                    if wavelength <= 0.0 {
                        return Err(fail("E0320", "`jitter:` wavelength must be positive")
                            .fix("write `jitter:1.5,60`"));
                    }
                    let jitter = identity::seed(sc.file(), &key.jitter(), explicit, &mut self.uses);
                    let mut rng = Rng::new(jitter);
                    geometry = geom::wobble(&geometry, amount, &mut rng, wavelength);
                }
            }
            // A patch that sets stroke: is a value, so its spelling is gone. expanded_style is the
            // style word after patches and motion, which is the style expand_style applied.
            let stroke_src = if patch_stroke {
                None
            } else {
                stroke_expr(named, expanded_style.as_deref(), sc)
            };
            let (shape, rows) = self.paint_shape(
                &a,
                geometry,
                &knots,
                closed,
                sc,
                &world,
                stroke_src.as_ref(),
                &key,
                named,
                st.span,
            )?;
            node = new_node(path.clone(), origin, m, NodeKind::Shape(Box::new(shape)));
            if let Some(id) = decl {
                let r = self.place(
                    sc,
                    id,
                    &key,
                    sc.world.clone(),
                    Some(node.clone()),
                    None,
                    at,
                    None,
                );
                r.borrow_mut().rows = Rc::new(rows);
                r.borrow_mut().seam = seam;
                record = Some(r);
            }
            let gb = match &node.kind {
                NodeKind::Shape(s) => geom::bounds(&s.geometry.to_sk()),
                _ => Rect::new_empty(),
            };
            body_frame = operand_frame.unwrap_or(gb);
            if operand_frame.is_none() && body.scenes().next().is_some() {
                let mut inner = sc.child(key.clone(), gb, world.clone());
                inner.style = child_style;
                branch_fx = Some(inner.collect_branch_fx());
                let kids = self.eval_block_nodes(body.scenes(), &mut inner);
                if let NodeKind::Shape(s) = &mut node.kind {
                    s.children = kids;
                }
            }
        }
        let fill_key = first_fill(&node).unwrap_or(Rgba::GRAY);
        let fx_scope = sc.child(key.clone(), sc.frame, world.clone());
        let branch_fx = branch_fx.map(|fx| fx.take()).unwrap_or_default();
        if let Some(of) = sc.operand_of.clone() {
            self.report_operand_fx(&of, &branch_fx, sc);
        }
        let lines = in_source_order(body.fx(), &branch_fx, &fx_scope);
        let ordinals = fx_ordinals(body.fx(), body.scenes());
        let sites: Vec<FxSite> = lines
            .iter()
            .map(|(line, scope)| FxSite { line, scope, ordinal: ordinals[&line.span], frame: body_frame, world: world.clone() })
            .collect();
        node.effects = self.lower_fx(&sites, &key, fill_key, a.seed()?);
        // The outline rasterizes the node offscreen, where a brush has no backdrop to read.
        if let NodeKind::Shape(shape) = &node.kind
            && shape.reads_below()
            && node.effects.iter().any(|e| matches!(e.op, ramose_ir::EffectOp::Outline { width, .. } if width > 0.0))
            && let Some(site) = sites.iter().find(|s| s.line.op == "outline").or(sites.first())
        {
            let f = fail("E0362", "`fx outline` on a node whose brush reads `m.below`")
                .note("the outline draws the node offscreen, where its brush has no backdrop to read")
                .fix("outline a parent instead, or use a brush that does not read `m.below`");
            self.report(f, site.scope.file(), site.line.span, &key.to_string());
            return Err(Fail::reported());
        }
        self.mask(&mut node, &a, body.mask(), sc, body_frame, &world, &key)?;
        let z = self.finish_node(&mut node, &a, sc, warp_cell)?;
        if let Some(r) = record {
            r.borrow_mut().node = Some(node.clone());
        }
        out.items.push((node, z));
        Ok(())
    }

    fn binding(&self, name: &str, sc: &Scope) -> Option<Value> {
        sc.env
            .iter()
            .rev()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
            .or_else(|| sc.module.global(name))
    }

    /// `Some(None)` when the verb is bound to `none`, so the line draws nothing.
    fn component_for(&self, verb: &str, sc: &Scope) -> Option<Option<Callee>> {
        match self.binding(verb, sc)? {
            Value::Component(comp) => Some(Some(Callee { comp, alias: None })),
            Value::Alias(a) => Some(Some(Callee {
                comp: a.comp.clone(),
                alias: Some(a),
            })),
            Value::None => Some(None),
            _ => None,
        }
    }

    /// A bound component is a call even when its name is also a built-in shape.
    fn verb_is_call(&self, verb: &str, sc: &Scope) -> bool {
        matches!(
            self.binding(verb, sc),
            Some(Value::Component(_) | Value::Alias(_))
        ) || identity::calls_def(verb)
    }

    /// Binds each `def NAME = target …` that `declare` left unbound because its target is
    /// declared below it, following a chain of aliases to the def with a body.
    fn declare_aliases(&mut self, module: &Rc<Module>, parsed: &File) {
        let decls: Vec<&Decl> = parsed
            .decls()
            .filter(|d| matches!(d.kind, DeclKind::Alias { .. }))
            .collect();
        let mut done: BTreeSet<Span> = decls
            .iter()
            .filter(|d| match &d.kind {
                DeclKind::Alias { name, .. } => {
                    module.lost(name)
                        || matches!(module.global(name), Some(Value::Alias(a)) if a.span == d.span)
                }
                _ => false,
            })
            .map(|d| d.span)
            .collect();
        for d in &decls {
            if done.contains(&d.span) {
                continue;
            }
            let mut chain = Vec::new();
            if let Err(f) = self.bind_alias(module, &decls, d, &mut chain, &mut done) {
                self.alias_failed(module, d, f);
            }
        }
    }

    /// Reports the failure at the alias's own line. Its uses then stay quiet, as a failed
    /// `let`'s do.
    fn alias_failed(&mut self, module: &Module, decl: &Decl, f: Fail) {
        if let DeclKind::Alias { name, .. } = &decl.kind {
            module.lost.borrow_mut().insert(name.clone());
        }
        self.report(f, &module.path, decl.span, "");
    }

    fn bind_alias(
        &mut self,
        module: &Rc<Module>,
        decls: &[&Decl],
        decl: &Decl,
        chain: &mut Vec<String>,
        done: &mut BTreeSet<Span>,
    ) -> R<()> {
        let DeclKind::Alias {
            name,
            target,
            args,
            named,
        } = &decl.kind
        else {
            return Ok(());
        };
        if chain.iter().any(|n| n == name) {
            chain.push(name.clone());
            return Err(fail("E0323", format!("`{name}` is an alias of itself"))
                .note(format!("through {}", chain.join(" -> ")))
                .fix("name a def with a body"));
        }
        if !done.insert(decl.span) {
            return Ok(());
        }
        chain.push(name.clone());
        if module.global(target).is_none()
            && let Some(inner) = decls
                .iter()
                .find(|d| matches!(&d.kind, DeclKind::Alias { name: n, .. } if n == target))
            && let Err(f) = self.bind_alias(module, decls, inner, chain, done)
        {
            self.alias_failed(module, inner, f);
            return Err(Fail::reported());
        }
        let (comp, mut fixed) = match module.global(target) {
            Some(Value::Component(c)) => (c, Vec::new()),
            Some(Value::Alias(a)) => (
                a.comp.clone(),
                a.fixed
                    .iter()
                    .map(|f| Fixed {
                        key: f.key.clone(),
                        value: f.value.clone(),
                        module: f.module.clone(),
                    })
                    .collect(),
            ),
            Some(other) => {
                return Err(fail(
                    "E0323",
                    format!("`{target}` is {}, not a def", other.kind()),
                )
                .fix("alias a def"));
            }
            None if module.lost(target) => return Err(Fail::reported()),
            None => {
                return Err(
                    fail("E0301", format!("unknown def `{target}`")).fix(format!(
                        "check the spelling, or `use` the library that defines `{target}`"
                    )),
                );
            }
        };
        let is_fixed = |k: &str| fixed.iter().any(|f| f.key == k);
        let open: Vec<String> = comp
            .params
            .iter()
            .map(|p| p.key.clone())
            .filter(|k| !is_fixed(k))
            .collect();
        if args.len() > open.len() {
            return Err(fail(
                "E0321",
                format!("`{target}` takes {} positional argument(s)", open.len()),
            )
            .fix("pass the rest as key:value"));
        }
        let mut own: Vec<Fixed> = args
            .iter()
            .zip(&open)
            .map(|(value, key)| Fixed {
                key: key.clone(),
                value: value.clone(),
                module: module.path.clone(),
            })
            .collect();
        for n in named {
            if is_fixed(&n.key) || own.iter().any(|f| f.key == n.key) {
                return Err(
                    fail("E0321", format!("`{target}` fixes `{}`", n.key)).fix(format!(
                        "drop `{}:`, or alias `{}` directly",
                        n.key, comp.name
                    )),
                );
            }
            if !open.contains(&n.key)
                && n.key != "seed"
                && n.key != "palette"
                && !NODE_KEYS.contains(&n.key.as_str())
            {
                return Err(no_parameter(target, &comp.name, &n.key, &open));
            }
            own.push(Fixed {
                key: n.key.clone(),
                value: n.value.clone(),
                module: module.path.clone(),
            });
        }
        fixed.extend(own);
        module.define(
            name,
            Value::Alias(Rc::new(AliasDef {
                name: name.clone(),
                comp,
                fixed,
                module: module.path.clone(),
                span: decl.span,
            })),
        );
        Ok(())
    }

    /// Merges the arguments `alias` fixed into a call on it. The call's positionals fill the
    /// parameters the alias leaves open, in the def's order. A fixed argument is evaluated in
    /// the module that fixed it, with the instance's palette as `p`.
    fn fix_args(&mut self, alias: &AliasDef, a: &mut Args, sc: &Scope) -> R<()> {
        let is_fixed = |k: &str| alias.fixed.iter().any(|f| f.key == k);
        let open: Vec<&str> = alias
            .comp
            .params
            .iter()
            .map(|p| p.key.as_str())
            .filter(|k| !is_fixed(k))
            .collect();
        for (k, _) in &a.named {
            if is_fixed(k) {
                return Err(
                    fail("E0321", format!("`{}` fixes `{k}`", alias.name)).fix(format!(
                        "drop `{k}:`, or instance `{}` directly",
                        alias.comp.name
                    )),
                );
            }
            if !open.contains(&k.as_str())
                && k != "seed"
                && k != "palette"
                && !NODE_KEYS.contains(&k.as_str())
            {
                return Err(no_parameter(&alias.name, &alias.comp.name, k, &open));
            }
        }
        let pos = std::mem::take(&mut a.pos);
        if pos.len() > open.len() {
            return Err(fail(
                "E0321",
                format!(
                    "`{}` takes {} positional argument(s)",
                    alias.name,
                    open.len()
                ),
            )
            .fix("pass the rest as key:value"));
        }
        for (key, v) in open.iter().zip(pos) {
            a.named.push((key.to_string(), v));
        }
        for (i, by) in std::mem::take(&mut a.timed_pos) {
            a.timed.push((open[i].to_string(), by));
        }
        let mut palette = a.get("palette").cloned().or_else(|| sc.palette.clone());
        let fixed_palette = alias.fixed.iter().position(|f| f.key == "palette");
        let order = fixed_palette
            .into_iter()
            .chain((0..alias.fixed.len()).filter(|&i| Some(i) != fixed_palette));
        for i in order {
            let f = &alias.fixed[i];
            let module = self
                .modules
                .get(&f.module)
                .cloned()
                .ok_or_else(|| fail("E0210", format!("module `{}` is not loaded", f.module)))?;
            let mut fsc = self.module_scope(&module);
            fsc.palette = palette.clone();
            let v = self.expr(&f.value, &fsc).map_err(|e| {
                e.note(format!(
                    "in `{}:` fixed by `{}` ({}:{})",
                    f.key, alias.name, alias.module, alias.span.line
                ))
            })?;
            if f.key == "palette" && a.get("palette").is_none() {
                palette = Some(v.clone());
            }
            a.named.push((f.key.clone(), v));
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn mask(
        &mut self,
        node: &mut Node,
        a: &Args,
        mask: Option<&ast::Mask>,
        sc: &Scope,
        frame: Rect,
        world: &Place,
        key: &Key,
    ) -> R<()> {
        let feather = a.num("feather").unwrap_or(0.0) as f32;
        if let Some(mask) = mask {
            let origin = node.origin.clone();
            self.begin_seed(sc.file(), key.mask());
            node.mask = self
                .lower_mask(mask, feather, sc, frame, world, &key.mask(), &origin)?
                .map(Box::new);
        }
        Ok(())
    }

    /// A `mask` line's source, evaluated in the masked node's frame, or `None` for `mask none`.
    /// `mask_key` names its nodes and `origin` is the line they are reported at.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn lower_mask(
        &mut self,
        mask: &ast::Mask,
        feather: f32,
        sc: &Scope,
        frame: Rect,
        world: &Place,
        mask_key: &Key,
        origin: &str,
    ) -> R<Option<Mask>> {
        let source = match mask {
            ast::Mask::Block(body) => {
                let mut inner = sc.child(mask_key.clone(), frame, world.clone());
                Some(MaskSource::Nodes(self.eval_block_nodes(body, &mut inner)))
            }
            ast::Mask::Value(e) => match self.expr(e, sc)? {
                Value::None => None,
                Value::Ref(r) => {
                    self.outside_copy(&r, sc)?;
                    let geometry = world.unmap_path(&r.silhouette());
                    let shape = Shape {
                        geometry: geometry.to_ir(),
                        fill: Some(IrFill::plain(Paint::Solid(
                            Rgba {
                                r: 1.0,
                                g: 1.0,
                                b: 1.0,
                                a: 1.0,
                            }
                            .to_ir(),
                        ))),
                        stroke: None,
                        blur: 0.0,
                        stamps: vec![],
                        children: vec![],
                    };
                    let n = new_node(
                        mask_key.to_string(),
                        origin.to_string(),
                        Matrix::new_identity(),
                        NodeKind::Shape(Box::new(shape)),
                    );
                    Some(MaskSource::Nodes(vec![n]))
                }
                Value::FieldMask(f, t) => Some(MaskSource::Field {
                    field: (*f).clone(),
                    threshold: Some(t as f32),
                }),
                Value::Field(f) => Some(MaskSource::Field {
                    field: (*f).clone(),
                    threshold: None,
                }),
                other => {
                    return Err(fail(
                        "E0320",
                        format!(
                            "`mask` expects a node reference or a field, got {}",
                            other.kind()
                        ),
                    )
                    .fix("write `mask @body` or `mask simplex(30) > .5`"));
                }
            },
        };
        Ok(source.map(|source| Mask {
            source,
            luminance: false,
            feather,
            invert: false,
        }))
    }

    pub(crate) fn target_palette(&mut self, e: &Expr, sc: &Scope) -> R<Value> {
        match self.expr(e, sc)? {
            v @ (Value::Palette(_) | Value::Module(_)) => Ok(v),
            other => Err(fail(
                "E0206",
                format!("`palette:` takes a palette, got {}", other.kind()),
            )
            .fix("write `palette:elite`")),
        }
    }

    /// The target's `pivot:` point in artboard units.
    pub(crate) fn pivot(&mut self, e: &Expr, sc: &Scope) -> R<(f64, f64)> {
        let bad = || {
            fail("E0206", "`pivot:` needs a point")
                .fix("write `pivot:48,96`, `pivot:50%,100%`, or `pivot:@leg.hip`")
        };
        let value = self.expr(e, sc)?;
        if let Value::Ref(r) = &value {
            return match (r.record.anchor.is_some(), r.artboard) {
                (true, None) => {
                    let p = r.record.anchor_canvas().ok_or_else(|| hidden_anchor(&r.label))?;
                    Ok((p.x as f64, p.y as f64))
                }
                _ => Err(fail(
                    "E0206",
                    format!("`pivot:@{}` is a node, not a point", r.label),
                )
                .fix(format!(
                    "name an anchor such as `pivot:@{0}.hip`, or write `pivot:@{0}.cx,@{0}.cy`",
                    r.label
                ))),
            };
        }
        let (x, y) = value.pair().ok_or_else(bad)?;
        let size = self.job.artboard.size;
        let at = (x.pos(0.0, f64::from(size.w())), y.pos(0.0, f64::from(size.h())));
        if at.0.is_finite() && at.1.is_finite() {
            Ok(at)
        } else {
            Err(bad())
        }
    }

    fn anchor(&mut self, span: Span, args: &[Expr], sc: &Scope) -> R<()> {
        args.first().and_then(ident_of).ok_or_else(|| {
            fail("E0320", "`anchor` needs a name").fix("write `anchor neck 0,-4`")
        })?;
        let at = match args.get(1) {
            Some(e) => self.expr(e, sc)?.vec2().unwrap_or((0.0, 0.0)),
            None => (0.0, 0.0),
        };
        let p = Point::new(at.0 as f32, at.1 as f32);
        let key = sc.node_key(span);
        if let Some(id) = sc.decl_at(span) {
            let region = sc.names.borrow().region();
            let own = sc.pending.arrive(id, &region, &sc.scope_copies).own;
            for patch in &own {
                identity::reject_anchor_patch(
                    patch,
                    &sc.node_name(),
                    &mut self.reported,
                    &mut self.diags,
                );
            }
            self.place(sc, id, &key, sc.world.clone(), None, Some(p), at, None);
        }
        Ok(())
    }

    fn mirror(&mut self, body: &DrawBody, sc: &mut Scope, flip: (f32, f32)) -> Vec<Node> {
        let originals = self.eval_block_nodes(body.scenes(), sc);
        let mut out = originals.clone();
        for n in originals {
            let mut m = n.clone();
            identity::reflect(&mut m);
            m.transform = (Matrix::scale(flip) * n.transform.to_sk()).to_ir();
            out.push(m);
        }
        out
    }

    /// A boolean reads only the geometry of its operands, so paint, compositing, and `fx` lines
    /// on an operand would be dropped. A component parameter of the same name is geometry input.
    fn report_operand_paint(
        &mut self,
        of: &OperandOf,
        named: &[Named],
        body: &DrawBody,
        comp: Option<&Component>,
        sc: &Scope,
    ) {
        let ignored = named.iter().filter(|n| {
            OPERAND_IGNORES.contains(&n.key.as_str())
                && !comp.is_some_and(|c| c.params.iter().any(|p| p.key == n.key))
        });
        let ignored: Vec<(String, Span)> = ignored
            .map(|n| (format!("`{}:`", n.key), n.span))
            .chain(body.fx().map(|f| (format!("`fx {}`", f.op), f.span)))
            .collect();
        self.report_operand_ignored(of, ignored, sc);
    }

    /// The `fx` lines under a taken `if`, known only once the operand's block has run.
    fn report_operand_fx(&mut self, of: &OperandOf, fx: &[BranchFx], sc: &Scope) {
        let ignored = fx
            .iter()
            .map(|b| (format!("`fx {}`", b.line.op), b.line.span))
            .collect();
        self.report_operand_ignored(of, ignored, sc);
    }

    fn report_operand_ignored(&mut self, of: &OperandOf, ignored: Vec<(String, Span)>, sc: &Scope) {
        let fix = match &of.name {
            Some(name) => format!("to paint over the result, draw it in `inside @{name}`"),
            None => format!(
                "to paint over the result, name the `{}` and draw it in `inside @<name>`",
                of.verb
            ),
        };
        let (file, node) = (sc.file().to_string(), sc.node_name());
        for (what, span) in ignored {
            let f = fail(
                "E0321",
                format!(
                    "{what} on an operand of `{}` is ignored; an operand gives only its geometry",
                    of.verb
                ),
            )
            .fix(fix.clone());
            self.report(f, &file, span, &node);
        }
    }

    fn extra_positional(&mut self, verb: &str, takes: usize, items: &[Expr], sc: &Scope) -> Fail {
        let extra = format_expr(&items[takes]);
        let takes_text = match takes {
            0 => "no positional items".to_string(),
            1 => "1 positional item".to_string(),
            n => format!("{n} positional items"),
        };
        let f = fail(
            "E0321",
            format!("`{verb}` takes {takes_text}, so `{extra}` is extra"),
        );
        let sized = matches!(verb, "rect" | "grid" | "ellipse")
            && matches!(self.expr(&items[0], sc), Ok(Value::Num(_)));
        if sized {
            f.note("a vector `w,h` is a size").fix(format!(
                "write `{verb} {},{}`",
                format_expr(&items[0]),
                format_expr(&items[items.len() - 1])
            ))
        } else {
            f.fix(format!("remove `{extra}`, or pass it as key:value"))
        }
    }

    fn repeat(
        &mut self,
        body: &DrawBody,
        sc: &mut Scope,
        a: &Args,
        span: Span,
    ) -> R<Vec<Node>> {
        if let Some(by) = a.timed_at(0) {
            return Err(by.sink("the `repeat` count"));
        }
        let n = a.pos.first().and_then(Value::num).ok_or_else(|| {
            fail("E0320", "`repeat` needs a count").fix("write `repeat 8 rot:45`")
        })?;
        let rot = a.rot()?;
        let step = a.get("step").map(|v| v.vec2_for("step")).transpose()?.unwrap_or((0.0, 0.0));
        let mut out = Vec::new();
        for i in 0..(n.max(0.0) as usize).min(10_000) {
            let fi = i as f64;
            let copy_m = Matrix::translate(((step.0 * fi) as f32, (step.1 * fi) as f32))
                * Matrix::rotate_deg((rot * fi) as f32);
            let mut inner = sc.iteration(span, i as u32);
            inner.env.push(("i".into(), Value::Num(fi)));
            inner.world = sc.world.inner(copy_m);
            for mut node in self.eval_block_nodes(body.scenes(), &mut inner) {
                node.transform = (copy_m * node.transform.to_sk()).to_ir();
                out.push(node);
            }
        }
        Ok(out)
    }

    fn grid(
        &mut self,
        body: &DrawBody,
        sc: &mut Scope,
        a: &Args,
        span: Span,
    ) -> R<Vec<Node>> {
        if let Some(by) = a.timed_at(0) {
            return Err(by.sink("the `grid` count"));
        }
        let (cols, rows) = a.pos.first().and_then(Value::vec2).ok_or_else(|| {
            fail("E0320", "`grid` needs columns x rows").fix("write `grid 4x3 gap:8`")
        })?;
        let (cols, rows) = (cols.max(1.0) as usize, rows.max(1.0) as usize);
        let gap = a.num("gap").unwrap_or(0.0);
        let mut pitch = a
            .get("cell")
            .map(|v| v.vec2_for("cell"))
            .transpose()?
            .map(|(cw, ch)| (cw + gap, ch + gap));
        let cell_at = |(sx, sy): (f64, f64), row: usize, col: usize| {
            let (ox, oy) = ((cols - 1) as f64 * sx / 2.0, (rows - 1) as f64 * sy / 2.0);
            Matrix::translate(((col as f64 * sx - ox) as f32, (row as f64 * sy - oy) as f32))
        };
        let mut out = Vec::new();
        for row in 0..rows {
            for col in 0..cols {
                let i = row * cols + col;
                let mut inner = sc.iteration(span, i as u32);
                inner.env.push(("i".into(), Value::Num(i as f64)));
                inner.env.push(("row".into(), Value::Num(row as f64)));
                inner.env.push(("col".into(), Value::Num(col as f64)));
                let (t, nodes) = match pitch {
                    Some(p) => {
                        let t = cell_at(p, row, col);
                        inner.world = sc.world.inner(t);
                        (t, self.eval_block_nodes(body.scenes(), &mut inner))
                    }
                    None => {
                        let before = record_ids(&sc.names);
                        if rows * cols > 1 {
                            inner.provisional = Some(Rc::new(Provisional {
                                names: sc.names.clone(),
                                before: before.clone(),
                            }));
                        }
                        let nodes = self.eval_block_nodes(body.scenes(), &mut inner);
                        let b = union_bounds(&nodes).unwrap_or(Rect::new_empty());
                        let p = (b.width() as f64 + gap, b.height() as f64 + gap);
                        pitch = Some(p);
                        let t = cell_at(p, row, col);
                        shift_placed(&records_after(&sc.names, &before), &sc.world, &t);
                        (t, nodes)
                    }
                };
                for mut n in nodes {
                    n.transform = (t * n.transform.to_sk()).to_ir();
                    out.push(n);
                }
            }
        }
        Ok(out)
    }

    fn primitive(
        &mut self,
        verb: &str,
        a: &Args,
        frame: Rect,
        sc: &Scope,
        closed_flag: bool,
    ) -> R<Outline> {
        let (fx, fy, fw, fh) = (
            frame.left as f64,
            frame.top as f64,
            frame.width() as f64,
            frame.height() as f64,
        );
        let first = a.pos.first();
        let need = |what: &str, example: &str| {
            if first.is_some_and(|v| v.triple().is_some()) {
                return Value::three_in_2d(&format!("`{verb}`"), example);
            }
            fail("E0320", format!("`{verb}` needs {what}")).fix(format!("write `{example}`"))
        };
        Ok(match verb {
            "rect" => {
                let (w, h) = first
                    .and_then(Value::pair)
                    .ok_or_else(|| need("a size", "rect 40x20"))?;
                check_percent(w, frame, sc)?;
                check_percent(h, frame, sc)?;
                let radii = match a.get("r").map(numbers) {
                    None => [0.0; 4],
                    Some(Some(r)) if r.len() == 1 => [r[0]; 4],
                    Some(Some(r)) if r.len() == 4 => [r[0], r[1], r[2], r[3]],
                    Some(_) => return Err(need("one corner radius or four", "rect 40x20 r:4,4,0,0")),
                };
                let (w, h) = (w.len(fw), h.len(fh));
                let corner = radii.iter().any(|r| *r > 0.0).then_some((-w / 2.0, -h / 2.0));
                (geom::rect(w, h, radii), Vec::new(), true, corner)
            }
            "circle" => {
                let r = first
                    .and_then(Value::coord)
                    .ok_or_else(|| need("a radius", "circle 12"))?;
                check_percent(r, frame, sc)?;
                let r = r.len(fw.min(fh));
                (geom::ellipse(r, r), Vec::new(), true, None)
            }
            "ellipse" => {
                let (rx, ry) = first
                    .and_then(Value::pair)
                    .ok_or_else(|| need("two radii", "ellipse 20,12"))?;
                check_percent(rx, frame, sc)?;
                check_percent(ry, frame, sc)?;
                (geom::ellipse(rx.len(fw), ry.len(fh)), Vec::new(), true, None)
            }
            "poly" | "star" => {
                let n = first
                    .and_then(Value::num)
                    .filter(|n| n.is_finite() && *n <= 100_000.0)
                    .ok_or_else(|| need("a point count up to 100000", "star 5 r:20"))?;
                let r = a.num("r").unwrap_or(10.0);
                let inner = (verb == "star").then(|| a.num("inner").unwrap_or(0.5));
                let mut p = geom::regular(n as usize, r, inner);
                let mut first = None;
                if let Some(round) = a.num("round").filter(|v| *v > 0.0) {
                    let v = p.points()[0];
                    first = Some((f64::from(v.x), f64::from(v.y)));
                    p = geom::round_corners(&p, round);
                }
                (p, Vec::new(), true, first)
            }
            "curve" => {
                let mut values: Vec<Value> = Vec::with_capacity(a.pos.len());
                for v in &a.pos {
                    match v {
                        Value::List(xs) => values.extend(xs.iter().cloned()),
                        point => values.push(point.clone()),
                    }
                }
                let mut pts = Vec::with_capacity(values.len());
                for v in &values {
                    let (x, y) = v
                        .pair()
                        .ok_or_else(|| need("points like 0,0 20,-15", "curve 0,0 20,-15 40,0"))?;
                    check_percent(x, frame, sc)?;
                    check_percent(y, frame, sc)?;
                    pts.push((x.pos(fx, fw), y.pos(fy, fh)));
                }
                if pts.len() < 2 {
                    return Err(need("at least two points", "curve 0,0 20,-15 40,0"));
                }
                let closed = closed_flag && pts.len() >= 3;
                let tension = a.num("tension").unwrap_or(0.5);
                let round = a
                    .num("round")
                    .filter(|v| *v > 0.0 && closed && tension == 0.0);
                let first = round.map(|_| pts[0]);
                let path = match round {
                    // A tension-0 spline is cubics. Skia's corner effect only rounds line joins,
                    // so the straight sides are a polygon before the same effect `poly` uses.
                    Some(round) => geom::round_corners(&geom::polygon(&pts), round),
                    None => geom::spline(&pts, tension, closed, true),
                };
                (path, vec![pts], closed, first)
            }
            "path" => {
                if let Some(by) = a.timed_at(0) {
                    return Err(by.sink("the `path` data"));
                }
                let d = first.and_then(|v| match v {
                    Value::Str(s) => Some(s.clone()),
                    _ => None,
                });
                let d = d.ok_or_else(|| need("SVG path data", "path \"M0 0 L10 0 Z\""))?;
                let p = skia_safe::utils::parse_path::from_svg(&d).ok_or_else(|| {
                    fail("E0320", "invalid SVG path data").fix("check the path string")
                })?;
                let closed =
                    skia_safe::ContourMeasureIter::new(&p, false, None).all(|c| c.is_closed());
                let knots = geom::vertices(&p);
                (p, knots, closed, None)
            }
            "blob" => {
                let r = first
                    .and_then(Value::coord)
                    .ok_or_else(|| need("a radius", "blob 30 seed:4 wobble:.2"))?;
                check_percent(r, frame, sc)?;
                let r = r.len(fw.min(fh));
                let explicit = a.seed()?;
                let blob =
                    identity::seed(sc.file(), &self.seed_key.blob(), explicit, &mut self.uses);
                let mut rng = Rng::new(blob);
                let points = a.num("points").unwrap_or(7.0) as usize;
                (
                    geom::blob(r, points, a.num("wobble").unwrap_or(0.2), &mut rng),
                    Vec::new(),
                    true,
                    None,
                )
            }
            _ => unreachable!("primitive {verb}"),
        })
    }

    /// `named` is this line's own keys. An error about a brush or channel key on the line is
    /// reported at that key; an inherited key's error lands on the statement with a note.
    #[allow(clippy::too_many_arguments)]
    fn paint_shape(
        &mut self,
        a: &Args,
        geometry: Path,
        knots: &[Vec<P>],
        closed: bool,
        sc: &Scope,
        world: &Place,
        stroke_src: Option<&Expr>,
        key: &Key,
        named: &[Named],
        span: Span,
    ) -> R<(Shape, Vec<RowRecord>)> {
        let style: &[(String, Value)] = &sc.style;
        let pick = |k: &str| {
            a.get(k).cloned().or_else(|| {
                style
                    .iter()
                    .rev()
                    .find(|(n, _)| n == k)
                    .map(|(_, v)| v.clone())
            })
        };
        let bounds = geom::bounds(&geometry);
        let fill_v = pick("fill");
        let fill = match &fill_v {
            Some(Value::None) => None,
            Some(v) => Some(to_paint(v, bounds).ok_or_else(|| {
                fail(
                    "E0320",
                    format!(
                        "`fill:` expects a color, gradient or field, got {}",
                        v.kind()
                    ),
                )
                .fix("write `fill:p.skin` or `fill:grad(down a b)`")
            })?),
            None if closed => Some(Paint::Solid(Rgba::GRAY.to_ir())),
            None => None,
        };
        let line_of =
            |fill: &Option<Paint>| color::line(fill.as_ref().map(key_color).unwrap_or(Rgba::GRAY));
        enum W {
            Const(f64),
            List(Vec<f64>),
        }
        let (file, node) = (sc.file().to_string(), sc.node_name());
        let site = |keys: &[&str]| keys.iter().find_map(|k| named.iter().find(|n| n.key == *k).map(|n| n.span));
        let at_key = |this: &mut Self, f: Fail, keys: &[&str]| -> Fail {
            match site(keys) {
                Some(span) => {
                    this.report(f, &file, span, &node);
                    Fail::reported()
                }
                None => f.note(format!("`{}:` is inherited from an enclosing line or style", keys[0])),
            }
        };
        let sets = match pick("stamp") {
            None => Vec::new(),
            Some(v) => match self.stamp_stack(&v, sc) {
                Ok(sets) => sets,
                Err(f) => return Err(at_key(self, f, &["stamp"])),
            },
        };
        let stroke_v = pick("stroke");
        let (paint_v, width) = match stroke_v {
            // A stamped open path with no stroke draws no line: the stamps are its ink.
            None if !closed && fill.is_none() && sets.is_empty() => (
                Value::Color(Col::plain(color::line(Rgba::GRAY))),
                Some(W::Const(2.0)),
            ),
            None | Some(Value::None) => (Value::None, None),
            Some(Value::Num(w)) => (Value::Color(Col::plain(line_of(&fill))), Some(W::Const(w))),
            Some(Value::Range(_)) => return Err(range_width(stroke_src)),
            Some(Value::List(ws)) => (
                Value::Color(Col::plain(line_of(&fill))),
                Some(W::List(ws.iter().filter_map(Value::num).collect())),
            ),
            Some(Value::Tuple(xs)) if xs.len() == 2 => {
                let w = match &xs[1] {
                    Value::Num(w) => W::Const(*w),
                    Value::Range(_) => return Err(range_width(stroke_src)),
                    Value::List(ws) => W::List(ws.iter().filter_map(Value::num).collect()),
                    other => {
                        return Err(fail(
                            "E0320",
                            format!(
                                "stroke width must be a number or list, got {}",
                                other.kind()
                            ),
                        )
                        .fix("write `stroke:p.ink,2`"));
                    }
                };
                (xs[0].clone(), Some(w))
            }
            Some(v) if v.kind() == Kind::Color => (v, Some(W::Const(1.0))),
            Some(v) => {
                return Err(fail(
                    "E0320",
                    format!("`stroke:` expects color,width, got {}", v.kind()),
                )
                .fix("write `stroke:p.ink,2`"));
            }
        };
        let taper = match pick("taper") {
            Some(Value::Num(t)) => Some((t, t)),
            Some(v) => Some(v.vec2_for("taper")?),
            None => None,
        };
        let blur = pick("blur").and_then(|v| v.num()).unwrap_or(0.0) as f32;
        let (fill_marks, stroke_marks) = match pick("brush") {
            None => (Vec::new(), Vec::new()),
            Some(bv) => {
                let stack = match self.brush_stack(&bv, sc) {
                    Ok(stack) => stack,
                    Err(f) => return Err(at_key(self, f, &["brush"])),
                };
                let lands = |b: &BrushVal, slot: Slot| {
                    let has = match slot {
                        Slot::Fill => fill.is_some(),
                        Slot::Stroke => width.is_some(),
                    };
                    has && b.place.on.is_none_or(|on| on == slot)
                };
                let lands_any = |b: &BrushVal| lands(b, Slot::Fill) || lands(b, Slot::Stroke);
                if site(&["brush"]).is_some() && stack.iter().any(|b| !lands_any(b)) {
                    for (i, b) in stack.iter().enumerate().filter(|(_, b)| !lands_any(b)) {
                        let what = if stack.len() == 1 {
                            "`brush:` on a node with no paint to brush".to_string()
                        } else {
                            format!("stage {} of `brush:` (`{}`) has no paint to brush on this node", i + 1, b.decl.name)
                        };
                        let f = fail("E0362", what)
                            .note(if fill.is_none() && width.is_none() {
                                "this node has no fill and no stroke"
                            } else {
                                "the brush is narrowed to a slot this node does not have"
                            })
                            .fix("give the node that paint, or remove the brush");
                        at_key(self, f, &["brush"]);
                    }
                    return Err(Fail::reported());
                }
                let fill_stack: Vec<Rc<BrushVal>> = stack.iter().filter(|b| lands(b, Slot::Fill)).cloned().collect();
                let stroke_stack: Vec<Rc<BrushVal>> = stack.iter().filter(|b| lands(b, Slot::Stroke)).cloned().collect();
                if blur > 0.0 && !(fill_stack.is_empty() && stroke_stack.is_empty()) {
                    let f = fail("E0362", "`blur:` on a brushed shape")
                        .fix("blur the parent instead: wrap the node in a `group` with `fx blur`");
                    return Err(at_key(self, f, &["blur", "brush"]));
                }
                if !stroke_stack.is_empty() && pick("dash").is_some() {
                    let f = fail("E0362", "`dash:` on a brushed stroke")
                        .note("a ribbon has no dash")
                        .fix("remove the dash, or add `on:fill` to the brush");
                    return Err(at_key(self, f, &["dash", "brush"]));
                }
                if !stroke_stack.is_empty() {
                    for k in ["cap", "join"] {
                        if let Some(span) = site(&[k]) {
                            let f = fail("E0362", format!("`{k}:` on a brushed stroke"))
                                .note("a brushed stroke has round ends and corners, and the brush shapes its ends")
                                .fix(format!("remove `{k}:`, or add `on:fill` to the brush"));
                            self.report(f, &file, span, &node);
                            return Err(Fail::reported());
                        }
                    }
                }
                let explicit = a.seed()?;
                (
                    self.area_marks(&fill_stack, key, explicit, &file),
                    self.ribbon_marks(&stroke_stack, key, explicit, &file),
                )
            }
        };
        let mut channels = Channels::default();
        for (k, _, example) in CHANNEL_KEYS {
            let Some(v) = pick(k) else { continue };
            if stroke_marks.is_empty() && !sets.iter().any(|s| s.reads(k)) {
                let Some(span) = site(&[k]) else { continue };
                let why = if !sets.is_empty() {
                    format!("the node's stamps do not read `{k}`")
                } else if width.is_none() {
                    "this node has no stroke".to_string()
                } else {
                    "this node's stroke has no brush".to_string()
                };
                let f = fail("E0362", format!("`{k}:` has no effect because nothing on this node reads it"))
                    .note(why)
                    .fix(format!("brush the stroke, read `{k}` in a stamp, or remove `{k}:`"));
                self.report(f, &file, span, &node);
                return Err(Fail::reported());
            }
            let values = numbers(&v).filter(|xs| !xs.is_empty() && xs.iter().all(|x| x.is_finite()));
            let Some(values) = values else {
                let f = fail("E0320", format!("`{k}:` takes a number or a list of numbers, got {}", v.kind()))
                    .fix(format!("write `{example}`"));
                return Err(at_key(self, f, &[k]));
            };
            channels.set(k, values);
        }
        let fill = fill.map(|paint| IrFill { paint, marks: fill_marks });
        let tapered = matches!(width, Some(W::List(_))) || (width.is_some() && taper.is_some());
        if tapered && stroke_marks.is_empty() {
            if pick("dash").is_some() {
                let f = fail("E0362", "`dash:` on a tapered stroke")
                    .note("a tapered stroke is a filled outline, which has no dash")
                    .fix("remove the dash, or give the stroke one width and no `taper:`");
                return Err(at_key(self, f, &["dash", "stroke", "taper"]));
            }
            for k in ["cap", "join"] {
                if let Some(span) = site(&[k]) {
                    let f = fail("E0362", format!("`{k}:` on a tapered stroke"))
                        .note("a tapered stroke has round ends and corners")
                        .fix(format!("remove `{k}:`, or give the stroke one width and no `taper:`"));
                    self.report(f, &file, span, &node);
                    return Err(Fail::reported());
                }
            }
        }
        let widths = match &width {
            None => vec![0.0],
            Some(W::Const(w)) => vec![*w],
            Some(W::List(ws)) if ws.is_empty() => vec![1.0],
            Some(W::List(ws)) => ws.clone(),
        };
        let stroke_paint = match &width {
            Some(_) => Some(to_paint(&paint_v, bounds).ok_or_else(|| fail("E0320", "stroke color must be a color or gradient"))?),
            None => None,
        };
        let spine = (!sets.is_empty() || !stroke_marks.is_empty())
            .then(|| brush::Spine::new(&geometry, knots, &widths, &channels));
        let (stamps, rows) = match &spine {
            Some(spine) if !sets.is_empty() => {
                let host = crate::stamp::Host {
                    key,
                    explicit: a.seed()?,
                    world,
                    own_site: site(&["stamp"]),
                    span,
                    paint: stroke_paint.clone().or_else(|| fill.as_ref().map(|f| f.paint.clone())),
                };
                let mut at_stamp = |this: &mut Self, f: Fail| at_key(this, f, &["stamp"]);
                self.stamps(&sets, spine, &host, sc, &mut at_stamp)?
            }
            _ => (Vec::new(), Vec::new()),
        };
        if width.is_none() {
            return Ok((
                Shape {
                    geometry: geometry.to_ir(),
                    fill,
                    stroke: None,
                    blur,
                    stamps,
                    children: vec![],
                },
                rows,
            ));
        }
        let paint = stroke_paint.expect("a stroke has a paint");
        if let Some(spine) = spine.filter(|_| !stroke_marks.is_empty()) {
            let ribbon = brush::ribbon(&spine, taper.unwrap_or((0.0, 0.0)), stroke_marks);
            let ink = tapered.then(|| {
                // The same 0.75 output pixel step as the unbrushed tapered outline below.
                let px = (axis_scale(&world.flat()) * self.job.ink_scale()).max(1e-6);
                geom::ribbon_ink(&geometry, &ribbon, 0.75 / px)
            });
            return Ok((
                Shape {
                    geometry: geometry.to_ir(),
                    fill,
                    stroke: Some(Stroke {
                        paint,
                        width: widths.iter().copied().fold(0.0, f64::max) as f32,
                        cap: Cap::Round,
                        join: Join::Round,
                        dash: Vec::new(),
                        ink: ink.as_ref().map(ToIr::to_ir),
                        ribbon: Some(ribbon),
                    }),
                    blur,
                    stamps,
                    children: vec![],
                },
                rows,
            ));
        }
        if tapered {
            let spine = brush::Spine::new(&geometry, knots, &widths, &Channels::default());
            let ribbon = brush::ribbon(&spine, taper.unwrap_or((0.0, 0.0)), Vec::new());
            // 0.75 output pixels. The local step divides by the accumulated transform
            // scale and the target scale, so a unit-space stroke matches one drawn in pixels.
            let px = (axis_scale(&world.flat()) * self.job.ink_scale()).max(1e-6);
            let outline = geom::ribbon_ink(&geometry, &ribbon, 0.75 / px);
            // The gradient spans the ink, not the spine.
            let ob = geom::bounds(&outline);
            let paint = to_paint(&paint_v, ob)
                .ok_or_else(|| fail("E0320", "stroke color must be a color or gradient"))?;
            return Ok((
                Shape {
                    geometry: geometry.to_ir(),
                    fill,
                    stroke: Some(Stroke {
                        paint,
                        width: widths.iter().copied().fold(0.0, f64::max) as f32,
                        cap: Cap::Round,
                        join: Join::Round,
                        dash: Vec::new(),
                        ink: Some(outline.to_ir()),
                        ribbon: None,
                    }),
                    blur,
                    stamps,
                    children: vec![],
                },
                rows,
            ));
        }
        let w = widths[0];
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
        let dash = match pick("dash") {
            None => Vec::new(),
            Some(v) => numbers(&v)
                .filter(|d| !d.is_empty() && d.iter().all(|x| x.is_finite() && *x >= 0.0))
                .ok_or_else(|| fail("E0320", "`dash:` takes lengths").fix("write `dash:4,2`"))?
                .into_iter()
                .map(|v| v as f32)
                .collect(),
        };
        Ok((
            Shape {
                geometry: geometry.to_ir(),
                fill,
                stroke: Some(Stroke {
                    paint,
                    width: w as f32,
                    cap,
                    join,
                    dash,
                    ink: None,
                    ribbon: None,
                }),
                blur,
                stamps,
                children: vec![],
            },
            rows,
        ))
    }

    /// The value of a parameter default: `one_of(..)` is its first word, `none(T)` is `none`,
    /// and anything else is evaluated.
    pub(crate) fn default_value(&mut self, e: &Expr, sc: &Scope) -> R<Value> {
        match marker_value(e) {
            Some(v) => Ok(v),
            None => self.expr(e, sc),
        }
    }

    fn parameter_literal(&mut self, value: &Expr, scope: &Scope) -> R<Value> {
        match value {
            Expr::Ident(word) if !matches!(word.as_str(), "true" | "false" | "none") => Ok(Value::Ident(word.clone())),
            Expr::Tuple(values) => Value::tuple(values.iter().map(|v| self.parameter_literal(v, scope)).collect::<R<_>>()?),
            Expr::List(values) => Ok(Value::List(Rc::new(values.iter().map(|v| self.parameter_literal(v, scope)).collect::<R<Vec<_>>>()?))),
            _ => self.expr(value, scope),
        }
    }

    /// One entry `param`, bound to its `--set` value when the job carries one for this file.
    fn bind_entry_param(&mut self, p: &Named, module: &Rc<Module>, scope: &Scope) -> R<Vec<(String, Value)>> {
        let mut given = Vec::new();
        if module.path == self.job.source
            && let Some(value) = self.job.parameters.get(&p.key)
        {
            given.push((p.key.clone(), self.parameter_literal(value, scope)?));
        }
        let sig = self.project.outlines().entry_params(&module.path).cloned().expect("an entry file with `param` lines has entry parameters");
        self.bind_params(std::slice::from_ref(p), &sig, module, &given, p.span, scope)
    }

    /// Binds `params` to the `given` arguments, each checked against `sig`, or to their defaults,
    /// evaluated in `module` with the caller's palette.
    pub(crate) fn bind_params(
        &mut self,
        params: &[Named],
        sig: &Signature,
        module: &Rc<Module>,
        given: &[(String, Value)],
        span: Span,
        caller: &Scope,
    ) -> R<Vec<(String, Value)>> {
        for (k, v) in given {
            if let Some(p) = sig.get(k) {
                check_param(&sig.name, p, v)?;
            }
        }
        let mut def_scope = self.module_scope(module);
        def_scope.palette = caller.palette.clone();
        let mut env: Vec<(String, Value)> = Vec::new();
        for p in params {
            let value = match given.iter().rev().find(|(k, _)| k == &p.key) {
                Some((_, v)) => v.clone(),
                None => {
                    let mut dsc = def_scope.clone();
                    dsc.env = env.clone();
                    self.default_value(&p.value, &dsc).map_err(|f| {
                        f.note(format!(
                            "in the default of `{}` ({}:{})",
                            p.key, module.path, span.line
                        ))
                    })?
                }
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
        key: Key,
        origin: String,
        decl: Option<DeclId>,
        forward: Vec<PendingPatch>,
        span: Span,
        name_rejected: bool,
    ) -> R<()> {
        let path = key.to_string();
        let module = self
            .modules
            .get(&comp.module)
            .cloned()
            .ok_or_else(|| fail("E0210", format!("module `{}` is not loaded", comp.module)))?;
        let param_names: Vec<&str> = comp.params.iter().map(|p| p.key.as_str()).collect();
        let mut given: Vec<(String, Value)> = Vec::new();
        for (i, v) in a.pos.iter().enumerate() {
            let p = comp.params.get(i).ok_or_else(|| {
                fail(
                    "E0321",
                    format!(
                        "`{}` takes {} positional argument(s)",
                        comp.name,
                        comp.params.len().min(1)
                    ),
                )
                .fix("pass the rest as key:value")
            })?;
            given.push((p.key.clone(), v.clone()));
        }
        let mut node_args = Args::default();
        let mut key = key;
        let mut palette = sc.palette.clone();
        if let Some(by) = a.timed_key("seed") {
            return Err(by.sink("`seed:`"));
        }
        for (k, v) in &a.named {
            if param_names.contains(&k.as_str()) {
                given.push((k.clone(), v.clone()));
            } else if k == "seed" {
                key = key.seeded(&comp.name, v.seed()?);
            } else if k == "palette" {
                if !matches!(v, Value::Palette(_) | Value::Module(_)) {
                    return Err(fail(
                        "E0320",
                        format!("`palette:` takes a palette, got {}", v.kind()),
                    )
                    .fix("write `palette:elite`"));
                }
                palette = Some(v.clone());
            } else if NODE_KEYS.contains(&k.as_str()) {
                node_args.named.push((k.clone(), v.clone()));
            } else {
                return Err(no_parameter(&comp.name, &comp.name, k, &param_names));
            }
        }
        for (k, v) in &given {
            if let Some(p) = comp.sig.get(k) {
                check_param(&comp.name, p, v)?;
            }
        }
        let (m, at) = self.transform(&node_args, (0.0, 0.0), sc.frame, sc, true)?;
        let caller_local = inverse(&m).map_rect(sc.frame).0;
        let mut box_frame = Rect::new_empty();
        let mut def_scope = self.module_scope(&module);
        def_scope.palette = palette.clone();
        for (i, by) in &a.timed_pos {
            if let Some(p) = comp.params.get(*i) {
                def_scope.timed.insert(p.key.clone(), by.clone());
            }
        }
        for (k, by) in &a.timed {
            if param_names.contains(&k.as_str()) {
                def_scope.timed.insert(k.clone(), by.clone());
            }
        }
        let mut env: Vec<(String, Value)> = Vec::new();
        let mut typed_params = Vec::new();
        for p in &comp.params {
            let ty = comp.sig.get(&p.key).map(|p| &p.ty);
            if let Some(ty @ (Ty::Brush(_) | Ty::Stamp(_))) = ty {
                typed_params.push((env.len(), ty.clone()));
            }
            let value = match given.iter().rev().find(|(k, _)| k == &p.key) {
                Some((_, v)) => v.clone(),
                None => {
                    let mut dsc = def_scope.clone();
                    dsc.env = env.clone();
                    let (default, by) = self.timed_by(|ev| ev.default_value(&p.value, &dsc));
                    if let Some(by) = by {
                        def_scope.timed.insert(p.key.clone(), by);
                    }
                    default.map_err(|f| {
                        f.note(format!(
                            "in the default of `{}` ({}:{})",
                            p.key, comp.module, comp.span.line
                        ))
                    })?
                }
            };
            let value = if p.key == "size" {
                let (value, rect) =
                    resolve_size(&value, ty == Some(&Ty::Vector), caller_local, sc, &comp.name)?;
                box_frame = rect;
                value
            } else {
                value
            };
            env.push((p.key.clone(), value));
        }
        let def_ids = self
            .ids
            .get(&comp.module)
            .cloned()
            .unwrap_or_else(|| Rc::new(ModuleIds::blank()));
        let region = def_ids
            .region(comp.span)
            .unwrap_or_else(|| def_ids.file());
        if !name_rejected {
            self.unstable_scope(&key, &region, sc.file(), span);
        }
        if let Some(err) = self.needs_of(comp, sc) {
            return Err(err);
        }
        let mut imports = Vec::new();
        if let Some(host) = &sc.host {
            for need in &comp.needs {
                if let Some((name, names)) = identity::host_import(&host.names, need) {
                    imports.push(identity::Import {
                        name,
                        names,
                        copies: host.copies.clone(),
                        loops: host.loops.clone(),
                    });
                }
            }
        }
        let (world, warp_cell) = content_place(&sc.world, &node_args, m);
        let mut fills = Vec::new();
        let mut call_sc = sc.clone();
        call_sc.palette = palette.clone();
        let call_fx = call_sc.collect_branch_fx();
        let mut call_patches = Vec::new();
        for b in block.items() {
            self.absorb_item(b, &mut call_sc, &key, &comp.name, &mut fills, &mut call_patches)?;
        }
        let names = Rc::new(RefCell::new(Names::new(region.clone())));
        let inst = Rc::new(Instance {
            def_name: comp.name.clone(),
            fills,
            names: names.clone(),
            used_fills: RefCell::new(Vec::new()),
            record: RefCell::new(None),
        });
        let mut incoming = forward;
        incoming.extend(call_patches);
        let pending = self.admit(&region, incoming, &path);
        let lookup = Lookup::with_imports(names.clone(), imports);
        let checks = Rc::new(RefCell::new(Vec::new()));
        let mut body_scope = sc.enter(
            key.clone(),
            names.clone(),
            lookup,
            pending,
            box_frame,
            world.clone(),
            sc.depth + 1,
        );
        body_scope.env = env;
        body_scope.timed = def_scope.timed;
        body_scope.typed_params = Rc::new(typed_params);
        body_scope.module = module;
        body_scope.ids = def_ids;
        body_scope.inst = Some(inst.clone());
        body_scope.host = None;
        body_scope.checks = checks.clone();
        body_scope.style = sc.style.clone();
        body_scope.palette = palette;
        let def_fx = body_scope.collect_branch_fx();
        let placeholder = new_node(
            path.clone(),
            origin.clone(),
            m,
            NodeKind::Group {
                isolate: true,
                children: vec![],
            },
        );
        let rec = match decl {
            Some(id) => self.place(
                sc,
                id,
                &key,
                sc.world.clone(),
                Some(placeholder),
                None,
                at,
                Some(names.clone()),
            ),
            None => {
                let rec = Rc::new(RefCell::new(Record {
                    path: path.clone(),
                    key: key.clone(),
                    copies: Vec::new(),
                    parent_world: sc.world.clone(),
                    node: Some(placeholder),
                    anchor: None,
                    at,
                    home: None,
                    decl: None,
                    inner: Some(names.clone()),
                    rows: Rc::default(),
                    seam: 0.0,
                }));
                self.placed.borrow_mut().insert(path.clone(), rec.clone());
                rec
            }
        };
        let children =
            self.eval_block_nodes(comp.body.iter().filter_map(DefItem::scene), &mut body_scope);
        self.report_unlanded(&body_scope);
        let mut node = new_node(
            path.clone(),
            origin,
            m,
            NodeKind::Group {
                isolate: true,
                children,
            },
        );
        let f = first_fill(&node).unwrap_or(Rgba::GRAY);
        let fx_scope = body_scope.child(key.clone(), body_scope.frame, world.clone());
        let (def_fx, call_fx) = (def_fx.take(), call_fx.take());
        if let Some(of) = sc.operand_of.clone() {
            self.report_operand_fx(&of, &call_fx, sc);
        }
        let def_lines = in_source_order(comp.body.iter().filter_map(DefItem::fx), &def_fx, &fx_scope);
        let call_lines = in_source_order(block.fx(), &call_fx, &call_sc);
        let def_ordinals = fx_ordinals(comp.body.iter().filter_map(DefItem::fx), comp.body.iter().filter_map(DefItem::scene));
        let call_ordinals = fx_ordinals(block.fx(), block.scenes());
        let sites: Vec<FxSite> = def_lines
            .iter()
            .map(|l| (l, def_ordinals[&l.0.span]))
            .chain(call_lines.iter().map(|l| (l, def_ordinals.len() as u32 + call_ordinals[&l.0.span])))
            .map(|((line, scope), ordinal)| FxSite { line, scope, ordinal, frame: call_sc.frame, world: world.clone() })
            .collect();
        node.effects = self.lower_fx(&sites, &key, f, node_args.seed()?);
        self.mask(&mut node, &node_args, block.mask(), &call_sc, call_sc.frame, &world, &key)?;
        let z = self.finish_node(&mut node, &node_args, sc, warp_cell)?;
        rec.borrow_mut().node = Some(node.clone());
        *inst.record.borrow_mut() = Some(rec.clone());
        self.run_checks(&checks);
        for fill in &inst.fills {
            if !inst.used_fills.borrow().contains(&fill.name) {
                let file = fill.scope.file().to_string();
                self.report(
                    fail(
                        "E0335",
                        format!("`{}` has no slot `{}`", comp.name, fill.name),
                    )
                    .fix("fill one of the slots the def declares with `slot <name>`"),
                    &file,
                    fill.span,
                    &path,
                );
            }
        }
        out.items.push((node, z));
        Ok(())
    }

    fn needs_of(&self, comp: &Component, sc: &Scope) -> Option<Fail> {
        if comp.needs.is_empty() {
            return None;
        }
        let Some(host) = &sc.host else {
            let need = &comp.needs[0];
            return Some(
                fail(
                    "E0311",
                    format!(
                        "{} needs @{need}, but it is not slot content",
                        comp.name
                    ),
                )
                .fix("fill a slot on a host that has the part"),
            );
        };
        for need in &comp.needs {
            if let Err(parts) = identity::host_part(&host.names.borrow(), need) {
                let mut f = fail(
                    "E0311",
                    format!(
                        "{} needs @{need}, but the host `{}` has no part named {need}",
                        comp.name, host.def_name
                    ),
                )
                .note(format!("parts available: {}", parts.join(", ")));
                f = match parts.first() {
                    Some(first) => f.fix(format!(
                        "rename {}'s `{first}` to `{need}`, or fill a slot on a host that has `{need}`",
                        host.def_name
                    )),
                    None => f.fix(format!("give `{}` a part named `{need}`", host.def_name)),
                };
                return Some(f);
            }
        }
        None
    }

    /// Checks patches that hop through `comp` against that def's names, and keeps
    /// checking the tail after each instance or slot. An off instance never reaches
    /// `instance`, so a one-hop check would accept `.hero.head.typo`.
    fn admit_forward(
        &mut self,
        comp: &Component,
        forward: &[PendingPatch],
        path: &str,
        call: &DrawBody,
        caller_ids: &Rc<ModuleIds>,
    ) {
        if forward.is_empty() {
            return;
        }
        let ids = self
            .ids
            .get(&comp.module)
            .cloned()
            .unwrap_or_else(|| Rc::new(ModuleIds::blank()));
        let region = ids
            .region(comp.span)
            .unwrap_or_else(|| ModuleIds::blank().file());
        let scenes: Vec<Scene> = comp.body.iter().filter_map(DefItem::scene).cloned().collect();
        let scope = PatchScope {
            ids,
            region,
            scenes,
            fills: fills_in(call, caller_ids),
        };
        self.admit_scope(&scope, forward.to_vec(), path);
    }

    fn admit_scope(&mut self, scope: &PatchScope, patches: Vec<PendingPatch>, path: &str) {
        let pending = self.admit(&scope.region, patches, path);
        let kept: Vec<PendingPatch> = pending.items().to_vec();
        for patch in kept {
            self.follow_scope(scope, &patch, path);
        }
    }

    /// Follows a patch that still has hops after a scope-opening name.
    /// An arm whose component is not known statically is left unchecked.
    /// Exclusive arms report E0340 only when every known arm rejects the tail.
    fn follow_scope(&mut self, scope: &PatchScope, patch: &PendingPatch, path: &str) {
        let Ok(walked) = scope.region.walk(&patch.rest) else {
            return;
        };
        if walked.rest.is_empty() {
            return;
        }
        let mut children = Vec::new();
        let mut unresolved = false;
        for id in walked.ids {
            let decl = scope.region.decl(id);
            if !decl.kind.opens_region() {
                continue;
            }
            match self.child_scope(scope, decl) {
                Some(child) => children.push((child, walked.rest.clone())),
                None => unresolved = true,
            }
        }
        if unresolved || children.is_empty() {
            return;
        }
        if children.len() == 1 {
            let (child, tail) = children.pop().unwrap();
            let mut next = patch.clone();
            next.rest = tail;
            self.admit_scope(&child, vec![next], path);
            return;
        }
        let saved_diags = self.diags.clone();
        let saved_reported = self.reported.clone();
        let mut failure = None;
        for (child, tail) in children {
            self.diags.clone_from(&saved_diags);
            self.reported.clone_from(&saved_reported);
            let before = self.diags.len();
            let mut next = patch.clone();
            next.rest = tail;
            self.admit_scope(&child, vec![next], path);
            if self.diags.len() == before {
                return;
            }
            if failure.is_none() {
                failure = Some((self.diags.clone(), self.reported.clone()));
            }
        }
        if let Some((diags, reported)) = failure {
            self.diags = diags;
            self.reported = reported;
        }
    }

    fn child_scope(&self, scope: &PatchScope, decl: &identity::NameDecl) -> Option<PatchScope> {
        match decl.kind {
            identity::NameKind::Slot => {
                if let Some(fill) = scope.fills.iter().find(|f| f.name == decl.name.as_str()) {
                    let region = fill
                        .ids
                        .region(fill.span)
                        .unwrap_or_else(|| ModuleIds::blank().file());
                    return Some(PatchScope {
                        ids: fill.ids.clone(),
                        region,
                        scenes: fill.body.clone(),
                        fills: Vec::new(),
                    });
                }
                let ids = self
                    .ids
                    .get(&*decl.site.file)
                    .cloned()
                    .unwrap_or_else(|| scope.ids.clone());
                let region = ids
                    .region(decl.site.span)
                    .unwrap_or_else(|| ModuleIds::blank().file());
                let scenes = slot_fallback(&scope.scenes, decl.site.span);
                Some(PatchScope {
                    ids,
                    region,
                    scenes,
                    fills: Vec::new(),
                })
            }
            identity::NameKind::Instance => {
                let verb = decl.call.as_deref()?;
                let module = self.modules.get(&*decl.site.file)?;
                let comp = match module.global(verb)? {
                    Value::Component(c) => c,
                    Value::Alias(a) => a.comp.clone(),
                    _ => return None,
                };
                let ids = self.ids.get(&comp.module)?.clone();
                let region = ids
                    .region(comp.span)
                    .unwrap_or_else(|| ModuleIds::blank().file());
                let scenes: Vec<Scene> =
                    comp.body.iter().filter_map(DefItem::scene).cloned().collect();
                let fills = find_draw(&scope.scenes, decl.site.span)
                    .map(|d| fills_in(&d.body, &scope.ids))
                    .unwrap_or_default();
                Some(PatchScope {
                    ids,
                    region,
                    scenes,
                    fills,
                })
            }
            identity::NameKind::Shape | identity::NameKind::Group | identity::NameKind::Anchor => {
                None
            }
        }
    }

    /// The region a hidden slot would have drawn, fill first, then the fallback.
    fn slot_scope(
        &self,
        inst: &Instance,
        name: &str,
        span: Span,
        fallback: &[Scene],
        sc: &Scope,
    ) -> PatchScope {
        if let Some(fill) = inst.fills.iter().find(|f| f.name == name) {
            let ids = fill.scope.ids.clone();
            let region = ids
                .region(fill.span)
                .unwrap_or_else(|| ModuleIds::blank().file());
            return PatchScope {
                ids,
                region,
                scenes: fill.body.clone(),
                fills: Vec::new(),
            };
        }
        let ids = sc.ids.clone();
        let region = ids
            .region(span)
            .unwrap_or_else(|| ModuleIds::blank().file());
        PatchScope {
            ids,
            region,
            scenes: fallback.to_vec(),
            fills: Vec::new(),
        }
    }

    fn admit(
        &mut self,
        region: &identity::Region,
        incoming: Vec<PendingPatch>,
        at: &str,
    ) -> Pending {
        Pending::enter(
            region,
            incoming,
            at,
            &mut self.reported,
            &mut self.diags,
        )
    }

    pub(crate) fn report_unlanded(&mut self, sc: &Scope) {
        let pending = Rc::clone(&sc.pending);
        let owner = sc.owner().clone();
        let at = sc.node_name();
        let names = sc.names.borrow();
        identity::report_misses(
            &pending,
            &names,
            &owner,
            &at,
            &mut self.reported,
            &mut self.diags,
        );
    }

    fn slot(
        &mut self,
        st: &Scene,
        name: &str,
        named: &[Named],
        fallback: &[Scene],
        sc: &mut Scope,
        out: &mut Siblings,
    ) -> R<()> {
        let inst = sc.inst.clone().ok_or_else(|| {
            fail("E0334", "`slot` is only valid inside a `def`").fix("move it into a def body")
        })?;
        let key = sc.node_key(st.span);
        let path = key.to_string();
        let decl = sc.decl_at(st.span);
        let host_region = sc.names.borrow().region();
        let arrival = match decl {
            Some(id) => sc.pending.arrive(id, &host_region, &sc.scope_copies),
            None => Arrival {
                own: Vec::new(),
                forward: Vec::new(),
            },
        };
        let mark_used = |inst: &Instance| {
            if inst.fills.iter().any(|f| f.name == name) {
                inst.used_fills.borrow_mut().push(name.to_string());
            }
        };
        if arrival
            .own
            .iter()
            .any(|p| p.channel.is_none() && p.off)
        {
            mark_used(&inst);
            let scope = self.slot_scope(&inst, name, st.span, fallback, sc);
            self.admit_scope(&scope, arrival.forward.clone(), &path);
            return Ok(());
        }
        self.seed_node(sc.file(), &key);
        let mut a = self.eval_args(&[], named, sc, 0)?;
        if self.land(&mut a, &arrival.own, (0.0, 0.0), sc.frame, sc, &path).is_none() {
            mark_used(&inst);
            let scope = self.slot_scope(&inst, name, st.span, fallback, sc);
            self.admit_scope(&scope, arrival.forward.clone(), &path);
            return Ok(());
        }
        let (m, _) = self.transform(&a, (0.0, 0.0), sc.frame, sc, true)?;
        let (world, warp_cell) = content_place(&sc.world, &a, m);
        let mapped = inverse(&m).map_rect(sc.frame).0;
        // Centered on the slot origin, because content with no `at:` defaults to the frame center.
        let local_frame = size_box(mapped.width() as f64, mapped.height() as f64);
        let filled = inst
            .fills
            .iter()
            .find(|f| f.name == name)
            .map(|f| (f.body.clone(), f.scope.clone(), f.span));
        let (ids, region, base) = if let Some((_, scope, span)) = &filled {
            mark_used(&inst);
            let ids = scope.ids.clone();
            let region = ids
                .region(*span)
                .unwrap_or_else(|| ModuleIds::blank().file());
            (ids, region, scope.clone())
        } else {
            let ids = sc.ids.clone();
            let region = ids
                .region(st.span)
                .unwrap_or_else(|| ModuleIds::blank().file());
            (ids, region, sc.clone())
        };
        let names = Rc::new(RefCell::new(Names::new(region.clone())));
        let lookup = if filled.is_some() {
            Lookup::slot(
                names.clone(),
                inst.names.clone(),
                &sc.scope_copies,
                &sc.scope_loops,
                &base.lookup,
                &base.scope_copies,
                &base.scope_loops,
            )
        } else {
            Lookup::under(names.clone(), &sc.lookup, &sc.scope_copies, &sc.scope_loops)
        };
        let pending = self.admit(&region, arrival.forward, &path);
        let mut inner = base.enter(
            key.clone(),
            names.clone(),
            lookup,
            pending,
            local_frame,
            world,
            sc.depth + 1,
        );
        inner.ids = ids;
        if filled.is_some() {
            inner.host = Some(Rc::new(Host {
                def_name: inst.def_name.clone(),
                names: inst.names.clone(),
                copies: sc.scope_copies.clone(),
                loops: sc.scope_loops.clone(),
            }));
        }
        let placed = decl.map(|id| {
            self.place(
                sc,
                id,
                &key,
                sc.world.clone(),
                None,
                None,
                (0.0, 0.0),
                Some(names),
            )
        });
        let children = match &filled {
            Some((body, _, _)) => self.eval_block_nodes(body, &mut inner),
            None => self.eval_block_nodes(fallback, &mut inner),
        };
        self.report_unlanded(&inner);
        let mut node = new_node(
            path,
            format!("{}:{}:{}", sc.file(), st.span.line, st.span.col),
            m,
            NodeKind::Group {
                isolate: false,
                children,
            },
        );
        let z = self.finish_node(&mut node, &a, sc, warp_cell)?;
        if let Some(rec) = placed {
            rec.borrow_mut().node = Some(node.clone());
        }
        out.items.push((node, z));
        Ok(())
    }

    pub fn collect_entry(&mut self, scenes: &[Scene], sc: &mut Scope) {
        let saved = sc.module.globals.borrow().clone();
        let mut scratch = sc.clone();
        let names = Rc::new(RefCell::new(Names::new(sc.ids.file())));
        scratch.names = names.clone();
        scratch.lookup = Lookup::own(names);
        self.prepass = true;
        self.collect_surface(scenes, &mut scratch, true);
        self.prepass = false;
        *sc.module.globals.borrow_mut() = saved;
        let region = sc.names.borrow().region();
        let incoming = std::mem::take(&mut self.incoming);
        let pending = self.admit(&region, incoming, "");
        sc.pending = Rc::new(pending);
    }

    fn collect_surface(&mut self, scenes: &[Scene], sc: &mut Scope, schedule: bool) {
        let stmts: Vec<&Scene> = scenes.iter().collect();
        self.run_block(&stmts, sc, |ev, st, sc, _| ev.collect_one(st, sc, schedule));
    }

    fn collect_one(&mut self, st: &Scene, sc: &mut Scope, schedule: bool) -> R<()> {
        match &st.kind {
            SceneKind::Let { name, value } => {
                self.seed_stmt(sc, st.span);
                let v = match self.quiet_expr(value, sc) {
                    Ok(v) => v,
                    Err(f) if f.code == "E0344" => Value::Unseen,
                    Err(_) => Value::None,
                };
                if sc.top && sc.publish {
                    sc.module.bind(name, v.clone());
                }
                sc.env.push((name.clone(), v));
                Ok(())
            }
            SceneKind::If {
                cond,
                then,
                otherwise,
            } => {
                self.seed_stmt(sc, st.span);
                let taken = self.quiet_expr(cond, sc).map(|c| c.truthy());
                if schedule && (holds_play(then) || holds_play(otherwise)) {
                    let decided = taken.as_ref().ok().map(|&b| usize::from(b));
                    self.guards.insert(position(sc, st.span), decided);
                }
                let Ok(taken) = taken else {
                    return Ok(());
                };
                let mut inner = sc.clone();
                inner.publish = false;
                let branch = if taken { then } else { otherwise };
                self.collect_surface(branch, &mut inner, schedule);
                Ok(())
            }
            SceneKind::Each {
                index,
                var,
                iter,
                body,
            } => {
                self.seed_stmt(sc, st.span);
                let items = self.quiet_expr(iter, sc).and_then(each_items);
                if schedule && holds_play(body) {
                    let decided = items.as_ref().ok().map(Vec::len);
                    self.guards.insert(position(sc, st.span), decided);
                }
                let Ok(items) = items else {
                    return Ok(());
                };
                for (i, v) in items.into_iter().enumerate() {
                    let mut inner = sc.iteration(st.span, i as u32);
                    inner.publish = false;
                    inner.bind_loop(index.as_deref(), var, i, v);
                    self.collect_surface(body, &mut inner, schedule);
                }
                Ok(())
            }
            SceneKind::Play { name, start } if schedule => {
                let patches = self.schedule_play(name.as_deref(), start.as_ref(), sc, st.span, &sc.owner);
                self.incoming.extend(patches);
                Ok(())
            }
            SceneKind::Draw(draw) => {
                self.collect_draw(draw, sc, st.span);
                Ok(())
            }
            SceneKind::Play { .. }
            | SceneKind::Check { .. }
            | SceneKind::Slot { .. }
            | SceneKind::Fx(_) => Ok(()),
        }
    }

    /// A name the real pass will register, recorded so a later `if @name` can take its branch.
    /// The stub has no geometry: `@name` is visible, `@name.w` is not. The skips match `eval_node`.
    fn collect_draw(&mut self, draw: &Draw, sc: &Scope, span: Span) {
        let verb = draw.verb.as_str();
        if verb == "artboard" || matches!(self.binding(verb, sc), Some(Value::None | Value::Unseen)) {
            return;
        }
        let key = sc.node_key(span);
        if let Some(id) = sc.decl_at(span) {
            self.place(sc, id, &key, sc.world.clone(), None, None, (0.0, 0.0), None);
        }
        if self.verb_is_call(verb, sc) {
            return;
        }
        if verb == "repeat" || verb == "grid" {
            self.collect_loop(draw, sc, span, &key);
            return;
        }
        self.collect_body(draw, sc, &key);
    }

    fn collect_body(&mut self, draw: &Draw, sc: &Scope, key: &Key) {
        let mut inner = sc.child(key.clone(), sc.frame, sc.world.clone());
        let ordinals = fx_ordinals(draw.body.fx(), draw.body.scenes());
        for item in draw.body.items() {
            match item {
                DrawItem::Scene(st) => {
                    let _ = self.collect_one(st, &mut inner, false);
                }
                DrawItem::Mask {
                    mask: ast::Mask::Block(body),
                    ..
                } => {
                    let mut mask_sc = sc.child(key.mask(), sc.frame, sc.world.clone());
                    self.collect_surface(body, &mut mask_sc, false);
                }
                DrawItem::Fx(f) if !f.mask_block().is_empty() => {
                    let mut mask_sc = sc.child(key.fx_mask(ordinals[&f.span]), sc.frame, sc.world.clone());
                    self.collect_surface(f.mask_block(), &mut mask_sc, false);
                }
                _ => {}
            }
        }
    }

    /// Registers each `repeat` or `grid` copy the draw pass will register.
    /// A count that cannot be read registers none, so a later `if @name.0` does not see a stub.
    fn collect_loop(&mut self, draw: &Draw, sc: &Scope, span: Span, key: &Key) {
        self.seed_node(sc.file(), key);
        let n = self.loop_copies(draw, sc);
        if n == 0 {
            return;
        }
        for item in draw.body.items() {
            if let DrawItem::Mask {
                mask: ast::Mask::Block(body),
                ..
            } = item
            {
                let mut mask_sc = sc.child(key.mask(), sc.frame, sc.world.clone());
                self.collect_surface(body, &mut mask_sc, false);
            }
        }
        for i in 0..n {
            let mut inner = sc.iteration(span, i as u32);
            for item in draw.body.items() {
                if let DrawItem::Scene(st) = item {
                    let _ = self.collect_one(st, &mut inner, false);
                }
            }
        }
    }

    fn loop_copies(&mut self, draw: &Draw, sc: &Scope) -> usize {
        let Some(expr) = draw.args.first() else {
            return 0;
        };
        let Ok(v) = self.quiet_expr(expr, sc) else {
            return 0;
        };
        match draw.verb.as_str() {
            "repeat" => v
                .num()
                .map(|n| (n.max(0.0) as usize).min(10_000))
                .unwrap_or(0),
            "grid" => v
                .vec2()
                .map(|(cols, rows)| cols.max(1.0) as usize * rows.max(1.0) as usize)
                .unwrap_or(0),
            _ => 0,
        }
    }

    /// The real pass checks the prepass's answer at a guard around an entry-file play. Those
    /// plays were scheduled from that answer, so a different one would play the wrong anims.
    fn confirm_guard(&mut self, st: &Scene, word: &str, decided: usize, sc: &Scope) {
        if !sc.top {
            return;
        }
        let Some(&planned) = self.guards.get(&position(sc, st.span)) else {
            return;
        };
        let file = sc.file().to_string();
        let reported = self
            .diags
            .iter()
            .any(|d| d.code == "E0344" && d.file == file && d.span == st.span);
        if planned == Some(decided) || reported {
            return;
        }
        self.report(
            fail(
                "E0344",
                format!("this `{word}` decides a top-level `play` and reads the drawn scene"),
            )
            .note("top-level plays are scheduled before anything is drawn, so `@name.w` and names inside other draws are not known yet")
            .fix("guard the play with `let`s, `clip`, or `@name` of an earlier top-level draw, or move it under the instance it drives"),
            &file,
            st.span,
            &sc.node_name(),
        );
    }

    fn absorb_item(
        &mut self,
        item: &DrawItem,
        sc: &mut Scope,
        owner: &Key,
        def_name: &str,
        fills: &mut Vec<Fill>,
        dest: &mut Vec<PendingPatch>,
    ) -> R<()> {
        match item {
            DrawItem::SlotFill(fill) => {
                fills.push(Fill {
                    name: fill.name.clone(),
                    body: fill.body.clone(),
                    scope: sc.clone(),
                    span: fill.span,
                });
                Ok(())
            }
            DrawItem::Patch(patch) => {
                self.push_patch(&patch.path, &patch.named, patch.off, sc, patch.span, None, None, dest)?;
                Ok(())
            }
            DrawItem::Fx(_) | DrawItem::Mask { .. } => Ok(()),
            DrawItem::Scene(st) => self.absorb_scene(st, sc, owner, def_name, false, dest),
        }
    }

    fn absorb_scene(
        &mut self,
        st: &Scene,
        sc: &mut Scope,
        owner: &Key,
        def_name: &str,
        nested: bool,
        dest: &mut Vec<PendingPatch>,
    ) -> R<()> {
        let node = owner.to_string();
        match &st.kind {
            SceneKind::If {
                cond,
                then,
                otherwise,
            } => {
                self.seed_stmt(sc, st.span);
                let c = self.untimed("this `if`", cond, sc)?;
                let mut inner = sc.clone();
                let branch = if c.truthy() { then } else { otherwise };
                for child in branch {
                    self.absorb_scene(child, &mut inner, owner, def_name, true, dest)?;
                }
                Ok(())
            }
            SceneKind::Each {
                index,
                var,
                iter,
                body,
            } => {
                self.seed_stmt(sc, st.span);
                let items = each_items(self.untimed("this `each`", iter, sc)?)?;
                for (i, v) in items.into_iter().enumerate() {
                    let mut inner = sc.iteration(st.span, i as u32);
                    inner.bind_loop(index.as_deref(), var, i, v);
                    for child in body {
                        self.absorb_scene(child, &mut inner, owner, def_name, true, dest)?;
                    }
                }
                Ok(())
            }
            SceneKind::Play { name, start } => {
                dest.extend(self.schedule_play(name.as_deref(), start.as_ref(), sc, st.span, owner));
                Ok(())
            }
            SceneKind::Let { name, value } => {
                self.seed_stmt(sc, st.span);
                self.bind_let(sc, name, value)?;
                Ok(())
            }
            SceneKind::Check { .. } if !nested => {
                sc.checks.borrow_mut().push(CheckPending {
                    stmt: st.clone(),
                    scope: sc.clone(),
                });
                Ok(())
            }
            SceneKind::Fx(line) => sc.branch_fx(line),
            _ if nested => {
                self.report(
                    fail("E0330", format!("only patches, `play`, and `fx` under `if` may sit inside `if` or `each` under `{def_name}`"))
                        .fix("keep slot fills and checks as direct lines under the instance"),
                    sc.file(),
                    st.span,
                    &node,
                );
                Ok(())
            }
            _ => {
                self.report(
                    fail("E0330", format!("only slot fills, patches, checks, fx and one mask may go under an instance of `{def_name}`"))
                        .fix("wrap the content in a slot fill such as `head:`, or dedent it"),
                    sc.file(),
                    st.span,
                    &node,
                );
                Ok(())
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn push_patch(
        &mut self,
        segs: &[String],
        named: &[Named],
        off: bool,
        sc: &Scope,
        span: Span,
        anim: Option<String>,
        play: Option<Site>,
        dest: &mut Vec<PendingPatch>,
    ) -> R<()> {
        let mut vals = Vec::new();
        let mut timed = Vec::new();
        for n in named {
            if n.key == "name" {
                let note = anim
                    .as_ref()
                    .map(|a| format!("in anim `{a}`"))
                    .unwrap_or_default();
                let mut f = fail("E0320", "`name:` is the node's identity")
                    .fix("set `name:` on the draw; a patch cannot rename it");
                if !note.is_empty() {
                    f = f.note(note);
                }
                self.report(f, sc.file(), n.span, &sc.node_name());
                continue;
            }
            let (v, by) = self.timed_by(|ev| ev.expr(&n.value, sc));
            if let Some(by) = by {
                timed.push((n.key.clone(), by));
            }
            vals.push((n.key.clone(), v?));
        }
        let Some(rest) = identity::hops_from(segs) else {
            self.report(
                fail(
                    "E0340",
                    format!("patch `.{}` is not an address", segs.join(".")),
                )
                .fix("write names and whole-number copy indices, as in `.eye.0`"),
                sc.file(),
                span,
                &sc.node_name(),
            );
            return Ok(());
        };
        if rest.is_empty() {
            return Ok(());
        }
        let report = play.clone().unwrap_or_else(|| Site::new(sc.file(), span));
        dest.push(PendingPatch {
            written: rest.clone(),
            rest,
            origin_depth: sc.depth,
            order: Order::Evaluated(self.next_order()),
            named: vals,
            timed,
            off,
            channel: None,
            report,
            anim,
            play,
        });
        Ok(())
    }

    fn schedule_play(
        &mut self,
        name: Option<&str>,
        start: Option<&Expr>,
        sc: &Scope,
        span: Span,
        owner: &Key,
    ) -> Vec<PendingPatch> {
        let Some(name) = name else {
            return Vec::new();
        };
        let ord = match sc.inst {
            None => Order::Position(position(sc, span)),
            Some(_) => Order::Evaluated(self.next_order()),
        };
        let node = owner.to_string();
        let anim = match sc.module.anims.borrow().get(name).cloned() {
            Some(a) => a,
            None if sc.module.lost(name) => return Vec::new(),
            None => {
                self.report(
                    fail("E0308", format!("no anim `{name}`"))
                        .fix(format!("declare `anim {name}` or import it")),
                    sc.file(),
                    span,
                    &node,
                );
                return Vec::new();
            }
        };
        let snap = self.snap_seed();
        let tag = sc.ids.tag(span).unwrap_or_default();
        self.begin_seed(sc.file(), owner.at_stmt(&[], tag));
        let start_v = match start {
            Some(e) => match self.untimed("`start:`", e, sc).and_then(secs) {
                Ok(n) => n,
                Err(f) => {
                    self.restore_seed(snap);
                    self.report(f, sc.file(), span, &node);
                    return Vec::new();
                }
            },
            None => 0.0,
        };
        let module = self.modules.get(&anim.module).cloned();
        let dur_scope = match &module {
            Some(m) => {
                let mut dur_scope = self.module_scope(m);
                dur_scope.palette = sc.palette.clone();
                dur_scope
            }
            None => sc.clone(),
        };
        self.begin_seed(&anim.module, owner.at_stmt(&[], &format!("anim {}", anim.name)));
        let dur = self.expr(&anim.dur, &dur_scope).and_then(secs);
        self.restore_seed(snap);
        let dur = match dur {
            Ok(n) if n > 0.0 => n,
            Ok(_) => {
                self.report(
                    fail(
                        "E0320",
                        format!("`{}` duration must be positive", anim.name),
                    )
                    .note(format!("played at {}:{}", sc.file(), span.line))
                    .fix("write `dur:1s`"),
                    &anim.module,
                    anim.span,
                    &node,
                );
                return Vec::new();
            }
            Err(f) => {
                self.report(
                    f.note(format!("in anim `{}`", anim.name)).note(format!(
                        "played at {}:{}",
                        sc.file(),
                        span.line
                    )),
                    &anim.module,
                    anim.span,
                    &node,
                );
                return Vec::new();
            }
        };
        self.plays.push(PlayMark {
            end: start_v + dur,
            loops: anim.loop_,
        });
        let raw = self.time - start_v;
        let live = if anim.loop_ {
            true
        } else {
            raw >= 0.0 && raw < dur
        };
        let local = if anim.loop_ {
            raw.rem_euclid(dur)
        } else {
            raw.clamp(0.0, dur)
        };
        let saved = self.anim_clock.replace(AnimClock { local, dur, patch: None });
        let snap = self.snap_seed();
        let play = Site::new(sc.file(), span);
        let mut out = Vec::new();
        let mut counts = BTreeMap::new();
        for patch in &anim.body {
            let path = patch.path.join(".");
            let n = counts.entry(path.clone()).or_insert(0);
            *n += 1;
            let tag = format!("anim {} .{path}~{n}", anim.name);
            self.begin_seed(&anim.module, owner.at_stmt(&[], &tag));
            let Some(rest) = identity::hops_from(&patch.path) else {
                self.report(
                    fail(
                        "E0340",
                        format!(
                            "`{name}` patch `.{}` is not an address",
                            patch.path.join(".")
                        ),
                    )
                    .fix("write names and whole-number copy indices"),
                    sc.file(),
                    span,
                    &node,
                );
                continue;
            };
            if rest.is_empty() {
                continue;
            }
            if patch.off || patch.named.is_empty() {
                out.push(PendingPatch {
                    written: rest.clone(),
                    rest,
                    origin_depth: sc.depth,
                    order: ord.clone(),
                    named: Vec::new(),
                    timed: Vec::new(),
                    off: patch.off,
                    channel: Some(ChannelPatch {
                        key: String::new(),
                        absolute: true,
                        value: Value::None,
                        live: live && patch.off,
                        anim: name.to_string(),
                        patch_span: patch.span,
                        patch_file: anim.module.clone(),
                    }),
                    report: play.clone(),
                    anim: Some(name.to_string()),
                    play: Some(play.clone()),
                });
                continue;
            }
            for n in &patch.named {
                if n.key == "name" {
                    self.report(
                        fail("E0320", "`name:` is the node's identity")
                            .note(format!("in anim `{name}`"))
                            .fix("set `name:` on the draw; an anim cannot rename it"),
                        &anim.module,
                        n.span,
                        &node,
                    );
                    continue;
                }
                let (absolute, expr) = match absolute_channel(&n.key, &n.value) {
                    Some(inner) => (true, inner),
                    None => (!offset_channel(&n.key), &n.value),
                };
                if let Some(clock) = &mut self.anim_clock {
                    clock.patch = Some(Rc::new(Timed {
                        key: n.key.clone(),
                        anim: name.to_string(),
                        file: anim.module.clone(),
                        line: n.span.line,
                    }));
                }
                match self.timed_by(|ev| ev.expr(expr, &dur_scope)).0 {
                    Ok(value) => out.push(PendingPatch {
                        written: rest.clone(),
                        rest: rest.clone(),
                        origin_depth: sc.depth,
                        order: ord.clone(),
                        named: Vec::new(),
                        timed: Vec::new(),
                        off: false,
                        channel: Some(ChannelPatch {
                            key: n.key.clone(),
                            absolute,
                            value,
                            live,
                            anim: name.to_string(),
                            patch_span: n.span,
                            patch_file: anim.module.clone(),
                        }),
                        report: play.clone(),
                        anim: Some(name.to_string()),
                        play: Some(play.clone()),
                    }),
                    Err(f) => self.report(
                        f.note(format!("in anim `{name}`")),
                        &anim.module,
                        n.span,
                        &node,
                    ),
                }
            }
        }
        self.restore_seed(snap);
        self.anim_clock = saved;
        out
    }

    pub fn key_call(&mut self, args: &[Expr], named: &[Named], sc: &Scope) -> R<Value> {
        let Some(clock) = &self.anim_clock else {
            return Err(outside_anim("`key(...)`"));
        };
        let (local, anim_dur) = (clock.local, clock.dur);
        self.timed = clock.patch.clone().or(self.timed.take());
        for n in named {
            if !matches!(n.key.as_str(), "at" | "ease" | "dur") {
                return Err(fail("E0320", format!("`key` does not take `{}:`", n.key))
                    .fix("use at:, ease:, dur: or a bare `loop`"));
            }
        }
        let mut values = Vec::new();
        let mut loop_ = false;
        for e in args {
            if ident_of(e) == Some("loop") {
                loop_ = true;
            } else {
                values.push(self.expr(e, sc)?);
            }
        }
        let dur = match named.iter().find(|n| n.key == "dur") {
            Some(n) => secs(self.expr(&n.value, sc)?)?,
            None => anim_dur,
        };
        if !(dur.is_finite() && dur > 0.0) {
            return Err(fail("E0320", "`key` duration must be positive").fix("write `dur:1s`"));
        }
        let u = if loop_ {
            local.rem_euclid(dur) / dur
        } else if local <= 0.0 {
            0.0
        } else if local >= dur {
            1.0
        } else {
            local / dur
        };
        let at = match named.iter().find(|n| n.key == "at") {
            None => crate::key::even_at(values.len()),
            Some(n) => {
                let v = self.expr(&n.value, sc)?;
                let items = v.items().ok_or_else(|| {
                    fail("E0320", "`at:` needs a list of fractions").fix("write `at:[0 .5 1]`")
                })?;
                items
                    .iter()
                    .map(|x| match x {
                        Value::Num(n) => Ok(*n),
                        _ => Err(fail("E0320", "`at:` values are numbers")),
                    })
                    .collect::<R<Vec<_>>>()?
            }
        };
        let segments = values.len().saturating_sub(1);
        let eases = match named.iter().find(|n| n.key == "ease") {
            None => vec![Ease::Smooth; segments],
            Some(n) => {
                let v = self.expr(&n.value, sc)?;
                let one = |name: &str| {
                    crate::key::ease_named(name).ok_or_else(|| {
                        fail("E0320", format!("unknown ease `{name}`"))
                            .fix("use linear, in, out, in_out or hold")
                    })
                };
                match v {
                    Value::Ident(s) => vec![one(&s)?; segments],
                    Value::List(xs) => {
                        if xs.len() != segments {
                            return Err(fail(
                                "E0320",
                                format!(
                                    "`ease:` has {} value(s) for {segments} segment(s)",
                                    xs.len()
                                ),
                            ));
                        }
                        xs.iter()
                            .map(|x| one(x.word().unwrap_or("")))
                            .collect::<R<Vec<_>>>()?
                    }
                    other => {
                        return Err(fail(
                            "E0320",
                            format!("`ease:` expects a name or a list, got {}", other.kind()),
                        ));
                    }
                }
            }
        };
        crate::key::sample_track(&values, &at, &eases, u).map_err(|m| fail("E0320", m))
    }

    pub fn lookup_ident(&self, name: &str, sc: &Scope) -> Option<Value> {
        if let Some((_, v)) = sc.env.iter().rev().find(|(k, _)| k == name) {
            return Some(v.clone());
        }
        if sc.module.parameters.borrow().contains(name) {
            return sc.module.global(name);
        }
        if name == "target" {
            return Some(
                self.job
                    .label
                    .clone()
                    .map(Value::Ident)
                    .unwrap_or(Value::None),
            );
        }
        if name == "t" {
            return self.anim_clock.as_ref().map(|c| Value::Num(c.local));
        }
        if name == "clip" {
            return Some(
                self.job
                    .clip
                    .clone()
                    .map(Value::Ident)
                    .unwrap_or(Value::None),
            );
        }
        if let Some((_, v)) = self.job.matrix.iter().find(|(k, _)| k == name) {
            return Some(match v {
                MatrixValue::Word(w) => Value::Ident(w.clone()),
                MatrixValue::Num(n) => Value::Num(*n),
            });
        }
        if name == "p"
            && let Some(v) = &sc.palette
            && !sc.module.binds_p.get()
        {
            return Some(v.clone());
        }
        if let Some(v) = sc.module.global(name) {
            return Some(v);
        }
        match name {
            "scale" => Some(Value::Num(self.job.sample_scale)),
            "true" => Some(Value::Bool(true)),
            "false" => Some(Value::Bool(false)),
            "none" => Some(Value::None),
            "artboard" => Some(Value::Ref(Rc::new(RefVal {
                label: "artboard".into(),
                record: Record {
                    path: "artboard".into(),
                    key: Key::root(),
                    copies: Vec::new(),
                    parent_world: Place::identity(),
                    node: None,
                    anchor: None,
                    at: (0.0, 0.0),
                    home: None,
                    decl: None,
                    inner: None,
                    rows: Rc::default(),
                    seam: 0.0,
                },
                source: None,
                artboard: Some(self.artboard),
                extent: Extent::Visual,
            }))),
            _ => None,
        }
    }

    pub fn expr(&mut self, e: &Expr, sc: &Scope) -> R<Value> {
        Ok(match e {
            Expr::Num(n) => Value::Num(*n),
            Expr::Percent(p) => Value::Percent(*p),
            Expr::Angle(d) => Value::Num(*d),
            Expr::Time(n) => Value::Num(*n),
            Expr::Size { w, h, depth: d } => {
                let (w, h) = (self.expr(w, sc)?, self.expr(h, sc)?);
                let d = d.as_ref().map(|d| self.expr(d, sc)).transpose()?;
                match (w.coord(), h.coord(), d.as_ref().map(Value::coord)) {
                    (Some(a), Some(b), None) => Value::Size(a, b),
                    (Some(a), Some(b), Some(Some(d))) => Value::size3(a, b, d)?,
                    _ => return Err(fail("E0320", "a size is two or three numbers or percents")),
                }
            }
            Expr::Hex(h) => Value::Color(Col::plain(
                Rgba::hex(h).ok_or_else(|| fail("E0320", format!("bad color #{h}")))?,
            )),
            Expr::Str(s) => Value::Str(s.clone()),
            Expr::Ident(name) => match self.lookup_ident(name, sc) {
                Some(Value::Unseen) => return Err(unseen(name)),
                Some(v) => {
                    if let Some(by) = sc.timed.get(name) {
                        self.timed = Some(by.clone());
                    } else if name == "t"
                        && let Some(by) = self.anim_clock.as_ref().and_then(|c| c.patch.clone())
                    {
                        self.timed = Some(by);
                    }
                    v
                }
                None if name == "canvas" => return Err(canvas_renamed()),
                None if name == "t" => return Err(outside_anim("`t`")),
                None if sc.module.lost.borrow().contains(name) => return Err(Fail::reported()),
                None if sc.module.skipped.borrow().contains(name) => {
                    return Err(fail(
                        "E0201",
                        format!(
                            "`{name}` is a top-level `let` of the entry file `{}`, and `use` does not run it",
                            sc.module.path
                        ),
                    )
                    .fix("move the `let` into the def, or into a library that both files `use`"));
                }
                None => Value::Ident(name.clone()),
            },
            Expr::Ref(segs) => self.reference(segs, sc)?,
            Expr::Member(base, seg) => {
                let b = self.expr(base, sc)?;
                self.member(&b, seg, sc)?
            }
            Expr::Index(base, key) => match &**base {
                Expr::Ref(segs) => {
                    let mut segs = segs.clone();
                    segs.push(self.index_segment(key, sc)?);
                    self.reference(&segs, sc)?
                }
                _ => {
                    let b = self.expr(base, sc)?;
                    let seg = self.index_segment(key, sc)?;
                    self.member(&b, &seg, sc)?
                }
            },
            Expr::Tuple(xs) => Value::tuple(xs.iter().map(|x| self.expr(x, sc)).collect::<R<_>>()?)?,
            Expr::List(xs) => Value::List(Rc::new(
                xs.iter().map(|x| self.expr(x, sc)).collect::<R<_>>()?,
            )),
            Expr::Each {
                index,
                var,
                iter,
                body,
            } => {
                let items = each_items(self.expr(iter, sc)?)?;
                let mut out = Vec::with_capacity(items.len());
                for (i, v) in items.into_iter().enumerate() {
                    let mut inner = sc.clone();
                    inner.bind_loop(index.as_deref(), var, i, v);
                    out.push(self.expr(body, &inner)?);
                }
                Value::List(Rc::new(out))
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
                    Value::Field(f) => Value::Field(Rc::new(ramose_ir::Field::Unary(
                        ramose_ir::FieldFn::Neg,
                        Box::new((*f).clone()),
                    ))),
                    other => arith(Arith::Mul, &other, &Value::Num(-1.0))
                        .map_err(|m| fail("E0320", m))?,
                }
            }
            Expr::Unary(UnOp::Not, x) => Value::Bool(!self.expr(x, sc)?.truthy()),
            Expr::Binary(op, l, r) => self.binary(*op, l, r, sc)?,
            Expr::Call { func, args, named } => self.call(func, args, named, sc)?,
            Expr::If {
                cond,
                then,
                otherwise,
            } => {
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
            return Ok(Value::Bool(
                self.expr(l, sc)?.truthy() && self.expr(r, sc)?.truthy(),
            ));
        }
        if op == BinOp::Or {
            return Ok(Value::Bool(
                self.expr(l, sc)?.truthy() || self.expr(r, sc)?.truthy(),
            ));
        }
        let (a, b) = (self.expr(l, sc)?, self.expr(r, sc)?);
        let ar = |o| arith(o, &a, &b).map_err(|m| fail("E0320", m));
        Ok(match op {
            BinOp::Add => ar(Arith::Add)?,
            BinOp::Sub => ar(Arith::Sub)?,
            BinOp::Mul | BinOp::Div => match (&a, b.num()) {
                (Value::Range(r), Some(k)) => {
                    let arith_op = if op == BinOp::Mul {
                        Arith::Mul
                    } else {
                        Arith::Div
                    };
                    Value::Range(r.scale(arith_op, k))
                }
                _ => ar(if op == BinOp::Mul {
                    Arith::Mul
                } else {
                    Arith::Div
                })?,
            },
            BinOp::Eq => Value::Bool(equal(&a, &b)),
            BinOp::Ne => Value::Bool(!equal(&a, &b)),
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                let above = matches!(op, BinOp::Gt | BinOp::Ge);
                let threshold = match (&a, &b) {
                    (Value::Field(f), t) => t.num().map(|t| (f, t, above)),
                    (t, Value::Field(f)) => t.num().map(|t| (f, t, !above)),
                    _ => None,
                };
                if let Some((f, t, above)) = threshold {
                    if above {
                        return Ok(Value::FieldMask(f.clone(), t));
                    }
                    return Ok(mask_below(f, t));
                }
                let (x, y) = match (a.num(), b.num()) {
                    (Some(x), Some(y)) => (x, y),
                    _ => {
                        return Err(fail(
                            "E0320",
                            format!("cannot compare {} and {}", a.kind(), b.kind()),
                        ));
                    }
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
                    (_, _, Some(p), Some(q)) => {
                        (p.0 - q.0).abs() <= tol && (p.1 - q.1).abs() <= tol
                    }
                    _ if let (Some(p), Some(q)) = (a.vec3(), b.vec3()) => {
                        (p.0 - q.0).abs() <= tol && (p.1 - q.1).abs() <= tol && (p.2 - q.2).abs() <= tol
                    }
                    _ => {
                        return Err(fail(
                            "E0320",
                            format!(
                                "`~=` compares numbers or vectors, got {} and {}",
                                a.kind(),
                                b.kind()
                            ),
                        ));
                    }
                };
                if !ok {
                    self.note(format!(
                        "{} ~= {} is off by more than tol {}",
                        a.show(),
                        b.show(),
                        num(tol)
                    ));
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
                _ => {
                    return Err(fail(
                        "E0320",
                        format!("`in` needs a range or list on the right, got {}", b.kind()),
                    ));
                }
            },
            BinOp::Map => {
                let field = match &a {
                    Value::Field(f) => f.clone(),
                    _ => {
                        return Err(fail("E0320", "`->` maps a field through colors")
                            .fix("write `fill:fbm(simplex(30)) -> p.stone`"));
                    }
                };
                let colors: Vec<Rgba> = match &b {
                    Value::Ramp(r) => r.stops.clone(),
                    other => other
                        .items()
                        .unwrap_or_default()
                        .iter()
                        .filter_map(|v| v.color().map(|c| c.rgba))
                        .collect(),
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

    fn index_segment(&mut self, key: &Expr, sc: &Scope) -> R<String> {
        let k = self.expr(key, sc)?;
        k.word()
            .map(str::to_string)
            .or(k.num().map(num))
            .ok_or_else(|| {
                fail(
                    "E0320",
                    format!("an index is a name or a number, got {}", k.kind()),
                )
                .fix("write a number or a name inside `{}`, such as `list.{i}`")
            })
    }

    fn member(&mut self, base: &Value, seg: &str, sc: &Scope) -> R<Value> {
        let missing = |what: &str, names: Vec<String>| {
            fail("E0350", format!("{what} has no member `{seg}`"))
                .note(format!("members: {}", names.join(", ")))
                .fix("use one of the listed members")
        };
        Ok(match base {
            Value::Palette(p) => p.get(seg).ok_or_else(|| {
                missing(
                    &format!("palette `{}`", p.name),
                    p.entries.iter().map(|e| e.0.clone()).collect(),
                )
            })?,
            Value::Module(m) => m.member(seg).ok_or_else(|| {
                let mut names: Vec<String> = m.exports().into_iter().map(|e| e.0).collect();
                names.extend(
                    m.palettes
                        .borrow()
                        .iter()
                        .flat_map(|p| p.entries.iter().map(|e| e.0.clone())),
                );
                missing(&format!("`{}`", m.path), names)
            })?,
            Value::Ramp(r) => match seg {
                "line" => Value::Color(Col::plain(r.line())),
                "base" => base.clone(),
                _ => {
                    let i: f64 = seg.parse().map_err(|_| {
                        missing("a ramp", vec!["0..n".into(), "line".into(), "base".into()])
                    })?;
                    Value::Color(Col {
                        rgba: r.at(i),
                        ramp: Some((r.clone(), i)),
                    })
                }
            },
            Value::Color(c) if seg == "line" => Value::Color(Col::plain(color::line(c.rgba))),
            Value::Vec2(x, y) | Value::Size(x, y) => {
                let c = match seg {
                    "x" | "w" | "0" => *x,
                    "y" | "h" | "1" => *y,
                    _ => {
                        return Err(missing(
                            "a vector",
                            vec!["x".into(), "y".into(), "0".into(), "1".into()],
                        ));
                    }
                };
                match c {
                    Coord::Num(v) => Value::Num(v),
                    Coord::Pct(p) => Value::Percent(p),
                }
            }
            Value::Vec3(x, y, z) | Value::Size3(x, y, z) => {
                let c = match seg {
                    "x" | "w" | "0" => *x,
                    "y" | "h" | "1" => *y,
                    "z" | "2" => Coord::Num(*z),
                    _ => {
                        return Err(missing(
                            "a three-component vector",
                            vec!["x".into(), "y".into(), "z".into(), "0".into(), "1".into(), "2".into()],
                        ));
                    }
                };
                match c {
                    Coord::Num(v) => Value::Num(v),
                    Coord::Pct(p) => Value::Percent(p),
                }
            }
            Value::Ref(r) if self.prepass && r.artboard.is_none() => {
                return Err(unseen(&format!("@{}.{seg}", r.label)));
            }
            Value::Ref(r) => self.measure(r, seg, sc)?,
            Value::Tuple(xs) => {
                let i: usize = seg
                    .parse()
                    .map_err(|_| missing("a list", vec!["0..n".into()]))?;
                xs.get(i)
                    .cloned()
                    .ok_or_else(|| fail("E0350", format!("index {i} is out of range")))?
            }
            Value::List(xs) => {
                let i: usize = seg
                    .parse()
                    .map_err(|_| missing("a list", vec!["0..n".into()]))?;
                xs.get(i)
                    .cloned()
                    .ok_or_else(|| fail("E0350", format!("index {i} is out of range")))?
            }
            Value::Color(_) => {
                return Err(fail("E0350", format!("a plain color has no step `.{seg}`; only ramps are indexed"))
                    .fix("declare the palette entry as `ramp name <color> 5` to index it, or drop the index"));
            }
            other => {
                return Err(fail(
                    "E0350",
                    format!("{} has no member `.{seg}`", other.kind()),
                ));
            }
        })
    }

    /// Checks measure in artboard coordinates. Everything else reads a ref in the current local frame.
    fn view(&self, sc: &Scope) -> Place {
        if self.check.is_some() { Place::identity() } else { sc.world.clone() }
    }

    /// A position read from outside the first copy of a `grid` without `cell:` would not move
    /// with that copy.
    fn outside_copy(&self, r: &RefVal, sc: &Scope) -> R<()> {
        let Some(p) = sc.provisional.as_ref().filter(|_| self.check.is_none()) else {
            return Ok(());
        };
        let fresh = placed_tree(&records_after(&p.names, &p.before));
        if fresh.iter().any(|rec| Some(Rc::as_ptr(rec) as usize) == r.source) {
            return Ok(());
        }
        Err(fail(
            "E0352",
            format!(
                "a `grid` without `cell:` sizes its cells from the first copy, so that copy cannot read `@{}` from outside the grid",
                r.label
            ),
        )
        .fix("give the grid a `cell:` size, such as `cell:20x20`"))
    }

    /// `edge @body from:.2 to:.6`: that stretch of the shape's contour as an open path in this
    /// node's parent frame.
    fn edge(&mut self, a: &Args, named: &[Named], span: Span, sc: &Scope) -> R<Path> {
        let (file, node) = (sc.file().to_string(), sc.node_name());
        let mut fractions = [0.0, 1.0];
        for (slot, k) in fractions.iter_mut().zip(["from", "to"]) {
            let Some(v) = a.get(k) else { continue };
            match v.num().filter(|x| x.is_finite() && (0.0..=1.0).contains(x)) {
                Some(x) => *slot = x,
                None => {
                    let got = v.num().map_or_else(|| v.kind().to_string(), num);
                    let f = fail("E0320", format!("`{k}:` is a fraction from 0 to 1, got {got}"))
                        .fix(format!("write `{k}:.5`"));
                    let span = named.iter().rfind(|n| n.key == k).map_or(span, |n| n.span);
                    self.report(f, &file, span, &node);
                    return Err(Fail::reported());
                }
            }
        }
        let (r, node, shape) = one_shape(a.pos.first(), "edge", "edge @body from:.2 to:.6")?;
        self.outside_copy(r, sc)?;
        let [from, to] = fractions;
        let contour = geom::edge(&shape.geometry.to_sk(), r.record.seam, from, to);
        let placed = r.record.parent_world.inner_node(node).map_path(&contour, crate::scope::FLATTEN_TOL);
        Ok(self.view(sc).unmap_path(&placed))
    }

    /// `path(@body)`: the shape's whole contour as a path value, with its positions measured in
    /// the shape's own frame as `edge` measures them and its samples placed in the current local
    /// frame as `edge` places its stretch.
    pub(crate) fn path_of(&mut self, a: &[Value], sc: &Scope) -> R<Value> {
        let (r, node, shape) = one_shape(a.first(), "path", "path(@body)")?;
        self.outside_copy(r, sc)?;
        let own = shape.geometry.to_sk();
        let place = r.record.parent_world.inner_node(node).outer(crate::place::inverse(&self.view(sc).flat()));
        let points = geom::vertices(&own)
            .iter()
            .flatten()
            .filter_map(|&(x, y)| {
                let p = place.map_point(Point::new(x as f32, y as f32))?;
                Some((f64::from(p.x), f64::from(p.y)))
            })
            .collect();
        Ok(Value::Path(Rc::new(PathVal { own, seam: r.record.seam, place, points })))
    }

    /// `at:@body.neck`: the anchor's position in the current local frame, the same point
    /// `at:@body.neck.x,@body.neck.y` reads.
    fn anchor_point(&mut self, r: &RefVal, sc: &Scope) -> R<(f64, f64)> {
        if r.record.anchor.is_none() || r.artboard.is_some() {
            return Err(
                fail("E0320", format!("`at:@{}` is a node, not a point", r.label)).fix(format!(
                    "name an anchor such as `at:@{0}.neck`, or write `at:@{0}.cx,@{0}.cy`",
                    r.label
                )),
            );
        }
        self.outside_copy(r, sc)?;
        let p = r.record.anchor_canvas().ok_or_else(|| hidden_anchor(&r.label))?;
        let p = self.view(sc).unmap_point(p);
        let (x, y) = (f64::from(p.x), f64::from(p.y));
        self.note(format!("{} = {},{}", r.label, num(x), num(y)));
        Ok((x, y))
    }

    pub fn measure(&mut self, r: &RefVal, m: &str, sc: &Scope) -> R<Value> {
        if moves(m) {
            self.outside_copy(r, sc)?;
        }
        let b = r
            .bounds_in(&self.view(sc))
            .ok_or_else(|| fail("E0351", format!("@{} has no geometry", r.label)))?;
        let v = measure_rect(b, m).ok_or_else(|| {
            fail("E0350", format!("unknown measurement `.{m}`"))
                .note(format!("measurements: {}", MEASURES.join(" ")))
        })?;
        self.note(format!("{}.{m} = {}", r.label, num(v)));
        Ok(Value::Num(v))
    }

    /// `@x.outer`: the box of everything `r` draws, in artboard coordinates. The artboard and an
    /// anchor draw only themselves.
    fn outer(&self, r: &RefVal, label: &str) -> R<RefVal> {
        let no_ink = || fail("E0351", format!("@{label} draws nothing"));
        let rect = match &r.record.node {
            None => r.bounds().ok_or_else(no_ink)?,
            Some(node) => {
                let placed = self.placed.borrow();
                let records: Vec<(&str, std::cell::Ref<'_, Record>)> = placed
                    .iter()
                    .map(|(p, rec)| (p.as_str(), rec.borrow()))
                    .collect();
                let nodes: HashMap<&str, (&Node, Matrix)> = records
                    .iter()
                    .filter_map(|(p, rec)| Some((*p, (rec.node.as_ref()?, rec.parent_world.flat()))))
                    .collect();
                let find = |p: &str| nodes.get(p).copied();
                let local = node
                    .outer_bounds(&r.record.parent_world.flat(), &find)
                    .ok_or_else(no_ink)?;
                r.record.parent_world.map_rect(local)
            }
        };
        Ok(RefVal {
            label: format!("{label}.outer"),
            extent: Extent::Outer(rect),
            ..r.clone()
        })
    }

    fn reference(&mut self, segs: &[String], sc: &Scope) -> R<Value> {
        let first = &segs[0];
        if first == "self" {
            let inst = sc
                .inst
                .clone()
                .ok_or_else(|| fail("E0350", "`@self` is only valid inside a def"))?;
            let rec = inst
                .record
                .borrow()
                .clone()
                .ok_or_else(|| fail("E0350", "`@self` is only available in checks"))?;
            let cur = RefVal {
                label: "self".into(),
                record: rec.borrow().clone(),
                source: Some(Rc::as_ptr(&rec) as usize),
                artboard: None,
                extent: Extent::Visual,
            };
            return self.walk_ref(vec![cur], segs, 1, "self".into(), sc, 0);
        }
        if first == "canvas" {
            return Err(canvas_renamed());
        }
        if first == "artboard" {
            let Some(Value::Ref(r)) = self.lookup_ident("artboard", sc) else {
                unreachable!("artboard is a reference");
            };
            return self.walk_ref(vec![(*r).clone()], segs, 1, "artboard".into(), sc, 0);
        }
        let resolved = identity::resolve(&sc.lookup, first, &sc.scope_copies, &sc.scope_loops);
        if self.prepass && matches!(resolved, Resolved::Undrawn(..)) {
            return Err(unseen(&format!("@{first}")));
        }
        let (cur, depth) = self.refs_of(resolved, segs)?;
        if self.prepass && segs[1..].iter().any(|s| !is_copy_index(s)) {
            return Err(unseen(&format!("@{}", segs.join("."))));
        }
        self.walk_ref(cur, segs, 1, first.clone(), sc, depth)
    }

    fn refs_of(&self, resolved: Resolved, segs: &[String]) -> R<(Vec<RefVal>, usize)> {
        let label = &segs[0];
        match resolved {
            Resolved::One(e, depth) => Ok((vec![ref_from(&e, label)], depth)),
            Resolved::Many(es, depth) => {
                Ok((es.iter().map(|e| ref_from(e, label)).collect(), depth))
            }
            Resolved::Undrawn(region, id) => {
                let decl = region.decl(id);
                let hop = segs[1..].iter().position(|s| !is_copy_index(s)).map(|i| i + 1);
                if let Some(i) = hop
                    && decl.kind == identity::NameKind::Group
                    && region.has_named_below(id, &segs[i])
                {
                    return Err(group_hop(segs, 0, i));
                }
                if self.broken.contains(&decl.site) {
                    return Err(Fail::reported());
                }
                match self.blocked.iter().find(|b| b.holds.contains(&decl.site)) {
                    Some(b) => Err(cycle(label, b)),
                    None => Err(fail(
                        "E0350",
                        format!(
                            "`{label}` is declared at {}:{} and not drawn",
                            decl.site.file, decl.site.span.line
                        ),
                    )
                    .fix("draw it, or remove the reference")),
                }
            }
            Resolved::Missing(names) => Err(missing_part(label, &names)),
            Resolved::Nested(found) => Err(nested_ref(segs, 0, 0, &found)),
            Resolved::Ambiguous(notes) => {
                let mut f = fail(
                    "E0343",
                    format!("`{label}` is a name in more than one scope"),
                );
                for n in notes {
                    f = f.note(n);
                }
                Err(f.fix("rename one of them"))
            }
        }
    }

    fn walk_ref(
        &mut self,
        mut cur: Vec<RefVal>,
        segs: &[String],
        mut i: usize,
        mut label: String,
        sc: &Scope,
        mut index_depth: usize,
    ) -> R<Value> {
        let mut named_at = 0;
        // Where the path of the region `cur` sits in starts, so a fix can spell a full path there.
        let mut region_path_start = 0;
        while i < segs.len() {
            let seg = segs[i].as_str();
            let last = i + 1 == segs.len();
            if seg == "*" {
                if !last {
                    return Err(fail("E0343", "`*` is only a final hop").fix("move `*` to the end"));
                }
                if cur.is_empty() {
                    return Err(missing_part(&label, &[]));
                }
                let kids: Vec<RefVal> = cur.iter().flat_map(|r| r.children()).collect();
                return Ok(Value::List(Rc::new(
                    kids.into_iter().map(|c| Value::Ref(Rc::new(c))).collect(),
                )));
            }
            if is_copy_index(seg) {
                let Ok(ix) = seg.parse::<u32>() else {
                    return Err(fail(
                        "E0350",
                        format!("`@{label}.{seg}` is not a copy index"),
                    )
                    .fix("write a whole number that fits"));
                };
                cur.retain(|r| r.record.copies.get(index_depth) == Some(&ix));
                index_depth += 1;
                label = format!("{label}.{seg}");
                i += 1;
                continue;
            }
            if seg == "geom" && i + 1 < segs.len() {
                let one = self.one_ref(&cur, &label, sc)?;
                let m = segs[i + 1].as_str();
                if moves(m) {
                    self.outside_copy(&one, sc)?;
                }
                let b = one
                    .geom_bounds_in(&self.view(sc))
                    .ok_or_else(|| fail("E0351", format!("@{label} has no geometry")))?;
                let v = measure_rect(b, m)
                    .ok_or_else(|| fail("E0350", format!("unknown measurement `.geom.{m}`")))?;
                self.note(format!("{label}.geom.{m} = {}", num(v)));
                return Ok(Value::Num(v));
            }
            let one = self.one_ref(&cur, &label, sc)?;
            let mut through_row = false;
            let (mut matched, mut names): (Vec<RefVal>, Vec<String>) = match &one.record.inner {
                Some(inner) => {
                    let region = inner.borrow().region();
                    let parts = one.parts(seg);
                    if parts.is_empty() {
                        if let Some(&id) = region.lookup(seg).first() {
                            return Err(undrawn_part(&label, seg, &region.decl(id).site));
                        }
                        let nested = region.nested(None, seg);
                        if !nested.is_empty() {
                            let found: Vec<(Rc<identity::Region>, DeclId)> =
                                nested.into_iter().map(|id| (region.clone(), id)).collect();
                            return Err(nested_ref(segs, i, i, &found));
                        }
                    }
                    (parts, region.names().iter().map(|n| n.to_string()).collect())
                }
                None => match one.home_decl() {
                    Some((region, decl)) if region.decl(decl).kind == identity::NameKind::Shape => {
                        let parts = one.parts_under(seg);
                        if parts.is_empty() {
                            if let Some(&id) = region.lookup_in(Some(decl), seg).first() {
                                return Err(undrawn_part(&label, seg, &region.decl(id).site));
                            }
                            let nested = region.nested(Some(decl), seg);
                            if !nested.is_empty() {
                                let found: Vec<(Rc<identity::Region>, DeclId)> =
                                    nested.into_iter().map(|id| (region.clone(), id)).collect();
                                return Err(nested_ref(segs, region_path_start, i, &found));
                            }
                        }
                        let names = region
                            .names_in(Some(decl))
                            .iter()
                            .map(|n| n.to_string())
                            .collect();
                        (parts, names)
                    }
                    Some((region, decl)) => {
                        if region.has_named_below(decl, seg) {
                            return Err(group_hop(segs, named_at, i));
                        }
                        (Vec::new(), Vec::new())
                    }
                    None => (Vec::new(), Vec::new()),
                },
            };
            if one.record.inner.is_none() {
                if let Some(row) = one.record.rows.iter().find(|r| r.name.as_str() == seg) {
                    matched.extend(row.stamps.iter().map(|r| RefVal {
                        label: format!("{label}.{seg}"),
                        record: r.borrow().clone(),
                        source: Some(Rc::as_ptr(r) as usize),
                        artboard: None,
                        extent: Extent::Visual,
                    }));
                    through_row = true;
                }
                names.extend(one.record.rows.iter().map(|r| r.name.to_string()));
            }
            if matched.is_empty() {
                if last && MEASURES.contains(&seg) {
                    return self.measure(&one, seg, sc);
                }
                if seg == "outer" {
                    let outer = self.outer(&one, &label)?;
                    return match &segs[i + 1..] {
                        [] => Ok(Value::Ref(Rc::new(outer))),
                        [m] if MEASURES.contains(&m.as_str()) => self.measure(&outer, m, sc),
                        [m, ..] => Err(fail("E0350", format!("unknown measurement `.outer.{m}`"))
                            .note(format!("measurements: {}", MEASURES.join(" ")))),
                    };
                }
                return Err(fail("E0350", format!("@{label} has no part `{seg}`"))
                    .note(format!(
                        "parts: {}",
                        if names.is_empty() {
                            "(none)".into()
                        } else {
                            names.join(", ")
                        }
                    ))
                    .fix("use one of the listed parts"));
            }
            cur = matched;
            named_at = i;
            label = format!("{label}.{seg}");
            if one.record.inner.is_some() || through_row {
                index_depth = 0;
                region_path_start = i + 1;
            } else {
                index_depth = one.record.copies.len();
            }
            i += 1;
        }
        match cur.len() {
            0 => Err(missing_part(&label, &[])),
            1 => Ok(Value::Ref(Rc::new(cur.pop().unwrap()))),
            _ if self.accept_family => Ok(Value::List(Rc::new(
                cur.into_iter().map(|c| Value::Ref(Rc::new(c))).collect(),
            ))),
            _ => Err(family_error(&label, &cur, &sc.owner)),
        }
    }

    fn one_ref(&self, cur: &[RefVal], label: &str, sc: &Scope) -> R<RefVal> {
        match cur {
            [one] => Ok(one.clone()),
            [] => Err(missing_part(label, &[])),
            many => Err(family_error(label, many, &sc.owner)),
        }
    }
}

struct FillHop {
    name: String,
    span: Span,
    ids: Rc<ModuleIds>,
    body: Vec<Scene>,
}

struct PatchScope {
    ids: Rc<ModuleIds>,
    region: Rc<identity::Region>,
    scenes: Vec<Scene>,
    fills: Vec<FillHop>,
}

fn fills_in(body: &DrawBody, ids: &Rc<ModuleIds>) -> Vec<FillHop> {
    body.items()
        .iter()
        .filter_map(|item| {
            let DrawItem::SlotFill(fill) = item else {
                return None;
            };
            Some(FillHop {
                name: fill.name.clone(),
                span: fill.span,
                ids: ids.clone(),
                body: fill.body.clone(),
            })
        })
        .collect()
}

fn find_draw(scenes: &[Scene], span: Span) -> Option<&Draw> {
    for scene in scenes {
        if scene.span == span {
            return match &scene.kind {
                SceneKind::Draw(draw) => Some(draw),
                _ => None,
            };
        }
        match &scene.kind {
            SceneKind::If { then, otherwise, .. } => {
                if let Some(draw) = find_draw(then, span).or_else(|| find_draw(otherwise, span)) {
                    return Some(draw);
                }
            }
            SceneKind::Each { body, .. } | SceneKind::Slot { fallback: body, .. } => {
                if let Some(draw) = find_draw(body, span) {
                    return Some(draw);
                }
            }
            SceneKind::Draw(draw) => {
                for item in draw.body.items() {
                    let nested = match item {
                        DrawItem::Scene(child) => std::slice::from_ref(child),
                        DrawItem::SlotFill(fill) => fill.body.as_slice(),
                        DrawItem::Mask {
                            mask: ast::Mask::Block(body),
                            ..
                        } => body.as_slice(),
                        DrawItem::Fx(f) => f.mask_block(),
                        _ => continue,
                    };
                    if let Some(draw) = find_draw(nested, span) {
                        return Some(draw);
                    }
                }
            }
            SceneKind::Fx(f) => {
                if let Some(draw) = find_draw(f.mask_block(), span) {
                    return Some(draw);
                }
            }
            SceneKind::Let { .. } | SceneKind::Check { .. } | SceneKind::Play { .. } => {}
        }
    }
    None
}

fn slot_fallback(scenes: &[Scene], span: Span) -> Vec<Scene> {
    scenes
        .iter()
        .find_map(|scene| slot_fallback_one(scene, span))
        .unwrap_or_default()
}

fn slot_fallback_one(scene: &Scene, span: Span) -> Option<Vec<Scene>> {
    if scene.span == span {
        return match &scene.kind {
            SceneKind::Slot { fallback, .. } => Some(fallback.clone()),
            _ => Some(Vec::new()),
        };
    }
    match &scene.kind {
        SceneKind::If { then, otherwise, .. } => slot_fallback_in(then, span)
            .or_else(|| slot_fallback_in(otherwise, span)),
        SceneKind::Each { body, .. } | SceneKind::Slot { fallback: body, .. } => {
            slot_fallback_in(body, span)
        }
        SceneKind::Draw(draw) => {
            for item in draw.body.items() {
                let nested = match item {
                    DrawItem::Scene(child) => std::slice::from_ref(child),
                    DrawItem::SlotFill(fill) => fill.body.as_slice(),
                    DrawItem::Mask {
                        mask: ast::Mask::Block(body),
                        ..
                    } => body.as_slice(),
                    DrawItem::Fx(f) => f.mask_block(),
                    _ => continue,
                };
                if let Some(found) = slot_fallback_in(nested, span) {
                    return Some(found);
                }
            }
            None
        }
        SceneKind::Fx(f) => slot_fallback_in(f.mask_block(), span),
        SceneKind::Let { .. } | SceneKind::Check { .. } | SceneKind::Play { .. } => None,
    }
}

fn slot_fallback_in(scenes: &[Scene], span: Span) -> Option<Vec<Scene>> {
    scenes.iter().find_map(|scene| slot_fallback_one(scene, span))
}

fn component_names(module: &Module) -> BTreeSet<String> {
    module
        .globals
        .borrow()
        .iter()
        .filter(|(_, v)| matches!(v, Value::Component(_) | Value::Alias(_)))
        .map(|(name, _)| name.clone())
        .collect()
}

/// The fix drops the group hop the way a patch's does, so `@row.1.gem.0` becomes `@gem.1.0`.
fn group_hop(segs: &[String], at: usize, i: usize) -> Fail {
    let written = segs.join(".");
    let fail = fail(
        "E0350",
        format!("`@{written}` hops through `{}`, which is a group", segs[at]),
    )
    .note(identity::GROUP_NOTE);
    let Some(addr) = identity::hops_from(&segs[at + 1..])
        .and_then(|hops| identity::skip_hop(&hops[..i - at - 1], &hops[i - at - 1..]))
    else {
        return fail.fix("name the part directly");
    };
    let scope: Vec<&str> = segs[..at].iter().map(String::as_str).collect();
    let fix = if scope.is_empty() {
        format!("write @{addr}")
    } else {
        format!("write @{}.{addr}", scope.join("."))
    };
    fail.fix(fix)
}

/// `segs[at]` is a name that sits deeper than the node before it, under one or more shapes.
/// The fix spells each full path, with the copy indices the author wrote on the shapes they
/// belong to.
fn nested_ref(
    segs: &[String],
    region_path_start: usize,
    at: usize,
    found: &[(Rc<identity::Region>, DeclId)],
) -> Fail {
    let written = segs.join(".");
    let seg = &segs[at];
    let mut indices: Vec<u32> = segs[region_path_start..at].iter().filter_map(|s| s.parse().ok()).collect();
    let mut rest = at + 1;
    while rest < segs.len() && is_copy_index(&segs[rest]) {
        indices.extend(segs[rest].parse::<u32>());
        rest += 1;
    }
    let prefix: String = segs[..region_path_start].iter().map(|s| format!("{s}.")).collect();
    let tail: String = segs[rest..].iter().map(|s| format!(".{s}")).collect();
    let mut paths: Vec<String> = Vec::new();
    for (region, id) in found {
        let path = format!("@{prefix}{}{tail}", region.spell(*id, &indices));
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    if let [path] = paths.as_slice() {
        let (region, id) = &found[0];
        let under = region.spell(region.decl(*id).scope.unwrap(), &[]);
        return fail(
            "E0350",
            format!("`@{written}` names `{seg}`, which is nested under `{prefix}{under}`"),
        )
        .fix(format!("write {path}"));
    }
    let mut f = fail(
        "E0350",
        format!("`@{written}` names `{seg}`, which sits under more than one shape"),
    );
    for p in &paths {
        f = f.note(format!("`{p}`"));
    }
    f.fix("write one of the listed paths")
}

fn cycle(label: &str, b: &order::Blocker) -> Fail {
    let at = format!("{}:{}", b.site.file, b.site.span.line);
    let (who, it) = match &b.name {
        Some(name) => (format!("`{name}`"), format!("`{name}`")),
        None => (format!("the statement at {at}"), "that statement".to_string()),
    };
    let f = fail(
        "E0350",
        format!("`@{label}` is read before {who} is drawn, and {it} reads back to this line"),
    )
    .fix(format!(
        "draw {it} with no reference back to this node, and paint this node behind it with `order:behind`"
    ));
    match b.name {
        Some(_) => f.note(format!("{who} is drawn at {at}")),
        None => f,
    }
}

fn undrawn_part(label: &str, seg: &str, site: &Site) -> Fail {
    fail(
        "E0350",
        format!(
            "`@{label}.{seg}` is declared at {}:{} and not drawn",
            site.file, site.span.line
        ),
    )
    .fix("draw it, or remove the reference")
}

fn missing_part(label: &str, names: &[String]) -> Fail {
    fail("E0350", format!("no part named `{label}` here"))
        .note(format!(
            "named so far: {}",
            if names.is_empty() {
                "(none)".into()
            } else {
                names.join(", ")
            }
        ))
        .fix("name the node with `name:` before this line, or fix the spelling")
}

fn family_error(label: &str, entries: &[RefVal], owner: &Key) -> Fail {
    let mut notes = Vec::new();
    for r in entries {
        notes.push(
            r.record
                .key
                .address_from(owner)
                .unwrap_or_else(|| r.record.path.clone()),
        );
    }
    let first = notes
        .first()
        .cloned()
        .unwrap_or_else(|| format!(".{label}"));
    let mut f = fail("E0343", format!("`@{label}` is a family of copies"));
    for n in notes {
        f = f.note(n);
    }
    f.fix(format!("write {first}"))
}

fn is_paint_key(k: &str) -> bool {
    PAINT_KEYS.contains(&k)
}

/// An anchor past the visible half of a cylinder, or behind a corners horizon, has no artboard point.
fn hidden_anchor(label: &str) -> Fail {
    fail("E0320", format!("`@{label}` is hidden by the warp of the node that holds it"))
        .fix("move the anchor onto the part of the content the warp shows")
}

pub(crate) fn unknown_key(verb: &str, keys: &[&str], key: &str) -> Fail {
    let f = fail("E0321", format!("`{verb}` has no key `{key}`"));
    if let Some(new) = renamed(key).filter(|new| keys.contains(new)) {
        return f.fix(format!("`{key}:` is now `{new}:`"));
    }
    let near = keys
        .iter()
        .map(|k| (osa_distance(key, k), k))
        .min_by_key(|(d, _)| *d)
        .filter(|(d, _)| *d <= (key.len() + 1) / 3);
    match near {
        Some((_, k)) => f.fix(format!("did you mean `{k}:`?")),
        None => f
            .note(format!("keys: {}", keys.join(", ")))
            .fix("use one of the listed keys"),
    }
}

pub(crate) fn osa_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut d = vec![vec![0; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    d[0] = (0..=b.len()).collect();
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}

fn mask_below(f: &ramose_ir::Field, t: f64) -> Value {
    let complement = ramose_ir::Field::Binary(
        ramose_ir::FieldOp::Sub,
        Box::new(ramose_ir::Field::Const(1.0)),
        Box::new(f.clone()),
    );
    Value::FieldMask(Rc::new(complement), 1.0 - t)
}

pub(crate) fn numbers(v: &Value) -> Option<Vec<f64>> {
    match v {
        Value::Num(n) => Some(vec![*n]),
        _ => match (v.vec2(), v.vec3()) {
            (Some((a, b)), _) => Some(vec![a, b]),
            (_, Some((a, b, c))) => Some(vec![a, b, c]),
            _ => v.items()?.iter().map(Value::num).collect(),
        },
    }
}

fn one_shape<'a>(v: Option<&'a Value>, verb: &str, example: &str) -> R<(&'a RefVal, &'a Node, &'a Shape)> {
    let Some(Value::Ref(r)) = v else {
        return Err(fail("E0320", format!("`{verb}` needs a node reference")).fix(format!("write `{example}`")));
    };
    let node = r.record.node.as_ref().filter(|_| r.artboard.is_none() && r.extent == Extent::Visual);
    match node {
        Some(node @ Node { kind: NodeKind::Shape(shape), .. }) => Ok((r, node, shape)),
        _ => Err(fail("E0320", format!("`{verb}` reads one shape's contour"))
            .fix(format!("write `{example}`, naming one shape"))),
    }
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
