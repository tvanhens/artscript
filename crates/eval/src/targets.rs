use std::collections::BTreeMap;

use ramose_ir::{Align, Edge, Fit, Framing, Profile, Rect, Size};
use ramose_syntax::Diagnostic;
use ramose_syntax::ast::{Directive, Expr, File, Span, UnOp};

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
    /// The output's size and density, and how the artboard lands in it.
    pub framing: Framing,
}

/// The space a file draws in: its size in artboard units, the same for every output.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Artboard {
    pub size: Size,
    /// The artboard crops a larger scene, so `off-artboard` stays quiet.
    pub viewport: bool,
}

impl Artboard {
    pub fn rect(self) -> Rect {
        self.size.rect()
    }
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
    pub parameters: BTreeMap<String, Expr>,
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
    pub artboard: Artboard,
    pub bg: Option<Expr>,
    /// The `pivot:` expression, a point in artboard units. PNG and SVG targets only.
    pub pivot: Option<Expr>,
    /// The `palette:` expression: a palette or a palette module that `p` names for the whole scene.
    pub palette: Option<Expr>,
    pub profile: Profile,
    /// Pixels per artboard unit this evaluation draws at, what the file reads as `scale`.
    pub sample_scale: f64,
    /// The largest `scale:`, never below 1.
    pub tile_scale: f32,
    pub outputs: Vec<OutputSpec>,
}

impl TargetJob {
    /// What the file reads as `scale` when it draws `out`: that output's pixels per artboard unit,
    /// its fit zoom times its `scale:`.
    pub fn draw_scale(&self, out: &OutputSpec) -> f64 {
        let zoom = out.framing.view(self.artboard.rect()).zoom;
        zoom.to_string().parse().unwrap_or(f64::from(zoom))
    }

    /// The scale baked tiles and tapered strokes sample at for `out`: a PNG or GIF samples at its
    /// `draw_scale`, and an SVG at its zoom times the largest `scale:`.
    pub fn bake_scale(&self, out: &OutputSpec) -> f32 {
        let zoom = out.framing.view(self.artboard.rect()).zoom;
        match out.format {
            Format::Png | Format::Gif => zoom,
            Format::Svg => zoom * self.tile_scale,
        }
    }

    /// Pixels per artboard unit this evaluation's tapered strokes sample at: `sample_scale`, and
    /// for an SVG that times the largest `scale:`, the density its tiles bake at.
    pub fn ink_scale(&self) -> f64 {
        if self.outputs.iter().any(|o| o.format == Format::Svg) {
            self.sample_scale * f64::from(self.tile_scale)
        } else {
            self.sample_scale
        }
    }

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
        let file = match ramose_syntax::parse(src) {
            Ok(f) => f,
            Err(errs) => {
                if !project.is_lib_file(path) || src.contains("@target") {
                    diagnostics.extend(errs.into_iter().map(|d| d.in_file(path)));
                }
                continue;
            }
        };
        for d in file
            .directives
            .iter()
            .filter(|d| !matches!(d.name.as_str(), "target" | "artboard"))
        {
            diagnostics.push(
                Diagnostic::error("E0208", d.span, format!("unknown directive `@{}`", d.name))
                    .in_file(path)
                    .with_fix("the directives are `@artboard <W>x<H>` and `@target <path>`"),
            );
        }
        if !is_entry(&file) {
            for d in file.directives.iter().filter(|d| d.name == "artboard") {
                diagnostics.push(
                    Diagnostic::error("E0206", d.span, "a library has no artboard")
                        .in_file(path)
                        .with_fix("remove it, or add a `@target`"),
                );
            }
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
        let art = match artboard(&file) {
            Ok(art) => art,
            Err(diag) => {
                diagnostics.push(diag.in_file(path));
                continue;
            }
        };
        for d in file.directives.iter().filter(|d| d.name == "target") {
            match expand(project, path, art, d) {
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

const OUTPUT_VARS: [&str; 3] = ["scale", "w", "h"];

const TARGET_KEYS: [&str; 12] = [
    "scale", "as", "fps", "dur", "t", "clip", "bg", "profile", "pivot", "palette", "fit", "align",
];

fn artboard(file: &File) -> Result<Artboard, Diagnostic> {
    let mut lines = file.directives.iter().filter(|d| d.name == "artboard");
    let Some(d) = lines.next() else {
        let size = file
            .directives
            .iter()
            .find(|d| d.name == "target")
            .and_then(|d| match d.args.first() {
                Some(Expr::Size { w, h, depth: None }) => Some(format!("{}x{}", num(number(w)?), num(number(h)?))),
                _ => None,
            })
            .unwrap_or_else(|| "512x512".to_string());
        let span = file.directives.first().map(|d| d.span).unwrap_or_default();
        return Err(
            Diagnostic::error("E0206", span, "an entry file needs one `@artboard`")
                .with_fix(format!("add `@artboard {size}`")),
        );
    };
    if let Some(again) = lines.next() {
        return Err(Diagnostic::error(
            "E0206",
            again.span,
            format!("`@artboard` is already set at line {}", d.span.line),
        )
        .with_fix("remove this line"));
    }
    let bad_size = || {
        Diagnostic::error("E0206", d.span, "`@artboard` needs a positive WxH size")
            .with_fix("write `@artboard 512x512`")
    };
    let size = match (d.path.as_deref(), d.args.first()) {
        (None, Some(Expr::Size { w, h, depth: None })) => match (number(w), number(h)) {
            (Some(w), Some(h)) => Size::new(w as f32, h as f32).ok_or_else(bad_size)?,
            _ => return Err(bad_size()),
        },
        (None, Some(Expr::Size { depth: Some(_), .. })) => {
            return Err(
                Diagnostic::error("E0206", d.span, "`@artboard` takes a two-component size, got three")
                    .with_fix("an artboard has no depth yet; drop the third component, as in `512x512`"),
            );
        }
        _ => return Err(bad_size()),
    };
    let mut viewport = false;
    for arg in &d.args[1..] {
        match arg {
            Expr::Ident(w) if w == "viewport" => viewport = true,
            Expr::Ident(w) => {
                return Err(
                    Diagnostic::error("E0206", d.span, format!("unknown artboard word `{w}`"))
                        .with_fix("only the word `viewport` follows the size"),
                );
            }
            _ => {
                return Err(
                    Diagnostic::error("E0206", d.span, "`@artboard` takes only the word `viewport` after the size")
                        .with_fix("only the word `viewport` follows the size"),
                );
            }
        }
    }
    if let Some(n) = d.named.first() {
        return Err(
            Diagnostic::error("E0206", d.span, format!("unknown artboard key `{}`", n.key))
                .with_fix("only the word `viewport` follows the size"),
        );
    }
    Ok(Artboard { size, viewport })
}

fn framing_keys(d: &Directive, sized: bool) -> Result<(Fit, Align), Diagnostic> {
    let named = |k: &str| d.named.iter().find(|n| n.key == k).map(|n| &n.value);
    let fit = match named("fit") {
        None => Fit::Contain,
        Some(Expr::Ident(w)) if w == "contain" => Fit::Contain,
        Some(Expr::Ident(w)) if w == "cover" => Fit::Cover,
        Some(Expr::Ident(w)) if w == "none" => Fit::None,
        Some(e) => {
            let word = match e {
                Expr::Ident(w) => format!("unknown fit `{w}`"),
                _ => "`fit:` needs a word".to_string(),
            };
            return Err(Diagnostic::error("E0206", d.span, word)
                .with_fix("use contain, cover, or none"));
        }
    };
    let edge = |w: &str, start: &str, end: &str| match w {
        _ if w == start => Some(Edge::Start),
        "center" => Some(Edge::Center),
        _ if w == end => Some(Edge::End),
        _ => None,
    };
    let align = match named("align") {
        None => Align::default(),
        Some(Expr::Tuple(xs)) if xs.len() == 2 => {
            let words = (ident_of(&xs[0]), ident_of(&xs[1]));
            match words {
                (Some(x), Some(y)) => match (edge(x, "left", "right"), edge(y, "top", "bottom")) {
                    (Some(x), Some(y)) => Align { x, y },
                    _ => return Err(align_error(d.span, &format!("unknown align `{x},{y}`"))),
                },
                _ => return Err(align_error(d.span, "`align:` needs two words")),
            }
        }
        Some(Expr::Ident(w)) => return Err(align_error(d.span, &format!("unknown align `{w}`"))),
        Some(_) => return Err(align_error(d.span, "`align:` needs two words")),
    };
    if !sized && (named("fit").is_some() || named("align").is_some()) {
        let key = if named("fit").is_some() { "fit" } else { "align" };
        return Err(
            Diagnostic::error("E0206", d.span, format!("`{key}:` needs an output size"))
                .with_fix("add the size, as in `@target x.png 240x240`"),
        );
    }
    Ok((fit, align))
}

fn align_error(span: Span, message: &str) -> Diagnostic {
    Diagnostic::error("E0206", span, message).with_fix("write `align:left,top`: x is left, center or right, then y is top, center or bottom")
}

fn ident_of(e: &Expr) -> Option<&str> {
    match e {
        Expr::Ident(s) => Some(s.as_str()),
        _ => None,
    }
}

/// The `as:` label of one `@target` line, which the file reads as `target`.
pub(crate) fn label(d: &Directive) -> Option<String> {
    match d.named.iter().find(|n| n.key == "as").map(|n| &n.value) {
        Some(Expr::Ident(s)) => Some(s.clone()),
        _ => None,
    }
}

fn expand(
    project: &Project,
    file: &str,
    artboard: Artboard,
    d: &Directive,
) -> Result<Vec<TargetJob>, Diagnostic> {
    let Some(raw_path) = d.path.clone() else {
        return Err(
            Diagnostic::error("E0206", d.span, "`@target` needs an output path")
                .with_fix("write `@target chars/hero.png`"),
        );
    };
    let mut args = d.args.iter();
    let out = match args.next() {
        None => None,
        Some(Expr::Size { w, h, depth: None }) => match (number(w), number(h)) {
            (Some(w), Some(h)) => Some(Size::new(w as f32, h as f32).ok_or_else(|| size_error(d.span))?),
            _ => return Err(size_error(d.span)),
        },
        Some(Expr::Size { depth: Some(_), .. }) => {
            return Err(
                Diagnostic::error("E0206", d.span, "`@target` takes a two-component output size, got three")
                    .with_fix("an image has no depth; drop the third component, as in `240x240`"),
            );
        }
        Some(Expr::Ident(w)) if w == "viewport" => {
            return Err(
                Diagnostic::error("E0206", d.span, "`viewport` belongs to the artboard")
                    .with_fix("move it to `@artboard WxH viewport`"),
            );
        }
        Some(Expr::Ident(w)) => {
            return Err(
                Diagnostic::error("E0206", d.span, format!("unknown target word `{w}`"))
                    .with_fix("after the path, write only an output size such as `240x240`"),
            );
        }
        Some(_) => return Err(size_error(d.span)),
    };
    if let Some(extra) = args.next() {
        return Err(match extra {
            Expr::Ident(w) if w == "viewport" => {
                Diagnostic::error("E0206", d.span, "`viewport` belongs to the artboard")
                    .with_fix("move it to `@artboard WxH viewport`")
            }
            _ => Diagnostic::error("E0206", d.span, "`@target` takes only an output size after the path")
                .with_fix("remove it"),
        });
    }
    if let Some(n) = d
        .named
        .iter()
        .find(|n| !TARGET_KEYS.contains(&n.key.as_str()))
    {
        return Err(
            Diagnostic::error("E0206", d.span, format!("unknown target key `{}`", n.key))
                .with_fix(format!("use one of {}", TARGET_KEYS.join(", "))),
        );
    }
    let (fit, align) = framing_keys(d, out.is_some())?;
    let out = out.unwrap_or(artboard.size);
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
    let label = label(d);
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
    if format == Format::Gif && named("pivot").is_some() {
        return Err(
            Diagnostic::error("E0206", d.span, "`pivot:` needs a .png or .svg target")
                .with_fix("remove `pivot:`; pivots are for png and svg targets"),
        );
    }
    let (profile_word, explicit) = match named("profile") {
        None => (project.manifest.profile.as_str(), false),
        Some(Expr::Ident(s)) => (s.as_str(), true),
        Some(_) => {
            return Err(Diagnostic::error("E0206", d.span, "unknown profile")
                .with_fix("use profile:vector, profile:svg or profile:raster"));
        }
    };
    let profile = match (format, profile_word) {
        (_, "raster") | (Format::Png | Format::Gif, "auto") => Profile::Raster,
        (Format::Svg, "vector") => Profile::Vector,
        (Format::Svg, "svg" | "auto") => Profile::Svg,
        (Format::Png | Format::Gif, "vector" | "svg") if !explicit => Profile::Raster,
        (Format::Png | Format::Gif, word @ ("vector" | "svg")) => {
            return Err(Diagnostic::error(
                "E0206",
                d.span,
                format!("`profile:{word}` needs an .svg target"),
            )
            .with_fix("remove `profile:`, or end the path with .svg"));
        }
        (_, other) => {
            return Err(
                Diagnostic::error("E0206", d.span, format!("unknown profile `{other}`"))
                    .with_fix("use profile:vector, profile:svg or profile:raster"),
            );
        }
    };
    let mut cells: Vec<Vec<(String, MatrixValue)>> = vec![Vec::new()];
    for axis in &d.each {
        if matches!(axis.key.as_str(), "w" | "h") {
            return Err(Diagnostic::error(
                "E0206",
                axis.span,
                format!(
                    "matrix axis `{0}` collides with the `{{{0}}}` output size in the path",
                    axis.key
                ),
            )
            .with_fix(format!(
                "rename the axis, for example `each {0}_:[..]`",
                axis.key
            )));
        }
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
    let tile_scale = scales.iter().copied().fold(1.0f32, f32::max);
    let framing = |scale: f32| Framing { out, scale, fit, align };
    let sized_name = OUTPUT_VARS
        .iter()
        .any(|var| raw_path.contains(&format!("{{{var}}}")));
    cells
        .into_iter()
        .map(|matrix| {
            let name = |framing: &Framing| {
                let px = framing.pixels();
                interpolate(&raw_path, |var| match var {
                    "scale" => Some(num(f64::from(framing.scale))),
                    "w" => Some(px.w.to_string()),
                    "h" => Some(px.h.to_string()),
                    _ => matrix.iter().find(|(k, _)| k == var).map(|(_, v)| v.text()),
                })
                .map_err(|var| {
                    Diagnostic::error(
                        "E0207",
                        d.span,
                        format!("`{{{var}}}` in the path is not a matrix axis"),
                    )
                    .with_fix(format!(
                        "add `each {var}:[..]`, or use one of {}",
                        OUTPUT_VARS.map(|v| format!("`{{{v}}}`")).join(", ")
                    ))
                })
            };
            let outputs = match format {
                Format::Svg => {
                    let framing = framing(1.0);
                    vec![OutputSpec {
                        path: name(&framing)?,
                        format,
                        framing,
                    }]
                }
                Format::Png | Format::Gif => scales
                    .iter()
                    .map(|s| {
                        let framing = framing(*s);
                        let path = name(&framing)?;
                        Ok(OutputSpec {
                            path: if sized_name {
                                path
                            } else {
                                scaled_name(&path, *s)
                            },
                            format,
                            framing,
                        })
                    })
                    .collect::<Result<_, Diagnostic>>()?,
            };
            let clip = clip_of(clip_expr.as_ref(), &matrix, d.span)?;
            let mut job = TargetJob {
                parameters: BTreeMap::new(),
                source: file.to_string(),
                line: d.span.line,
                span: d.span,
                label: label.clone(),
                fps,
                dur,
                clip,
                frame_t,
                matrix,
                artboard,
                bg: named("bg").cloned(),
                pivot: named("pivot").cloned(),
                palette: named("palette").cloned(),
                profile,
                sample_scale: 0.0,
                tile_scale,
                outputs,
            };
            job.sample_scale = job.outputs.iter().map(|o| job.draw_scale(o)).fold(0.0, f64::max);
            Ok(job)
        })
        .collect()
}

fn size_error(span: Span) -> Diagnostic {
    Diagnostic::error("E0206", span, "an output size is a positive `<W>x<H>`")
        .with_fix("write `@artboard 256x256\n@target chars/hero.png`, or leave the size out for the artboard's")
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

fn interpolate(path: &str, lookup: impl Fn(&str) -> Option<String>) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = path;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let Some(j) = rest[i..].find('}') else { break };
        let var = &rest[i + 1..i + j];
        out.push_str(&lookup(var).ok_or_else(|| var.to_string())?);
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
            "art/k.ramo",
            "@artboard 128x128\n@target chars/knight-{armor}-{team}.png scale:1,2 as:knights each armor:[plate leather] team:[red blue]\nrect 10x10\n",
        )]);
        let plan = plan(&p);
        assert!(plan.diagnostics.is_empty(), "{:?}", plan.diagnostics);
        assert!(plan.jobs.iter().all(|j| j.sample_scale == 2.0));
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

    fn output_paths(target: &str) -> Vec<String> {
        let p = project(&[("art/k.ramo", &format!("{target}\nrect 1\n"))]);
        let plan = plan(&p);
        assert!(plan.diagnostics.is_empty(), "{:?}", plan.diagnostics);
        plan.jobs
            .iter()
            .flat_map(|j| j.outputs.iter().map(|o| o.path.clone()))
            .collect()
    }

    #[test]
    fn size_placeholders_name_each_output_and_drop_the_scale_suffix() {
        assert_eq!(
            output_paths("@artboard 33x20\n@target icon_{w}x{h}.png scale:.5,1,2.5"),
            ["icon_17x10.png", "icon_33x20.png", "icon_83x50.png"]
        );
        assert_eq!(
            output_paths("@artboard 32x32\n@target icon_{scale}x.png scale:.5,1,2"),
            ["icon_0.5x.png", "icon_1x.png", "icon_2x.png"]
        );
    }

    #[test]
    fn size_placeholders_mix_with_matrix_axes() {
        assert_eq!(
            output_paths("@artboard 8x8\n@target {team}_{w}.png scale:1,2 each team:[red blue]"),
            ["red_8.png", "red_16.png", "blue_8.png", "blue_16.png"]
        );
    }

    #[test]
    fn a_path_without_a_placeholder_keeps_its_suffix_names() {
        assert_eq!(
            output_paths("@artboard 32x32\n@target icon.png scale:.5,1,2"),
            ["icon@0.5x.png", "icon.png", "icon@2x.png"]
        );
    }

    #[test]
    fn an_svg_fills_placeholders_at_scale_one() {
        assert_eq!(
            output_paths("@artboard 24x12\n@target s_{w}x{h}_{scale}.svg scale:1,2"),
            ["s_24x12_1.svg"]
        );
    }

    #[test]
    fn a_gif_names_each_scale_like_a_png() {
        assert_eq!(
            output_paths("@artboard 24x12\n@target g_{w}x{h}_{scale}.gif scale:.5,2 bg:#ffffff"),
            ["g_12x6_0.5.gif", "g_48x24_2.gif"]
        );
        assert_eq!(
            output_paths("@artboard 8x8\n@target t.gif scale:1,2 bg:#ffffff"),
            ["t.gif", "t@2x.gif"]
        );
    }

    #[test]
    fn colliding_sized_names_are_a_duplicate_output() {
        let p = project(&[("art/k.ramo", "@artboard 8x8\n@target i_{h}.png scale:.4,.5\nrect 1\n")]);
        let plan = plan(&p);
        assert!(
            plan.diagnostics.iter().any(|d| d.code == "E0202"),
            "{:?}",
            plan.diagnostics
        );
    }

    #[test]
    fn a_w_or_h_axis_collides_with_the_size_placeholders() {
        for axis in ["w", "h"] {
            let p = project(&[(
                "art/k.ramo",
                &format!("@artboard 8x8\n@target t_{{{axis}}}.png each {axis}:[1 2]\nrect 1\n"),
            )]);
            let plan = plan(&p);
            let hit = plan
                .diagnostics
                .iter()
                .find(|d| d.code == "E0206" && d.message.contains(&format!("`{axis}`")));
            assert!(hit.is_some(), "{:?}", plan.diagnostics);
            assert!(plan.jobs.is_empty());
        }
    }

    #[test]
    fn an_unknown_path_variable_is_still_e0207() {
        let p = project(&[("art/k.ramo", "@artboard 8x8\n@target t_{x}.png\nrect 1\n")]);
        let plan = plan(&p);
        assert!(
            plan.diagnostics.iter().any(|d| d.code == "E0207"),
            "{:?}",
            plan.diagnostics
        );
    }

    #[test]
    fn a_png_bakes_at_its_own_scale_and_an_svg_never_below_one() {
        let bake = |target: &str| {
            let p = project(&[("art/k.ramo", &format!("{target}\nrect 1\n"))]);
            let job = &plan(&p).jobs[0];
            job.outputs.iter().map(|o| job.bake_scale(o)).collect::<Vec<_>>()
        };
        assert_eq!(bake("@artboard 8x8\n@target t.png scale:.5"), [0.5]);
        assert_eq!(bake("@artboard 8x8\n@target t.png scale:.5,.25"), [0.5, 0.25]);
        assert_eq!(bake("@artboard 8x8\n@target t.svg scale:.5"), [1.0]);
        assert_eq!(bake("@artboard 8x8\n@target t.svg scale:1,2"), [2.0]);
    }

    #[test]
    fn a_gif_writes_one_output_per_scale_and_bakes_at_the_highest() {
        let p = project(&[(
            "art/k.ramo",
            "@artboard 8x8\n@target t.gif scale:.5,2 bg:#ffffff\nrect 1\n",
        )]);
        let plan = plan(&p);
        assert!(plan.diagnostics.is_empty(), "{:?}", plan.diagnostics);
        assert_eq!(plan.jobs.len(), 1);
        let job = &plan.jobs[0];
        let outputs: Vec<_> = job.outputs.iter().map(|o| (o.path.as_str(), o.framing.scale)).collect();
        assert_eq!(outputs, [("t@0.5x.gif", 0.5), ("t@2x.gif", 2.0)]);
        assert_eq!(job.sample_scale, 2.0);
        assert_eq!(job.draw_scale(&job.outputs[0]), 0.5);
    }

    #[test]
    fn a_non_list_after_an_axis_stays_an_axis() {
        let p = project(&[("art/k.ramo", "@artboard 8x8\n@target t.png each a:[x] b:oops\nrect 1\n")]);
        let plan = plan(&p);
        assert!(
            plan.diagnostics
                .iter()
                .any(|d| d.code == "E0206" && d.message.contains("`b`")),
            "{:?}",
            plan.diagnostics
        );
        assert!(plan.jobs.is_empty(), "{:?}", plan.jobs);
    }

    #[test]
    fn a_known_key_after_each_stays_a_target_key() {
        let p = project(&[(
            "art/k.ramo",
            "@artboard 8x8\n@target t.png each a:[x] clip:go fps:12\nrect 1\n",
        )]);
        let plan = plan(&p);
        assert!(plan.diagnostics.is_empty(), "{:?}", plan.diagnostics);
        assert_eq!(plan.jobs.len(), 1);
        assert_eq!(plan.jobs[0].clip.as_deref(), Some("go"));
        assert_eq!(plan.jobs[0].fps, 12.0);
    }

    #[test]
    fn duplicate_outputs_are_e0202_naming_both_lines() {
        let p = project(&[
            ("art/a.ramo", "@artboard 8x8\n@target x.png\nrect 1x1\n"),
            ("art/b.ramo", "\n@artboard 8x8\n@target x.png\nrect 1x1\n"),
        ]);
        let plan = plan(&p);
        assert_eq!(plan.diagnostics.len(), 1);
        let d = &plan.diagnostics[0];
        assert_eq!(
            (d.code.as_str(), d.file.as_str(), d.span.line),
            ("E0202", "art/b.ramo", 3)
        );
        assert_eq!(d.notes, ["first declared at art/a.ramo:2"]);
        assert_eq!(plan.jobs.len(), 1);
    }

    #[test]
    fn a_target_frames_the_artboard_into_its_own_size() {
        let p = project(&[(
            "art/t.ramo",
            "@artboard 64x32 viewport\n@target a.png\n@target b.png 48x48 fit:cover align:left,bottom\n@target c_{w}x{h}.png 48x48 scale:1,2\nrect 1x1\n",
        )]);
        let plan = plan(&p);
        assert!(plan.diagnostics.is_empty(), "{:?}", plan.diagnostics);
        let artboard = Size::new(64.0, 32.0).unwrap();
        assert!(plan.jobs.iter().all(|j| (j.artboard.size, j.artboard.viewport) == (artboard, true)));
        let out: Vec<(&str, Size, f32, Fit, Align)> = plan
            .jobs
            .iter()
            .flat_map(|j| j.outputs.iter())
            .map(|o| (o.path.as_str(), o.framing.out, o.framing.scale, o.framing.fit, o.framing.align))
            .collect();
        let square = Size::new(48.0, 48.0).unwrap();
        assert_eq!(
            out,
            [
                ("a.png", artboard, 1.0, Fit::Contain, Align::default()),
                ("b.png", square, 1.0, Fit::Cover, Align { x: Edge::Start, y: Edge::End }),
                ("c_48x48.png", square, 1.0, Fit::Contain, Align::default()),
                ("c_96x96.png", square, 2.0, Fit::Contain, Align::default()),
            ]
        );
    }

    #[test]
    fn every_artboard_and_framing_mistake_is_e0206_with_its_fix() {
        let align_fix = "write `align:left,top`: x is left, center or right, then y is top, center or bottom";
        let only_viewport = "only the word `viewport` follows the size";
        let size_fix = "write `@artboard 512x512`";
        let rows: &[(&str, u32, &str, &str)] = &[
            ("@target t.png 64x64\nrect 1x1\n", 1, "an entry file needs one `@artboard`", "add `@artboard 64x64`"),
            ("@target t.png\nrect 1x1\n", 1, "an entry file needs one `@artboard`", "add `@artboard 512x512`"),
            ("@artboard 8x8\n@artboard 9x9\n@target t.png\n", 2, "`@artboard` is already set at line 1", "remove this line"),
            ("@artboard 0x10\n@target t.png\n", 1, "`@artboard` needs a positive WxH size", size_fix),
            ("@artboard 10\n@target t.png\n", 1, "`@artboard` needs a positive WxH size", size_fix),
            ("@artboard\n@target t.png\n", 1, "`@artboard` needs a positive WxH size", size_fix),
            ("@artboard 64x64 foo\n@target t.png\n", 1, "unknown artboard word `foo`", only_viewport),
            ("@artboard 64x64 bg:none\n@target t.png\n", 1, "unknown artboard key `bg`", only_viewport),
            ("@artboard 8x8\n@target t.png viewport\n", 2, "`viewport` belongs to the artboard", "move it to `@artboard WxH viewport`"),
            ("@artboard 8x8\n@target t.png 8x8 viewport\n", 2, "`viewport` belongs to the artboard", "move it to `@artboard WxH viewport`"),
            ("@artboard 8x8\n@target t.png 8x8 fit:stretch\n", 2, "unknown fit `stretch`", "use contain, cover, or none"),
            ("@artboard 8x8\n@target t.png 8x8 align:middle\n", 2, "unknown align `middle`", align_fix),
            ("@artboard 8x8\n@target t.png 8x8 align:top,left\n", 2, "unknown align `top,left`", align_fix),
            ("@artboard 8x8\n@target t.png fit:cover\n", 2, "`fit:` needs an output size", "add the size, as in `@target x.png 240x240`"),
            ("@artboard 8x8\n@target t.png align:left,top\n", 2, "`align:` needs an output size", "add the size, as in `@target x.png 240x240`"),
            ("@artboard 8x8\n@target t.png 8x8 each fit:[cover]\n", 2, "`fit:` needs a word", "use contain, cover, or none"),
        ];
        for (src, line, message, fix) in rows {
            let p = project(&[("art/t.ramo", src)]);
            let plan = plan(&p);
            let d = plan.diagnostics.first().unwrap_or_else(|| panic!("{src}: no diagnostic"));
            assert_eq!(
                (d.code.as_str(), d.span.line, d.message.as_str(), d.fix.as_deref()),
                ("E0206", *line, *message, Some(*fix)),
                "{src}"
            );
            assert_eq!(plan.diagnostics.len(), 1, "{src}: {:?}", plan.diagnostics);
            assert!(plan.jobs.is_empty(), "{src}");
        }
        let p = project(&[("art/l.ramo", "@artboard 8x8\ndef x\n  rect 1x1\n")]);
        let plan = plan(&p);
        let d = &plan.diagnostics[0];
        assert_eq!(
            (d.code.as_str(), d.message.as_str(), d.fix.as_deref()),
            ("E0206", "a library has no artboard", Some("remove it, or add a `@target`"))
        );
    }

    #[test]
    fn targets_under_a_lib_root_are_e0203() {
        let p = project(&[("lib/x.ramo", "@artboard 8x8\n@target x.png\nrect 1x1\n")]);
        let plan = plan(&p);
        assert_eq!(plan.diagnostics[0].code, "E0203");
        assert!(plan.jobs.is_empty());
    }
}
