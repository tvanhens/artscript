use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::rc::Rc;

use artscript_syntax::ast::{
    DeclKind, DefItem, Draw, DrawItem, Expr, File, Mask, Scene, SceneKind, Span,
};
use artscript_syntax::{Diagnostic, format_expr};

use crate::scope::Record;
use crate::seed::{explicit_seed, node_seed};
use crate::value::Value;

const BUILTIN: &[&str] = &[
    "rect",
    "circle",
    "ellipse",
    "poly",
    "star",
    "curve",
    "path",
    "blob",
    "copy",
    "subtract",
    "union",
    "intersect",
    "xor",
    "layer",
    "group",
    "inside",
    "outside",
    "mirror",
    "repeat",
    "grid",
    "anchor",
    "canvas",
];

const REFLECTION: &str = "~m";

/// An identifier the author wrote as `name:x`, `layer x`, `group x`, `slot x`, or `anchor x`.
/// A def name, a verb, and a generated string are never a `Name`.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Name(Rc<str>);

impl Name {
    /// `None` unless `text` is an identifier.
    pub fn parse(text: &str) -> Option<Name> {
        let mut cs = text.chars();
        let first = cs.next()?;
        ((first.is_ascii_alphabetic() || first == '_')
            && cs.all(|c| c.is_ascii_alphanumeric() || c == '_'))
        .then(|| Name(text.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::borrow::Borrow<str> for Name {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Where a statement was written.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Site {
    pub file: Rc<str>,
    pub span: Span,
}

impl Site {
    pub fn new(file: &str, span: Span) -> Site {
        Site {
            file: file.into(),
            span,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Anon {
    pub verb: Rc<str>,
    pub ordinal: u32,
    pub site: Site,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
enum Seg {
    Name(Name),
    Copy(u32),
    Anon(Anon),
    Mask,
}

impl fmt::Display for Seg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Seg::Name(n) => f.write_str(n.as_str()),
            Seg::Copy(i) => write!(f, "{i}"),
            Seg::Anon(a) => write!(f, "{}~{}", a.verb, a.ordinal),
            Seg::Mask => f.write_str("mask~1"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Use {
    Draw(u64),
    Blob,
    Jitter,
    Grain,
}

#[derive(Clone, Default, PartialEq, Eq, Hash, Debug)]
struct Tail {
    binding: Option<Rc<str>>,
    stmt: Option<(Rc<[u32]>, Rc<str>)>,
    used: Option<Use>,
}

/// A node's identity. Names are unique per region, counts are unique per block, and copies add
/// an index, so two nodes in one scene never share a key. The scene root is the empty key.
#[derive(Clone, Debug)]
pub struct Key {
    segs: Rc<[Seg]>,
    tail: Option<Rc<Tail>>,
    /// It changes seeds only, so equality and hashing leave it out.
    seed_root: Option<Rc<SeedRoot>>,
}

/// An instance's `seed:`. Every seed under the instance hashes the def and the value in place
/// of the file and the instance's own segs, so it is the same from any caller in any file.
#[derive(Debug)]
struct SeedRoot {
    /// How many leading segs the root replaces: the instance key and its loop copies.
    segs: usize,
    /// The def's module path.
    module: Rc<str>,
    def: Rc<str>,
    seed: u64,
}

impl PartialEq for Key {
    fn eq(&self, other: &Key) -> bool {
        self.segs == other.segs && self.tail == other.tail
    }
}

impl Eq for Key {}

impl std::hash::Hash for Key {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.segs.hash(state);
        self.tail.hash(state);
    }
}

impl Default for Key {
    fn default() -> Key {
        Key::root()
    }
}

impl Key {
    pub fn root() -> Key {
        Key {
            segs: Rc::from(Vec::new()),
            tail: None,
            seed_root: None,
        }
    }

    pub fn is_root(&self) -> bool {
        self.segs.is_empty()
    }

    pub fn named(owner: &Key, name: &Name, copies: &[u32]) -> Key {
        owner.extend(Seg::Name(name.clone()), copies)
    }

    /// The tree parent's key, then `verb~N`, then the loop indices since that parent.
    pub fn anon(parent: &Key, anon: &Anon, copies: &[u32]) -> Key {
        parent.extend(Seg::Anon(anon.clone()), copies)
    }

    pub fn mask(&self) -> Key {
        self.extend(Seg::Mask, &[])
    }

    fn extend(&self, seg: Seg, copies: &[u32]) -> Key {
        let mut segs = self.segs.to_vec();
        segs.push(seg);
        segs.extend(copies.iter().map(|&i| Seg::Copy(i)));
        Key {
            segs: segs.into(),
            tail: None,
            seed_root: self.seed_root.clone(),
        }
    }

    /// This instance key of `def` with `seed:`. An inner seeded instance replaces an outer one.
    pub fn seeded(&self, module: &str, def: &str, seed: u64) -> Key {
        let root = SeedRoot {
            segs: self.segs.len(),
            module: module.into(),
            def: def.into(),
            seed,
        };
        Key {
            seed_root: Some(Rc::new(root)),
            ..self.clone()
        }
    }

    fn seed_skip(&self) -> usize {
        self.seed_root.as_ref().map_or(0, |r| r.segs)
    }

    /// Loop indices after the last name or positional segment.
    pub fn trailing_copies(&self) -> Vec<u32> {
        let mut out = Vec::new();
        for seg in self.segs.iter().rev() {
            match seg {
                Seg::Copy(i) => out.push(*i),
                _ => break,
            }
        }
        out.reverse();
        out
    }

    pub fn copy_at(&self, depth: usize) -> Option<u32> {
        self.trailing_copies().get(depth).copied()
    }

    pub fn is_stable(&self) -> bool {
        self.segs
            .iter()
            .all(|s| matches!(s, Seg::Name(_) | Seg::Copy(_)))
    }

    /// `.a.b.0` from `scope` to this key, or `None` when the rest of the key is not stable.
    pub fn address_from(&self, scope: &Key) -> Option<String> {
        let rest = self.segs.strip_prefix(&scope.segs[..])?;
        match rest.first() {
            Some(Seg::Name(_)) => {}
            _ => return None,
        }
        if !rest
            .iter()
            .all(|s| matches!(s, Seg::Name(_) | Seg::Copy(_)))
        {
            return None;
        }
        Some(rest.iter().map(|s| format!(".{s}")).collect())
    }

    /// The seed key of a module-level `let`, which has no node.
    pub fn binding(name: &str) -> Key {
        Key::root().with_tail(|t| t.binding = Some(name.into()))
    }

    /// The seed key of a `let`, `if`, `each`, `play`, or anim line under this node, after the loop
    /// indices since this node. `stmt` is the statement's `let y~1` among its siblings.
    pub fn at_stmt(&self, copies: &[u32], stmt: &str) -> Key {
        self.with_tail(|t| t.stmt = Some((copies.into(), stmt.into())))
    }

    /// The `n`th draw-site seed (`rand`, `pick`, noise) under this key.
    pub fn draw(&self, n: u64) -> Key {
        self.with_tail(|t| t.used = Some(Use::Draw(n)))
    }

    pub fn blob(&self) -> Key {
        self.with_tail(|t| t.used = Some(Use::Blob))
    }

    pub fn jitter(&self) -> Key {
        self.with_tail(|t| t.used = Some(Use::Jitter))
    }

    pub fn grain(&self) -> Key {
        self.with_tail(|t| t.used = Some(Use::Grain))
    }

    fn with_tail(&self, f: impl FnOnce(&mut Tail)) -> Key {
        let mut tail = self.tail.as_deref().cloned().unwrap_or_default();
        f(&mut tail);
        Key {
            tail: Some(Rc::new(tail)),
            ..self.clone()
        }
    }

    fn seed_text(&self) -> String {
        let mut s = match &self.seed_root {
            Some(r) => {
                let mut s = format!("\0{} seed:{}", r.def, r.seed);
                for seg in &self.segs[r.segs..] {
                    s.push_str(&format!(".{seg}"));
                }
                s
            }
            None => self.to_string(),
        };
        if let Some(t) = &self.tail {
            if let Some(b) = &t.binding {
                s.push_str(b);
            }
            if let Some((copies, stmt)) = &t.stmt {
                for c in copies.iter() {
                    s.push_str(&format!("#{c}"));
                }
                s.push_str(&format!("\0{stmt}"));
            }
            match t.used {
                Some(Use::Draw(n)) => s.push_str(&format!("\0{n}")),
                Some(Use::Blob) => s.push_str("\0blob"),
                Some(Use::Jitter) => s.push_str("#jitter"),
                Some(Use::Grain) => s.push_str("#grain"),
                None => {}
            }
        }
        s
    }
}

fn join(segs: &[Seg]) -> String {
    segs.iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>()
        .join(".")
}

/// `a.b.0.circle~2`, then `~m` per reflection. `Node.path`, `Diagnostic.node`, and `explain`
/// print this.
impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&join(&self.segs))
    }
}

pub fn reflect(node: &mut artscript_ir::Node) {
    node.path.push_str(REFLECTION);
    if let Some(mask) = &mut node.mask
        && let artscript_ir::MaskSource::Nodes(ns) = &mut mask.source
    {
        ns.iter_mut().for_each(reflect);
    }
    match &mut node.kind {
        artscript_ir::NodeKind::Shape(s) => s.children.iter_mut().for_each(reflect),
        artscript_ir::NodeKind::Group { children, .. } => children.iter_mut().for_each(reflect),
    }
}

/// An unnamed statement whose key fed a seed.
#[derive(Clone, Debug)]
pub struct Positional {
    pub verb: Rc<str>,
    /// The statement's own key, through its `verb~N`.
    pub statement: String,
}

/// Every seed records its key here. An unstable key records each unnamed statement in it once,
/// first use kept. `seeded` is the node keys that drew a seed, for `explain`.
#[derive(Default)]
pub struct PositionalUses {
    sites: BTreeMap<Site, Positional>,
    seeded: BTreeSet<String>,
}

impl PositionalUses {
    fn note(&mut self, key: &Key) {
        if key.segs.is_empty() {
            return;
        }
        let text = key.to_string();
        for (i, seg) in key.segs.iter().enumerate().skip(key.seed_skip()) {
            if let Seg::Anon(a) = seg {
                self.sites
                    .entry(a.site.clone())
                    .or_insert_with(|| Positional {
                        verb: a.verb.clone(),
                        statement: join(&key.segs[..=i]),
                    });
            }
        }
        self.seeded.insert(text);
    }

    pub fn sites(&self) -> impl Iterator<Item = (&Site, &Positional)> {
        self.sites.iter()
    }

    pub fn seeded(&self) -> &BTreeSet<String> {
        &self.seeded
    }
}

/// The author's `seed:` when there is one, else `node_seed` over `file` and the key's seed text.
/// Under a seeded instance the def's module stands in for `file`. When the key's seed text holds
/// an unnamed segment, `uses` records that statement for L0104.
pub fn seed(file: &str, key: &Key, explicit: Option<u64>, uses: &mut PositionalUses) -> u64 {
    if let Some(s) = explicit {
        return explicit_seed(s);
    }
    uses.note(key);
    let file = key.seed_root.as_ref().map_or(file, |r| &r.module);
    node_seed(file, &key.seed_text())
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegionKind {
    File,
    Def,
    /// A slot fill at a call site, or a slot's fallback block in a def.
    Slot,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct DeclId(usize);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NameKind {
    /// A shape, structure verb, `copy`, or boolean, `layer x` and `group x` included.
    Draw,
    /// A def call. A patch may hop through it.
    Instance,
    /// `slot x`. A patch may hop into its content.
    Slot,
    /// `anchor x`. It has no node.
    Anchor,
}

impl NameKind {
    pub fn opens_scope(self) -> bool {
        matches!(self, NameKind::Instance | NameKind::Slot)
    }

    pub fn has_node(self) -> bool {
        self != NameKind::Anchor
    }
}

#[derive(Clone, Debug)]
pub struct NameDecl {
    pub name: Name,
    pub site: Site,
    pub kind: NameKind,
    /// The `each`, `repeat`, and `grid` statements between the region root and this one. More
    /// than none makes a family.
    pub loops: Vec<Span>,
    arms: Vec<(Span, bool)>,
    /// The nearest named draw above this one in the region. A final `*` matches on it.
    pub tree_parent: Option<DeclId>,
    /// The component verb, when this decl is a call. A patch that hops through it
    /// while the call is off uses this to find the def's region.
    pub call: Option<Rc<str>>,
}

/// One naming scope's static shape: every name it declares at any depth, in source order.
#[derive(Debug)]
pub struct Region {
    pub kind: RegionKind,
    /// `this file`, `` `eye` ``, or `` the `form:` fill ``, for messages.
    pub label: String,
    decls: Vec<NameDecl>,
    by_name: BTreeMap<Name, Vec<DeclId>>,
    by_span: BTreeMap<Span, DeclId>,
    anon: Vec<(Rc<str>, Site)>,
}

impl Region {
    fn new(kind: RegionKind, label: String) -> Region {
        Region {
            kind,
            label,
            decls: Vec::new(),
            by_name: BTreeMap::new(),
            by_span: BTreeMap::new(),
            anon: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.decls.is_empty()
    }

    pub fn decl(&self, id: DeclId) -> &NameDecl {
        &self.decls[id.0]
    }

    pub fn decl_ids(&self) -> impl Iterator<Item = DeclId> {
        (0..self.decls.len()).map(DeclId)
    }

    /// The decl a statement makes, or `None` for an unnamed statement.
    pub fn decl_at(&self, stmt: Span) -> Option<DeclId> {
        self.by_span.get(&stmt).copied()
    }

    /// Every decl with this name. More than one only across exclusive `if` arms, or after E0333.
    pub fn lookup(&self, name: &str) -> &[DeclId] {
        self.by_name.get(name).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Declared names in source order, without repeats.
    pub fn names(&self) -> Vec<&Name> {
        let mut out: Vec<&Name> = Vec::new();
        for d in &self.decls {
            if !out.contains(&&d.name) {
                out.push(&d.name);
            }
        }
        out
    }

    /// A `_` name in a def body or slot content is private to it. The entry file has no caller.
    fn is_private(&self, name: &str) -> bool {
        self.kind != RegionKind::File && name.starts_with('_')
    }

    /// The E0340 note that lists the names a patch can reach.
    fn names_note(&self) -> String {
        let names: Vec<String> = self
            .names()
            .into_iter()
            .filter(|n| !self.is_private(n.as_str()))
            .map(|n| n.to_string())
            .collect();
        if names.is_empty() {
            "names: (none)".into()
        } else {
            format!("names: {}", names.join(", "))
        }
    }

    /// The first unnamed statement whose verb `seg` spells, alone or with a count after it.
    pub fn anon_hint(&self, seg: &str) -> Option<&Site> {
        let mut verb: Option<&str> = None;
        for (name, _) in &self.anon {
            let Some(rest) = seg.strip_prefix(name.as_ref()) else {
                continue;
            };
            if !rest.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            if verb.is_none_or(|v| name.len() > v.len()) {
                verb = Some(name);
            }
        }
        let verb = verb?;
        let rest = seg.strip_prefix(verb).unwrap_or("");
        let want = if rest.is_empty() {
            1usize
        } else {
            match rest.parse::<usize>() {
                Ok(n) if n >= 1 => n,
                _ => return None,
            }
        };
        self.anon
            .iter()
            .filter(|(name, _)| name.as_ref() == verb)
            .nth(want - 1)
            .map(|(_, site)| site)
    }
}

fn exclusive(a: &[(Span, bool)], b: &[(Span, bool)]) -> bool {
    a.iter()
        .any(|(s, arm)| b.iter().any(|(t, other)| s == t && arm != other))
}

fn kind_of(verb: &str, components: &BTreeSet<String>) -> NameKind {
    if components.contains(verb) {
        return NameKind::Instance;
    }
    match verb {
        "anchor" => NameKind::Anchor,
        v if BUILTIN.contains(&v) => NameKind::Draw,
        _ => NameKind::Instance,
    }
}

/// Whether `verb` is not a built-in draw word.
pub fn calls_def(verb: &str) -> bool {
    !BUILTIN.contains(&verb)
}

fn calls_component(verb: &str, components: &BTreeSet<String>) -> bool {
    components.contains(verb) || calls_def(verb)
}

struct Declarer<'a> {
    file: Rc<str>,
    region: Region,
    loops: Vec<Span>,
    arms: Vec<(Span, bool)>,
    parent: Option<DeclId>,
    components: &'a BTreeSet<String>,
    nested: &'a mut BTreeMap<Span, Rc<Region>>,
    out: &'a mut Vec<Diagnostic>,
}

/// Declares one region. It walks draw bodies, mask blocks, `if` arms, and loop bodies, and it
/// stops at slot content, which becomes its own region in `nested`, keyed by the fill's or the
/// slot's span. A second name in the region is E0333 at its line, unless the two sit in exclusive
/// arms of one `if`. A `name:` that is not one identifier, or a second name on one node, is E0312.
/// It reads source only, so a statement that is never drawn is still checked.
pub fn declare(
    file: &Rc<str>,
    kind: RegionKind,
    label: String,
    stmts: &[&Scene],
    components: &BTreeSet<String>,
    nested: &mut BTreeMap<Span, Rc<Region>>,
    out: &mut Vec<Diagnostic>,
) -> Region {
    let mut d = Declarer {
        file: file.clone(),
        region: Region::new(kind, label),
        loops: Vec::new(),
        arms: Vec::new(),
        parent: None,
        components,
        nested,
        out,
    };
    d.scenes(stmts.iter().copied());
    d.region
}

impl Declarer<'_> {
    fn scenes<'s>(&mut self, stmts: impl IntoIterator<Item = &'s Scene>) {
        for s in stmts {
            self.scene(s);
        }
    }

    fn scene(&mut self, s: &Scene) {
        match &s.kind {
            SceneKind::If {
                then, otherwise, ..
            } => {
                self.arms.push((s.span, false));
                self.scenes(then);
                self.arms.pop();
                self.arms.push((s.span, true));
                self.scenes(otherwise);
                self.arms.pop();
            }
            SceneKind::Each { body, .. } => {
                self.loops.push(s.span);
                self.scenes(body);
                self.loops.pop();
            }
            SceneKind::Slot {
                name,
                named,
                fallback,
            } => {
                for n in named.iter().filter(|n| n.key == "name") {
                    self.two_names(n.span, "slot", name);
                }
                if let Some(n) = Name::parse(name) {
                    self.add(n, s.span, NameKind::Slot, None);
                }
                let stmts: Vec<&Scene> = fallback.iter().collect();
                let label = format!("the `{name}` fallback");
                self.sub(s.span, label, &stmts);
            }
            SceneKind::Draw(d) => self.draw(s.span, d),
            SceneKind::Let { .. }
            | SceneKind::Check { .. }
            | SceneKind::Play { .. }
            | SceneKind::Fx(_) => {}
        }
    }

    fn sub(&mut self, at: Span, label: String, stmts: &[&Scene]) {
        let r = declare(
            &self.file,
            RegionKind::Slot,
            label,
            stmts,
            self.components,
            self.nested,
            self.out,
        );
        self.nested.insert(at, Rc::new(r));
    }

    fn draw(&mut self, span: Span, d: &Draw) {
        if d.verb == "canvas" {
            return;
        }
        let kind = kind_of(&d.verb, self.components);
        let target = matches!(kind, NameKind::Instance).then(|| Rc::<str>::from(d.verb.as_str()));
        let id = self
            .name_of(d)
            .map(|name| self.add(name, span, kind, target.clone()));
        if id.is_none() {
            let site = Site {
                file: self.file.clone(),
                span,
            };
            self.region.anon.push((d.verb.as_str().into(), site));
        }
        let call = calls_component(&d.verb, self.components);
        let looped = matches!(d.verb.as_str(), "repeat" | "grid");
        let saved = self.parent;
        if id.is_some() {
            self.parent = id;
        }
        for item in d.body.items() {
            match item {
                DrawItem::Scene(s) if !call => {
                    if looped {
                        self.loops.push(span);
                    }
                    self.scene(s);
                    if looped {
                        self.loops.pop();
                    }
                }
                DrawItem::Mask {
                    mask: Mask::Block(b),
                    ..
                } => self.scenes(b),
                DrawItem::SlotFill(fill) if call => {
                    let stmts: Vec<&Scene> = fill.body.iter().collect();
                    self.sub(fill.span, format!("the `{}:` fill", fill.name), &stmts);
                }
                _ => {}
            }
        }
        self.parent = saved;
    }

    fn name_of(&mut self, d: &Draw) -> Option<Name> {
        let positional = matches!(d.verb.as_str(), "layer" | "group" | "anchor")
            .then(|| match d.args.first() {
                Some(Expr::Ident(s)) => Name::parse(s),
                _ => None,
            })
            .flatten();
        let mut chosen: Option<Name> = None;
        for n in d.named.iter().filter(|n| n.key == "name") {
            let name = match &n.value {
                Expr::Ident(s) => Name::parse(s),
                _ => None,
            };
            let Some(name) = name else {
                self.out.push(
                    Diagnostic::error(
                        "E0312",
                        n.span,
                        format!(
                            "`name:` takes one identifier, got `{}`",
                            format_expr(&n.value)
                        ),
                    )
                    .in_file(&*self.file)
                    .with_fix("write a single identifier `name:`, as in `name:eye`"),
                );
                continue;
            };
            match chosen.as_ref().or(positional.as_ref()) {
                Some(first) => {
                    let first = first.to_string();
                    self.two_names(n.span, &d.verb, &first);
                }
                None => chosen = Some(name),
            }
        }
        chosen.or(positional)
    }

    fn two_names(&mut self, span: Span, verb: &str, first: &str) {
        self.out.push(
            Diagnostic::error(
                "E0312",
                span,
                format!("this `{verb}` already has the name `{first}`"),
            )
            .in_file(&*self.file)
            .with_fix("write a single identifier `name:`"),
        );
    }

    fn add(&mut self, name: Name, span: Span, kind: NameKind, call: Option<Rc<str>>) -> DeclId {
        let site = Site {
            file: self.file.clone(),
            span,
        };
        let first = self
            .region
            .lookup(name.as_str())
            .iter()
            .map(|&i| self.region.decl(i))
            .find(|o| !exclusive(&o.arms, &self.arms))
            .map(|o| o.site.span.line);
        if let Some(line) = first {
            self.out.push(
                Diagnostic::error(
                    "E0333",
                    span,
                    format!("`{name}` is already a name in {}", self.region.label),
                )
                .in_file(&*self.file)
                .with_note(format!("first named at line {line}"))
                .with_fix("rename one of them"),
            );
        }
        let id = DeclId(self.region.decls.len());
        self.region.decls.push(NameDecl {
            name: name.clone(),
            site,
            kind,
            loops: self.loops.clone(),
            arms: self.arms.clone(),
            tree_parent: self.parent,
            call,
        });
        self.region.by_name.entry(name).or_default().push(id);
        self.region.by_span.insert(span, id);
        id
    }
}

/// The `verb~N` of every draw under `block`, by statement span. Within one block it counts every
/// draw with the same written verb, named ones too, through both `if` arms and once per loop
/// body. Each child block counts from 1 on its own. It evaluates nothing, so an alias counts
/// under the word written. `tags` gets the same count for `let y`, `if`, `each i`, and
/// `play bob` statements, so a statement seed does not move with its line.
pub fn ordinals(
    file: &Rc<str>,
    block: &[&Scene],
    out: &mut BTreeMap<Span, Anon>,
    tags: &mut BTreeMap<Span, Rc<str>>,
) {
    let mut children = Vec::new();
    count(file, block, &mut BTreeMap::new(), out, tags, &mut children);
    for c in children {
        ordinals(file, &c, out, tags);
    }
}

fn count<'s>(
    file: &Rc<str>,
    stmts: &[&'s Scene],
    counts: &mut BTreeMap<String, u32>,
    out: &mut BTreeMap<Span, Anon>,
    tags: &mut BTreeMap<Span, Rc<str>>,
    children: &mut Vec<Vec<&'s Scene>>,
) {
    for s in stmts {
        let tag = match &s.kind {
            SceneKind::Let { name, .. } => Some(format!("let {name}")),
            SceneKind::If { .. } => Some("if".to_string()),
            SceneKind::Each { var, .. } => Some(format!("each {var}")),
            SceneKind::Play { name, .. } => {
                Some(format!("play {}", name.as_deref().unwrap_or("none")))
            }
            _ => None,
        };
        if let Some(tag) = tag {
            let n = counts.entry(tag.clone()).or_insert(0);
            *n += 1;
            tags.insert(s.span, format!("{tag}~{n}").into());
        }
        match &s.kind {
            SceneKind::If {
                then, otherwise, ..
            } => {
                count(
                    file,
                    &then.iter().collect::<Vec<_>>(),
                    counts,
                    out,
                    tags,
                    children,
                );
                count(
                    file,
                    &otherwise.iter().collect::<Vec<_>>(),
                    counts,
                    out,
                    tags,
                    children,
                );
            }
            SceneKind::Each { body, .. } => {
                count(
                    file,
                    &body.iter().collect::<Vec<_>>(),
                    counts,
                    out,
                    tags,
                    children,
                );
            }
            SceneKind::Slot { fallback, .. } => children.push(fallback.iter().collect()),
            SceneKind::Draw(d) => {
                let n = counts.entry(d.verb.clone()).or_insert(0);
                *n += 1;
                out.insert(
                    s.span,
                    Anon {
                        verb: d.verb.as_str().into(),
                        ordinal: *n,
                        site: Site {
                            file: file.clone(),
                            span: s.span,
                        },
                    },
                );
                children.push(d.body.scenes().collect());
                for item in d.body.items() {
                    match item {
                        DrawItem::Mask {
                            mask: Mask::Block(b),
                            ..
                        } => children.push(b.iter().collect()),
                        DrawItem::SlotFill(f) => children.push(f.body.iter().collect()),
                        _ => {}
                    }
                }
            }
            SceneKind::Let { .. }
            | SceneKind::Check { .. }
            | SceneKind::Play { .. }
            | SceneKind::Fx(_) => {}
        }
    }
}

/// Identity facts for one loaded module, built once and shared by every instance.
pub struct ModuleIds {
    file: Rc<Region>,
    regions: BTreeMap<Span, Rc<Region>>,
    anon: BTreeMap<Span, Anon>,
    tags: BTreeMap<Span, Rc<str>>,
}

impl ModuleIds {
    pub fn build(
        path: &str,
        parsed: &File,
        components: &BTreeSet<String>,
        out: &mut Vec<Diagnostic>,
    ) -> ModuleIds {
        let file: Rc<str> = path.into();
        let mut regions = BTreeMap::new();
        let mut anon = BTreeMap::new();
        let mut tags = BTreeMap::new();
        let top: Vec<&Scene> = parsed.scenes().collect();
        let file_region = declare(
            &file,
            RegionKind::File,
            "this file".into(),
            &top,
            components,
            &mut regions,
            out,
        );
        ordinals(&file, &top, &mut anon, &mut tags);
        for d in parsed.decls() {
            if let DeclKind::Def { name, body, .. } = &d.kind {
                let stmts: Vec<&Scene> = body.iter().filter_map(DefItem::scene).collect();
                let r = declare(
                    &file,
                    RegionKind::Def,
                    format!("`{name}`"),
                    &stmts,
                    components,
                    &mut regions,
                    out,
                );
                regions.insert(d.span, Rc::new(r));
                ordinals(&file, &stmts, &mut anon, &mut tags);
            }
        }
        ModuleIds {
            file: Rc::new(file_region),
            regions,
            anon,
            tags,
        }
    }

    pub fn file(&self) -> Rc<Region> {
        self.file.clone()
    }

    pub fn region(&self, at: Span) -> Option<Rc<Region>> {
        self.regions.get(&at).cloned()
    }

    pub fn anon(&self, stmt: Span) -> Option<&Anon> {
        self.anon.get(&stmt)
    }

    /// The `let y~1` of a `let`, `if`, `each`, or `play` statement.
    pub fn tag(&self, stmt: Span) -> Option<&str> {
        self.tags.get(&stmt).map(|t| &**t)
    }

    pub fn blank() -> ModuleIds {
        ModuleIds {
            file: Rc::new(Region::new(RegionKind::File, "this file".into())),
            regions: BTreeMap::new(),
            anon: BTreeMap::new(),
            tags: BTreeMap::new(),
        }
    }
}

/// One hop of a patch or a reference. A copy index is a decimal segment. `*` is only final.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Hop {
    Name(Name),
    Index(u32),
    Star,
}

impl fmt::Display for Hop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Hop::Name(n) => f.write_str(n.as_str()),
            Hop::Index(i) => write!(f, "{i}"),
            Hop::Star => f.write_str("*"),
        }
    }
}

pub fn hop_text(hops: &[Hop]) -> String {
    hops.iter()
        .map(|h| h.to_string())
        .collect::<Vec<_>>()
        .join(".")
}

/// Patch segments. A non-final `*` fails. A segment that is all digits is an index.
pub fn hops_from(segs: &[String]) -> Option<Vec<Hop>> {
    let mut hops = Vec::with_capacity(segs.len());
    for (i, seg) in segs.iter().enumerate() {
        if seg == "*" {
            if i + 1 != segs.len() {
                return None;
            }
            hops.push(Hop::Star);
        } else if !seg.is_empty() && seg.chars().all(|c| c.is_ascii_digit()) {
            hops.push(Hop::Index(seg.parse().ok()?));
        } else {
            let name = Name::parse(seg)?;
            hops.push(Hop::Name(name));
        }
    }
    Some(hops)
}

/// One drawn node a name reaches. Children and the opened scope live on `record`.
#[derive(Clone)]
pub struct Entry {
    pub name: Name,
    pub record: Rc<RefCell<Record>>,
}

enum Row {
    Undrawn,
    One(Entry),
    Copies(BTreeMap<Vec<u32>, Entry>),
}

/// One scope instance: the file, one def body, or one slot fill.
pub struct Names {
    region: Rc<Region>,
    rows: Vec<Row>,
}

impl Names {
    pub fn new(region: Rc<Region>) -> Names {
        let n = region.decls.len();
        Names {
            region,
            rows: (0..n).map(|_| Row::Undrawn).collect(),
        }
    }

    pub fn region(&self) -> Rc<Region> {
        self.region.clone()
    }

    pub fn register(&mut self, decl: DeclId, copies: &[u32], entry: Entry) {
        let loops = self.region.decl(decl).loops.len();
        if loops == 0 {
            self.rows[decl.0] = Row::One(entry);
            return;
        }
        match &mut self.rows[decl.0] {
            Row::Copies(map) => {
                map.insert(copies.to_vec(), entry);
            }
            _ => {
                let mut map = BTreeMap::new();
                map.insert(copies.to_vec(), entry);
                self.rows[decl.0] = Row::Copies(map);
            }
        }
    }

    /// Drawn names whose tree parent is `parent`. `prefix` is that parent's own loop indices,
    /// so a hop through copy 1 does not see copy 0. An opened scope passes an empty prefix.
    pub fn children_of(&self, parent: Option<DeclId>, prefix: &[u32]) -> Vec<Entry> {
        let mut out = Vec::new();
        for id in self.region.decl_ids() {
            if self.region.decl(id).tree_parent != parent {
                continue;
            }
            match &self.rows[id.0] {
                Row::One(e) if prefix.is_empty() => out.push(e.clone()),
                Row::Copies(map) => out.extend(
                    map.iter()
                        .filter(|(k, _)| k.starts_with(prefix))
                        .map(|(_, e)| e.clone()),
                ),
                _ => {}
            }
        }
        out
    }

    pub fn drawn(&self, id: DeclId) -> Vec<Entry> {
        match &self.rows[id.0] {
            Row::Undrawn => Vec::new(),
            Row::One(e) => vec![e.clone()],
            Row::Copies(map) => map.values().cloned().collect(),
        }
    }

    pub fn records(&self) -> Vec<Rc<RefCell<Record>>> {
        let mut out = Vec::new();
        for row in &self.rows {
            match row {
                Row::Undrawn => {}
                Row::One(e) => out.push(e.record.clone()),
                Row::Copies(map) => out.extend(map.values().map(|e| e.record.clone())),
            }
        }
        out
    }
}

/// One scope a reference can see. Inherited frames keep the copies of the loop they were
/// captured in. The innermost frame uses the copies at the reference.
#[derive(Clone)]
struct LookFrame {
    names: Rc<RefCell<Names>>,
    copies: Vec<u32>,
    loops: Vec<Span>,
}

impl LookFrame {
    fn live(names: Rc<RefCell<Names>>) -> LookFrame {
        LookFrame {
            names,
            copies: Vec::new(),
            loops: Vec::new(),
        }
    }

    fn at(names: Rc<RefCell<Names>>, copies: &[u32], loops: &[Span]) -> LookFrame {
        LookFrame {
            names,
            copies: copies.to_vec(),
            loops: loops.to_vec(),
        }
    }
}

/// A `needs:` name, resolved in the host with the copies current at the slot.
#[derive(Clone)]
pub struct Import {
    pub name: Name,
    pub names: Rc<RefCell<Names>>,
    pub copies: Vec<u32>,
    pub loops: Vec<Span>,
}

/// Where `@name` looks, innermost frame first. `imports` re-resolve in the host that granted them.
#[derive(Clone)]
pub struct Lookup {
    frames: Vec<LookFrame>,
    imports: Vec<Import>,
}

impl Lookup {
    pub fn own(names: Rc<RefCell<Names>>) -> Lookup {
        Lookup {
            frames: vec![LookFrame::live(names)],
            imports: Vec::new(),
        }
    }

    pub fn with_imports(names: Rc<RefCell<Names>>, imports: Vec<Import>) -> Lookup {
        Lookup {
            frames: vec![LookFrame::live(names)],
            imports,
        }
    }

    /// `inner` is a nested scope. It sees its own names, then every frame `outer` already could.
    /// `copies` is the outer scope's current loops, kept on the frame that was live there.
    pub fn under(
        inner: Rc<RefCell<Names>>,
        outer: &Lookup,
        copies: &[u32],
        loops: &[Span],
    ) -> Lookup {
        let mut frames = vec![LookFrame::live(inner)];
        if let Some((first, rest)) = outer.frames.split_first() {
            frames.push(LookFrame::at(first.names.clone(), copies, loops));
            frames.extend(rest.iter().cloned());
        }
        Lookup {
            frames,
            imports: outer.imports.clone(),
        }
    }

    /// Slot content: the fill, then the host, then whatever the caller could see.
    /// The host and the caller each keep the copies of the loop they were in.
    pub fn slot(
        fill: Rc<RefCell<Names>>,
        host: Rc<RefCell<Names>>,
        host_copies: &[u32],
        host_loops: &[Span],
        caller: &Lookup,
        caller_copies: &[u32],
        caller_loops: &[Span],
    ) -> Lookup {
        let mut frames = vec![
            LookFrame::live(fill),
            LookFrame::at(host, host_copies, host_loops),
        ];
        if let Some((first, rest)) = caller.frames.split_first() {
            frames.push(LookFrame::at(first.names.clone(), caller_copies, caller_loops));
            frames.extend(rest.iter().cloned());
        }
        Lookup {
            frames,
            imports: caller.imports.clone(),
        }
    }
}

/// What a name reached.
pub enum Resolved {
    /// `usize` is how many leading copy indices the loop context already chose.
    One(Entry, usize),
    /// `usize` is how many leading copy indices the loop context already chose.
    Many(Vec<Entry>, usize),
    /// Declared, and not drawn on this pass.
    Undrawn(Site),
    /// No frame declares it. `names` is the innermost region's list.
    Missing(Vec<String>),
    /// The same name is declared in more than one frame. Each note is one spelling.
    Ambiguous(Vec<String>),
}

/// `here_loops` is the loop spans from the region root out to the reference, parallel to
/// `here_copies`. A family whose loops are a prefix of that is the current copy.
pub fn resolve(
    lookup: &Lookup,
    name: &str,
    here_copies: &[u32],
    here_loops: &[Span],
) -> Resolved {
    let mut hits: Vec<(String, Resolved)> = Vec::new();
    for (i, frame) in lookup.frames.iter().enumerate() {
        let names = frame.names.borrow();
        if names.region.lookup(name).is_empty() {
            continue;
        }
        let label = names.region.label.clone();
        let (copies, loops) = if i == 0 {
            (here_copies, here_loops)
        } else {
            (frame.copies.as_slice(), frame.loops.as_slice())
        };
        let resolved = resolve_in(&names, name, copies, loops);
        hits.push((label, resolved));
    }
    if let Some(import) = lookup
        .imports
        .iter()
        .rev()
        .find(|import| import.name.as_str() == name)
    {
        let resolved = resolve_in(
            &import.names.borrow(),
            name,
            &import.copies,
            &import.loops,
        );
        hits.push(("a `needs:` import".into(), resolved));
    }
    match hits.len() {
        0 => {
            let names = lookup
                .frames
                .first()
                .map(|f| {
                    f.names
                        .borrow()
                        .region
                        .names()
                        .iter()
                        .map(|n| n.to_string())
                        .collect()
                })
                .unwrap_or_default();
            Resolved::Missing(names)
        }
        1 => hits.pop().unwrap().1,
        _ => Resolved::Ambiguous(
            hits.into_iter()
                .map(|(label, _)| format!("`{name}` in {label}"))
                .collect(),
        ),
    }
}

fn resolve_in(names: &Names, name: &str, here_copies: &[u32], here_loops: &[Span]) -> Resolved {
    let mut drawn = Vec::new();
    let mut undrawn = None;
    let mut skipped = usize::MAX;
    for &id in names.region.lookup(name) {
        let decl = names.region.decl(id);
        match select(&names.rows[id.0], decl, here_copies, here_loops) {
            Some((entries, _)) if entries.is_empty() => {
                undrawn.get_or_insert_with(|| decl.site.clone());
            }
            Some((entries, depth)) => {
                skipped = skipped.min(depth);
                drawn.extend(entries);
            }
            None => {
                undrawn.get_or_insert_with(|| decl.site.clone());
            }
        }
    }
    match drawn.len() {
        0 => match undrawn {
            Some(site) => Resolved::Undrawn(site),
            None => Resolved::Missing(Vec::new()),
        },
        1 => Resolved::One(drawn.pop().unwrap(), skipped),
        _ => Resolved::Many(drawn, skipped),
    }
}

/// The `usize` is the number of copy indices already fixed by the shared loop prefix.
fn select(
    row: &Row,
    decl: &NameDecl,
    here_copies: &[u32],
    here_loops: &[Span],
) -> Option<(Vec<Entry>, usize)> {
    let loops = &decl.loops;
    let mut n = 0;
    while n < loops.len() && n < here_loops.len() && loops[n] == here_loops[n] {
        n += 1;
    }
    let inside = n == loops.len();
    match row {
        Row::Undrawn => None,
        Row::One(entry) => Some((vec![entry.clone()], 0)),
        Row::Copies(map) => {
            if inside {
                let key = here_copies.get(..loops.len()).unwrap_or(&[]);
                Some((map.get(key).cloned().into_iter().collect(), loops.len()))
            } else if n > 0 {
                let prefix = &here_copies[..n];
                Some((
                    map.iter()
                        .filter(|(k, _)| k.starts_with(prefix))
                        .map(|(_, e)| e.clone())
                        .collect(),
                    n,
                ))
            } else {
                Some((map.values().cloned().collect(), 0))
            }
        }
    }
}

#[derive(Clone)]
pub struct PendingPatch {
    pub rest: Vec<Hop>,
    /// The path the author wrote. Forwarding shortens `rest` and leaves this, so two anim
    /// paths that fail on the same last segment stay two diagnostics.
    pub written: Vec<Hop>,
    pub origin_depth: u32,
    pub order: Order,
    pub named: Vec<(String, Value)>,
    pub off: bool,
    pub channel: Option<ChannelPatch>,
    /// Where E0340 is reported. A `play` reports at the play.
    pub report: Site,
    pub anim: Option<String>,
    pub play: Option<Site>,
}

/// Where a patch applies among the others from one depth. The prepass schedules entry-file
/// plays before anything is drawn, so those are ordered by position, not by when they were
/// scheduled.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Order {
    /// A play in the entry file: the line and copy index of each enclosing loop, then its line.
    Position(Vec<u32>),
    /// A static patch, or a play in a `def` body, in evaluation order.
    Evaluated(u32),
}

#[derive(Clone)]
pub struct ChannelPatch {
    pub key: String,
    pub absolute: bool,
    pub value: Value,
    pub live: bool,
    pub anim: String,
    pub patch_span: Span,
    pub patch_file: String,
}

/// Patches that survived entry into one scope.
pub struct Pending {
    items: Vec<PendingPatch>,
    reached: Vec<Cell<bool>>,
}

pub struct Arrival {
    pub own: Vec<PendingPatch>,
    pub forward: Vec<PendingPatch>,
}

/// Dedupes E0340 across copies and frames of one evaluation.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ReportKey {
    pub file: String,
    pub line: u32,
    pub col: u32,
    pub play_line: u32,
    pub play_col: u32,
    pub anim: String,
    pub path: String,
}

impl Pending {
    /// Checks each patch's first hop against `region`. A miss, or a hop through anything
    /// that is not an instance or a slot, is E0340 once per patch line (or once per play,
    /// anim, and path).
    pub fn enter(
        region: &Region,
        incoming: Vec<PendingPatch>,
        at_node: &str,
        reported: &mut BTreeSet<ReportKey>,
        out: &mut Vec<Diagnostic>,
    ) -> Pending {
        let mut items = Vec::new();
        for patch in incoming {
            if validate(&patch, region, at_node, reported, out) {
                items.push(patch);
            }
        }
        Pending {
            reached: vec![Cell::new(false); items.len()],
            items,
        }
    }

    pub fn empty() -> Pending {
        Pending {
            items: Vec::new(),
            reached: Vec::new(),
        }
    }

    pub fn items(&self) -> &[PendingPatch] {
        &self.items
    }

    /// Patches for this decl at `copies`. A following name is forwarded when the decl opens a scope.
    pub fn arrive(&self, decl: DeclId, region: &Region, copies: &[u32]) -> Arrival {
        let d = region.decl(decl);
        let mut own = Vec::new();
        let mut forward = Vec::new();
        for (patch, reached) in self.items.iter().zip(&self.reached) {
            if matches!(patch.rest.first(), Some(Hop::Star)) && region.is_private(d.name.as_str()) {
                continue;
            }
            let Some(rest) = take(patch, d, copies) else {
                continue;
            };
            if rest.is_empty() {
                reached.set(true);
                own.push(patch.clone());
            } else if d.kind.opens_scope() {
                reached.set(true);
                let mut next = patch.clone();
                next.rest = rest;
                forward.push(next);
            }
        }
        own.sort_by(|a, b| {
            a.origin_depth
                .cmp(&b.origin_depth)
                .then(a.order.cmp(&b.order))
        });
        Arrival { own, forward }
    }
}

fn take(patch: &PendingPatch, decl: &NameDecl, copies: &[u32]) -> Option<Vec<Hop>> {
    let loops = &decl.loops;
    match patch.rest.first() {
        Some(Hop::Star) => (decl.tree_parent.is_none() && decl.kind.has_node()).then(Vec::new),
        Some(Hop::Name(n)) if *n == decl.name => {
            let mut i = 1;
            let mut c = 0;
            while i < patch.rest.len()
                && let Hop::Index(ix) = patch.rest[i]
            {
                if c < loops.len() && copies.get(c) != Some(&ix) {
                    return None;
                }
                if c >= loops.len() {
                    break;
                }
                c += 1;
                i += 1;
            }
            Some(patch.rest[i..].to_vec())
        }
        _ => None,
    }
}

fn validate(
    patch: &PendingPatch,
    region: &Region,
    at_node: &str,
    reported: &mut BTreeSet<ReportKey>,
    out: &mut Vec<Diagnostic>,
) -> bool {
    let mut fail = |msg: String, notes: Vec<String>, fix: String| {
        let key = report_key(patch);
        if !reported.insert(key) {
            return false;
        }
        let mut d = Diagnostic::error("E0340", patch.report.span, msg).with_fix(fix);
        for n in notes {
            d = d.with_note(n);
        }
        out.push(d.in_file(&*patch.report.file).at_node(at_node));
        false
    };
    let Some(first) = patch.rest.first() else {
        return fail(
            "patch has no name".into(),
            Vec::new(),
            "write a name, as in `.eye`".into(),
        );
    };
    let shown = hop_text(&patch.written);
    match first {
        Hop::Star => {
            if patch.rest.len() != 1 {
                return fail(
                    "`*` is only a final hop".into(),
                    Vec::new(),
                    "move `*` to the end".into(),
                );
            }
            if region
                .decls
                .iter()
                .any(|d| d.tree_parent.is_none() && d.kind.has_node() && !region.is_private(d.name.as_str()))
            {
                return true;
            }
            let notes = vec![region.names_note()];
            let msg = match &patch.anim {
                Some(anim) => format!("`{anim}` patch `.{shown}` matches nothing"),
                None => format!("patch `.{shown}` matches nothing in {}", region.label),
            };
            fail(msg, notes, "name a part in this scope".into())
        }
        Hop::Index(_) => fail(
            format!("patch `.{shown}` does not start with a name"),
            Vec::new(),
            "write a name before the copy index".into(),
        ),
        Hop::Name(name) => {
            let ids = region.lookup(name.as_str());
            if ids.is_empty() {
                let (notes, fix) = if let Some(site) = region.anon_hint(name.as_str()) {
                    let verb = region
                        .anon
                        .iter()
                        .find(|(_, s)| s.span == site.span && s.file == site.file)
                        .map(|(v, _)| v.to_string())
                        .unwrap_or_else(|| name.to_string());
                    (
                        vec![format!(
                            "an unnamed `{verb}` is at {}:{}",
                            site.file, site.span.line
                        )],
                        format!("add name: at {}:{}", site.file, site.span.line),
                    )
                } else {
                    (
                        vec![region.names_note()],
                        "use one of the listed names".into(),
                    )
                };
                let msg = match &patch.anim {
                    Some(anim) => format!("`{anim}` patch `.{shown}` matches nothing"),
                    None => format!("patch `.{shown}` matches nothing in {}", region.label),
                };
                return fail(msg, notes, fix);
            }
            if region.is_private(name.as_str()) {
                let notes = vec![region.names_note()];
                let msg = match &patch.anim {
                    Some(anim) => format!(
                        "`{anim}` patch `.{shown}` names `{name}`, which is private to {}",
                        region.label
                    ),
                    None => format!(
                        "patch `.{shown}` names `{name}`, which is private to {}",
                        region.label
                    ),
                };
                return fail(
                    msg,
                    notes,
                    format!("drop the `_` from `{name}` in {} to make it public", region.label),
                );
            }
            let loops = ids
                .iter()
                .map(|&id| region.decl(id).loops.len())
                .max()
                .unwrap_or(0);
            let mut i = 1;
            let mut seen = 0;
            while i < patch.rest.len() && matches!(patch.rest[i], Hop::Index(_)) {
                if seen < loops {
                    seen += 1;
                    i += 1;
                } else {
                    break;
                }
            }
            if patch.rest[i..].is_empty() {
                if ids.iter().any(|&id| region.decl(id).kind == NameKind::Anchor)
                    && ids.iter().all(|&id| region.decl(id).kind == NameKind::Anchor)
                {
                    return fail(
                        format!("patch `.{shown}` names anchor `{name}`, which has no node"),
                        Vec::new(),
                        "patch a shape or a group".into(),
                    );
                }
                return true;
            }
            if patch.rest[i..]
                .iter()
                .any(|h| matches!(h, Hop::Star))
                && !matches!(patch.rest.last(), Some(Hop::Star))
            {
                return fail(
                    "`*` is only a final hop".into(),
                    Vec::new(),
                    "move `*` to the end".into(),
                );
            }
            let opens = ids
                .iter()
                .all(|&id| region.decl(id).kind.opens_scope());
            if opens {
                return true;
            }
            let msg = match &patch.anim {
                Some(anim) => format!(
                    "`{anim}` patch `.{shown}` hops through `{name}`, which is not an instance or a slot"
                ),
                None => format!(
                    "patch `.{shown}` hops through `{name}`, which is not an instance or a slot"
                ),
            };
            let fix = child_fix(&patch.rest[1..i], &patch.rest[i..], region, ids);
            fail(msg, Vec::new(), fix)
        }
    }
}

fn child_fix(consumed: &[Hop], rest: &[Hop], region: &Region, parents: &[DeclId]) -> String {
    if rest.len() == 1 && matches!(rest.first(), Some(Hop::Star)) {
        let mut kids = Vec::new();
        for id in region.decl_ids() {
            let decl = region.decl(id);
            if decl.kind.has_node() && decl.tree_parent.is_some_and(|p| parents.contains(&p)) {
                let name = decl.name.to_string();
                if !kids.contains(&name) {
                    kids.push(name);
                }
            }
        }
        let Some(first) = kids.first() else {
            return "name the part under this node".into();
        };
        let mut addr = format!(".{first}");
        for hop in consumed {
            addr.push('.');
            addr.push_str(&hop.to_string());
        }
        return format!("write {addr}");
    }
    match skip_hop(consumed, rest) {
        Some(addr) => format!("write .{addr}"),
        None => "write a name, as in `.eye`".into(),
    }
}

/// The address with one group hop dropped: `consumed` are the copy indices after the group's
/// name, and they move after the next name in `rest`. `None` when `rest` names nothing.
pub fn skip_hop(consumed: &[Hop], rest: &[Hop]) -> Option<String> {
    let mut indices: Vec<String> = consumed.iter().map(|h| h.to_string()).collect();
    let mut name = None;
    let mut after = Vec::new();
    for hop in rest {
        match hop {
            Hop::Index(i) if name.is_none() => indices.push(i.to_string()),
            Hop::Name(n) if name.is_none() => name = Some(n.to_string()),
            other => after.push(other.to_string()),
        }
    }
    let mut parts = vec![name?];
    parts.extend(indices);
    parts.extend(after);
    Some(parts.join("."))
}

fn drawn_copy(patch: &PendingPatch, names: &Names, owner: &Key) -> Option<String> {
    let Hop::Name(name) = patch.rest.first()? else {
        return None;
    };
    let entry = names
        .region
        .lookup(name.as_str())
        .iter()
        .find_map(|&id| names.drawn(id).into_iter().next())?;
    let key = &entry.record.borrow().key;
    Some(key.address_from(owner).unwrap_or_else(|| format!(".{key}")))
}

/// Reports one E0340 when a patch that survived entry matches no drawn copy.
pub fn report_misses(
    pending: &Pending,
    names: &Names,
    owner: &Key,
    at_node: &str,
    reported: &mut BTreeSet<ReportKey>,
    out: &mut Vec<Diagnostic>,
) {
    for (patch, reached) in pending.items.iter().zip(&pending.reached) {
        if reached.get() {
            continue;
        }
        let Some(addr) = drawn_copy(patch, names, owner) else {
            continue;
        };
        if !reported.insert(report_key(patch)) {
            continue;
        }
        let hops = hop_text(&patch.written);
        let msg = match &patch.anim {
            Some(anim) => format!("`{anim}` patch `.{hops}` matches no drawn copy"),
            None => format!("patch `.{hops}` matches no drawn copy"),
        };
        out.push(
            Diagnostic::error("E0340", patch.report.span, msg)
                .with_fix(format!("write {addr}"))
                .in_file(&*patch.report.file)
                .at_node(at_node),
        );
    }
}

/// A patch that landed on an anchor has no node to change.
pub fn reject_anchor_patch(
    patch: &PendingPatch,
    at_node: &str,
    reported: &mut BTreeSet<ReportKey>,
    out: &mut Vec<Diagnostic>,
) {
    if !reported.insert(report_key(patch)) {
        return;
    }
    let shown = hop_text(&patch.written);
    let name = match patch.rest.first() {
        Some(Hop::Name(n)) => n.to_string(),
        _ => shown.clone(),
    };
    out.push(
        Diagnostic::error(
            "E0340",
            patch.report.span,
            format!("patch `.{shown}` names anchor `{name}`, which has no node"),
        )
        .with_fix("patch a shape or a group")
        .in_file(&*patch.report.file)
        .at_node(at_node),
    );
}

fn report_key(patch: &PendingPatch) -> ReportKey {
    let (play_line, play_col) = patch
        .play
        .as_ref()
        .map(|s| (s.span.line, s.span.col))
        .unwrap_or((0, 0));
    ReportKey {
        file: patch.report.file.to_string(),
        line: patch.report.span.line,
        col: patch.report.span.col,
        play_line,
        play_col,
        anim: patch.anim.clone().unwrap_or_default(),
        path: hop_text(&patch.written),
    }
}

pub fn host_part(host: &Names, name: &str) -> Result<(), Vec<String>> {
    if host.region.lookup(name).is_empty() {
        Err(host.region.names().iter().map(|n| n.to_string()).collect())
    } else {
        Ok(())
    }
}

/// Host scope a `needs:` name re-resolves in. The name must be declared there.
pub fn host_import(
    host: &Rc<RefCell<Names>>,
    name: &str,
) -> Option<(Name, Rc<RefCell<Names>>)> {
    let parsed = Name::parse(name)?;
    host_part(&host.borrow(), name).ok()?;
    Some((parsed, host.clone()))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn ids(src: &str) -> (ModuleIds, Vec<Diagnostic>) {
        let parsed = artscript_syntax::parse(src).unwrap_or_else(|e| panic!("{e:?}"));
        let mut out = Vec::new();
        let ids = ModuleIds::build("art/t.art", &parsed, &BTreeSet::new(), &mut out);
        (ids, out)
    }

    fn codes(ds: &[Diagnostic]) -> Vec<(String, u32)> {
        ds.iter().map(|d| (d.code.clone(), d.span.line)).collect()
    }

    fn name(s: &str) -> Name {
        Name::parse(s).unwrap()
    }

    fn line(l: u32) -> Span {
        Span { line: l, col: 1 }
    }

    fn anon_at(ids: &ModuleIds, l: u32) -> &Anon {
        ids.anon
            .iter()
            .find(|(s, _)| s.line == l)
            .map(|(_, a)| a)
            .unwrap_or_else(|| panic!("no draw at line {l}"))
    }

    const TARGET: &str = "@target t.png 100x100\n";

    #[test]
    fn identity_e0333_is_at_the_second_name_with_the_first_line_in_a_note() {
        let src = format!(
            "{TARGET}layer nuki\n  group name:beam\n    curve 0,0 10,0 name:strut\n    curve 0,0 10,10 name:beam\n"
        );
        let (_, ds) = ids(&src);
        assert_eq!(codes(&ds), [("E0333".into(), 5)]);
        assert_eq!(ds[0].file, "art/t.art");
        assert_eq!(ds[0].notes, ["first named at line 3"]);
        assert_eq!(ds[0].fix.as_deref(), Some("rename one of them"));
    }

    #[test]
    fn identity_e0333_fires_for_a_statement_that_is_never_drawn() {
        let src =
            format!("{TARGET}circle 4 name:dot\nif 0\n  each i in 0..0\n    rect 2x2 name:dot\n");
        let (_, ds) = ids(&src);
        assert_eq!(codes(&ds), [("E0333".into(), 5)]);
    }

    #[test]
    fn identity_two_defs_and_two_if_arms_may_share_a_name() {
        let src = "def a\n  circle 4 name:lips\ndef b\n  rect 4x4 name:lips\n";
        assert!(ids(src).1.is_empty());
        let src = format!("{TARGET}if 1\n  circle 4 name:lips\nelse\n  rect 4x4 name:lips\n");
        assert!(ids(&src).1.is_empty());
        let src = format!("{TARGET}if 1\n  circle 4 name:lips\nif 0\n  rect 4x4 name:lips\n");
        assert_eq!(codes(&ids(&src).1), [("E0333".into(), 5)]);
    }

    #[test]
    fn a_shape_word_in_the_component_set_opens_a_scope() {
        let src = "@target t.png 40x40\ndef circle\n  slot hole\n    rect 4 name:gem\ncircle name:c\n  hole:\n    rect 4 name:gem\n";
        let parsed = artscript_syntax::parse(src).unwrap_or_else(|e| panic!("{e:?}"));
        let mut bare_out = Vec::new();
        let bare = ModuleIds::build("art/t.art", &parsed, &BTreeSet::new(), &mut bare_out);
        let bare_id = bare.file().lookup("c")[0];
        assert!(!bare.file().decl(bare_id).kind.opens_scope());
        assert!(bare.region(Span { line: 6, col: 3 }).is_none());

        let mut out = Vec::new();
        let mut components = BTreeSet::new();
        components.insert("circle".to_string());
        let ids = ModuleIds::build("art/t.art", &parsed, &components, &mut out);
        assert!(out.is_empty(), "{out:?}");
        let id = ids.file().lookup("c")[0];
        assert!(ids.file().decl(id).kind.opens_scope());
        let fill = ids.region(Span { line: 6, col: 3 }).expect("fill region");
        assert_eq!(fill.kind, RegionKind::Slot);
        assert_eq!(fill.names(), [&name("gem")]);
    }

    #[test]
    fn an_empty_slot_fallback_is_an_empty_region() {
        let src = format!("{TARGET}def box\n  slot hole\nbox name:box\n");
        let (ids, ds) = ids(&src);
        assert!(ds.is_empty(), "{ds:?}");
        let region = ids
            .region(Span { line: 3, col: 3 })
            .expect("empty fallback region");
        assert!(region.is_empty());
        assert_eq!(region.kind, RegionKind::Slot);
    }

    #[test]
    fn identity_slot_content_is_its_own_region() {
        let src = format!(
            "{TARGET}_moss name:a\n  form:\n    curve 0,0 1,1 2,0 closed name:form\n_moss name:b\n  form:\n    curve 0,0 1,1 2,0 closed name:form\n"
        );
        let (ids, ds) = ids(&src);
        assert!(ds.is_empty(), "{ds:?}");
        let fill = ids.region(Span { line: 3, col: 3 }).expect("fill region");
        assert_eq!(fill.kind, RegionKind::Slot);
        assert_eq!(fill.names(), [&name("form")]);
        assert_eq!(ids.file().names(), [&name("a"), &name("b")]);
    }

    #[test]
    fn identity_e0312_for_a_dynamic_name_and_for_two_names_on_one_node() {
        let src = format!("{TARGET}circle 4 name:x+1\nlayer hat name:cap\ncircle 2 name:\"eye\"\n");
        let (ids, ds) = ids(&src);
        assert_eq!(
            codes(&ds),
            [
                ("E0312".into(), 2),
                ("E0312".into(), 3),
                ("E0312".into(), 4)
            ]
        );
        assert!(ds.iter().all(|d| {
            d.fix
                .as_deref()
                .unwrap()
                .contains("single identifier `name:`")
        }));
        assert!(
            ds[1].message.contains("already has the name `hat`"),
            "{}",
            ds[1].message
        );
        let file = ids.file();
        assert_eq!(file.decl_at(line(2)), None);
        assert_eq!(file.decl(file.decl_at(line(3)).unwrap()).name, name("hat"));
    }

    #[test]
    fn identity_ordinals_count_one_verb_through_if_arms_and_loops_but_not_child_blocks() {
        let src = format!(
            "{TARGET}circle 1\nrect 1x1\ncircle 1 name:dot\nif 1\n  circle 1\nelse\n  circle 1\neach i in 0..3\n  circle 1\ngroup\n  circle 1\nm 4\n"
        );
        let (ids, _) = ids(&src);
        let got: Vec<(u32, String)> = [2, 3, 4, 6, 8, 10, 11, 12, 13]
            .iter()
            .map(|&l| {
                let a = anon_at(&ids, l);
                (l, format!("{}~{}", a.verb, a.ordinal))
            })
            .collect();
        assert_eq!(
            got,
            [
                (2, "circle~1".into()),
                (3, "rect~1".into()),
                (4, "circle~2".into()),
                (6, "circle~3".into()),
                (8, "circle~4".into()),
                (10, "circle~5".into()),
                (11, "group~1".into()),
                (12, "circle~1".into()),
                (13, "m~1".into()),
            ]
        );
    }

    #[test]
    fn identity_a_named_key_drops_groups_and_layers() {
        let src = format!(
            "{TARGET}group name:face\n  layer eyes\n    each s in [-1 1]\n      circle 4 name:eye\n  circle 2 name:head\n  circle 1\n"
        );
        let (ids, ds) = ids(&src);
        assert!(ds.is_empty(), "{ds:?}");
        let file = ids.file();
        let head = file.decl(file.lookup("head")[0]);
        assert_eq!(head.site.span.line, 6);
        assert_eq!(file.decl(head.tree_parent.unwrap()).name, name("face"));
        let root = Key::root();
        assert_eq!(Key::named(&root, &head.name, &[]).to_string(), "head");
        let hero = Key::named(&root, &name("hero"), &[]);
        assert_eq!(Key::named(&hero, &head.name, &[]).to_string(), "hero.head");
        let eye = file.decl(file.lookup("eye")[0]);
        assert_eq!(eye.loops.len(), 1);
        assert_eq!(file.decl(eye.tree_parent.unwrap()).name, name("eyes"));
        assert_eq!(Key::named(&root, &eye.name, &[1]).to_string(), "eye.1");
        let face = Key::named(&root, &name("face"), &[]);
        let dot = Key::anon(&face, anon_at(&ids, 7), &[]);
        assert_eq!(dot.to_string(), "face.circle~2");
        assert!(!dot.is_stable());
        assert_eq!(dot.address_from(&root), None);
        assert_eq!(
            Key::named(&hero, &head.name, &[])
                .address_from(&root)
                .as_deref(),
            Some(".hero.head")
        );
        assert_eq!(
            Key::named(&hero, &head.name, &[])
                .address_from(&hero)
                .as_deref(),
            Some(".head")
        );
        assert_eq!(face.mask().to_string(), "face.mask~1");
    }

    #[test]
    fn identity_seed_hashes_the_same_bytes_as_the_path_seeds_before_it() {
        let mut uses = PositionalUses::default();
        let file = "art/t.art";
        let key = Key::named(
            &Key::named(&Key::root(), &name("hero"), &[]),
            &name("eye"),
            &[2],
        );
        let old = |base: &str, path: &str| node_seed(base, path);
        let mut s = |k: &Key| seed(file, k, None, &mut uses);
        assert_eq!(s(&key.draw(2)), old("art/t.art\0hero.eye.2", "2"));
        assert_eq!(s(&key.blob()), old("art/t.art\0hero.eye.2", "blob"));
        assert_eq!(s(&key.jitter()), old("art/t.art", "hero.eye.2#jitter"));
        assert_eq!(s(&key.grain()), old("art/t.art", "hero.eye.2#grain"));
        assert_eq!(
            s(&key.at_stmt(&[0, 4], "let y~1").draw(1)),
            old("art/t.art\0hero.eye.2#0#4\0let y~1", "1")
        );
        assert_eq!(
            s(&Key::root().at_stmt(&[], "if~2").draw(1)),
            old("art/t.art\0\0if~2", "1")
        );
        assert_eq!(
            s(&Key::binding("glint").draw(1)),
            old("art/t.art\0glint", "1")
        );
        assert_eq!(uses.sites().count(), 0);
        assert_eq!(uses.seeded().iter().collect::<Vec<_>>(), ["hero.eye.2"]);
    }

    #[test]
    fn identity_an_unstable_seed_records_each_unnamed_statement_once() {
        let src = format!("{TARGET}layer rock\n  _speckle 4\n");
        let (ids, _) = ids(&src);
        let rock = Key::named(&Key::root(), &name("rock"), &[]);
        let call = Key::anon(&rock, anon_at(&ids, 3), &[]);
        let circle = Anon {
            verb: "circle".into(),
            ordinal: 1,
            site: Site::new("lib/s.art", line(4)),
        };
        let mut uses = PositionalUses::default();
        for i in 0..3 {
            let key = Key::anon(&call, &circle, &[i]);
            seed("art/t.art", &key.draw(1), None, &mut uses);
        }
        let sites: Vec<(u32, &str, &str)> = uses
            .sites()
            .map(|(s, p)| (s.span.line, &*p.verb, p.statement.as_str()))
            .collect();
        assert_eq!(
            sites,
            [
                (3, "_speckle", "rock._speckle~1"),
                (4, "circle", "rock._speckle~1.circle~1")
            ]
        );
    }
}
