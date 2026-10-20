//! What names resolve to: modules and their exports, palettes, components, rules, styles,
//! finishes, and the records of named nodes that `@ref`s and checks measure.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::{Rc, Weak};

use ramose_ir::{Matrix, Node, NodeKind, Path, Point, Rect};
use ramose_syntax::ast::{DefItem, Expr, FxLine, Named, Patch, Scene, Span};

use crate::geom;
use crate::identity::{DeclId, Entry, Key, Name, NameKind, Names, Region, RegionKind};
use crate::place::{Place, inverse};
use crate::value::Value;

pub struct Module {
    /// Project-relative source file.
    pub path: String,
    pub globals: RefCell<BTreeMap<String, Value>>,
    pub styles: RefCell<BTreeMap<String, Rc<StyleDef>>>,
    pub finishes: RefCell<BTreeMap<String, Rc<FinishDef>>>,
    pub anims: RefCell<BTreeMap<String, Rc<AnimDef>>>,
    pub palettes: RefCell<Vec<Rc<Palette>>>,
    /// Names this module declares itself, in order. Exported unless they start with `_`.
    pub own: RefCell<Vec<String>>,
    /// Names a failed `use` would have bound. The `use` already reported the failure.
    pub lost: RefCell<BTreeSet<String>>,
    /// A failed `use` without an item list could have bound any name.
    pub lost_all: Cell<bool>,
    /// Top-level lets of an entry file that another file loaded with `use`. That import does not run them.
    pub skipped: RefCell<BTreeSet<String>>,
    pub binds_p: Cell<bool>,
    /// From the bundled standard library, not the author's files. Lints leave it alone.
    pub bundled: bool,
}

impl Module {
    pub fn new(path: &str) -> Module {
        Module {
            path: path.to_string(),
            globals: RefCell::new(BTreeMap::new()),
            styles: RefCell::new(BTreeMap::new()),
            finishes: RefCell::new(BTreeMap::new()),
            anims: RefCell::new(BTreeMap::new()),
            palettes: RefCell::new(Vec::new()),
            own: RefCell::new(Vec::new()),
            lost: RefCell::new(BTreeSet::new()),
            lost_all: Cell::new(false),
            skipped: RefCell::new(BTreeSet::new()),
            binds_p: Cell::new(false),
            bundled: false,
        }
    }

    pub fn global(&self, name: &str) -> Option<Value> {
        self.globals.borrow().get(name).cloned()
    }

    pub fn bind(&self, name: &str, v: Value) {
        self.binds_p.set(self.binds_p.get() || name == "p");
        self.globals.borrow_mut().insert(name.to_string(), v);
    }

    pub fn define(&self, name: &str, v: Value) {
        self.bind(name, v);
        let mut own = self.own.borrow_mut();
        if !own.iter().any(|n| n == name) {
            own.push(name.to_string());
        }
    }

    /// A use site of a lost name stays quiet instead of reporting the name as unknown.
    pub fn lost(&self, name: &str) -> bool {
        self.lost_all.get() || self.lost.borrow().contains(name)
    }

    pub fn exports(&self) -> Vec<(String, Value)> {
        let g = self.globals.borrow();
        self.own
            .borrow()
            .iter()
            .filter(|n| !n.starts_with('_'))
            .filter_map(|n| g.get(n).map(|v| (n.clone(), v.clone())))
            .collect()
    }

    /// `alias.name`: an export, or else an entry of one of the module's palettes.
    pub fn member(&self, name: &str) -> Option<Value> {
        if !name.starts_with('_')
            && self.own.borrow().iter().any(|n| n == name)
            && let Some(v) = self.global(name)
        {
            return Some(v);
        }
        self.palettes.borrow().iter().find_map(|p| p.get(name))
    }
}

pub struct Palette {
    pub name: String,
    pub entries: Vec<(String, Value)>,
}

impl Palette {
    pub fn get(&self, key: &str) -> Option<Value> {
        self.entries
            .iter()
            .rev()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    }
}

pub struct StyleDef {
    pub module: String,
    pub named: Vec<Named>,
}

pub struct AnimDef {
    pub name: String,
    pub dur: Expr,
    pub loop_: bool,
    pub body: Vec<Patch>,
    pub module: String,
    pub span: Span,
}

/// A named, ordered list of built-in `fx` lines. It holds no keys, so nothing about it reaches
/// `Scope.style`.
pub struct FinishDef {
    pub name: String,
    /// Named only, typed by their defaults, like `def` params.
    pub params: Vec<Named>,
    pub lines: Vec<FxLine>,
    /// The module whose names the lines and the param defaults resolve in.
    pub module: String,
    pub span: Span,
}

pub struct Component {
    pub name: String,
    pub params: Vec<Named>,
    /// Part names a host must have when this def is slot content (`needs:@torso`).
    pub needs: Vec<String>,
    pub body: Vec<DefItem>,
    pub module: String,
    pub span: Span,
}

/// `def arm_l = hero_arm tone:-1`: `comp` with `fixed` arguments. An instance of the alias is an
/// instance of `comp`, so it has `comp`'s anchors, parts, slots, and seeds.
pub struct AliasDef {
    pub name: String,
    pub comp: Rc<Component>,
    /// Every argument fixed along the alias chain, the innermost alias's first.
    pub fixed: Vec<Fixed>,
    pub module: String,
    pub span: Span,
}

pub struct Fixed {
    pub key: String,
    pub value: Expr,
    /// The module whose names `value` resolves in: the one that declared the alias fixing it.
    pub module: String,
}

pub struct RuleDef {
    pub name: String,
    pub params: Vec<Named>,
    pub body: Vec<Scene>,
    pub module: String,
    pub span: Span,
}

pub struct FnDef {
    pub name: String,
    pub params: Vec<String>,
    pub body: Expr,
    pub module: String,
    pub span: Span,
}

/// A node (or anchor) as created so far, with the place of its parent frame on the canvas.
#[derive(Clone)]
pub struct Record {
    pub path: String,
    pub key: Key,
    /// The loop indices of the key the scope registered it under.
    pub copies: Vec<u32>,
    pub parent_world: Place,
    pub node: Option<Node>,
    /// An anchor's position in the parent frame. Its canvas point is read through
    /// `parent_world`, so a warp filled later or a grid shift moves it.
    pub anchor: Option<Point>,
    /// The node's `at:` in its parent's coordinates (for suggested fixes).
    pub at: (f64, f64),
    /// The scope this record is registered in. That scope owns the record, so this is weak.
    pub home: Option<Weak<RefCell<Names>>>,
    pub decl: Option<DeclId>,
    /// Set when this node opens a scope (an instance or a slot).
    pub inner: Option<Rc<RefCell<Names>>>,
    /// The stamp sets this node placed, in `stamp:` order. A hop after this record that names
    /// no child looks here: `@vine.leaves.6`, then `.blade` continues in that stamp's `inner`.
    pub rows: Rc<Vec<RowRecord>>,
    /// Where contour position 0 lies on this shape, as a fraction of its total contour length
    /// from the geometry's own start, before `jitter:`.
    pub seam: f64,
}

impl Record {
    /// `None` when the record is a node, or its anchor is behind a warp.
    pub fn anchor_canvas(&self) -> Option<Point> {
        self.parent_world.map_point(self.anchor?)
    }
}

/// One stamp set a node placed: its name and one record per stamp, index by position.
pub struct RowRecord {
    pub name: Name,
    pub stamps: Vec<Rc<RefCell<Record>>>,
}

/// The resolved target of an `@ref`. Everything is measured in canvas coordinates.
#[derive(Clone)]
pub struct RefVal {
    /// The reference as written, without `@` (`eye_r`, `hero.head`).
    pub label: String,
    pub record: Record,
    /// Address of the record this was read from. `None` for the canvas.
    pub source: Option<usize>,
    /// `canvas`: the canvas rect itself.
    pub canvas: Option<Rect>,
    pub extent: Extent,
}

/// Which box a node reference measures.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Extent {
    /// Geometry plus half the stroke width (`@x`).
    Visual,
    /// Everything the node draws, effects included, in canvas coordinates (`@x.outer`). It is
    /// a box, so the ref's silhouette is that box too.
    Outer(Rect),
}

impl RefVal {
    pub fn bounds(&self) -> Option<Rect> {
        self.bounds_in(&Place::identity())
    }

    /// Bounds in the frame `view` places, read back through its affine steps.
    pub fn bounds_in(&self, view: &Place) -> Option<Rect> {
        let back = inverse(&view.flat());
        if let Extent::Outer(r) = self.extent {
            return Some(back.map_rect(r).0);
        }
        if let Some(c) = self.canvas {
            return Some(back.map_rect(c).0);
        }
        if self.record.anchor.is_some() {
            let p = back.map_point(self.record.anchor_canvas()?);
            return Some(Rect::from_xywh(p.x, p.y, 0.0, 0.0));
        }
        let local = self.record.node.as_ref()?.bounds()?;
        Some(self.record.parent_world.map_rect_then(local, &back))
    }

    pub fn geom_bounds_in(&self, view: &Place) -> Option<Rect> {
        if self.canvas.is_some() || self.record.anchor.is_some() || self.extent != Extent::Visual {
            return self.bounds_in(view);
        }
        let node = self.record.node.as_ref()?;
        geom_bounds(node, &self.record.parent_world, &inverse(&view.flat()))
    }

    /// Union of the node's shape geometries in canvas coordinates.
    pub fn silhouette(&self) -> Path {
        if let Extent::Outer(r) = self.extent {
            return Path::rect(r, None);
        }
        if let Some(c) = self.canvas {
            return Path::rect(c, None);
        }
        match &self.record.node {
            Some(n) => silhouette(n, &self.record.parent_world),
            None => Path::new(),
        }
    }

    fn home(&self) -> Option<Rc<RefCell<Names>>> {
        let home = self.record.home.as_ref()?;
        Some(home.upgrade().expect("a reference is walked while its scope is alive"))
    }

    /// Named direct tree children of this copy. An instance or a slot lists the scope it opens.
    pub fn children(&self) -> Vec<RefVal> {
        let (entries, prefix_len) = if let Some(inner) = &self.record.inner {
            (inner.borrow().children_of(None, &[]), 0)
        } else if let (Some(home), Some(decl)) = (self.home(), self.record.decl) {
            let prefix = &self.record.copies;
            (home.borrow().children_of(Some(decl), prefix), prefix.len())
        } else {
            return Vec::new();
        };
        entries.iter().map(|e| self.child(e, prefix_len)).collect()
    }

    /// The drawn parts named `name` at the root of the scope this instance or slot opens, under
    /// any groups and layers. Empty when this node opens no scope.
    pub fn parts(&self, name: &str) -> Vec<RefVal> {
        let Some(inner) = &self.record.inner else {
            return Vec::new();
        };
        inner
            .borrow()
            .drawn_in(None, name, &[])
            .iter()
            .map(|e| self.child(e, 0))
            .collect()
    }

    pub fn parts_under(&self, name: &str) -> Vec<RefVal> {
        let (Some(home), Some(decl)) = (self.home(), self.record.decl) else {
            return Vec::new();
        };
        home.borrow()
            .drawn_in(Some(decl), name, &self.record.copies)
            .iter()
            .map(|e| self.child(e, self.record.copies.len()))
            .collect()
    }

    pub fn home_decl(&self) -> Option<(Rc<Region>, DeclId)> {
        let (home, decl) = (self.home()?, self.record.decl?);
        let region = home.borrow().region();
        Some((region, decl))
    }

    fn child(&self, e: &Entry, prefix_len: usize) -> RefVal {
        let mut label = format!("{}.{}", self.label, e.name);
        for c in e.record.borrow().copies.iter().skip(prefix_len) {
            label.push_str(&format!(".{c}"));
        }
        ref_from(e, &label)
    }

    pub fn stroke_widths(&self) -> Vec<f64> {
        let mut out = Vec::new();
        if let Some(n) = &self.record.node {
            collect_strokes(n, &mut out);
        }
        out.sort_by(|a, b| a.partial_cmp(b).unwrap());
        out.dedup();
        out
    }
}

fn collect_strokes(n: &Node, out: &mut Vec<f64>) {
    if let NodeKind::Shape(s) = &n.kind
        && let Some(st) = &s.stroke
    {
        out.push((st.width as f64 * 100.0).round() / 100.0);
    }
    for c in n.descendants() {
        collect_strokes(c, out);
    }
}

pub fn ref_from(entry: &Entry, label: &str) -> RefVal {
    RefVal {
        label: label.to_string(),
        record: entry.record.borrow().clone(),
        source: Some(Rc::as_ptr(&entry.record) as usize),
        canvas: None,
        extent: Extent::Visual,
    }
}

/// Every anchor the file placed, by the path an author writes after `@`, in canvas units. A `_`
/// name inside a def or slot fill is private to it, so neither it nor anything under it appears.
pub fn placed_anchors(top: &Rc<RefCell<Names>>) -> BTreeMap<String, (f64, f64)> {
    let mut out = BTreeMap::new();
    anchors_in(&top.borrow(), None, &[], "", false, &mut out);
    out
}

/// The anchors under the shape `scope` of `names`, drawn in the copy `prefix`, each at
/// `label.name.copies`.
fn anchors_in(
    names: &Names,
    scope: Option<DeclId>,
    prefix: &[u32],
    label: &str,
    private_scope: bool,
    out: &mut BTreeMap<String, (f64, f64)>,
) {
    let region = names.region();
    for name in region.names_in(scope) {
        if private_scope && name.as_str().starts_with('_') {
            continue;
        }
        for e in names.drawn_in(scope, name.as_str(), prefix) {
            let rec = e.record.borrow();
            let mut path = if label.is_empty() {
                name.to_string()
            } else {
                format!("{label}.{name}")
            };
            for c in rec.copies.iter().skip(prefix.len()) {
                path.push_str(&format!(".{c}"));
            }
            let r = RefVal {
                label: path.clone(),
                record: rec.clone(),
                source: Some(Rc::as_ptr(&e.record) as usize),
                canvas: None,
                extent: Extent::Visual,
            };
            drop(rec);
            collect_anchors(&r, out);
            if let Some(decl) = r.record.decl
                && region.decl(decl).kind == NameKind::Shape
            {
                anchors_in(names, Some(decl), &r.record.copies, &path, private_scope, out);
            }
        }
    }
}

fn collect_anchors(r: &RefVal, out: &mut BTreeMap<String, (f64, f64)>) {
    if let Some(p) = r.record.anchor_canvas() {
        out.insert(r.label.clone(), (p.x as f64, p.y as f64));
    }
    if let Some(inner) = &r.record.inner {
        let private_scope = inner.borrow().region().kind != RegionKind::File;
        anchors_in(&inner.borrow(), None, &[], &r.label, private_scope, out);
    }
    for row in r.record.rows.iter() {
        for (i, stamp) in row.stamps.iter().enumerate() {
            let stamp = RefVal {
                label: format!("{}.{}.{i}", r.label, row.name),
                record: stamp.borrow().clone(),
                source: Some(Rc::as_ptr(stamp) as usize),
                canvas: None,
                extent: Extent::Visual,
            };
            collect_anchors(&stamp, out);
        }
    }
}

/// How close a flattened silhouette stays to a warped curve, in canvas units.
pub(crate) const FLATTEN_TOL: f32 = 0.1;

pub fn silhouette(node: &Node, parent_world: &Place) -> Path {
    let world = parent_world.inner_node(node);
    match &node.kind {
        NodeKind::Shape(s) => world.map_path(&s.clip(), FLATTEN_TOL),
        NodeKind::Group { children, .. } => {
            let parts: Vec<Path> = children
                .iter()
                .map(|c| silhouette(c, &world))
                .filter(|p| !p.is_empty())
                .collect();
            geom::union_all(&parts)
        }
    }
}

/// The box of the node's geometry under `after`, which maps the canvas into the reader's frame.
fn geom_bounds(node: &Node, parent_world: &Place, after: &Matrix) -> Option<Rect> {
    let world = parent_world.inner_node(node);
    match &node.kind {
        NodeKind::Shape(s) => (!s.geometry.is_empty())
            .then(|| world.map_rect_then(s.geometry.compute_tight_bounds(), after)),
        NodeKind::Group { children, .. } => children
            .iter()
            .filter_map(|c| geom_bounds(c, &world, after))
            .reduce(|mut a, b| {
                a.join(b);
                a
            }),
    }
}
