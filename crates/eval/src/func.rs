//! User functions: `fn solid(c r) = EXPR`.

use std::rc::Rc;

use artscript_syntax::ast::{DeclKind, Expr, File, Named, Span};

use crate::eval::{Evaluator, R, Scope, fail};
use crate::lints::walk_expr;
use crate::scope::{FnDef, Module};
use crate::value::Value;

impl Evaluator<'_> {
    pub(crate) fn declare_fn(&mut self, module: &Rc<Module>, name: &str, params: &[String], body: &Expr, span: Span) -> R<()> {
        let mut found = None;
        walk_expr(body, &mut |x| {
            if found.is_some() {
                return;
            }
            found = match x {
                Expr::Ref(segs) => Some(
                    fail("E0324", format!("fn `{name}` reads `@{}`", segs.join(".")))
                        .note("a fn body sees its arguments and its file's top-level names, not the drawn scene")
                        .fix("pass the measurement in as an argument"),
                ),
                Expr::Call { func, .. } if func == "rand" || func == "pick" => Some(
                    fail("E0324", format!("fn `{name}` calls `{func}`"))
                        .note("a random value is seeded by the draw site that asks for it, and a fn has none")
                        .fix("pass the random value in as an argument"),
                ),
                _ => None,
            };
        });
        if let Some(f) = found {
            return Err(f);
        }
        let def = FnDef {
            name: name.to_string(),
            params: params.to_vec(),
            body: body.clone(),
            module: module.path.clone(),
            span,
        };
        module.define(name, Value::Fn(Rc::new(def)));
        Ok(())
    }

    pub(crate) fn check_fn_cycles(&mut self, module: &Rc<Module>, file: &File) {
        let fns: Vec<(&str, &[String], &Expr, Span)> = file
            .decls()
            .filter_map(|d| match &d.kind {
                DeclKind::Fn { name, params, body }
                    if matches!(module.global(name), Some(Value::Fn(f)) if f.span == d.span) =>
                {
                    Some((name.as_str(), params.as_slice(), body, d.span))
                }
                _ => None,
            })
            .collect();
        let edges: Vec<Vec<usize>> = fns
            .iter()
            .map(|(_, params, body, _)| {
                let mut out: Vec<usize> = Vec::new();
                walk_expr(body, &mut |x| {
                    let n = match x {
                        Expr::Ident(n) | Expr::Call { func: n, .. } => n.as_str(),
                        _ => return,
                    };
                    if !params.iter().any(|p| p == n)
                        && let Some(j) = fns.iter().position(|f| f.0 == n)
                        && !out.contains(&j)
                    {
                        out.push(j);
                    }
                });
                out
            })
            .collect();
        for (i, (name, _, _, span)) in fns.iter().copied().enumerate() {
            let Some(path) = path_back_to(i, &edges) else {
                continue;
            };
            let shown: Vec<&str> = path.iter().map(|&j| fns[j].0).collect();
            let f = fail("E0325", format!("fn `{name}` calls itself"))
                .note(format!("through {}", shown.join(" -> ")))
                .fix("a fn cannot recurse; write the repetition as `each` or `repeat` at the call site");
            self.report(f, &module.path, span, "");
            module.globals.borrow_mut().remove(name);
            module.lost.borrow_mut().insert(name.to_string());
        }
    }

    pub(crate) fn call_fn(&mut self, f: &Rc<FnDef>, args: &[Expr], named: &[Named], sc: &Scope) -> R<Value> {
        let shape = format!("write `{}({})`", f.name, f.params.join(" "));
        if let Some(n) = named.first() {
            return Err(fail("E0320", format!("`{}` takes positional arguments only, got `{}:`", f.name, n.key)).fix(shape));
        }
        if args.len() != f.params.len() {
            let s = if f.params.len() == 1 { "" } else { "s" };
            return Err(fail(
                "E0320",
                format!("`{}` takes {} argument{s}, got {}", f.name, f.params.len(), args.len()),
            )
            .fix(shape));
        }
        let module = self.modules.get(&f.module).cloned().ok_or_else(|| fail("E0210", "fn module not loaded"))?;
        let mut env = Vec::with_capacity(args.len());
        for (p, e) in f.params.iter().zip(args) {
            env.push((p.clone(), self.expr(e, sc)?));
        }
        let mut fsc = sc.clone();
        fsc.module = module;
        fsc.env = env;
        fsc.typed_params = Rc::default();
        self.expr(&f.body, &fsc)
            .map_err(|e| e.note(format!("in fn `{}` ({}:{})", f.name, f.module, f.span.line)))
    }
}

fn path_back_to(start: usize, edges: &[Vec<usize>]) -> Option<Vec<usize>> {
    let mut path = vec![start];
    let mut seen = vec![false; edges.len()];
    fn go(at: usize, start: usize, edges: &[Vec<usize>], seen: &mut [bool], path: &mut Vec<usize>) -> bool {
        for &next in &edges[at] {
            if next == start {
                path.push(next);
                return true;
            }
            if !seen[next] {
                seen[next] = true;
                path.push(next);
                if go(next, start, edges, seen, path) {
                    return true;
                }
                path.pop();
            }
        }
        false
    }
    go(start, start, edges, &mut seen, &mut path).then_some(path)
}
