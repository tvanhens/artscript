//! Stable sibling indices and patch-path matching. The index counts statements, including
//! both arms of an `if` and the body of an `each`, and it does not walk into a node's own block.

use std::collections::HashMap;

use artscript_syntax::ast::{Expr, Named, Span, Stmt, StmtKind};

pub fn occurrences(stmts: &[Stmt]) -> HashMap<Span, usize> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut map = HashMap::new();
    walk(stmts, &mut counts, &mut map);
    map
}

fn walk(stmts: &[Stmt], counts: &mut HashMap<String, usize>, map: &mut HashMap<Span, usize>) {
    for st in stmts {
        match &st.kind {
            StmtKind::If {
                then, otherwise, ..
            } => {
                walk(then, counts, map);
                walk(otherwise, counts, map);
            }
            StmtKind::Each { body, .. } => walk(body, counts, map),
            StmtKind::Node {
                verb, args, named, ..
            } => {
                if let Some(base) = static_base(verb, args, named) {
                    let n = counts.entry(base).or_insert(0);
                    *n += 1;
                    map.insert(st.span, *n);
                }
            }
            _ => {}
        }
    }
}

pub fn static_base(verb: &str, args: &[Expr], named: &[Named]) -> Option<String> {
    if matches!(
        verb,
        "fill" | "stroke" | "mask" | "fx" | "anchor" | "canvas"
    ) {
        return None;
    }
    if let Some(name) = named
        .iter()
        .find(|n| n.key == "name")
        .and_then(|n| ident(&n.value))
    {
        return Some(name.to_string());
    }
    if verb == "slot" {
        return args.first().and_then(ident).map(str::to_string);
    }
    if matches!(verb, "layer" | "group")
        && let Some(name) = args.first().and_then(ident)
    {
        return Some(name.to_string());
    }
    Some(verb.to_string())
}

fn ident(e: &Expr) -> Option<&str> {
    match e {
        Expr::Ident(s) => Some(s),
        _ => None,
    }
}

/// `owner` is the instance path, or empty at the top of an entry file. `*` matches one segment.
pub fn path_match(full: &str, owner: &str, pattern: &[String]) -> bool {
    let rel = if owner.is_empty() {
        full
    } else {
        let prefix = format!("{owner}.");
        match full.strip_prefix(&prefix) {
            Some(rest) => rest,
            None => return false,
        }
    };
    let segs: Vec<&str> = if rel.is_empty() {
        Vec::new()
    } else {
        rel.split('.').collect()
    };
    if segs.len() != pattern.len() {
        return false;
    }
    segs.iter()
        .zip(pattern)
        .all(|(seg, pat)| pat == "*" || pat == seg)
}

/// `abs(v)` on a transform channel is absolute mode. Everywhere else `abs` stays the math function.
pub fn absolute_channel<'a>(key: &str, value: &'a Expr) -> Option<&'a Expr> {
    if !matches!(key, "at" | "rot" | "scale") {
        return None;
    }
    match value {
        Expr::Call { func, args, named }
            if func == "abs" && args.len() == 1 && named.is_empty() =>
        {
            Some(&args[0])
        }
        _ => None,
    }
}

pub fn offset_channel(key: &str) -> bool {
    matches!(key, "at" | "rot" | "scale")
}
