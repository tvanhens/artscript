//! AST to scene IR: modules, palettes, styles, components, slots, overrides, checks.
// `Diagnostic` is the error type every layer shares and lives in artscript-syntax.
#![allow(clippy::result_large_err)]

mod builtins;
mod checks;
pub mod color;
mod eval;
mod explain;
mod fx;
mod geom;
mod identity;
mod key;
mod lints;
mod motion;
mod paint;
pub mod project;
mod range;
mod scope;
mod seed;
pub mod targets;
mod value;

use std::collections::{BTreeMap, BTreeSet};

use artscript_ir::Scene;
use artscript_syntax::Diagnostic;
use artscript_syntax::ast::DeclKind;

pub use explain::explain;
pub use key::{frame_count, frame_delays};
pub use project::{BackendChoice, LintLevel, Manifest, Project, find_root, load};
pub use seed::fnv1a;
pub use targets::{Format, MatrixValue, OutputSpec, Plan, TargetJob, plan};

use crate::color::Rgba;
use crate::eval::{Evaluator, PlayMark};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Byte ceiling for one GIF raster and for the scenes one `sample` keeps.
pub const GIF_CLIP_BUDGET: u64 = 256 * 1024 * 1024;

pub struct Evaluation {
    /// `None` when the entry file could not be evaluated at all.
    pub scene: Option<Scene>,
    pub diagnostics: Vec<Diagnostic>,
    /// Every `play` the frame reached, including ones outside their active window.
    pub plays: Vec<PlayMark>,
    /// Node keys that drew a key-derived seed. `explain` prints `seed` on these.
    pub seeded: BTreeSet<String>,
    pub rig: Rig,
}

/// What a scene exposes to the program that places its image, in canvas units.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Rig {
    /// The `pivot:` point, `None` when the target has no `pivot:`.
    pub pivot: Option<(f64, f64)>,
    /// Every anchor the scene placed, keyed by the path an author writes after `@`.
    pub anchors: BTreeMap<String, (f64, f64)>,
}

pub struct Frame {
    pub t: f64,
    pub scene: Scene,
}

pub struct Sample {
    pub frames: Vec<Frame>,
    pub diagnostics: Vec<Diagnostic>,
    pub loops: bool,
    pub dur: f64,
    pub fps: f64,
}

/// Evaluates one target into a scene, with every diagnostic, check failure and lint it produced.
pub fn evaluate(project: &Project, job: &TargetJob) -> Evaluation {
    let mut ev = evaluate_frame(project, job);
    ev.diagnostics = fold_copies(ev.diagnostics);
    ev
}

/// One target's scene drawn at one scale, with the rig that scene placed.
pub struct ScaleScene {
    /// The scale the scene was evaluated at, as `TargetJob::sample_scale` names it.
    pub scale: f32,
    pub scene: Scene,
    pub rig: Rig,
}

pub struct OutputScenes {
    /// Every scale's diagnostics, a repeat of one scale-independent diagnostic kept once.
    pub diagnostics: Vec<Diagnostic>,
    /// One scene per distinct draw scale, in output order. A scale that could not be evaluated has none.
    pub scenes: Vec<ScaleScene>,
}

impl OutputScenes {
    pub fn at(&self, scale: f32) -> Option<&ScaleScene> {
        self.scenes.iter().find(|s| s.scale == scale)
    }
}

/// Evaluates a target once for each scale its outputs draw at, because a file reads `scale` and
/// may draw differently at each.
pub fn evaluate_outputs(project: &Project, job: &TargetJob) -> OutputScenes {
    let mut scales: Vec<f32> = Vec::new();
    for out in &job.outputs {
        let scale = job.sample_scale(out);
        if !scales.contains(&scale) {
            scales.push(scale);
        }
    }
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut scenes = Vec::new();
    for scale in scales {
        let at = TargetJob {
            bake_scale: scale,
            ..job.clone()
        };
        let ev = evaluate(project, &at);
        for d in ev.diagnostics {
            if !diagnostics.contains(&d) {
                diagnostics.push(d);
            }
        }
        scenes.extend(ev.scene.map(|scene| ScaleScene {
            scale,
            scene,
            rig: ev.rig,
        }));
    }
    OutputScenes {
        diagnostics,
        scenes,
    }
}

/// `evaluate` with each loop copy's diagnostic kept apart, so `sample` can fold them once across frames.
fn evaluate_frame(project: &Project, job: &TargetJob) -> Evaluation {
    let mut ev = Evaluator::new(project, job);
    let module = match ev.load_module(&job.source, true) {
        Ok(m) => m,
        Err(f) => {
            ev.report(f, &job.source, job.span, "");
            return Evaluation {
                scene: None,
                diagnostics: ev.diags,
                plays: Vec::new(),
                seeded: BTreeSet::new(),
                rig: Rig::default(),
            };
        }
    };
    let loaded = ev.diags.len();
    let file = ev.parsed[&job.source].clone();
    let mut sc = ev.top_scope(&module);
    let scenes: Vec<_> = file.scenes().cloned().collect();
    ev.collect_entry(&scenes, &mut sc);
    let children = ev.eval_block_nodes(file.scenes(), &mut sc);
    ev.report_unlanded(&sc);
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
        None => ev
            .background
            .or_else(|| project.manifest.bg.as_deref().and_then(Rgba::hex)),
    };
    let pivot = job.pivot.as_ref().and_then(|e| match ev.pivot(e, &sc) {
        Ok(p) => Some(p),
        Err(f) => {
            ev.report(f, &job.source, job.span, "");
            None
        }
    });
    let rig = Rig {
        pivot,
        anchors: scope::placed_anchors(&sc.names),
    };
    let scene = Scene {
        width: job.size.0,
        height: job.size.1,
        background: background.map(Rgba::to_ir),
        children,
    };
    let plays = ev.plays.clone();
    let seeded = ev.seeded().clone();
    let mut diagnostics = std::mem::take(&mut ev.diags);
    diagnostics.extend(lints::positional_seed(
        ev.positional(),
        &project.manifest,
    ));
    stamp_target(job, &mut diagnostics[loaded..]);
    let mut files: Vec<&String> = ev.parsed.keys().collect();
    files.sort();
    for f in files {
        let parsed = &ev.parsed[f];
        diagnostics.extend(lints::raw_color(parsed, f, &project.manifest));
        diagnostics.extend(lints::unused(parsed, f, f != &job.source, &project.manifest));
    }
    let mut placement = Vec::new();
    if !job.viewport {
        placement = lints::off_canvas(&scene, &project.manifest, &job.source);
    }
    placement.extend(lints::hidden_child(&scene, &project.manifest, &job.source));
    stamp_target(job, &mut placement);
    diagnostics.extend(placement);
    Evaluation {
        scene: Some(scene),
        diagnostics,
        plays,
        seeded,
        rig,
    }
}

/// Diagnostics that differ only in their node, such as one failure in every copy of a loop, kept
/// as the first copy with the note `also in <node>, ...`.
fn fold_copies(diags: Vec<Diagnostic>) -> Vec<Diagnostic> {
    let mut out: Vec<Diagnostic> = Vec::new();
    let mut also: Vec<Vec<String>> = Vec::new();
    for d in diags {
        let first = out.iter().position(|o| {
            (o.severity, &o.code, &o.file, o.span, &o.message, &o.notes, &o.fix)
                == (d.severity, &d.code, &d.file, d.span, &d.message, &d.notes, &d.fix)
        });
        match first {
            Some(i) => {
                if !d.node.is_empty() && d.node != out[i].node && !also[i].contains(&d.node) {
                    also[i].push(d.node);
                }
            }
            None => {
                out.push(d);
                also.push(Vec::new());
            }
        }
    }
    for (d, nodes) in out.iter_mut().zip(also) {
        if !nodes.is_empty() {
            d.notes.push(format!("also in {}", nodes.join(", ")));
        }
    }
    out
}

/// Frames `i` at `t = i/fps` for `t` in `[0, dur)`. Checks run on every frame and a failure is reported once.
pub fn sample(project: &Project, job: &TargetJob) -> Sample {
    let fps = if job.fps.is_finite() && job.fps > 0.0 {
        job.fps
    } else {
        24.0
    };
    let mut probe_job = job.clone();
    probe_job.frame_t = 0.0;
    let probe = evaluate_frame(project, &probe_job);
    let dur = job
        .dur
        .unwrap_or_else(|| probe.plays.iter().map(|p| p.end).fold(0.0, f64::max));
    let mut loops = probe.plays.iter().any(|p| p.loops);
    let n = match frame_count(dur, fps) {
        Ok(n) => n,
        Err(msg) => {
            let mut diagnostics = fold_copies(probe.diagnostics);
            let own = diagnostics.len();
            diagnostics.push(
                Diagnostic::error("E0206", job.span, msg)
                    .in_file(&job.source)
                    .with_fix("lower `fps` or shorten `dur`"),
            );
            stamp_target(job, &mut diagnostics[own..]);
            return Sample {
                frames: Vec::new(),
                diagnostics,
                loops,
                dur,
                fps,
            };
        }
    };
    let mut frames = Vec::new();
    let mut acc = AggAcc::default();
    let mut kept = 0u64;
    let mut peak = 0u64;
    let mut retained_error = None;
    if n == 0 {
        acc.fold(0.0, &probe.diagnostics);
    } else if let Some(err) = probe
        .scene
        .as_ref()
        .and_then(|scene| gif_retained_error(job, scene, &mut kept, &mut peak, n))
    {
        retained_error = Some(err);
        acc.fold(0.0, &probe.diagnostics);
    } else {
        if let Some(scene) = probe.scene {
            frames.push(Frame { t: 0.0, scene });
        }
        acc.fold(0.0, &probe.diagnostics);
        for i in 1..n {
            let t = i as f64 / fps;
            let mut frame_job = job.clone();
            frame_job.frame_t = t;
            let ev = evaluate_frame(project, &frame_job);
            loops |= ev.plays.iter().any(|p| p.loops);
            let left = n - frames.len();
            if let Some(err) = ev
                .scene
                .as_ref()
                .and_then(|scene| gif_retained_error(job, scene, &mut kept, &mut peak, left))
            {
                retained_error = Some(err);
                frames.clear();
                acc.fold(t, &ev.diagnostics);
                break;
            }
            if let Some(scene) = ev.scene {
                frames.push(Frame { t, scene });
            }
            acc.fold(t, &ev.diagnostics);
        }
    }
    let mut diagnostics = fold_copies(acc.finish());
    let own = diagnostics.len();
    if let Some(d) = retained_error {
        diagnostics.push(d);
    } else if !diagnostics.iter().any(|d| d.code.starts_with('E'))
        && let Some(d) = gif_timing_error(job, frames.len(), dur)
            .or_else(|| gif_background_error(job, &frames))
            .or_else(|| gif_dimension_error(job, &frames))
            .or_else(|| gif_raster_error(job, &frames))
    {
        diagnostics.push(d);
    }
    stamp_target(job, &mut diagnostics[own..]);
    Sample {
        frames,
        diagnostics,
        loops,
        dur,
        fps,
    }
}

/// The duration and centisecond checks `build` uses before it encodes. `check` reads them from `sample`.
fn gif_timing_error(job: &TargetJob, frames: usize, dur: f64) -> Option<Diagnostic> {
    if frames == 0 || dur <= 0.0 {
        return Some(
            Diagnostic::error("E0206", job.span, "clip has no duration")
                .in_file(&job.source)
                .with_fix("add a `play`, or set `dur:` on the target"),
        );
    }
    let msg = frame_delays(frames, dur).err()?;
    let fix = match msg {
        "a gif frame is longer than 65535cs" => {
            "raise `fps` or shorten `dur` so each frame is at most 65535cs"
        }
        _ => "lower `fps` or lengthen `dur` so each frame lasts at least 1cs",
    };
    Some(
        Diagnostic::error("E0206", job.span, msg)
            .in_file(&job.source)
            .with_fix(fix),
    )
}

/// The declared-background check `build` uses before it encodes. Pixel alpha still waits for the encoder.
fn gif_background_error(job: &TargetJob, frames: &[Frame]) -> Option<Diagnostic> {
    if !job.outputs.iter().any(|o| o.format == Format::Gif) {
        return None;
    }
    let opaque = frames.iter().all(|frame| {
        frame
            .scene
            .background
            .is_some_and(|color| color.a >= 1.0 - 1e-4)
    });
    if opaque {
        return None;
    }
    Some(
        Diagnostic::error("E0206", job.span, "a gif needs an opaque `bg:`")
            .in_file(&job.source)
            .with_fix("set `bg:` to an opaque color; transparent gifs come later"),
    )
}

fn gif_dimension_error(job: &TargetJob, frames: &[Frame]) -> Option<Diagnostic> {
    let scene = &frames.first()?.scene;
    let over = job.outputs.iter().filter(|o| o.format == Format::Gif).any(|o| {
        let w = (scene.width * o.scale).ceil();
        let h = (scene.height * o.scale).ceil();
        w > u16::MAX as f32 || h > u16::MAX as f32
    });
    if !over {
        return None;
    }
    Some(
        Diagnostic::error("E0206", job.span, "a gif is larger than 65535px")
            .in_file(&job.source)
            .with_fix("shrink the canvas"),
    )
}

fn gif_raster_bytes(width: f32, height: f32, scale: f32, frames: usize) -> u64 {
    let w = (width * scale).ceil().max(1.0);
    let h = (height * scale).ceil().max(1.0);
    if !w.is_finite() || !h.is_finite() {
        return u64::MAX;
    }
    (w as u64)
        .saturating_mul(h as u64)
        .saturating_mul(4)
        .saturating_mul(frames as u64)
}

fn gif_retained_error(
    job: &TargetJob,
    scene: &Scene,
    kept: &mut u64,
    peak: &mut u64,
    frames_left: usize,
) -> Option<Diagnostic> {
    if !job.outputs.iter().any(|o| o.format == Format::Gif) {
        return None;
    }
    let bytes = scene.retained_bytes();
    *peak = (*peak).max(bytes);
    if projected_retention(*kept, *peak, frames_left) > GIF_CLIP_BUDGET {
        let mib = GIF_CLIP_BUDGET / (1024 * 1024);
        return Some(
            Diagnostic::error(
                "E0206",
                job.span,
                format!("a gif retains more than {mib}MiB"),
            )
            .in_file(&job.source)
            .with_fix("lower `fps`, shorten `dur`, or draw less in each frame"),
        );
    }
    *kept = kept.saturating_add(bytes);
    None
}

fn projected_retention(kept: u64, peak: u64, frames_left: usize) -> u64 {
    kept.saturating_add(peak.saturating_mul(frames_left as u64))
}

/// The raster cap `build` enforces before it encodes. `check` reads it from `sample`.
fn gif_raster_error(job: &TargetJob, frames: &[Frame]) -> Option<Diagnostic> {
    let scene = &frames.first()?.scene;
    let over = job
        .outputs
        .iter()
        .filter(|o| o.format == Format::Gif)
        .any(|o| {
            gif_raster_bytes(scene.width, scene.height, o.scale, frames.len()) > GIF_CLIP_BUDGET
        });
    if !over {
        return None;
    }
    let mib = GIF_CLIP_BUDGET / (1024 * 1024);
    Some(
        Diagnostic::error(
            "E0206",
            job.span,
            format!("a gif raster is larger than {mib}MiB"),
        )
        .in_file(&job.source)
        .with_fix("lower `fps`, shorten `dur`, or shrink the canvas"),
    )
}

#[cfg(test)]
fn aggregate_frames(frames: &[(f64, Vec<Diagnostic>)]) -> Vec<Diagnostic> {
    let mut acc = AggAcc::default();
    for (t, diags) in frames {
        acc.fold(*t, diags);
    }
    acc.finish()
}

#[derive(Default)]
struct AggAcc {
    out: Vec<Diagnostic>,
    groups: Vec<Agg>,
    frames: usize,
}

impl AggAcc {
    fn fold(&mut self, t: f64, diags: &[Diagnostic]) {
        let i = self.frames;
        self.frames += 1;
        for d in diags {
            if d.code.starts_with('L') && !folded_lint(&d.code) {
                continue;
            }
            let message = if folded_lint(&d.code) {
                String::new()
            } else {
                d.message.clone()
            };
            let key = (
                d.file.clone(),
                d.span.line,
                d.span.col,
                d.code.clone(),
                message,
                d.node.clone(),
                d.notes
                    .iter()
                    .filter(|n| is_site_note(n))
                    .cloned()
                    .collect::<Vec<_>>(),
            );
            if let Some(agg) = self.groups.iter_mut().find(|g| g.key == key) {
                agg.frames += 1;
                let mag = note_magnitude(d);
                if let Some(mag) = mag
                    && (!agg.has_mag || mag > agg.mag)
                {
                    agg.has_mag = true;
                    agg.mag = mag;
                    agg.worst = t;
                    agg.diag = d.clone();
                } else if mag.is_none() && !agg.has_mag && t > agg.worst {
                    agg.worst = t;
                    agg.diag = d.clone();
                }
            } else {
                let mag = note_magnitude(d);
                self.groups.push(Agg {
                    key,
                    diag: d.clone(),
                    first: t,
                    worst: t,
                    mag: mag.unwrap_or(0.0),
                    has_mag: mag.is_some(),
                    frames: 1,
                });
            }
        }
        if i == 0 {
            for d in diags
                .iter()
                .filter(|d| d.code.starts_with('L') && !folded_lint(&d.code))
            {
                self.out.push(d.clone());
            }
        }
    }

    fn finish(mut self) -> Vec<Diagnostic> {
        let frames = self.frames;
        for mut agg in self.groups {
            if agg.diag.code == "L0105" {
                if agg.frames == frames {
                    self.out.push(agg.diag);
                }
                continue;
            }
            if frames > 1 || agg.diag.code.starts_with('C') || agg.diag.code.starts_with('E') {
                agg.diag
                    .notes
                    .push(format!("first at {}s", num_s(agg.first)));
                agg.diag
                    .notes
                    .push(format!("worst at {}s", num_s(agg.worst)));
            }
            self.out.push(agg.diag);
        }
        self.out
    }
}

struct Agg {
    key: (String, u32, u32, String, String, String, Vec<String>),
    diag: Diagnostic,
    first: f64,
    worst: f64,
    mag: f64,
    has_mag: bool,
    /// The frames that raised it. A child hidden only on some frames, such as an eye
    /// highlight under a blink, is drawn on the others and L0105 drops it.
    frames: usize,
}

fn folded_lint(code: &str) -> bool {
    matches!(code, "L0102" | "L0104" | "L0105")
}

/// A note naming the call site that produced a failure. It separates failures and is not a magnitude.
fn is_site_note(note: &str) -> bool {
    note.starts_with("via rule ") || note.starts_with("played at ") || is_entry_site(note)
}

/// The `in art/a.art:4` note: the entry-file line that reached a library's diagnostic.
pub(crate) fn is_entry_site(note: &str) -> bool {
    note.starts_with("in ") && !note.contains('`')
}

/// Names `job` on each diagnostic its evaluation raised in its entry file or reached from it. A
/// library's diagnostic with no entry line is about that library's text, so it stays unnamed and
/// prints once. Callers leave out text lints and load errors for the same reason.
fn stamp_target(job: &TargetJob, diags: &mut [Diagnostic]) {
    let target = job.outputs.first().map(|o| o.path.as_str()).unwrap_or_default();
    for d in diags {
        if d.file == job.source || d.notes.iter().any(|n| is_entry_site(n)) {
            d.target = target.to_string();
        }
    }
}

fn note_magnitude(d: &Diagnostic) -> Option<f64> {
    let mut best = None;
    for note in &d.notes {
        if is_site_note(note) {
            continue;
        }
        for token in note.split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-')) {
            if token.is_empty() || token == "-" || token == "." {
                continue;
            }
            if let Ok(n) = token.parse::<f64>()
                && n.is_finite()
            {
                let a = n.abs();
                if best.map(|b: f64| a > b).unwrap_or(true) {
                    best = Some(a);
                }
            }
        }
    }
    best
}

fn num_s(t: f64) -> String {
    let rounded = (t * 1e6).round() / 1e6;
    let raw = format!("{rounded:.6}");
    raw.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// The entry file plus every library it transitively imports (and the default palette),
/// as project-relative paths. Used for cache keys.
pub fn dependencies(project: &Project, entry: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut stack = vec![entry.to_string()];
    if let Some(p) = project
        .manifest
        .palette
        .as_deref()
        .and_then(|p| project.resolve_use(p))
    {
        stack.push(p);
    }
    while let Some(f) = stack.pop() {
        if !seen.insert(f.clone()) {
            continue;
        }
        let Some(src) = project.files.get(&f) else {
            continue;
        };
        let Ok(file) = artscript_syntax::parse(src) else {
            continue;
        };
        for d in file.decls() {
            if let DeclKind::Use { path, .. } = &d.kind {
                let rooted = format!("{path}.art");
                if let Some(t) = project
                    .resolve_use(path)
                    .or(project.files.contains_key(&rooted).then_some(rooted))
                {
                    stack.push(t);
                }
            }
        }
    }
    seen.into_iter().collect()
}

#[cfg(test)]
mod tests;
