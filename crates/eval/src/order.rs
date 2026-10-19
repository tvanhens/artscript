use std::collections::BTreeSet;

use artscript_syntax::ast::{Expr, Scene, SceneKind};

use crate::identity::{DeclId, Region, Site};
use crate::lints::{scene_line_and_direct_fx_mask_patch_exprs, walk_expr, walk_scene};

pub struct Step {
    pub stmt: usize,
    /// Names this statement may read that cannot be drawn before it: the reads form a cycle.
    pub blocked: Vec<Site>,
}

pub fn declared_in(st: &Scene, region: &Region) -> Vec<DeclId> {
    let mut out = Vec::new();
    walk_scene(st, &mut |s| out.extend(region.decl_at(s.span)));
    out
}

/// Source order, except that a statement runs after every sibling whose name it references and
/// after every earlier `let` whose name it mentions. The reads are taken from the source, so a
/// reference in an untaken arm still orders.
pub fn plan(stmts: &[&Scene], region: &Region) -> Vec<Step> {
    let n = stmts.len();
    let owned: Vec<Vec<DeclId>> = stmts.iter().map(|s| declared_in(s, region)).collect();
    let owner = |id: DeclId| owned.iter().position(|ds| ds.contains(&id));
    let lets: Vec<(usize, &str)> = stmts
        .iter()
        .enumerate()
        .filter_map(|(i, s)| match &s.kind {
            SceneKind::Let { name, .. } => Some((i, name.as_str())),
            _ => None,
        })
        .collect();
    let earlier_let = |i: usize, name: &str| {
        lets.iter()
            .filter(move |(j, n)| *j < i && *n == name)
            .map(|(j, _)| *j)
            .collect::<Vec<_>>()
    };
    let deps: Vec<BTreeSet<usize>> = stmts
        .iter()
        .enumerate()
        .map(|(i, st)| {
            let mut deps = BTreeSet::new();
            let mut exprs = Vec::new();
            walk_scene(st, &mut |s| {
                scene_line_and_direct_fx_mask_patch_exprs(s, &mut exprs)
            });
            for (e, _) in exprs {
                walk_expr(e, &mut |e| match e {
                    Expr::Ref(segs) => {
                        deps.extend(region.lookup(&segs[0]).iter().filter_map(|&id| owner(id)));
                    }
                    Expr::Ident(name) => deps.extend(earlier_let(i, name)),
                    _ => {}
                });
            }
            if let SceneKind::Let { name, .. } = &st.kind {
                deps.extend(earlier_let(i, name));
            }
            deps.remove(&i);
            deps
        })
        .collect();
    let mut done = vec![false; n];
    let mut steps = Vec::with_capacity(n);
    while steps.len() < n {
        let ready = (0..n).find(|&i| !done[i] && deps[i].iter().all(|&j| done[j]));
        let (stmt, blocked) = match ready {
            Some(i) => (i, Vec::new()),
            None => {
                let reach: Vec<BTreeSet<usize>> =
                    (0..n).map(|i| reachable(i, &deps, &done)).collect();
                let i = (0..n)
                    .find(|&i| !done[i] && reach[i].contains(&i))
                    .expect("a stuck plan has a cycle");
                let blocked = reach[i]
                    .iter()
                    .filter(|&&j| j != i && reach[j].contains(&i))
                    .flat_map(|&j| owned[j].iter().map(|&d| region.decl(d).site.clone()))
                    .collect();
                (i, blocked)
            }
        };
        done[stmt] = true;
        steps.push(Step { stmt, blocked });
    }
    steps
}

/// The statements that have not run and that `from` waits on, directly or through others.
fn reachable(from: usize, deps: &[BTreeSet<usize>], done: &[bool]) -> BTreeSet<usize> {
    let mut seen = BTreeSet::new();
    let mut todo: Vec<usize> = deps[from].iter().copied().collect();
    while let Some(j) = todo.pop() {
        if !done[j] && seen.insert(j) {
            todo.extend(deps[j].iter().copied());
        }
    }
    seen
}
