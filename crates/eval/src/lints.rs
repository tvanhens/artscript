use std::collections::{BTreeSet, HashMap, HashSet};

use ramose_ir::{Axis, MaskSource, Matrix, Node, NodeKind, Rect, WarpStage};
use ramose_syntax::ast::{
    BinOp, Decl, DeclKind, DefItem, DrawItem, Expr, File, Fx, Item, Mask, Named, Scene, SceneKind,
    Span, SwatchKind,
};
use ramose_syntax::{Diagnostic, Severity};

use crate::identity::{PositionalUses, Site};
use crate::place::Place;
use crate::project::{LintLevel, Manifest};
use crate::targets;
use crate::value::num;
use crate::warp::stage_text;

fn diag(level: LintLevel, code: &str, span: Span, msg: String) -> Option<Diagnostic> {
    let severity = match level {
        LintLevel::Off => return None,
        LintLevel::Info => Severity::Info,
        LintLevel::Warn => Severity::Warning,
        LintLevel::Error => Severity::Error,
    };
    Some(Diagnostic {
        severity,
        ..Diagnostic::error(code, span, msg)
    })
}

pub(crate) fn walk_expr(e: &Expr, f: &mut impl FnMut(&Expr)) {
    f(e);
    match e {
        Expr::Size(a, b) | Expr::Range(a, b) | Expr::Binary(_, a, b) | Expr::Index(a, b) => {
            walk_expr(a, f);
            walk_expr(b, f);
        }
        Expr::Member(a, _) | Expr::Unary(_, a) => walk_expr(a, f),
        Expr::Tuple(xs) | Expr::List(xs) => xs.iter().for_each(|x| walk_expr(x, f)),
        Expr::Each { iter, body, .. } => {
            walk_expr(iter, f);
            walk_expr(body, f);
        }
        Expr::Call { args, named, .. } => {
            args.iter().for_each(|x| walk_expr(x, f));
            named.iter().for_each(|n| walk_expr(&n.value, f));
        }
        Expr::If {
            cond,
            then,
            otherwise,
        } => {
            walk_expr(cond, f);
            walk_expr(then, f);
            walk_expr(otherwise, f);
        }
        _ => {}
    }
}

pub(crate) fn walk_scene<'a>(s: &'a Scene, f: &mut impl FnMut(&'a Scene)) {
    f(s);
    match &s.kind {
        SceneKind::If {
            then, otherwise, ..
        } => then
            .iter()
            .chain(otherwise.iter())
            .for_each(|c| walk_scene(c, f)),
        SceneKind::Each { body, .. } => body.iter().for_each(|c| walk_scene(c, f)),
        SceneKind::Slot { fallback, .. } => fallback.iter().for_each(|c| walk_scene(c, f)),
        SceneKind::Draw(d) => {
            for item in d.body.items() {
                match item {
                    DrawItem::Scene(c) => walk_scene(c, f),
                    DrawItem::SlotFill(fill) => fill.body.iter().for_each(|c| walk_scene(c, f)),
                    DrawItem::Mask {
                        mask: Mask::Block(body),
                        ..
                    } => body.iter().for_each(|c| walk_scene(c, f)),
                    DrawItem::Mask {
                        mask: Mask::Value(_),
                        ..
                    }
                    | DrawItem::Fx(_)
                    | DrawItem::Patch(_) => {}
                }
            }
        }
        SceneKind::Let { .. }
        | SceneKind::Check { .. }
        | SceneKind::Play { .. }
        | SceneKind::Fx(_) => {}
    }
}

fn all_scenes<'a>(items: impl IntoIterator<Item = &'a Item>, f: &mut impl FnMut(&'a Scene)) {
    for item in items {
        match item {
            Item::Scene(s) => walk_scene(s, f),
            Item::Decl(Decl {
                kind: DeclKind::Def { body, .. } | DeclKind::Stamp { body, .. },
                ..
            }) => body
                .iter()
                .filter_map(DefItem::scene)
                .for_each(|s| walk_scene(s, f)),
            Item::Decl(Decl {
                kind: DeclKind::Rule { body, .. },
                ..
            }) => body.iter().for_each(|s| walk_scene(s, f)),
            Item::Decl(_) => {}
        }
    }
}

pub(crate) type Exprs<'a> = Vec<(&'a Expr, Span)>;

fn named<'a>(ns: &'a [Named], out: &mut Exprs<'a>) {
    ns.iter().for_each(|n| out.push((&n.value, n.span)));
}

fn fx_exprs<'a>(fx: &'a Fx, out: &mut Exprs<'a>) {
    fx.args.iter().for_each(|a| out.push((a, fx.span)));
    named(&fx.named, out);
}

pub(crate) fn scene_line_and_direct_fx_mask_patch_exprs<'a>(s: &'a Scene, out: &mut Exprs<'a>) {
    match &s.kind {
        SceneKind::Let { value, .. } => out.push((value, s.span)),
        SceneKind::Check {
            expr, named: ns, ..
        } => {
            out.push((expr, s.span));
            named(ns, out);
        }
        SceneKind::If { cond, .. } => out.push((cond, s.span)),
        SceneKind::Each { iter, .. } => out.push((iter, s.span)),
        SceneKind::Slot { named: ns, .. } => named(ns, out),
        SceneKind::Play { start, .. } => {
            if let Some(e) = start {
                out.push((e, s.span));
            }
        }
        SceneKind::Fx(fx) => fx_exprs(fx, out),
        SceneKind::Draw(d) => {
            d.args.iter().for_each(|a| out.push((a, s.span)));
            named(&d.named, out);
            for item in d.body.items() {
                match item {
                    DrawItem::Fx(fx) => fx_exprs(fx, out),
                    DrawItem::Mask {
                        mask: Mask::Value(e),
                        span,
                    } => out.push((e, *span)),
                    DrawItem::Patch(p) => named(&p.named, out),
                    DrawItem::Scene(_) | DrawItem::SlotFill(_) | DrawItem::Mask { .. } => {}
                }
            }
        }
    }
}

fn file_exprs(file: &File, skip_palettes: bool) -> Exprs<'_> {
    let mut out = Vec::new();
    for d in file.decls() {
        match &d.kind {
            DeclKind::Palette { .. } if skip_palettes => {}
            DeclKind::Palette { body, .. } => {
                for sw in body {
                    match &sw.kind {
                        SwatchKind::Color(e) => out.push((e, sw.span)),
                        SwatchKind::Ramp { pos, hue } => {
                            pos.exprs().into_iter().for_each(|e| out.push((e, sw.span)));
                            if let Some(h) = hue {
                                out.push((h, sw.span));
                            }
                        }
                    }
                }
            }
            DeclKind::Style { named: ns, .. } | DeclKind::Light { named: ns, .. } => {
                named(ns, &mut out)
            }
            DeclKind::Alias { args, named: ns, .. } => {
                args.iter().for_each(|e| out.push((e, d.span)));
                named(ns, &mut out);
            }
            DeclKind::Finish { params, body, .. } => {
                named(params, &mut out);
                body.iter().for_each(|fx| fx_exprs(fx, &mut out));
            }
            DeclKind::Brush { params, .. } | DeclKind::Rule { params, .. } => {
                named(params, &mut out)
            }
            DeclKind::Fn { body, .. } => out.push((body, d.span)),
            DeclKind::Def { params, body, .. } | DeclKind::Stamp { params, body, .. } => {
                named(params, &mut out);
                body.iter()
                    .filter_map(DefItem::fx)
                    .for_each(|fx| fx_exprs(fx, &mut out));
            }
            DeclKind::Use { .. } => {}
            DeclKind::Anim { dur, body, .. } => {
                out.push((dur, d.span));
                for p in body {
                    named(&p.named, &mut out);
                }
            }
        }
    }
    all_scenes(&file.items, &mut |s| {
        scene_line_and_direct_fx_mask_patch_exprs(s, &mut out)
    });
    out
}

pub fn raw_color(file: &File, path: &str, manifest: &Manifest) -> Vec<Diagnostic> {
    let level = manifest.lint_level("raw-color");
    let mut out = Vec::new();
    for (e, span) in file_exprs(file, true) {
        walk_expr(e, &mut |x| {
            let lit = match x {
                Expr::Hex(h) => Some(format!("#{h}")),
                Expr::Call { func, .. } if func == "oklch" => Some("oklch(…)".to_string()),
                _ => None,
            };
            if let Some(lit) = lit
                && let Some(d) = diag(
                    level,
                    "L0101",
                    span,
                    format!("raw color {lit} outside a palette"),
                )
            {
                out.push(
                    d.in_file(path)
                        .with_fix("add it to a palette block and refer to it as `p.<name>`"),
                );
            }
        });
    }
    out
}

/// Names an entry file binds itself, whose value only evaluation knows.
fn local_names(file: &File, exprs: &Exprs<'_>) -> HashSet<String> {
    let mut out: HashSet<String> = file
        .directives
        .iter()
        .flat_map(|d| d.each.iter().map(|n| n.key.clone()))
        .collect();
    for d in file.decls() {
        if let DeclKind::Def { params, .. }
        | DeclKind::Stamp { params, .. }
        | DeclKind::Rule { params, .. }
        | DeclKind::Finish { params, .. }
        | DeclKind::Brush { params, .. } = &d.kind
        {
            out.extend(params.iter().map(|p| p.key.clone()));
        }
        if let DeclKind::Fn { params, .. } = &d.kind {
            out.extend(params.iter().cloned());
        }
    }
    all_scenes(&file.items, &mut |s| {
        if let SceneKind::Let { name, .. } | SceneKind::Each { var: name, .. } = &s.kind {
            out.insert(name.clone());
        }
    });
    for (e, _) in exprs {
        walk_expr(e, &mut |x| {
            if let Expr::Each { var, .. } = x {
                out.insert(var.clone());
            }
        });
    }
    out
}

/// `target == x` or `target != x` where `x` is a word or string that no `@target` in the entry
/// file labels. A name the file binds or imports (`bound`) is left alone.
pub fn unknown_target(
    file: &File,
    path: &str,
    manifest: &Manifest,
    bound: impl Fn(&str) -> bool,
) -> Vec<Diagnostic> {
    let level = manifest.lint_level("unknown-target");
    let mut labels = BTreeSet::new();
    let mut unlabeled = false;
    for d in file.directives.iter().filter(|d| d.name == "target") {
        match targets::label(d) {
            Some(l) => {
                labels.insert(l);
            }
            None => unlabeled = true,
        }
    }
    let (note, fix) = match labels.first() {
        None => (
            "no `@target` here sets `as:`, so `target` is always `none`".to_string(),
            "add `as:<label>` to a `@target`".to_string(),
        ),
        Some(first) => {
            let names: Vec<&str> = labels.iter().map(String::as_str).collect();
            let none = if unlabeled {
                ", and a target without `as:` makes it `none`"
            } else {
                ""
            };
            (
                format!("its targets are labeled {}{none}", names.join(", ")),
                format!("compare `target` with an `as:` label, such as `target == {first}`"),
            )
        }
    };
    let exprs = file_exprs(file, false);
    let local = local_names(file, &exprs);
    let is_target = |e: &Expr| matches!(e, Expr::Ident(n) if n == "target");
    let mut out = Vec::new();
    for (e, span) in &exprs {
        walk_expr(e, &mut |x| {
            let Expr::Binary(BinOp::Eq | BinOp::Ne, a, b) = x else {
                return;
            };
            let other = match (is_target(a), is_target(b)) {
                (true, false) => &**b,
                (false, true) => &**a,
                _ => return,
            };
            let (shown, possible) = match other {
                Expr::Str(s) => (format!("\"{s}\""), labels.contains(s)),
                Expr::Ident(w) if w == "none" => (w.clone(), unlabeled),
                Expr::Ident(w) if w == "t" || w == "clip" || local.contains(w) || bound(w) => {
                    return;
                }
                Expr::Ident(w) => (w.clone(), labels.contains(w)),
                _ => return,
            };
            if !possible
                && let Some(d) = diag(
                    level,
                    "L0106",
                    *span,
                    format!("`target` is never `{shown}` in this file"),
                )
            {
                out.push(
                    d.in_file(path)
                        .with_note(note.clone())
                        .with_fix(fix.clone()),
                );
            }
        });
    }
    out
}

/// `param == x` or `param != x` in a def body, where `param` is declared `one_of(...)` and `x` is
/// a word the list leaves out. A name the body binds, or another parameter, is left alone.
pub fn unknown_word(file: &File, path: &str, manifest: &Manifest) -> Vec<Diagnostic> {
    let level = manifest.lint_level("unknown-word");
    let mut out = Vec::new();
    for d in file.decls() {
        let DeclKind::Def { params, body, .. } = &d.kind else {
            continue;
        };
        let mut exprs = Vec::new();
        let mut scenes = Vec::new();
        body.iter().filter_map(DefItem::fx).for_each(|fx| fx_exprs(fx, &mut exprs));
        for s in body.iter().filter_map(DefItem::scene) {
            walk_scene(s, &mut |c| {
                scene_line_and_direct_fx_mask_patch_exprs(c, &mut exprs);
                scenes.push(c);
            });
        }
        let mut bound = HashSet::new();
        for s in &scenes {
            if let SceneKind::Let { name, .. } | SceneKind::Each { var: name, .. } = &s.kind {
                bound.insert(name.clone());
            }
        }
        for (e, _) in &exprs {
            walk_expr(e, &mut |x| {
                if let Expr::Each { var, .. } = x {
                    bound.insert(var.clone());
                }
            });
        }
        let declared: Vec<(&str, Vec<String>)> = params
            .iter()
            .filter(|p| !bound.contains(&p.key))
            .filter_map(|p| Some((p.key.as_str(), p.value.one_of_words()?.ok()?)))
            .collect();
        for (e, span) in &exprs {
            walk_expr(e, &mut |x| {
                let Expr::Binary(BinOp::Eq | BinOp::Ne, a, b) = x else {
                    return;
                };
                for (param, other) in [(&**a, &**b), (&**b, &**a)] {
                    let Expr::Ident(name) = param else { continue };
                    let Some((_, words)) = declared.iter().find(|(k, _)| k == name) else {
                        continue;
                    };
                    let Expr::Ident(w) = other else { continue };
                    let known = words.contains(w)
                        || bound.contains(w)
                        || params.iter().any(|p| &p.key == w);
                    if known {
                        continue;
                    }
                    let list = words.join(", ");
                    if let Some(d) = diag(level, "L0107", *span, format!("`{name}` is never `{w}`")) {
                        out.push(
                            d.in_file(path)
                                .with_note(format!("`{name}` is one of {list}"))
                                .with_fix(format!("compare `{name}` with one of {list}")),
                        );
                    }
                    break;
                }
            });
        }
    }
    out
}

/// Every identifier, function and verb a def or stamp body mentions, anywhere in it.
pub(crate) fn body_idents(body: &[DefItem]) -> BTreeSet<String> {
    let mut exprs = Vec::new();
    let mut scenes = Vec::new();
    body.iter().filter_map(DefItem::fx).for_each(|fx| fx_exprs(fx, &mut exprs));
    for s in body.iter().filter_map(DefItem::scene) {
        walk_scene(s, &mut |c| {
            scene_line_and_direct_fx_mask_patch_exprs(c, &mut exprs);
            scenes.push(c);
        });
    }
    let mut out = HashSet::new();
    idents(exprs, &scenes, &mut out);
    out.into_iter().collect()
}

fn idents(exprs: Exprs<'_>, scenes: &[&Scene], out: &mut HashSet<String>) {
    for (e, _) in exprs {
        walk_expr(e, &mut |x| match x {
            Expr::Ident(n) => {
                out.insert(n.clone());
            }
            Expr::Call { func, .. } => {
                out.insert(func.clone());
            }
            _ => {}
        });
    }
    for s in scenes {
        if let SceneKind::Play { name: Some(n), .. } = &s.kind {
            out.insert(n.clone());
        }
        if let SceneKind::Fx(fx) = &s.kind {
            out.insert(fx.op.clone());
        }
        if let SceneKind::Draw(d) = &s.kind {
            out.insert(d.verb.clone());
            if let Some(st) = d.named.iter().find(|n| n.key == "style")
                && let Expr::Ident(n) = &st.value
            {
                out.insert(n.clone());
            }
            for item in d.body.items() {
                if let DrawItem::Fx(fx) = item {
                    out.insert(fx.op.clone());
                }
            }
        }
    }
}

/// A library's public top-level `let` is an export, like a def, and is never reported.
pub fn unused(file: &File, path: &str, library: bool, manifest: &Manifest) -> Vec<Diagnostic> {
    let level = manifest.lint_level("unused");
    let mut scenes = Vec::new();
    all_scenes(&file.items, &mut |s| scenes.push(s));
    let mut used = HashSet::new();
    let mut exprs = file_exprs(file, false);
    for d in &file.directives {
        named(&d.named, &mut exprs);
    }
    idents(exprs, &scenes, &mut used);
    for d in file.decls() {
        match &d.kind {
            DeclKind::Def { body, .. } | DeclKind::Stamp { body, .. } => body.iter().filter_map(DefItem::fx).for_each(|fx| {
                used.insert(fx.op.clone());
            }),
            DeclKind::Palette {
                from: Some(parent), ..
            } => {
                used.insert(parent.clone());
            }
            DeclKind::Alias { target, .. } => {
                used.insert(target.clone());
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    let mut push = |span: Span, msg: String, fix: &str| {
        if let Some(d) = diag(level, "L0103", span, msg) {
            out.push(d.in_file(path).with_fix(fix));
        }
    };
    for d in file.decls() {
        match &d.kind {
            DeclKind::Use {
                path: p,
                alias,
                items,
            } => {
                if let Some(a) = alias
                    && !used.contains(a)
                {
                    push(
                        d.span,
                        format!("`use {p} as {a}` is never used"),
                        "remove the import",
                    );
                }
                for item in items {
                    if !used.contains(item) {
                        push(
                            d.span,
                            format!("imported `{item}` is never used"),
                            "remove it from the import list",
                        );
                    }
                }
            }
            DeclKind::Def { name, params, body } | DeclKind::Stamp { name, params, body } => {
                let mut exprs = Vec::new();
                let mut inner_scenes = Vec::new();
                body.iter()
                    .filter_map(DefItem::fx)
                    .for_each(|fx| fx_exprs(fx, &mut exprs));
                for s in body.iter().filter_map(DefItem::scene) {
                    walk_scene(s, &mut |c| {
                        scene_line_and_direct_fx_mask_patch_exprs(c, &mut exprs);
                        inner_scenes.push(c);
                    });
                }
                let mut inner = HashSet::new();
                idents(exprs, &inner_scenes, &mut inner);
                let placement = matches!(d.kind, DeclKind::Stamp { .. });
                for p in params.iter().filter(|p| {
                    p.key != "needs" && p.key != "size" && !(placement && matches!(p.key.as_str(), "spacing" | "count"))
                }) {
                    if !inner.contains(&p.key) {
                        push(
                            p.span,
                            format!("parameter `{}` of `{name}` is never used", p.key),
                            "use it in the body or remove it",
                        );
                    }
                }
            }
            DeclKind::Fn { name, params, body } => {
                let mut inner = HashSet::new();
                idents(vec![(body, d.span)], &[], &mut inner);
                for p in params.iter().filter(|p| !inner.contains(*p)) {
                    push(
                        d.span,
                        format!("parameter `{p}` of `{name}` is never used"),
                        "use it in the body or remove it",
                    );
                }
            }
            _ => {}
        }
    }
    let exported = |s: &Scene| {
        library
            && matches!(&s.kind, SceneKind::Let { name, .. } if !name.starts_with('_'))
            && file.scenes().any(|t| std::ptr::eq(t, s))
    };
    for s in &scenes {
        if let SceneKind::Let { name, .. } = &s.kind
            && !used.contains(name)
            && !exported(s)
        {
            push(
                s.span,
                format!("`let {name}` is never used"),
                "remove the binding",
            );
        }
    }
    out
}

/// Nodes whose drawn extent ([`Node::outer_bounds`], effects included) leaves the canvas. Each
/// shape not clipped by a parent shape is measured, cut to every `inside` clip and node mask on
/// its groups. A group or layer with effects is measured as well when none of its shapes left
/// the canvas, so one cut is reported once.
pub fn off_canvas(scene: &ramose_ir::Scene, manifest: &Manifest, file: &str) -> Vec<Diagnostic> {
    let level = manifest.lint_level("off-canvas");
    let canvas = Rect::from_wh(scene.width, scene.height).with_outset((1.0, 1.0));
    let mut out = Vec::new();
    type Hit<'n> = (String, String, Rect, Option<&'n str>);
    struct Walk<'n, 'f> {
        canvas: Rect,
        entry: &'f str,
        find: &'f dyn Fn(&str) -> Option<(&'n Node, Matrix)>,
        hits: Vec<Hit<'n>>,
    }
    impl<'n> Walk<'n, '_> {
        /// `kept` is what the ancestors' clips and masks keep, in canvas coordinates.
        fn node(
            &mut self,
            n: &'n Node,
            parent: &Place,
            kept: Option<Rect>,
            mut site: Option<&'n str>,
        ) -> bool {
            if source_span(&n.origin, "").0 == self.entry {
                site = Some(&n.origin);
            }
            match &n.kind {
                NodeKind::Shape(_) => self.measure(n, parent, kept, site),
                NodeKind::Group { children, .. } => {
                    let world = parent.inner_node(n);
                    let mut inner = kept;
                    let mut drawn = true;
                    for r in n.kept_bounds() {
                        let r = world.map_rect(r);
                        match &mut inner {
                            Some(k) => drawn &= k.intersect(r),
                            None => inner = Some(r),
                        }
                    }
                    let mut hit = false;
                    if drawn {
                        for c in children {
                            hit |= self.node(c, &world, inner, site);
                        }
                    }
                    hit || (!n.effects.is_empty() && self.measure(n, parent, kept, site))
                }
            }
        }

        fn measure(
            &mut self,
            n: &'n Node,
            parent: &Place,
            kept: Option<Rect>,
            site: Option<&'n str>,
        ) -> bool {
            let Some(b) = n.outer_bounds(&parent.flat(), self.find) else {
                return false;
            };
            let mut b = parent.map_rect(b);
            if let Some(k) = kept
                && !b.intersect(k)
            {
                return false;
            }
            let c = self.canvas;
            let inside =
                b.left >= c.left && b.top >= c.top && b.right <= c.right && b.bottom <= c.bottom;
            if !inside {
                self.hits.push((n.path.clone(), n.origin.clone(), b, site));
            }
            !inside
        }
    }
    let mut paths = HashMap::new();
    index(&scene.children, &Place::identity(), &mut paths);
    let find = |p: &str| paths.get(p).map(|(n, place)| (*n, place.flat()));
    let mut walk = Walk {
        canvas,
        entry: file,
        find: &find,
        hits: Vec::new(),
    };
    for c in &scene.children {
        walk.node(c, &Place::identity(), None, None);
    }
    let hits = walk.hits;
    for (path, origin, b, site) in hits {
        let (src, span) = source_span(&origin, file);
        let msg = if b.intersects(canvas) {
            "is clipped by the canvas edge"
        } else {
            "is entirely outside the canvas"
        };
        if let Some(d) = diag(
            level,
            "L0102",
            span,
            format!("`{path}` {msg}"),
        ) {
            let mut d = d
                .in_file(&src)
                .at_node(path)
                .with_note(format!(
                    "bounds {},{} {}x{}",
                    b.left.round(),
                    b.top.round(),
                    b.width().round(),
                    b.height().round()
                ))
                .with_fix("move or shrink it to fit the canvas, or clip it by nesting under a shape");
            if let Some(site) = site.filter(|_| src != file) {
                d = d.with_note(site_note(site));
            }
            out.push(d);
        }
    }
    out
}

/// The file and span a node's `origin` (`file:line:col`) names, or line 1 of `file` when it
/// names none.
fn source_span(origin: &str, file: &str) -> (String, Span) {
    match ramose_ir::parse_origin(origin) {
        Some((src, line, col)) => (src.to_string(), Span { line, col }),
        None => (file.to_string(), Span { line: 1, col: 1 }),
    }
}

/// The `in <entry>:<line>` note for the entry-file node `site` that reached a library's node.
fn site_note(site: &str) -> String {
    let (f, span) = source_span(site, "");
    format!("in {f}:{}", span.line)
}

/// Lines whose every node draws ([`Node::outer_bounds`]) wholly outside the shape it is nested
/// under or a clipping shape above it, so the line draws nothing. A loop whose copies only partly
/// land outside, the line's own copies or its parent's, is not reported. A hidden node's children
/// are not measured.
pub fn hidden_child(
    scene: &ramose_ir::Scene,
    manifest: &Manifest,
    file: &str,
) -> Vec<Diagnostic> {
    let level = manifest.lint_level("hidden-child");
    type Find<'n, 'f> = &'f dyn Fn(&str) -> Option<(&'n Node, Matrix)>;
    struct Hit<'n> {
        node: &'n Node,
        by: &'n str,
        bounds: Rect,
        site: Option<&'n str>,
    }
    /// The nodes one line drew from one entry-file site.
    #[derive(Default)]
    struct Line<'n> {
        drawn: usize,
        hidden: Vec<Hit<'n>>,
    }
    struct Walk<'n, 'f> {
        entry: &'f str,
        find: Find<'n, 'f>,
        /// The clipping shapes above the current node, innermost last, in canvas coordinates.
        clips: Vec<(Rect, &'n str)>,
        lines: Vec<Line<'n>>,
        by_key: HashMap<(&'n str, Option<&'n str>), usize>,
    }
    impl<'n> Walk<'n, '_> {
        fn node(&mut self, n: &'n Node, parent: &Place, mut site: Option<&'n str>) {
            if source_span(&n.origin, "").0 == self.entry {
                site = Some(&n.origin);
            }
            if !self.clips.is_empty()
                && let Some(b) = n.outer_bounds(&parent.flat(), self.find)
            {
                let bounds = parent.map_rect(b);
                let next = self.lines.len();
                let i = *self
                    .by_key
                    .entry((n.origin.as_str(), site))
                    .or_insert(next);
                if i == next {
                    self.lines.push(Line::default());
                }
                let line = &mut self.lines[i];
                line.drawn += 1;
                let hider = self
                    .clips
                    .iter()
                    .rev()
                    .find(|(r, _)| !bounds.intersects(*r));
                if let Some(&(_, by)) = hider {
                    line.hidden.push(Hit {
                        node: n,
                        by,
                        bounds,
                        site,
                    });
                    return;
                }
            }
            let world = parent.inner_node(n);
            let NodeKind::Shape(s) = &n.kind else {
                for c in n.descendants() {
                    self.node(c, &world, site);
                }
                return;
            };
            // Stamps are the shape's ink and spill past it; only the children are clipped.
            for c in &s.stamps {
                self.node(c, &world, site);
            }
            let r = world.map_rect(s.clip().compute_tight_bounds());
            self.clips.push((r, n.path.as_str()));
            for c in &s.children {
                self.node(c, &world, site);
            }
            self.clips.pop();
        }
    }
    let mut paths = HashMap::new();
    index(&scene.children, &Place::identity(), &mut paths);
    let find = |p: &str| paths.get(p).map(|(n, place)| (*n, place.flat()));
    let mut walk = Walk {
        entry: file,
        find: &find,
        clips: Vec::new(),
        lines: Vec::new(),
        by_key: HashMap::new(),
    };
    for c in &scene.children {
        walk.node(c, &Place::identity(), None);
    }
    let mut out = Vec::new();
    for line in walk.lines {
        if line.hidden.len() < line.drawn {
            continue;
        }
        let hit = &line.hidden[0];
        let (src, span) = source_span(&hit.node.origin, file);
        let path = &hit.node.path;
        let Some(d) = diag(
            level,
            "L0105",
            span,
            format!("`{path}` is entirely outside `{}`, which clips it", hit.by),
        ) else {
            continue;
        };
        let b = hit.bounds;
        let mut d = d
            .in_file(&src)
            .at_node(path)
            .with_note(format!(
                "bounds {},{} {}x{}",
                b.left.round(),
                b.top.round(),
                b.width().round(),
                b.height().round()
            ))
            .with_fix("move it inside the shape it is nested under, or draw it beside that shape");
        let mut also: Vec<&str> = Vec::new();
        for h in &line.hidden[1..] {
            if h.node.path != *path && !also.contains(&h.node.path.as_str()) {
                also.push(&h.node.path);
            }
        }
        if !also.is_empty() {
            d = d.with_note(format!("also in {}", also.join(", ")));
        }
        if let Some(site) = hit.site.filter(|_| src != file) {
            d = d.with_note(site_note(site));
        }
        out.push(d);
    }
    out
}

pub fn warp_cut(scene: &ramose_ir::Scene, manifest: &Manifest, file: &str) -> Vec<Diagnostic> {
    let level = manifest.lint_level("warp-cut");
    fn nodes<'n>(n: &'n Node, out: &mut Vec<&'n Node>) {
        out.push(n);
        for mask in n.masks() {
            if let MaskSource::Nodes(inner) = &mask.source {
                inner.iter().for_each(|c| nodes(c, out));
            }
        }
        n.descendants().for_each(|c| nodes(c, out));
    }
    let mut all = Vec::new();
    scene.children.iter().for_each(|c| nodes(c, &mut all));
    let mut out = Vec::new();
    for n in all {
        for stage in n.warp.stages() {
            let WarpStage::Cylinder(c) = &stage.kind else {
                continue;
            };
            let strips = stage.cut();
            if strips.is_empty() {
                continue;
            }
            let (hidden, reach) = match c.axis() {
                Axis::Y => (
                    strips.iter().map(Rect::width).sum::<f32>(),
                    stage.src.left.abs().max(stage.src.right.abs()),
                ),
                Axis::X => (
                    strips.iter().map(Rect::height).sum::<f32>(),
                    stage.src.top.abs().max(stage.src.bottom.abs()),
                ),
            };
            let fits = num(f64::from((reach * 2.0 / std::f32::consts::PI).ceil()));
            let (src, span) = source_span(&n.origin, file);
            let Some(d) = diag(
                level,
                "L0108",
                span,
                format!(
                    "`{}` hides {} units of `{}` past its visible half",
                    stage_text(&stage.kind),
                    num(f64::from(hidden.round())),
                    n.path
                ),
            ) else {
                continue;
            };
            let axis = match c.axis() {
                Axis::Y => "",
                Axis::X => " axis:x",
            };
            out.push(d.in_file(&src).at_node(&n.path).with_fix(format!(
                "a radius of {fits} shows all of it: `cylinder({fits}{axis})`"
            )));
        }
    }
    out
}

/// An anchor a warp hides has no canvas point, so the rig leaves it out.
pub fn hidden_anchors(hidden: &[(String, Site)], manifest: &Manifest) -> Vec<Diagnostic> {
    let level = manifest.lint_level("warp-cut");
    hidden
        .iter()
        .filter_map(|(label, site)| {
            let d = diag(
                level,
                "L0108",
                site.span,
                format!("anchor `{label}` is behind a warp, so the manifest leaves it out"),
            )?;
            Some(
                d.in_file(site.file.to_string())
                    .with_fix("move the anchor inside the warp's visible half, or widen the warp"),
            )
        })
        .collect()
}

fn index<'n>(nodes: &'n [Node], parent: &Place, out: &mut HashMap<&'n str, (&'n Node, Place)>) {
    for n in nodes {
        index_node(n, parent, out);
    }
}

fn index_node<'n>(n: &'n Node, parent: &Place, out: &mut HashMap<&'n str, (&'n Node, Place)>) {
    out.insert(n.path.as_str(), (n, parent.clone()));
    let world = parent.inner_node(n);
    if let Some(mask) = &n.mask
        && let MaskSource::Nodes(inner) = &mask.source
    {
        index(inner, &world, out);
    }
    for c in n.descendants() {
        index_node(c, &world, out);
    }
}

/// One warning per unnamed statement whose key fed a draw-site seed.
pub fn positional_seed(uses: &PositionalUses, manifest: &Manifest) -> Vec<Diagnostic> {
    let level = manifest.lint_level("positional-seed");
    let mut out = Vec::new();
    for (site, pos) in uses.sites() {
        let Some(d) = diag(
            level,
            "L0104",
            site.span,
            format!(
                "this `{}` has no name, and randomness is drawn from its position",
                pos.verb
            ),
        ) else {
            continue;
        };
        out.push(
            d.in_file(site.file.as_ref())
                .at_node(&pos.statement)
                .with_fix("add name:"),
        );
    }
    out
}
