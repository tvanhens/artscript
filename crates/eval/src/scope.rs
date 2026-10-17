//! What names resolve to: modules and their exports, palettes, components, rules, styles,
//! finishes, and the records of named nodes that `@ref`s and checks measure.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use artscript_ir::{Matrix, Node, NodeKind, Path, Point, Rect};
use artscript_syntax::ast::{DefItem, Expr, FxLine, Named, Patch, Scene, Span};

use crate::geom;
use crate::identity::{DeclId, Entry, Key, Names};
use crate::value::Value;

pub struct Module {
    /// Project-relative source file.
    pub path: String,
    pub globals: RefCell<HashMap<String, Value>>,
    pub styles: RefCell<HashMap<String, Rc<StyleDef>>>,
    pub finishes: RefCell<HashMap<String, Rc<FinishDef>>>,
    pub anims: RefCell<HashMap<String, Rc<AnimDef>>>,
    pub palettes: RefCell<Vec<Rc<Palette>>>,
    /// Names this module declares itself, in order. Exported unless they start with `_`.
    pub own: RefCell<Vec<String>>,
}

impl Module {
    pub fn new(path: &str) -> Module {
        Module {
            path: path.to_string(),
            globals: RefCell::new(HashMap::new()),
            styles: RefCell::new(HashMap::new()),
            finishes: RefCell::new(HashMap::new()),
            anims: RefCell::new(HashMap::new()),
            palettes: RefCell::new(Vec::new()),
            own: RefCell::new(Vec::new()),
        }
    }

    pub fn global(&self, name: &str) -> Option<Value> {
        self.globals.borrow().get(name).cloned()
    }

    pub fn define(&self, name: &str, v: Value) {
        self.globals.borrow_mut().insert(name.to_string(), v);
        let mut own = self.own.borrow_mut();
        if !own.iter().any(|n| n == name) {
            own.push(name.to_string());
        }
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

pub struct RuleDef {
    pub name: String,
    pub params: Vec<Named>,
    pub body: Vec<Scene>,
    pub module: String,
    pub span: Span,
}

/// A node (or anchor) as created so far, with the transform from its parent to the canvas.
#[derive(Clone)]
pub struct Record {
    pub name: String,
    pub path: String,
    pub key: Key,
    pub parent_world: Matrix,
    pub node: Option<Node>,
    /// Canvas position of an anchor.
    pub anchor: Option<Point>,
    /// The node's `at:` in its parent's coordinates (for suggested fixes).
    pub at: (f64, f64),
    pub home: Option<Rc<RefCell<Names>>>,
    pub decl: Option<DeclId>,
    /// Set when this node opens a scope (an instance or a slot).
    pub inner: Option<Rc<RefCell<Names>>>,
}

/// The resolved target of an `@ref`. Everything is measured in canvas coordinates.
#[derive(Clone)]
pub struct RefVal {
    /// The reference as written, without `@` (`eye_r`, `hero.head`).
    pub label: String,
    pub record: Record,
    /// `canvas`: the canvas rect itself.
    pub canvas: Option<Rect>,
}

impl RefVal {
    pub fn bounds(&self) -> Option<Rect> {
        if let Some(c) = self.canvas {
            return Some(c);
        }
        if let Some(p) = self.record.anchor {
            return Some(Rect::from_xywh(p.x, p.y, 0.0, 0.0));
        }
        let local = self.record.node.as_ref()?.bounds()?;
        Some(self.record.parent_world.map_rect(local).0)
    }

    pub fn geom_bounds(&self) -> Option<Rect> {
        if self.canvas.is_some() || self.record.anchor.is_some() {
            return self.bounds();
        }
        let node = self.record.node.as_ref()?;
        geom_bounds(node, &self.record.parent_world)
    }

    /// Union of the node's shape geometries in canvas coordinates.
    pub fn silhouette(&self) -> Path {
        if let Some(c) = self.canvas {
            return Path::rect(c, None);
        }
        match &self.record.node {
            Some(n) => silhouette(n, &self.record.parent_world),
            None => Path::new(),
        }
    }

    /// Named direct tree children of this copy. An instance or a slot lists the scope it opens.
    pub fn children(&self) -> Vec<RefVal> {
        let (entries, prefix_len) = if let Some(inner) = &self.record.inner {
            (inner.borrow().children_of(None, &[]), 0)
        } else if let (Some(home), Some(decl)) = (&self.record.home, self.record.decl) {
            let prefix = self.record.key.trailing_copies();
            let n = prefix.len();
            (home.borrow().children_of(Some(decl), &prefix), n)
        } else {
            return Vec::new();
        };
        entries
            .into_iter()
            .map(|e| {
                let extra = e.record.borrow().key.trailing_copies();
                let mut label = format!("{}.{}", self.label, e.name);
                for c in extra.iter().skip(prefix_len) {
                    label.push_str(&format!(".{c}"));
                }
                ref_from(&e, &label)
            })
            .collect()
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
    for c in n.children() {
        collect_strokes(c, out);
    }
}

pub fn ref_from(entry: &Entry, label: &str) -> RefVal {
    RefVal {
        label: label.to_string(),
        record: entry.record.borrow().clone(),
        canvas: None,
    }
}

pub fn silhouette(node: &Node, parent_world: &Matrix) -> Path {
    let world = *parent_world * node.transform;
    match &node.kind {
        NodeKind::Shape(s) => s.geometry.make_transform(&world),
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

fn geom_bounds(node: &Node, parent_world: &Matrix) -> Option<Rect> {
    let world = *parent_world * node.transform;
    match &node.kind {
        NodeKind::Shape(s) => {
            (!s.geometry.is_empty()).then(|| world.map_rect(s.geometry.compute_tight_bounds()).0)
        }
        NodeKind::Group { children, .. } => children
            .iter()
            .filter_map(|c| geom_bounds(c, &world))
            .reduce(|mut a, b| {
                a.join(b);
                a
            }),
    }
}
