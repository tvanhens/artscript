//! Node identity. A [`Key`] names a node, a [`Region`] holds the names one scope declares, and
//! [`seed`] hashes a key. The evaluator builds every key here and never formats a path itself.

use std::borrow::Borrow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::rc::Rc;

use artscript_syntax::ast::{
    DeclKind, DefItem, Draw, DrawItem, Expr, File, Mask, Scene, SceneKind, Span,
};
use artscript_syntax::{Diagnostic, format_expr};

use crate::seed::node_seed;

/// Verbs the language defines. Any other verb calls a def, so a name on it names an instance.
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

/// Names cannot hold `~`, so a mirror clone never collides with an original.
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

impl Borrow<str> for Name {
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

/// An unnamed statement's segment: the verb as written and its 1-based count among draws with
/// that verb in one block. `site` is fixed by the key prefix, verb, and count, so it never splits
/// two keys. L0104 reports there.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Anon {
    pub verb: Rc<str>,
    pub ordinal: u32,
    pub site: Site,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
enum Seg {
    Name(Name),
    /// One `each`, `repeat`, or `grid` iteration. Grid copies count row by row.
    Copy(u32),
    Anon(Anon),
    /// The parent of a draw's `mask` block. It prints `mask~1`, so a slot named `mask` stays apart.
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

/// What a seed hashes after the node key. These spellings are the bytes earlier builds hashed,
/// so a node whose key equals its old path keeps every seed.
#[derive(Clone, Default, PartialEq, Eq, Hash, Debug)]
struct Tail {
    binding: Option<Rc<str>>,
    line: Option<(Rc<[u32]>, u32)>,
    used: Option<Use>,
}

/// A node's identity. Names are unique per region, counts are unique per block, and copies add
/// an index, so two nodes in one scene never share a key. The scene root is the empty key.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Key {
    segs: Rc<[Seg]>,
    /// Each enclosing mirror clone adds one.
    reflected: u8,
    tail: Option<Rc<Tail>>,
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
            reflected: 0,
            tail: None,
        }
    }

    pub fn is_root(&self) -> bool {
        self.segs.is_empty() && self.reflected == 0
    }

    /// The owner scope's key, then `name`, then the index of every loop between the scope root
    /// and the statement. Groups, layers, shapes, and masks in between do not appear.
    pub fn named(owner: &Key, name: &Name, copies: &[u32]) -> Key {
        owner.extend(Seg::Name(name.clone()), copies)
    }

    /// The tree parent's key, then `verb~N`, then the loop indices since that parent.
    pub fn anon(parent: &Key, anon: &Anon, copies: &[u32]) -> Key {
        parent.extend(Seg::Anon(anon.clone()), copies)
    }

    /// The parent key of this draw's mask block, and the key of a `mask @ref` node.
    pub fn mask(&self) -> Key {
        self.extend(Seg::Mask, &[])
    }

    /// The key of this node's twin in a mirror clone.
    pub fn reflected(&self) -> Key {
        Key {
            reflected: self.reflected + 1,
            ..self.clone()
        }
    }

    fn extend(&self, seg: Seg, copies: &[u32]) -> Key {
        let mut segs = self.segs.to_vec();
        segs.push(seg);
        segs.extend(copies.iter().map(|&i| Seg::Copy(i)));
        Key {
            segs: segs.into(),
            reflected: self.reflected,
            tail: None,
        }
    }

    /// No `~` anywhere. A stable key, with a leading `.`, is a patch address from the root.
    pub fn is_stable(&self) -> bool {
        self.reflected == 0
            && self
                .segs
                .iter()
                .all(|s| matches!(s, Seg::Name(_) | Seg::Copy(_)))
    }

    /// The unnamed statements in this key, outermost first.
    pub fn positional(&self) -> impl Iterator<Item = &Anon> {
        self.segs.iter().filter_map(|s| match s {
            Seg::Anon(a) => Some(a),
            _ => None,
        })
    }

    /// `.a.b.0` from `scope` to this key, or `None` when the rest of the key is not stable.
    pub fn address_from(&self, scope: &Key) -> Option<String> {
        if self.reflected > 0 || scope.reflected > 0 {
            return None;
        }
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
    /// indices since this node.
    pub fn at_line(&self, copies: &[u32], line: u32) -> Key {
        self.with_tail(|t| t.line = Some((copies.into(), line)))
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

    /// The node key without seed tags.
    pub fn node(&self) -> Key {
        Key {
            tail: None,
            ..self.clone()
        }
    }

    fn seed_text(&self) -> String {
        let mut s = self.to_string();
        if let Some(t) = &self.tail {
            if let Some(b) = &t.binding {
                s.push_str(b);
            }
            if let Some((copies, line)) = &t.line {
                for c in copies.iter() {
                    s.push_str(&format!("#{c}"));
                }
                s.push_str(&format!("\0{line}"));
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
        f.write_str(&join(&self.segs))?;
        for _ in 0..self.reflected {
            f.write_str(REFLECTION)?;
        }
        Ok(())
    }
}

/// Appends the reflection marker to every key in a mirror clone, masks included.
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
    /// The key that drew the seed.
    pub key: String,
    /// Where the seed was drawn.
    pub at: Site,
}

/// Every seed records its key here. An unstable key records each unnamed statement in it once,
/// first use kept. `seeded` is the node keys that drew a seed, for `explain`.
#[derive(Default)]
pub struct PositionalUses {
    sites: BTreeMap<Site, Positional>,
    seeded: BTreeSet<String>,
}

impl PositionalUses {
    fn note(&mut self, key: &Key, at: Site) {
        if key.segs.is_empty() {
            return;
        }
        let text = key.to_string();
        for (i, seg) in key.segs.iter().enumerate() {
            if let Seg::Anon(a) = seg {
                self.sites
                    .entry(a.site.clone())
                    .or_insert_with(|| Positional {
                        verb: a.verb.clone(),
                        statement: join(&key.segs[..=i]),
                        key: text.clone(),
                        at: at.clone(),
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

/// `node_seed` over `file` and the key's seed text, plus `seed:`. When the key holds an unnamed
/// segment, `uses` records that statement for L0104. `site` is where the seed is drawn.
pub fn seed(file: &str, key: &Key, extra: u64, uses: &mut PositionalUses, site: Site) -> u64 {
    uses.note(key, site);
    node_seed(file, &key.seed_text(), extra)
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
}

#[derive(Clone, Debug)]
pub struct NameDecl {
    pub name: Name,
    pub site: Site,
    pub kind: NameKind,
    pub verb: Rc<str>,
    /// The `each`, `repeat`, and `grid` statements between the region root and this one. More
    /// than none makes a family.
    pub loops: Vec<Span>,
    /// `(if, else arm)` pairs, outermost first. Two decls share a name only when these diverge.
    arms: Vec<(Span, bool)>,
    /// The nearest named draw above this one in the region. A final `*` matches on it.
    pub tree_parent: Option<DeclId>,
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
    /// Unnamed draws in source order, for the `add name:` hint.
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

    /// The first unnamed statement whose verb `seg` spells, alone or with a count after it.
    pub fn anon_hint(&self, seg: &str) -> Option<&Site> {
        self.anon
            .iter()
            .find(|(verb, _)| {
                seg.strip_prefix(&**verb)
                    .is_some_and(|rest| rest.chars().all(|c| c.is_ascii_digit()))
            })
            .map(|(_, site)| site)
    }
}

fn exclusive(a: &[(Span, bool)], b: &[(Span, bool)]) -> bool {
    a.iter()
        .any(|(s, arm)| b.iter().any(|(t, other)| s == t && arm != other))
}

fn kind_of(verb: &str) -> NameKind {
    match verb {
        "anchor" => NameKind::Anchor,
        v if BUILTIN.contains(&v) => NameKind::Draw,
        _ => NameKind::Instance,
    }
}

/// Whether `verb` calls a def rather than naming a built-in.
pub fn calls_def(verb: &str) -> bool {
    !BUILTIN.contains(&verb)
}

struct Declarer<'a> {
    file: Rc<str>,
    region: Region,
    loops: Vec<Span>,
    arms: Vec<(Span, bool)>,
    parent: Option<DeclId>,
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
    nested: &mut BTreeMap<Span, Rc<Region>>,
    out: &mut Vec<Diagnostic>,
) -> Region {
    let mut d = Declarer {
        file: file.clone(),
        region: Region::new(kind, label),
        loops: Vec::new(),
        arms: Vec::new(),
        parent: None,
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
                    self.add(n, s.span, NameKind::Slot, "slot");
                }
                if !fallback.is_empty() {
                    let stmts: Vec<&Scene> = fallback.iter().collect();
                    let label = format!("the `{name}` fallback");
                    self.sub(s.span, label, &stmts);
                }
            }
            SceneKind::Draw(d) => self.draw(s.span, d),
            SceneKind::Let { .. } | SceneKind::Check { .. } | SceneKind::Play { .. } => {}
        }
    }

    fn sub(&mut self, at: Span, label: String, stmts: &[&Scene]) {
        let r = declare(
            &self.file,
            RegionKind::Slot,
            label,
            stmts,
            self.nested,
            self.out,
        );
        self.nested.insert(at, Rc::new(r));
    }

    fn draw(&mut self, span: Span, d: &Draw) {
        if d.verb == "canvas" {
            return;
        }
        let id = self
            .name_of(d)
            .map(|name| self.add(name, span, kind_of(&d.verb), &d.verb));
        if id.is_none() {
            let site = Site {
                file: self.file.clone(),
                span,
            };
            self.region.anon.push((d.verb.as_str().into(), site));
        }
        let call = calls_def(&d.verb);
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

    /// The first identifier `name:`, else a layer's, group's, or anchor's positional name.
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

    fn add(&mut self, name: Name, span: Span, kind: NameKind, verb: &str) -> DeclId {
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
            verb: verb.into(),
            loops: self.loops.clone(),
            arms: self.arms.clone(),
            tree_parent: self.parent,
        });
        self.region.by_name.entry(name).or_default().push(id);
        self.region.by_span.insert(span, id);
        id
    }
}

/// The `verb~N` of every draw under `block`, by statement span. Within one block it counts every
/// draw with the same written verb, named ones too, through both `if` arms and once per loop
/// body. Each child block counts from 1 on its own. It evaluates nothing, so an alias counts
/// under the word written.
pub fn ordinals(file: &Rc<str>, block: &[&Scene], out: &mut BTreeMap<Span, Anon>) {
    let mut children = Vec::new();
    count(file, block, &mut BTreeMap::new(), out, &mut children);
    for c in children {
        ordinals(file, &c, out);
    }
}

fn count<'s>(
    file: &Rc<str>,
    stmts: &[&'s Scene],
    counts: &mut BTreeMap<&'s str, u32>,
    out: &mut BTreeMap<Span, Anon>,
    children: &mut Vec<Vec<&'s Scene>>,
) {
    for s in stmts {
        match &s.kind {
            SceneKind::If {
                then, otherwise, ..
            } => {
                count(
                    file,
                    &then.iter().collect::<Vec<_>>(),
                    counts,
                    out,
                    children,
                );
                count(
                    file,
                    &otherwise.iter().collect::<Vec<_>>(),
                    counts,
                    out,
                    children,
                );
            }
            SceneKind::Each { body, .. } => {
                count(
                    file,
                    &body.iter().collect::<Vec<_>>(),
                    counts,
                    out,
                    children,
                );
            }
            SceneKind::Slot { fallback, .. } => children.push(fallback.iter().collect()),
            SceneKind::Draw(d) => {
                let n = counts.entry(d.verb.as_str()).or_insert(0);
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
            SceneKind::Let { .. } | SceneKind::Check { .. } | SceneKind::Play { .. } => {}
        }
    }
}

/// Identity facts for one loaded module, built once and shared by every instance.
pub struct ModuleIds {
    file: Rc<Region>,
    /// By the def's span, the fill's span, or the slot's span for a fallback.
    regions: BTreeMap<Span, Rc<Region>>,
    anon: BTreeMap<Span, Anon>,
}

impl ModuleIds {
    pub fn build(path: &str, parsed: &File, out: &mut Vec<Diagnostic>) -> ModuleIds {
        let file: Rc<str> = path.into();
        let mut regions = BTreeMap::new();
        let mut anon = BTreeMap::new();
        let top: Vec<&Scene> = parsed.scenes().collect();
        let file_region = declare(
            &file,
            RegionKind::File,
            "this file".into(),
            &top,
            &mut regions,
            out,
        );
        ordinals(&file, &top, &mut anon);
        for d in parsed.decls() {
            if let DeclKind::Def { name, body, .. } = &d.kind {
                let stmts: Vec<&Scene> = body.iter().filter_map(DefItem::scene).collect();
                let r = declare(
                    &file,
                    RegionKind::Def,
                    format!("`{name}`"),
                    &stmts,
                    &mut regions,
                    out,
                );
                regions.insert(d.span, Rc::new(r));
                ordinals(&file, &stmts, &mut anon);
            }
        }
        ModuleIds {
            file: Rc::new(file_region),
            regions,
            anon,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(src: &str) -> (ModuleIds, Vec<Diagnostic>) {
        let parsed = artscript_syntax::parse(src).unwrap_or_else(|e| panic!("{e:?}"));
        let mut out = Vec::new();
        let ids = ModuleIds::build("art/t.art", &parsed, &mut out);
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
        assert_eq!(
            Key::named(&root, &name("ear"), &[]).reflected().to_string(),
            "ear~m"
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
        let site = Site::new(file, line(9));
        let old = |base: &str, path: &str| node_seed(base, path, 3);
        let mut s = |k: &Key| seed(file, k, 3, &mut uses, site.clone());
        assert_eq!(s(&key.draw(2)), old("art/t.art\0hero.eye.2", "2"));
        assert_eq!(s(&key.blob()), old("art/t.art\0hero.eye.2", "blob"));
        assert_eq!(s(&key.jitter()), old("art/t.art", "hero.eye.2#jitter"));
        assert_eq!(s(&key.grain()), old("art/t.art", "hero.eye.2#grain"));
        assert_eq!(
            s(&key.at_line(&[0, 4], 12).draw(1)),
            old("art/t.art\0hero.eye.2#0#4\012", "1")
        );
        assert_eq!(
            s(&Key::root().at_line(&[], 7).draw(1)),
            old("art/t.art\0\07", "1")
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
            seed(
                "art/t.art",
                &key.draw(1),
                0,
                &mut uses,
                Site::new("lib/s.art", line(4)),
            );
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
