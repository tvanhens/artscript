//! AST to scene IR: modules, palettes, styles, components, slots, overrides, checks.
// `Diagnostic` is the error type every layer shares and lives in artscript-syntax.
#![allow(clippy::result_large_err)]

mod builtins;
mod checks;
pub mod color;
mod eval;
mod explain;
mod geom;
mod lints;
mod paint;
pub mod project;
mod scope;
mod seed;
pub mod targets;
mod value;

use std::collections::BTreeSet;

use artscript_ir::Scene;
use artscript_syntax::Diagnostic;
use artscript_syntax::ast::StmtKind;

pub use explain::explain;
pub use project::{LintLevel, Manifest, Project, find_root, load};
pub use seed::fnv1a;
pub use targets::{Format, MatrixValue, OutputSpec, Plan, TargetJob, plan};

use crate::color::Rgba;
use crate::eval::Evaluator;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct Evaluation {
    /// `None` when the entry file could not be evaluated at all.
    pub scene: Option<Scene>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Evaluates one target into a scene, with every diagnostic, check failure and lint it produced.
pub fn evaluate(project: &Project, job: &TargetJob) -> Evaluation {
    let mut ev = Evaluator::new(project, job);
    let module = match ev.load_module(&job.source, true) {
        Ok(m) => m,
        Err(f) => {
            ev.report(f, &job.source, job.span, "");
            return Evaluation { scene: None, diagnostics: ev.diags };
        }
    };
    let file = ev.parsed[&job.source].clone();
    let mut sc = ev.top_scope(&module);
    let children = ev.eval_block_nodes(&file.stmts, &mut sc);
    let checks = sc.checks.clone();
    ev.run_checks(&checks);
    let background = match &job.bg {
        Some(e) => match ev.expr(e, &sc) {
            Ok(v) => v.color().map(|c| c.rgba),
            Err(f) => {
                ev.report(f, &job.source, job.span, "");
                None
            }
        },
        None => ev.background.or_else(|| project.manifest.bg.as_deref().and_then(Rgba::hex)),
    };
    let scene = Scene { width: job.size.0, height: job.size.1, background: background.map(Rgba::to_ir), children };
    let mut diagnostics = ev.diags;
    let mut files: Vec<&String> = ev.parsed.keys().collect();
    files.sort();
    for f in files {
        let parsed = &ev.parsed[f];
        diagnostics.extend(lints::raw_color(parsed, f, &project.manifest));
        diagnostics.extend(lints::unused(parsed, f, &project.manifest));
    }
    diagnostics.extend(lints::off_canvas(&scene, &project.manifest, &job.source));
    Evaluation { scene: Some(scene), diagnostics }
}

/// The entry file plus every library it transitively imports (and the default palette),
/// as project-relative paths. Used for cache keys.
pub fn dependencies(project: &Project, entry: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut stack = vec![entry.to_string()];
    if let Some(p) = project.manifest.palette.as_deref().and_then(|p| project.resolve_use(p)) {
        stack.push(p);
    }
    while let Some(f) = stack.pop() {
        if !seen.insert(f.clone()) {
            continue;
        }
        let Some(src) = project.files.get(&f) else { continue };
        let Ok(file) = artscript_syntax::parse(src) else { continue };
        for st in &file.stmts {
            if let StmtKind::Use { path, .. } = &st.kind {
                let rooted = format!("{path}.art");
                if let Some(t) = project.resolve_use(path).or(project.files.contains_key(&rooted).then_some(rooted)) {
                    stack.push(t);
                }
            }
        }
    }
    seen.into_iter().collect()
}

#[cfg(test)]
mod tests;
