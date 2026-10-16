//! Taste lints: raw-color (L0101), off-canvas (L0102) and unused (L0103).

use std::collections::HashSet;

use artscript_ir::{Matrix, Node, NodeKind, Rect};
use artscript_syntax::ast::{
    Decl, DeclKind, DefItem, DrawItem, Expr, File, Fx, Item, Mask, Named, Scene, SceneKind, Span,
    SwatchKind,
};
use artscript_syntax::{Diagnostic, Severity};

use crate::project::{LintLevel, Manifest};

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

fn walk_expr(e: &Expr, f: &mut impl FnMut(&Expr)) {
    f(e);
    match e {
        Expr::Size(a, b) | Expr::Range(a, b) | Expr::Binary(_, a, b) => {
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

fn walk_scene<'a>(s: &'a Scene, f: &mut impl FnMut(&'a Scene)) {
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
        SceneKind::Let { .. } | SceneKind::Check { .. } | SceneKind::Play { .. } => {}
    }
}

fn all_scenes<'a>(items: impl IntoIterator<Item = &'a Item>, f: &mut impl FnMut(&'a Scene)) {
    for item in items {
        match item {
            Item::Scene(s) => walk_scene(s, f),
            Item::Decl(Decl {
                kind: DeclKind::Def { body, .. },
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

type Exprs<'a> = Vec<(&'a Expr, Span)>;

fn named<'a>(ns: &'a [Named], out: &mut Exprs<'a>) {
    ns.iter().for_each(|n| out.push((&n.value, n.span)));
}

fn fx_exprs<'a>(fx: &'a Fx, out: &mut Exprs<'a>) {
    fx.args.iter().for_each(|a| out.push((a, fx.span)));
    named(&fx.named, out);
}

fn scene_line_and_direct_fx_mask_patch_exprs<'a>(s: &'a Scene, out: &mut Exprs<'a>) {
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
            DeclKind::Style { named: ns, .. }
            | DeclKind::Light { named: ns, .. }
            | DeclKind::Brush { named: ns, .. } => named(ns, &mut out),
            DeclKind::Finish { params, body, .. } => {
                named(params, &mut out);
                body.iter().for_each(|fx| fx_exprs(fx, &mut out));
            }
            DeclKind::Shader { params, .. } | DeclKind::Rule { params, .. } => {
                named(params, &mut out)
            }
            DeclKind::Def { params, body, .. } => {
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
    let level = manifest.lint_level("raw-color", LintLevel::Warn);
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

fn idents(exprs: Exprs<'_>, scenes: &[&Scene], out: &mut HashSet<String>) {
    for (e, _) in exprs {
        walk_expr(e, &mut |x| match x {
            Expr::Ident(n) => {
                out.insert(n.clone());
            }
            Expr::Call { func, .. } => {
                out.insert(func.clone());
            }
            Expr::Member(_, seg) => {
                if let Some(v) = seg.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                    out.insert(v.to_string());
                }
            }
            _ => {}
        });
    }
    for s in scenes {
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

pub fn unused(file: &File, path: &str, manifest: &Manifest) -> Vec<Diagnostic> {
    let level = manifest.lint_level("unused", LintLevel::Warn);
    let mut scenes = Vec::new();
    all_scenes(&file.items, &mut |s| scenes.push(s));
    let mut used = HashSet::new();
    idents(file_exprs(file, false), &scenes, &mut used);
    for d in file.decls() {
        if let DeclKind::Def { body, .. } = &d.kind {
            body.iter().filter_map(DefItem::fx).for_each(|fx| {
                used.insert(fx.op.clone());
            });
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
            DeclKind::Def { name, params, body } => {
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
                for p in params
                    .iter()
                    .filter(|p| p.key != "needs" && p.key != "size")
                {
                    if !inner.contains(&p.key) {
                        push(
                            p.span,
                            format!("parameter `{}` of `{name}` is never used", p.key),
                            "use it in the body or remove it",
                        );
                    }
                }
            }
            _ => {}
        }
    }
    for s in &scenes {
        if let SceneKind::Let { name, .. } = &s.kind
            && !used.contains(name)
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

/// Shapes not clipped by a parent shape whose bounds leave the canvas.
pub fn off_canvas(scene: &artscript_ir::Scene, manifest: &Manifest, file: &str) -> Vec<Diagnostic> {
    let level = manifest.lint_level("off-canvas", LintLevel::Warn);
    let canvas = Rect::from_wh(scene.width, scene.height).with_outset((1.0, 1.0));
    let mut out = Vec::new();
    fn walk(n: &Node, parent: Matrix, canvas: Rect, hits: &mut Vec<(String, String, Rect)>) {
        match &n.kind {
            NodeKind::Shape(_) => {
                if let Some(b) = n.bounds() {
                    let b = parent.map_rect(b).0;
                    if !(b.left >= canvas.left
                        && b.top >= canvas.top
                        && b.right <= canvas.right
                        && b.bottom <= canvas.bottom)
                    {
                        hits.push((n.path.clone(), n.origin.clone(), b));
                    }
                }
            }
            NodeKind::Group { children, .. } => {
                if n.clip.is_some() {
                    return;
                }
                let world = parent * n.transform;
                children.iter().for_each(|c| walk(c, world, canvas, hits));
            }
        }
    }
    let mut hits = Vec::new();
    scene
        .children
        .iter()
        .for_each(|c| walk(c, Matrix::new_identity(), canvas, &mut hits));
    for (path, origin, b) in hits {
        let line = origin
            .rsplit(':')
            .next()
            .and_then(|l| l.parse().ok())
            .unwrap_or(1);
        let src = origin
            .rsplit_once(':')
            .map(|(f, _)| f.to_string())
            .unwrap_or(file.to_string());
        let msg = if b.intersects(canvas) {
            "is clipped by the canvas edge"
        } else {
            "is entirely outside the canvas"
        };
        if let Some(d) = diag(
            level,
            "L0102",
            Span { line, col: 1 },
            format!("`{path}` {msg}"),
        ) {
            out.push(
                d.in_file(src)
                    .at_node(path)
                    .with_note(format!(
                        "bounds {},{} {}x{}",
                        b.left.round(),
                        b.top.round(),
                        b.width().round(),
                        b.height().round()
                    ))
                    .with_fix(
                        "move or shrink it to fit the canvas, or clip it by nesting under a shape",
                    ),
            );
        }
    }
    out
}
