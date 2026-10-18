//! `artscript` command line: build, check, fmt, explain, init, docs.
//!
//! `docs` prints the core language card and an index of topics. `docs <topic>...` prints the card and those topic files.

mod build;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use artscript_eval::{Format, Project, find_root, load, plan, project::rel_path};
use artscript_syntax::ast::Span;
use artscript_syntax::{Diagnostic, Severity};

const USAGE: &str = "usage: artscript <command>

commands:
  build [--only GLOB]... [--allow-warnings] [--backend auto|gpu|cpu]
                                           render every @target, write art-manifest.json
  check [--json]                           evaluate stills and sample GIFs, without rendering
  fmt [--check] [FILE...]                  rewrite .art files. no files means the whole project. --check lists changes and writes nothing
  explain <file>                           print the evaluated node tree of a file's first target
  init                                     create a starter project in the current directory
  docs [TOPIC...]                          print the core language card and topic index, or the card and the named topics

exit codes: 0 ok, 1 compile error, 2 assert or lint error, 3 warnings only";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let code = match args.first().map(String::as_str) {
        Some("build") => match parse_build(&args[1..]) {
            Ok(opts) => with_project(&cwd, |p| build::build(p, &opts)),
            Err(msg) => usage_error(&msg),
        },
        Some("check") => match flag_set(&args[1..], &["--json"]) {
            Ok(json) => with_project(&cwd, |p| check(p, json)),
            Err(msg) => usage_error(&msg),
        },
        Some("fmt") => match parse_fmt(&args[1..]) {
            Ok((check, files)) => with_project(&cwd, |p| fmt(p, &cwd, check, &files)),
            Err(msg) => usage_error(&msg),
        },
        Some("explain") => match parse_explain(&args[1..]) {
            Ok(file) => with_project(&cwd, |p| explain(p, &cwd.join(file))),
            Err(msg) => usage_error(&msg),
        },
        Some("init") => match flag_set(&args[1..], &[]) {
            Ok(_) => init(&cwd),
            Err(msg) => usage_error(&msg),
        },
        Some("docs") => match args[1..].iter().find(|a| a.starts_with('-')) {
            Some(flag) => usage_error(&format!("unknown flag `{flag}`")),
            None => docs(&args[1..]),
        },
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

const CARD: &str = include_str!("../../../docs/language.md");

/// One entry per file in `docs/topics/`. The file's first line, `# <name>: <summary>`, is its index line.
const TOPICS: &[(&str, &str)] = &[
    ("targets", include_str!("../../../docs/topics/targets.md")),
    ("values", include_str!("../../../docs/topics/values.md")),
    ("color", include_str!("../../../docs/topics/color.md")),
    ("shapes", include_str!("../../../docs/topics/shapes.md")),
    ("structure", include_str!("../../../docs/topics/structure.md")),
    ("fields", include_str!("../../../docs/topics/fields.md")),
    ("components", include_str!("../../../docs/topics/components.md")),
    ("anim", include_str!("../../../docs/topics/anim.md")),
    ("checks", include_str!("../../../docs/topics/checks.md")),
    ("cli", include_str!("../../../docs/topics/cli.md")),
    ("craft", include_str!("../../../docs/topics/craft.md")),
];

fn docs(names: &[String]) -> u8 {
    if names.is_empty() {
        print!("{CARD}\n{}", topic_index());
        println!("\nRun `artscript docs <topic> [topic...]` and name every topic the task touches in one call.");
        return 0;
    }
    let mut bodies = Vec::new();
    for name in names {
        match TOPICS.iter().find(|(n, _)| n == name) {
            Some((_, body)) => bodies.push(*body),
            None => {
                eprintln!("error: unknown docs topic `{name}`\n\n{}", topic_index());
                return 1;
            }
        }
    }
    print!("{CARD}\n{}", bodies.join("\n"));
    0
}

fn topic_index() -> String {
    let width = TOPICS.iter().map(|(n, _)| n.len()).max().unwrap_or(0);
    let mut out = String::from("## Topics\n\n");
    for (name, body) in TOPICS {
        let first = body.lines().next().unwrap_or("");
        let summary = first
            .strip_prefix("# ")
            .and_then(|l| l.strip_prefix(name))
            .and_then(|l| l.strip_prefix(": "))
            .unwrap_or(first);
        out.push_str(&format!("{name:<width$}  {summary}\n"));
    }
    out
}

fn usage_error(msg: &str) -> u8 {
    eprintln!("error: {msg}\n\n{USAGE}");
    1
}

/// Known flags only. Returns whether `flag` was present.
fn flag_set(args: &[String], known: &[&str]) -> Result<bool, String> {
    let mut seen = false;
    for a in args {
        if known.contains(&a.as_str()) {
            seen = true;
        } else if a.starts_with('-') {
            return Err(format!("unknown flag `{a}`"));
        } else {
            return Err(format!("unexpected argument `{a}`"));
        }
    }
    Ok(seen)
}

fn parse_build(args: &[String]) -> Result<build::Options, String> {
    let mut opts = build::Options::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--allow-warnings" => opts.allow_warnings = true,
            "--only" => {
                let glob = it.next().ok_or("`--only` needs a glob")?;
                opts.only.push(parse_glob(glob)?);
            }
            s if s.starts_with("--only=") => opts.only.push(parse_glob(&s["--only=".len()..])?),
            "--backend" => {
                let value = it.next().ok_or("`--backend` needs auto, gpu or cpu")?;
                opts.backend = Some(value.parse()?);
            }
            s if s.starts_with("--backend=") => {
                opts.backend = Some(s["--backend=".len()..].parse()?);
            }
            other => return Err(format!("unknown build flag `{other}`")),
        }
    }
    Ok(opts)
}

fn parse_explain(args: &[String]) -> Result<&str, String> {
    let mut file = None;
    for a in args {
        if a.starts_with('-') {
            return Err(format!("unknown flag `{a}`"));
        }
        if file.is_some() {
            return Err(format!("unexpected argument `{a}`"));
        }
        file = Some(a.as_str());
    }
    file.ok_or_else(|| "`explain` needs a file".to_string())
}

fn parse_fmt(args: &[String]) -> Result<(bool, Vec<String>), String> {
    let mut check = false;
    let mut files = Vec::new();
    for a in args {
        if a == "--check" {
            check = true;
        } else if a.starts_with('-') {
            return Err(format!("unknown flag `{a}`"));
        } else {
            files.push(a.clone());
        }
    }
    Ok((check, files))
}

fn with_project(cwd: &Path, f: impl FnOnce(&Project) -> u8) -> u8 {
    let Some(root) = find_root(cwd) else {
        eprintln!(
            "{}",
            Diagnostic::error(
                "E0200",
                Span::default(),
                "no artscript.toml in this directory or any parent"
            )
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
    diags.sort_by(|a, b| {
        (&a.file, a.span.line, a.span.col).cmp(&(&b.file, b.span.line, b.span.col))
    });
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
        let frame_diags = if job.outputs.iter().any(|o| o.format == Format::Gif) {
            artscript_eval::sample(project, job).diagnostics
        } else {
            artscript_eval::evaluate(project, job).diagnostics
        };
        diags.extend(frame_diags);
    }
    report(diags, false, json)
}

fn fmt(project: &Project, cwd: &Path, check: bool, files: &[String]) -> u8 {
    let selected = if files.is_empty() {
        project.files.keys().cloned().collect::<Vec<_>>()
    } else {
        let mut out = Vec::new();
        for arg in files {
            match resolve_art(project, cwd, arg) {
                Ok(rel) if !out.contains(&rel) => out.push(rel),
                Ok(_) => {}
                Err(msg) => return usage_error(&msg),
            }
        }
        out
    };
    let mut diags = Vec::new();
    let mut dirty = Vec::new();
    for rel in &selected {
        let Some(src) = project.files.get(rel) else {
            return usage_error(&format!("`{rel}` is not an .art file in this project"));
        };
        match artscript_syntax::format_source(src) {
            Ok(out) if &out != src => {
                if check {
                    dirty.push(rel.clone());
                } else if let Err(e) = fs::write(project.root.join(rel), &out) {
                    diags.push(
                        Diagnostic::error("E0200", Span::default(), format!("cannot write: {e}"))
                            .in_file(rel),
                    );
                } else {
                    println!("formatted {rel}");
                }
            }
            Ok(_) => {}
            Err(errs) => diags.extend(errs.into_iter().map(|d| d.in_file(rel))),
        }
    }
    if check {
        for rel in &dirty {
            println!("{rel}");
        }
    }
    let code = report(diags, false, false);
    if check && !dirty.is_empty() { 1 } else { code }
}

fn resolve_art(project: &Project, cwd: &Path, arg: &str) -> Result<String, String> {
    let path = cwd.join(arg);
    let canon = fs::canonicalize(&path).map_err(|e| format!("cannot read `{arg}`: {e}"))?;
    if !canon.is_file() {
        return Err(format!("`{arg}` is not a file"));
    }
    let root = fs::canonicalize(&project.root).unwrap_or_else(|_| project.root.clone());
    let rel = rel_path(&root, &canon);
    if project.files.contains_key(&rel) {
        Ok(rel)
    } else {
        Err(format!("`{arg}` is not an .art file in this project"))
    }
}

fn explain(project: &Project, file: &Path) -> u8 {
    let file = fs::canonicalize(file).unwrap_or(file.to_path_buf());
    let root = fs::canonicalize(&project.root).unwrap_or(project.root.clone());
    let rel = rel_path(&root, &file);
    let plan = plan(project);
    let Some(job) = plan.jobs.iter().find(|j| j.source == rel) else {
        let own: Vec<Diagnostic> = plan
            .diagnostics
            .into_iter()
            .filter(|d| d.file == rel)
            .collect();
        if !own.is_empty() {
            return report(own, false, false);
        }
        eprintln!(
            "{}",
            Diagnostic::error(
                "E0200",
                Span::default(),
                format!("`{rel}` has no @target to explain")
            )
            .with_note(format!("entry files: {}", plan.entries.join(", ")))
            .with_fix("pass a file that declares `@target`")
        );
        return 1;
    };
    let ev = artscript_eval::evaluate(project, job);
    if let Some(scene) = &ev.scene {
        print!("{}", artscript_eval::explain(scene, &ev.seeded));
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

const INIT_PALETTE: &str =
    "palette house\n  ramp sky #7fb8e6 5\n  ramp leaf #6cbf6a 5\n  ramp ink #3b2a2a 5\n";

const INIT_ART: &str =
    "@target hello.png 512x512 scale:1,2\n\ncircle 160 fill:p.sky stroke:p.ink.line,6\n";

fn init(cwd: &Path) -> u8 {
    if cwd.join("artscript.toml").exists() {
        eprintln!(
            "{}",
            Diagnostic::error(
                "E0200",
                Span::default(),
                "artscript.toml already exists here"
            )
            .with_fix("run `artscript build` instead")
        );
        return 1;
    }
    let files = [
        ("artscript.toml", INIT_MANIFEST),
        ("lib/palette/house.art", INIT_PALETTE),
        ("art/hello.art", INIT_ART),
    ];
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

fn parse_glob(glob: &str) -> Result<String, String> {
    let mut depth = 0usize;
    for c in glob.chars() {
        match c {
            '{' => depth += 1,
            '}' => depth = depth.checked_sub(1).ok_or(format!("`}}` without `{{` in glob `{glob}`"))?,
            _ => {}
        }
    }
    if depth > 0 {
        return Err(format!("`{{` without `}}` in glob `{glob}`"));
    }
    Ok(glob.to_string())
}

/// `*` matches within a path segment, `**` across segments, `?` one char, `{a,b}` either.
/// Braces balance, because `parse_glob` checks every `--only`.
pub(crate) fn glob_match(pat: &str, text: &str) -> bool {
    fn go(p: &[u8], t: &[u8]) -> bool {
        match p.first() {
            None => t.is_empty(),
            Some(b'{') => {
                let (mut depth, mut start, mut close, mut alts) = (0, 1, p.len(), Vec::new());
                for (i, &c) in p.iter().enumerate().skip(1) {
                    match c {
                        b'{' => depth += 1,
                        b'}' if depth > 0 => depth -= 1,
                        b'}' => {
                            alts.push(&p[start..i]);
                            close = i + 1;
                            break;
                        }
                        b',' if depth == 0 => {
                            alts.push(&p[start..i]);
                            start = i + 1;
                        }
                        _ => {}
                    }
                }
                alts.iter().any(|alt| go(&[alt, &p[close..]].concat(), t))
            }
            Some(b'*') if p.get(1) == Some(&b'*') => {
                let rest = p[2..].strip_prefix(b"/").unwrap_or(&p[2..]);
                (0..=t.len()).any(|i| go(rest, &t[i..]) || (i < t.len() && go(&p[2..], &t[i..])))
            }
            Some(b'*') => (0..=t.len())
                .take_while(|&i| i == 0 || t[i - 1] != b'/')
                .any(|i| go(&p[1..], &t[i..])),
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
        assert!(glob_match(
            "art/**/momo?peach.art",
            "art/produce/momo_peach.art"
        ));
        assert!(glob_match("art/{tiles,props}/x/*.art", "art/props/x/c.art"));
        assert!(glob_match("{a,b{c,d}}.png", "bd.png"));
        assert!(glob_match("x{,_big}.png", "x.png"));
        assert!(!glob_match("art/{tiles,props}/*.art", "art/fx/c.art"));
        assert!(parse_glob("art/{tiles").is_err());
        assert!(parse_glob("art/tiles}").is_err());
    }

    #[test]
    fn exit_codes() {
        let e = |code: &str, sev: Severity| Diagnostic {
            severity: sev,
            ..Diagnostic::error(code, Span::default(), "x")
        };
        assert_eq!(exit_code(&[], false), 0);
        assert_eq!(
            exit_code(
                &[e("E0301", Severity::Error), e("C0101", Severity::Error)],
                false
            ),
            1
        );
        assert_eq!(exit_code(&[e("C0101", Severity::Error)], false), 2);
        assert_eq!(exit_code(&[e("L0101", Severity::Error)], false), 2);
        assert_eq!(exit_code(&[e("C0201", Severity::Warning)], false), 3);
        assert_eq!(exit_code(&[e("C0201", Severity::Warning)], true), 0);
    }
}
