use std::collections::BTreeMap;

use artscript_ir::Profile;
use artscript_syntax::Diagnostic;
use artscript_syntax::ast::{Directive, Expr, File, Span, UnOp};

use crate::project::Project;
use crate::value::num;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Png,
    Svg,
    Gif,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OutputSpec {
    /// Relative to the manifest `out` directory.
    pub path: String,
    pub format: Format,
    pub scale: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum MatrixValue {
    Word(String),
    Num(f64),
}

impl MatrixValue {
    pub fn text(&self) -> String {
        match self {
            MatrixValue::Word(w) => w.clone(),
            MatrixValue::Num(n) => num(*n),
        }
    }
}

/// One `@target` line after matrix expansion: one scene, one or more output files.
#[derive(Clone, Debug)]
pub struct TargetJob {
    pub source: String,
    pub line: u32,
    pub span: Span,
    /// The `as:` label, read by the file as `target`.
    pub label: Option<String>,
    pub fps: f64,
    /// Explicit clip length in seconds. `None` means the latest `play` end.
    pub dur: Option<f64>,
    /// Binds `clip`. `None` is `none`.
    pub clip: Option<String>,
    pub frame_t: f64,
    pub matrix: Vec<(String, MatrixValue)>,
    pub size: (f32, f32),
    pub bg: Option<Expr>,
    pub profile: Profile,
    /// Scale baked SVG tiles render at: the target's highest scale.
    pub bake_scale: f32,
    pub outputs: Vec<OutputSpec>,
}

impl TargetJob {
    /// Identifies the declaration and matrix cell, stable across builds.
    pub fn key(&self) -> String {
        let m: Vec<String> = self
            .matrix
            .iter()
            .map(|(k, v)| format!("{k}={}", v.text()))
            .collect();
        let dur = self.dur.map(num).unwrap_or_else(|| "auto".into());
        let clip = self.clip.clone().unwrap_or_else(|| "none".into());
        format!(
            "{}:{}[{}] fps={} dur={} clip={} t={}",
            self.source,
            self.line,
            m.join(","),
            num(self.fps),
            dur,
            clip,
            num(self.frame_t)
        )
    }
}

pub struct Plan {
    pub jobs: Vec<TargetJob>,
    pub diagnostics: Vec<Diagnostic>,
    /// Entry files: any file with at least one `@target`.
    pub entries: Vec<String>,
}

pub fn is_entry(file: &File) -> bool {
    file.directives.iter().any(|d| d.name == "target")
}

pub fn plan(project: &Project) -> Plan {
    let mut jobs = Vec::new();
    let mut diagnostics = Vec::new();
    let mut entries = Vec::new();
    for (path, src) in &project.files {
        let file = match artscript_syntax::parse(src) {
            Ok(f) => f,
            Err(errs) => {
                if !project.is_lib_file(path) || src.contains("@target") {
                    diagnostics.extend(errs.into_iter().map(|d| d.in_file(path)));
                }
                continue;
            }
        };
        if !is_entry(&file) {
            continue;
        }
        if project.is_lib_file(path) {
            for d in file.directives.iter().filter(|d| d.name == "target") {
                diagnostics.push(
                    Diagnostic::error("E0203", d.span, "a library root cannot contain `@target`")
                        .in_file(path)
                        .with_note(format!("lib roots: {}", project.manifest.lib.join(", ")))
                        .with_fix("move this file out of the lib root (for example into art/), or delete the `@target` lines"),
                );
            }
            continue;
        }
        entries.push(path.clone());
        for d in &file.directives {
            match expand(project, path, d) {
                Ok(mut js) => jobs.append(&mut js),
                Err(diag) => diagnostics.push(diag.in_file(path)),
            }
        }
    }
    let mut seen: BTreeMap<String, (String, u32)> = BTreeMap::new();
    let mut dup_jobs = Vec::new();
    for (ji, job) in jobs.iter().enumerate() {
        for out in &job.outputs {
            if let Some((file, line)) = seen.get(&out.path) {
                diagnostics.push(
                    Diagnostic::error(
                        "E0202",
                        job.span,
                        format!("output `{}` is declared twice", out.path),
                    )
                    .in_file(&job.source)
                    .with_note(format!("first declared at {file}:{line}"))
                    .with_fix(
                        "give each `@target` a distinct path, or add `{name}` for each matrix axis",
                    ),
                );
                dup_jobs.push(ji);
            } else {
                seen.insert(out.path.clone(), (job.source.clone(), job.line));
            }
        }
    }
    dup_jobs.dedup();
    for ji in dup_jobs.into_iter().rev() {
        jobs.remove(ji);
    }
    Plan {
        jobs,
        diagnostics,
        entries,
    }
}

fn matrix_value(e: &Expr) -> Option<MatrixValue> {
    Some(match e {
        Expr::Ident(s) | Expr::Str(s) => MatrixValue::Word(s.clone()),
        Expr::Num(n) => MatrixValue::Num(*n),
        Expr::Unary(UnOp::Neg, inner) => match **inner {
            Expr::Num(n) => MatrixValue::Num(-n),
            _ => return None,
        },
        _ => return None,
    })
}

fn number(e: &Expr) -> Option<f64> {
    match e {
        Expr::Num(n) => Some(*n),
        _ => None,
    }
}

fn time_literal(e: &Expr) -> Option<f64> {
    match e {
        Expr::Time(s) | Expr::Num(s) => Some(*s),
        _ => None,
    }
}

fn expand(project: &Project, file: &str, d: &Directive) -> Result<Vec<TargetJob>, Diagnostic> {
    if d.name != "target" {
        return Err(
            Diagnostic::error("E0208", d.span, format!("unknown directive `@{}`", d.name))
                .with_fix("the only directive in v0.1 is `@target <path> <W>x<H>`"),
        );
    }
    let Some(raw_path) = d.path.clone() else {
        return Err(
            Diagnostic::error("E0206", d.span, "`@target` needs an output path")
                .with_fix("write `@target chars/hero.png 256x256`"),
        );
    };
    let size = match d.args.first() {
        Some(Expr::Size(w, h)) => match (number(w), number(h)) {
            (Some(w), Some(h)) => (w as f32, h as f32),
            _ => return Err(size_error(d.span)),
        },
        _ => return Err(size_error(d.span)),
    };
    let named = |k: &str| d.named.iter().find(|n| n.key == k).map(|n| &n.value);
    let scales: Vec<f32> = match named("scale") {
        None => project.manifest.scale.clone(),
        Some(Expr::Tuple(xs)) | Some(Expr::List(xs)) => {
            xs.iter().filter_map(number).map(|v| v as f32).collect()
        }
        Some(e) => number(e).map(|v| vec![v as f32]).unwrap_or_default(),
    };
    if scales.is_empty() || scales.iter().any(|s| *s <= 0.0) {
        return Err(
            Diagnostic::error("E0206", d.span, "`scale:` must list positive numbers")
                .with_fix("write `scale:1,2`"),
        );
    }
    let label = match named("as") {
        Some(Expr::Ident(s)) => Some(s.clone()),
        _ => None,
    };
    let fps = match named("fps") {
        None => 24.0,
        Some(Expr::Time(_)) => {
            return Err(
                Diagnostic::error("E0206", d.span, "`fps:` is a count, not a time")
                    .with_fix("write `fps:24`"),
            );
        }
        Some(e) => match number(e) {
            Some(n) if n.is_finite() && n > 0.0 => n,
            _ => {
                return Err(
                    Diagnostic::error("E0206", d.span, "`fps:` must be a positive number")
                        .with_fix("write `fps:24`"),
                );
            }
        },
    };
    let dur = match named("dur") {
        None => None,
        Some(e) => match time_literal(e) {
            Some(n) if n.is_finite() && n > 0.0 => Some(n),
            _ => {
                return Err(
                    Diagnostic::error("E0206", d.span, "`dur:` must be a positive time")
                        .with_fix("write `dur:2s`"),
                );
            }
        },
    };
    let frame_t = match named("t") {
        None => 0.0,
        Some(e) => match time_literal(e) {
            Some(n) if n.is_finite() && n >= 0.0 => n,
            _ => {
                return Err(Diagnostic::error("E0206", d.span, "`t:` must be a time")
                    .with_fix("write `t:100ms`"));
            }
        },
    };
    let clip_expr = named("clip").cloned();
    let format = if raw_path.ends_with(".svg") {
        Format::Svg
    } else if raw_path.ends_with(".png") {
        Format::Png
    } else if raw_path.ends_with(".gif") {
        Format::Gif
    } else {
        return Err(Diagnostic::error(
            "E0206",
            d.span,
            format!("`{raw_path}` has no known extension"),
        )
        .with_fix("end the path with .png, .svg or .gif"));
    };
    let profile_word = match named("profile") {
        Some(Expr::Ident(s)) => s.clone(),
        _ => project.manifest.profile.clone(),
    };
    let profile = match (format, profile_word.as_str()) {
        (Format::Png | Format::Gif, _) | (_, "raster") => Profile::Raster,
        (Format::Svg, "vector") => Profile::Vector,
        (Format::Svg, "svg" | "auto") => Profile::Svg,
        (_, other) => {
            return Err(
                Diagnostic::error("E0206", d.span, format!("unknown profile `{other}`"))
                    .with_fix("use profile:vector, profile:svg or profile:raster"),
            );
        }
    };
    let mut cells: Vec<Vec<(String, MatrixValue)>> = vec![Vec::new()];
    for axis in &d.each {
        let Expr::List(items) = &axis.value else {
            return Err(Diagnostic::error(
                "E0206",
                axis.span,
                format!("matrix axis `{}` needs a list", axis.key),
            )
            .with_fix(format!("write `each {}:[a b]`", axis.key)));
        };
        let values: Vec<MatrixValue> = items.iter().filter_map(matrix_value).collect();
        cells = cells
            .into_iter()
            .flat_map(|cell| {
                values.iter().map(move |v| {
                    let mut c = cell.clone();
                    c.push((axis.key.clone(), v.clone()));
                    c
                })
            })
            .collect();
    }
    let bake_scale = scales.iter().copied().fold(1.0f32, f32::max);
    cells
        .into_iter()
        .map(|matrix| {
            let path = interpolate(&raw_path, &matrix).map_err(|var| {
                Diagnostic::error(
                    "E0207",
                    d.span,
                    format!("`{{{var}}}` in the path is not a matrix axis"),
                )
                .with_fix(format!("add `each {var}:[..]`, or remove `{{{var}}}`"))
            })?;
            let outputs = match format {
                Format::Svg => vec![OutputSpec {
                    path,
                    format,
                    scale: 1.0,
                }],
                Format::Png | Format::Gif => scales
                    .iter()
                    .map(|s| OutputSpec {
                        path: scaled_name(&path, *s),
                        format,
                        scale: *s,
                    })
                    .collect(),
            };
            let clip = clip_of(clip_expr.as_ref(), &matrix, d.span)?;
            Ok(TargetJob {
                source: file.to_string(),
                line: d.span.line,
                span: d.span,
                label: label.clone(),
                fps,
                dur,
                clip,
                frame_t,
                matrix,
                size,
                bg: named("bg").cloned(),
                profile,
                bake_scale,
                outputs,
            })
        })
        .collect()
}

fn size_error(span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0206",
        span,
        "`@target` needs a canvas size after the path",
    )
    .with_fix("write the size as `<W>x<H>`, e.g. `@target chars/hero.png 256x256`")
}

fn clip_of(
    expr: Option<&Expr>,
    matrix: &[(String, MatrixValue)],
    span: Span,
) -> Result<Option<String>, Diagnostic> {
    let Some(expr) = expr else { return Ok(None) };
    match expr {
        Expr::Ident(s) if s == "none" => Ok(None),
        Expr::Ident(s) => {
            if let Some((_, v)) = matrix.iter().find(|(k, _)| k == s) {
                let text = v.text();
                if text == "none" {
                    Ok(None)
                } else {
                    Ok(Some(text))
                }
            } else {
                Ok(Some(s.clone()))
            }
        }
        Expr::Str(s) => Ok(Some(s.clone())),
        _ => Err(Diagnostic::error("E0206", span, "`clip:` needs a name")
            .with_fix("write `clip:idle` or `each c:[idle] clip:c`")),
    }
}

fn interpolate(path: &str, matrix: &[(String, MatrixValue)]) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = path;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let Some(j) = rest[i..].find('}') else { break };
        let var = &rest[i + 1..i + j];
        match matrix.iter().find(|(k, _)| k == var) {
            Some((_, v)) => out.push_str(&v.text()),
            None => return Err(var.to_string()),
        }
        rest = &rest[i + j + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

fn scaled_name(path: &str, scale: f32) -> String {
    if scale == 1.0 {
        return path.to_string();
    }
    let (stem, ext) = path.rsplit_once('.').unwrap_or((path, ""));
    format!("{stem}@{}x.{ext}", num(scale as f64))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(files: &[(&str, &str)]) -> Project {
        let files = files
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Project::from_sources("[project]\nout = \"out\"\nlib = [\"lib\"]\n", files).unwrap()
    }

    #[test]
    fn matrix_expands_with_interpolated_paths_and_scales() {
        let p = project(&[(
            "art/k.art",
            "@target chars/knight-{armor}-{team}.png 128x128 scale:1,2 as:knights each armor:[plate leather] team:[red blue]\nrect 10x10\n",
        )]);
        let plan = plan(&p);
        assert!(plan.diagnostics.is_empty(), "{:?}", plan.diagnostics);
        let paths: Vec<String> = plan
            .jobs
            .iter()
            .flat_map(|j| j.outputs.iter().map(|o| o.path.clone()))
            .collect();
        assert_eq!(
            paths,
            [
                "chars/knight-plate-red.png",
                "chars/knight-plate-red@2x.png",
                "chars/knight-plate-blue.png",
                "chars/knight-plate-blue@2x.png",
                "chars/knight-leather-red.png",
                "chars/knight-leather-red@2x.png",
                "chars/knight-leather-blue.png",
                "chars/knight-leather-blue@2x.png",
            ]
        );
        assert_eq!(plan.jobs[1].label.as_deref(), Some("knights"));
        assert_eq!(
            plan.jobs[1].matrix[1],
            ("team".to_string(), MatrixValue::Word("blue".into()))
        );
    }

    #[test]
    fn duplicate_outputs_are_e0202_naming_both_lines() {
        let p = project(&[
            ("art/a.art", "@target x.png 8x8\nrect 1x1\n"),
            ("art/b.art", "\n@target x.png 8x8\nrect 1x1\n"),
        ]);
        let plan = plan(&p);
        assert_eq!(plan.diagnostics.len(), 1);
        let d = &plan.diagnostics[0];
        assert_eq!(
            (d.code.as_str(), d.file.as_str(), d.span.line),
            ("E0202", "art/b.art", 2)
        );
        assert_eq!(d.notes, ["first declared at art/a.art:1"]);
        assert_eq!(plan.jobs.len(), 1);
    }

    #[test]
    fn targets_under_a_lib_root_are_e0203() {
        let p = project(&[("lib/x.art", "@target x.png 8x8\nrect 1x1\n")]);
        let plan = plan(&p);
        assert_eq!(plan.diagnostics[0].code, "E0203");
        assert!(plan.jobs.is_empty());
    }
}
