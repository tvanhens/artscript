//! `artscript` command line: build, check, fmt, explain, init.

mod build;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use artscript_eval::{Project, find_root, load, plan, project::rel_path};
use artscript_syntax::ast::Span;
use artscript_syntax::{Diagnostic, Severity};

const USAGE: &str = "usage: artscript <command>

commands:
  build [--only GLOB] [--allow-warnings]   render every @target, write art-manifest.json
  check [--json]                           evaluate and run checks without rendering
  fmt                                      rewrite every .art file in canonical form
  explain <file>                           print the evaluated node tree of a file's first target
  init                                     create a starter project in the current directory

exit codes: 0 ok, 1 compile error, 2 assert or lint error, 3 warnings only";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let code = match args.first().map(String::as_str) {
        Some("build") => with_project(&cwd, |p| {
            let mut opts = build::Options::default();
            let mut it = args[1..].iter();
            while let Some(a) = it.next() {
                match a.as_str() {
                    "--allow-warnings" => opts.allow_warnings = true,
                    "--only" => opts.only = it.next().cloned(),
                    s if s.starts_with("--only=") => opts.only = Some(s["--only=".len()..].to_string()),
                    other => return usage_error(&format!("unknown build flag `{other}`")),
                }
            }
            build::build(p, &opts)
        }),
        Some("check") => with_project(&cwd, |p| check(p, args.iter().any(|a| a == "--json"))),
        Some("fmt") => with_project(&cwd, fmt),
        Some("explain") => match args.get(1) {
            Some(file) => with_project(&cwd, |p| explain(p, &cwd.join(file))),
            None => usage_error("`explain` needs a file"),
        },
        Some("init") => init(&cwd),
        Some("--version" | "-V") => {
            println!("artscript {}", artscript_eval::VERSION);
            0
        }
        Some("--help" | "-h" | "help") | None => {
            println!("{USAGE}");
            0
        }
        Some(other) => usage_error(&format!("unknown command `{other}`")),
    };
    ExitCode::from(code)
}

fn usage_error(msg: &str) -> u8 {
    eprintln!("error: {msg}\n\n{USAGE}");
    1
}

fn with_project(cwd: &Path, f: impl FnOnce(&Project) -> u8) -> u8 {
    let Some(root) = find_root(cwd) else {
        eprintln!(
            "{}",
            Diagnostic::error("E0200", Span::default(), "no artscript.toml in this directory or any parent")
                .with_fix("run `artscript init` to create a project here")
        );
        return 1;
    };
    match load(&root) {
        Ok(p) => f(&p),
        Err(d) => {
            eprintln!("{d}");
            1
        }
    }
}

/// Dedupes, orders and prints diagnostics; returns the exit code they imply.
pub(crate) fn report(mut diags: Vec<Diagnostic>, allow_warnings: bool, json: bool) -> u8 {
    let mut seen = std::collections::HashSet::new();
    diags.retain(|d| seen.insert(d.to_string()));
    diags.sort_by(|a, b| (&a.file, a.span.line, a.span.col).cmp(&(&b.file, b.span.line, b.span.col)));
    if json {
        println!("{}", serde_json::json!({ "diagnostics": diags }));
    } else {
        for d in &diags {
            eprintln!("{d}\n");
        }
    }
    exit_code(&diags, allow_warnings)
}

pub(crate) fn exit_code(diags: &[Diagnostic], allow_warnings: bool) -> u8 {
    let errors = || diags.iter().filter(|d| d.severity == Severity::Error);
    if errors().any(|d| d.code.starts_with('E')) {
        1
    } else if errors().next().is_some() {
        2
    } else if !allow_warnings && diags.iter().any(|d| d.severity == Severity::Warning) {
        3
    } else {
        0
    }
}

fn check(project: &Project, json: bool) -> u8 {
    let plan = plan(project);
    let mut diags = plan.diagnostics;
    for job in &plan.jobs {
        diags.extend(artscript_eval::evaluate(project, job).diagnostics);
    }
    report(diags, false, json)
}

fn fmt(project: &Project) -> u8 {
    let mut diags = Vec::new();
    for (rel, src) in &project.files {
        match artscript_syntax::format_source(src) {
            Ok(out) if &out != src => {
                if let Err(e) = fs::write(project.root.join(rel), out) {
                    diags.push(Diagnostic::error("E0200", Span::default(), format!("cannot write: {e}")).in_file(rel));
                } else {
                    println!("formatted {rel}");
                }
            }
            Ok(_) => {}
            Err(errs) => diags.extend(errs.into_iter().map(|d| d.in_file(rel))),
        }
    }
    report(diags, false, false)
}

fn explain(project: &Project, file: &Path) -> u8 {
    let file = fs::canonicalize(file).unwrap_or(file.to_path_buf());
    let root = fs::canonicalize(&project.root).unwrap_or(project.root.clone());
    let rel = rel_path(&root, &file);
    let plan = plan(project);
    let Some(job) = plan.jobs.iter().find(|j| j.source == rel) else {
        let own: Vec<Diagnostic> = plan.diagnostics.into_iter().filter(|d| d.file == rel).collect();
        if !own.is_empty() {
            return report(own, false, false);
        }
        eprintln!(
            "{}",
            Diagnostic::error("E0200", Span::default(), format!("`{rel}` has no @target to explain"))
                .with_note(format!("entry files: {}", plan.entries.join(", ")))
                .with_fix("pass a file that declares `@target`")
        );
        return 1;
    };
    let ev = artscript_eval::evaluate(project, job);
    if let Some(scene) = &ev.scene {
        print!("{}", artscript_eval::explain(scene));
    }
    report(ev.diagnostics, true, false)
}

const INIT_MANIFEST: &str = r#"[project]
name = "art"
out  = "assets"
lib  = ["lib"]
std  = "0.1"

[defaults]
palette = "palette/house"
scale   = [1]
bg      = "none"
profile = "auto"

[lint]
raw-color = "warn"
"#;

const INIT_PALETTE: &str = "palette house\n  sky ramp #7fb8e6 5\n  leaf ramp #6cbf6a 5\n  ink ramp #3b2a2a 5\n";

const INIT_ART: &str = "@target hello.png 512x512 scale:1,2\n\ncircle 160 fill:p.sky stroke:p.ink.line,6\n";

fn init(cwd: &Path) -> u8 {
    if cwd.join("artscript.toml").exists() {
        eprintln!(
            "{}",
            Diagnostic::error("E0200", Span::default(), "artscript.toml already exists here").with_fix("run `artscript build` instead")
        );
        return 1;
    }
    let files = [("artscript.toml", INIT_MANIFEST), ("lib/palette/house.art", INIT_PALETTE), ("art/hello.art", INIT_ART)];
    for (rel, body) in files {
        let path = cwd.join(rel);
        if path.exists() {
            continue;
        }
        if let Some(dir) = path.parent()
            && let Err(e) = fs::create_dir_all(dir).and_then(|_| fs::write(&path, body))
        {
            eprintln!("error: cannot write {rel}: {e}");
            return 1;
        }
        println!("created {rel}");
    }
    0
}

/// `*` matches within a path segment, `**` across segments, `?` one char.
pub(crate) fn glob_match(pat: &str, text: &str) -> bool {
    fn go(p: &[u8], t: &[u8]) -> bool {
        match p.first() {
            None => t.is_empty(),
            Some(b'*') if p.get(1) == Some(&b'*') => {
                let rest = p[2..].strip_prefix(b"/").unwrap_or(&p[2..]);
                (0..=t.len()).any(|i| go(rest, &t[i..]) || (i < t.len() && go(&p[2..], &t[i..])))
            }
            Some(b'*') => (0..=t.len()).take_while(|&i| i == 0 || t[i - 1] != b'/').any(|i| go(&p[1..], &t[i..])),
            Some(b'?') => !t.is_empty() && t[0] != b'/' && go(&p[1..], &t[1..]),
            Some(c) => t.first() == Some(c) && go(&p[1..], &t[1..]),
        }
    }
    go(pat.as_bytes(), text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob() {
        assert!(glob_match("produce/*.png", "produce/momo_peach.png"));
        assert!(!glob_match("*.png", "produce/momo_peach.png"));
        assert!(glob_match("**/*.png", "produce/momo_peach.png"));
        assert!(glob_match("**", "a/b/c"));
        assert!(glob_match("art/**/momo?peach.art", "art/produce/momo_peach.art"));
    }

    #[test]
    fn exit_codes() {
        let e = |code: &str, sev: Severity| Diagnostic { severity: sev, ..Diagnostic::error(code, Span::default(), "x") };
        assert_eq!(exit_code(&[], false), 0);
        assert_eq!(exit_code(&[e("E0301", Severity::Error), e("C0101", Severity::Error)], false), 1);
        assert_eq!(exit_code(&[e("C0101", Severity::Error)], false), 2);
        assert_eq!(exit_code(&[e("L0101", Severity::Error)], false), 2);
        assert_eq!(exit_code(&[e("C0201", Severity::Warning)], false), 3);
        assert_eq!(exit_code(&[e("C0201", Severity::Warning)], true), 0);
    }
}
