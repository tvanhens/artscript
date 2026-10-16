//! Taste lints: raw-color (L0101), off-canvas (L0102) and unused (L0103).

use std::collections::HashSet;

use artscript_ir::{Matrix, Node, NodeKind, Rect, Scene};
use artscript_syntax::ast::{Expr, File, Named, Span, Stmt, StmtKind};
use artscript_syntax::{Diagnostic, Severity};

use crate::project::{LintLevel, Manifest};

fn diag(level: LintLevel, code: &str, span: Span, msg: String) -> Option<Diagnostic> {
    let severity = match level {
        LintLevel::Off => return None,
        LintLevel::Info => Severity::Info,
        LintLevel::Warn => Severity::Warning,
        LintLevel::Error => Severity::Error,
    };
    Some(Diagnostic { severity, ..Diagnostic::error(code, span, msg) })
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
        Expr::If { cond, then, otherwise } => {
            walk_expr(cond, f);
            walk_expr(then, f);
            walk_expr(otherwise, f);
        }
        _ => {}
    }
}

/// Every expression of a statement with the span to report it at, plus nested statements.
fn stmt_exprs<'a>(st: &'a Stmt, out: &mut Vec<(&'a Expr, Span)>, skip_palettes: bool) {
    let named = |ns: &'a [Named], out: &mut Vec<(&'a Expr, Span)>| ns.iter().for_each(|n| out.push((&n.value, n.span)));
    match &st.kind {
        StmtKind::Palette { .. } if skip_palettes => {}
        StmtKind::Palette { body, .. } => body.iter().for_each(|s| stmt_exprs(s, out, skip_palettes)),
        StmtKind::Style { named: ns, .. } | StmtKind::Light { named: ns, .. } | StmtKind::Brush { named: ns, .. } => named(ns, out),
        StmtKind::Def { params, body, .. } | StmtKind::Rule { params, body, .. } => {
            named(params, out);
            body.iter().for_each(|s| stmt_exprs(s, out, skip_palettes));
        }
        StmtKind::Finish { params, body, .. } => {
            named(params, out);
            for line in body {
                line.args.iter().for_each(|a| out.push((a, line.span)));
                named(&line.named, out);
            }
        }
        StmtKind::Shader { params, .. } => named(params, out),
        StmtKind::Let { value, .. } => out.push((value, st.span)),
        StmtKind::Check { expr, named: ns, .. } => {
            out.push((expr, st.span));
            named(ns, out);
        }
        StmtKind::If { cond, then, otherwise } => {
            out.push((cond, st.span));
            then.iter().chain(otherwise).for_each(|s| stmt_exprs(s, out, skip_palettes));
        }
        StmtKind::Each { iter, body, .. } => {
            out.push((iter, st.span));
            body.iter().for_each(|s| stmt_exprs(s, out, skip_palettes));
        }
        StmtKind::Node { args, named: ns, body, .. } => {
            args.iter().for_each(|a| out.push((a, st.span)));
            named(ns, out);
            body.iter().for_each(|s| stmt_exprs(s, out, skip_palettes));
        }
        StmtKind::Fx(line) => {
            line.args.iter().for_each(|a| out.push((a, st.span)));
            named(&line.named, out);
        }
        StmtKind::SlotFill { body, .. } => body.iter().for_each(|s| stmt_exprs(s, out, skip_palettes)),
        StmtKind::Patch { named: ns, .. } => named(ns, out),
        StmtKind::Use { .. } => {}
    }
}

pub fn raw_color(file: &File, path: &str, manifest: &Manifest) -> Vec<Diagnostic> {
    let level = manifest.lint_level("raw-color", LintLevel::Warn);
    let mut exprs = Vec::new();
    file.stmts.iter().for_each(|s| stmt_exprs(s, &mut exprs, true));
    let mut out = Vec::new();
    for (e, span) in exprs {
        walk_expr(e, &mut |x| {
            let lit = match x {
                Expr::Hex(h) => Some(format!("#{h}")),
                Expr::Call { func, .. } if func == "oklch" => Some("oklch(…)".to_string()),
                _ => None,
            };
            if let Some(lit) = lit
                && let Some(d) = diag(level, "L0101", span, format!("raw color {lit} outside a palette"))
            {
                out.push(d.in_file(path).with_fix("add it to a palette block and refer to it as `p.<name>`"));
            }
        });
    }
    out
}

fn idents(stmts: &[Stmt], out: &mut HashSet<String>) {
    let mut exprs = Vec::new();
    stmts.iter().for_each(|s| stmt_exprs(s, &mut exprs, false));
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
    fn verbs(stmts: &[Stmt], out: &mut HashSet<String>) {
        for s in stmts {
            match &s.kind {
                StmtKind::Node { verb, body, named, .. } => {
                    out.insert(verb.clone());
                    if let Some(st) = named.iter().find(|n| n.key == "style")
                        && let Expr::Ident(n) = &st.value
                    {
                        out.insert(n.clone());
                    }
                    verbs(body, out);
                }
                StmtKind::Fx(line) => {
                    out.insert(line.op.clone());
                }
                StmtKind::Def { body, .. } | StmtKind::SlotFill { body, .. } | StmtKind::Each { body, .. } | StmtKind::Rule { body, .. } => {
                    verbs(body, out)
                }
                StmtKind::If { then, otherwise, .. } => {
                    verbs(then, out);
                    verbs(otherwise, out);
                }
                _ => {}
            }
        }
    }
    verbs(stmts, out);
}

pub fn unused(file: &File, path: &str, manifest: &Manifest) -> Vec<Diagnostic> {
    let level = manifest.lint_level("unused", LintLevel::Warn);
    let mut used = HashSet::new();
    idents(&file.stmts, &mut used);
    let mut out = Vec::new();
    let mut push = |span: Span, msg: String, fix: &str| {
        if let Some(d) = diag(level, "L0103", span, msg) {
            out.push(d.in_file(path).with_fix(fix));
        }
    };
    fn lets(stmts: &[Stmt], acc: &mut Vec<(String, Span)>) {
        for s in stmts {
            match &s.kind {
                StmtKind::Let { name, .. } => acc.push((name.clone(), s.span)),
                StmtKind::Node { body, .. } | StmtKind::Def { body, .. } | StmtKind::SlotFill { body, .. } | StmtKind::Each { body, .. } => lets(body, acc),
                StmtKind::If { then, otherwise, .. } => {
                    lets(then, acc);
                    lets(otherwise, acc);
                }
                _ => {}
            }
        }
    }
    for st in &file.stmts {
        match &st.kind {
            StmtKind::Use { path: p, alias, items } => {
                if let Some(a) = alias
                    && !used.contains(a)
                {
                    push(st.span, format!("`use {p} as {a}` is never used"), "remove the import");
                }
                for item in items {
                    if !used.contains(item) {
                        push(st.span, format!("imported `{item}` is never used"), "remove it from the import list");
                    }
                }
            }
            StmtKind::Def { name, params, body } => {
                let mut inner = HashSet::new();
                idents(body, &mut inner);
                for p in params.iter().filter(|p| p.key != "needs" && p.key != "size") {
                    if !inner.contains(&p.key) {
                        push(p.span, format!("parameter `{}` of `{name}` is never used", p.key), "use it in the body or remove it");
                    }
                }
            }
            _ => {}
        }
    }
    let mut ls = Vec::new();
    lets(&file.stmts, &mut ls);
    for (name, span) in ls {
        if !used.contains(&name) {
            push(span, format!("`let {name}` is never used"), "remove the binding");
        }
    }
    out
}

/// Shapes not clipped by a parent shape whose bounds leave the canvas.
pub fn off_canvas(scene: &Scene, manifest: &Manifest, file: &str) -> Vec<Diagnostic> {
    let level = manifest.lint_level("off-canvas", LintLevel::Warn);
    let canvas = Rect::from_wh(scene.width, scene.height).with_outset((1.0, 1.0));
    let mut out = Vec::new();
    fn walk(n: &Node, parent: Matrix, canvas: Rect, hits: &mut Vec<(String, String, Rect)>) {
        match &n.kind {
            NodeKind::Shape(_) => {
                if let Some(b) = n.bounds() {
                    let b = parent.map_rect(b).0;
                    if !(b.left >= canvas.left && b.top >= canvas.top && b.right <= canvas.right && b.bottom <= canvas.bottom) {
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
    scene.children.iter().for_each(|c| walk(c, Matrix::new_identity(), canvas, &mut hits));
    for (path, origin, b) in hits {
        let line = origin.rsplit(':').next().and_then(|l| l.parse().ok()).unwrap_or(1);
        let src = origin.rsplit_once(':').map(|(f, _)| f.to_string()).unwrap_or(file.to_string());
        let msg = if b.intersects(canvas) { "is clipped by the canvas edge" } else { "is entirely outside the canvas" };
        if let Some(d) = diag(level, "L0102", Span { line, col: 1 }, format!("`{path}` {msg}")) {
            out.push(
                d.in_file(src)
                    .at_node(path)
                    .with_note(format!("bounds {},{} {}x{}", b.left.round(), b.top.round(), b.width().round(), b.height().round()))
                    .with_fix("move or shrink it to fit the canvas, or clip it by nesting under a shape"),
            );
        }
    }
    out
}
