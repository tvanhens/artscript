use std::collections::HashMap;

use artscript_syntax::ast::{Expr, Named, Scene, SceneKind, Span};

pub fn occurrences<'a>(scenes: impl IntoIterator<Item = &'a Scene>) -> HashMap<Span, usize> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut map = HashMap::new();
    walk(scenes, &mut counts, &mut map);
    map
}

fn walk<'a>(
    scenes: impl IntoIterator<Item = &'a Scene>,
    counts: &mut HashMap<String, usize>,
    map: &mut HashMap<Span, usize>,
) {
    for scene in scenes {
        match &scene.kind {
            SceneKind::If {
                then, otherwise, ..
            } => {
                walk(then, counts, map);
                walk(otherwise, counts, map);
            }
            SceneKind::Each { body, .. } => walk(body, counts, map),
            SceneKind::Draw(draw) => {
                if let Some(base) = static_base(&draw.verb, &draw.args, &draw.named) {
                    let n = counts.entry(base).or_insert(0);
                    *n += 1;
                    map.insert(scene.span, *n);
                }
            }
            SceneKind::Slot { name, .. } => {
                let n = counts.entry(name.clone()).or_insert(0);
                *n += 1;
                map.insert(scene.span, *n);
            }
            SceneKind::Let { .. } | SceneKind::Check { .. } | SceneKind::Play { .. } => {}
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

pub fn absolute_channel<'a>(key: &str, value: &'a Expr) -> Option<&'a Expr> {
    if !offset_channel(key) {
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
