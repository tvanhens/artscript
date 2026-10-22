//! Shared input decoding and oracles for the fuzz targets.

use std::collections::BTreeMap;

use ramose_eval::{
    Evaluation, Format, Project, Sample, TargetJob, dependencies, evaluate, explain, frame_delays, plan, sample,
};
use ramose_ir::{Framing, Profile};
use ramose_render::{Backend, raster_only_features, render_gif, render_png, render_svg};
use ramose_syntax::{Diagnostic, Severity};

pub mod generator;

pub const MANIFEST: &str = r#"[project]
name = "fuzz"
out = "out"
lib = ["lib"]

[defaults]
palette = "palette/house"
scale = [1]
bg = "none"
profile = "auto"

[lint]
raw-color = "warn"
unused = "warn"
off-artboard = "warn"
"#;

const LIBS: &[(&str, &str)] = &[
    ("lib/face/parts.ramo", include_str!("../../examples/chibi/lib/face/parts.ramo")),
    ("lib/shade/cel.ramo", include_str!("../../examples/chibi/lib/shade/cel.ramo")),
    ("lib/palette/house.ramo", include_str!("../../examples/chibi/lib/palette/house.ramo")),
];

pub const MAX_FRAMES: f64 = 6.0;

pub const MAX_SIDE: f32 = 48.0;

/// Decodes an eval or render input: the entry file `art/t.ramo`, then NUL-separated files that each
/// start with their project path on the first line. A file named `ramose.toml` replaces
/// [`MANIFEST`]. `None` when that manifest does not parse.
pub fn project(data: &[u8]) -> Option<Project> {
    let text = String::from_utf8_lossy(data);
    let mut parts = text.split('\0');
    let mut files: BTreeMap<String, String> =
        LIBS.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    files.insert("art/t.ramo".into(), parts.next().unwrap_or_default().to_string());
    let mut manifest = MANIFEST.to_string();
    for part in parts {
        let (path, src) = part.split_once('\n').unwrap_or((part, ""));
        if path == ramose_eval::project::MANIFEST {
            manifest = src.to_string();
        } else {
            files.insert(path.to_string(), src.to_string());
        }
    }
    Project::from_sources(&manifest, files).ok()
}

pub fn ast_ignoring_spans(file: &ramose_syntax::ast::File) -> String {
    let mut file = file.clone();
    for d in &mut file.directives {
        d.named.sort_by(|a, b| a.key.cmp(&b.key));
    }
    let text = format!("{file:?}");
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some(i) = rest.find("Span {") {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        rest = &tail[tail.find('}').map_or(tail.len(), |j| j + 1)..];
    }
    out.push_str(rest);
    out
}

pub fn exercise_format(src: &str) {
    let Ok(file) = ramose_syntax::parse(src) else { return };
    let once = ramose_syntax::format(&file);
    let reparsed = ramose_syntax::parse(&once)
        .unwrap_or_else(|e| panic!("formatted output does not parse: {e:?}\n{once}"));
    assert_eq!(ast_ignoring_spans(&file), ast_ignoring_spans(&reparsed), "format changed the program:\n{once}");
    assert_eq!(once, ramose_syntax::format(&reparsed), "format is not a fixed point");

    let kept = ramose_syntax::format_source(src).expect("source parsed once already");
    let again = ramose_syntax::format_source(&kept)
        .unwrap_or_else(|e| panic!("format_source output does not parse: {e:?}\n{kept}"));
    assert_eq!(kept, again, "format_source is not a fixed point");
}

pub fn check_locations(project: &Project, diags: &[Diagnostic]) {
    for d in diags {
        let _ = d.to_string();
        let Some(src) = project.files.get(&d.file) else {
            assert!(
                d.file == ramose_eval::project::MANIFEST,
                "diagnostic in unknown file {:?}: {d:?}",
                d.file
            );
            continue;
        };
        let lines = src.split('\n').count() as u32;
        assert!(
            d.span.line <= lines,
            "diagnostic past the end of {} ({lines} lines): {d:?}",
            d.file
        );
    }
}

pub fn short_clip(job: &TargetJob, probe: &Evaluation) -> Option<TargetJob> {
    let dur = job.dur.unwrap_or_else(|| probe.plays.iter().map(|p| p.end).fold(0.0, f64::max));
    if !dur.is_finite() || dur <= 0.0 {
        return None;
    }
    let mut short = job.clone();
    if !(job.fps.is_finite() && job.fps > 0.0 && job.fps * dur <= MAX_FRAMES) {
        short.fps = MAX_FRAMES / dur;
    }
    Some(short)
}

pub fn is_gif(job: &TargetJob) -> bool {
    job.outputs.iter().any(|o| o.format == Format::Gif)
}

pub fn small_scale(job: &TargetJob, scale: f32) -> Option<f32> {
    let art = job.artboard.size;
    let side = art.w().max(art.h()) * scale;
    if !side.is_finite() || side <= 0.0 {
        return None;
    }
    Some(scale.min(scale * MAX_SIDE / side))
}

pub fn jobs(project: &Project) -> Vec<TargetJob> {
    let plan = plan(project);
    check_locations(project, &plan.diagnostics);
    plan.jobs
}

/// The scene's `Debug` text without the fields Skia fills in lazily: a path's per-process
/// generation counter and a matrix's cached type mask, which `explain` and the renderer read.
pub fn fingerprint(scene: &Option<ramose_ir::Scene>) -> String {
    let mut text = format!("{scene:?}");
    for field in ["generation_id: ", "type_mask: "] {
        let mut out = String::with_capacity(text.len());
        let mut rest = text.as_str();
        while let Some(i) = rest.find(field) {
            out.push_str(&rest[..i]);
            rest = rest[i..].trim_start_matches(field).trim_start_matches(|c: char| c.is_ascii_digit());
        }
        out.push_str(rest);
        text = out;
    }
    text
}

pub fn exercise_eval(p: &Project) {
    let _ = dependencies(p, "art/t.ramo");
    for (i, job) in jobs(p).iter().take(3).enumerate() {
        let ev = evaluate(p, job);
        check_locations(p, &ev.diagnostics);
        if let Some(scene) = &ev.scene {
            let _ = explain(scene, &ev.seeded);
        }
        if i == 0 {
            let again = evaluate(p, job);
            assert_eq!(ev.diagnostics, again.diagnostics, "diagnostics differ between runs");
            assert_eq!(fingerprint(&ev.scene), fingerprint(&again.scene), "scene differs between runs");
        }
        if let Some(clip) = short_clip(job, &ev) {
            check_locations(p, &sample(p, &clip).diagnostics);
        }
    }
}

pub fn exercise_render(p: &Project) {
    let Some(job) = jobs(p).into_iter().next() else { return };
    let ev = evaluate(p, &job);
    if ev.diagnostics.iter().any(|d| d.severity == Severity::Error && d.code.starts_with('E')) {
        return;
    }
    let Some(scene) = &ev.scene else { return };
    let Some(scale) = small_scale(&job, 1.0) else { return };
    let framing = Framing::plain(job.artboard.size, scale);
    let _ = raster_only_features(scene);
    if is_gif(&job) {
        let Some(clip) = short_clip(&job, &ev) else { return };
        let s = sample(p, &clip);
        let Ok(delays) = frame_delays(s.frames.len(), s.dur) else { return };
        if delays.is_empty() {
            return;
        }
        let gif = |s: &Sample| {
            let scenes: Vec<_> = s.frames.iter().map(|f| &f.scene).collect();
            render_gif(&scenes, &framing, &delays, s.loops, Backend::Cpu).ok()
        };
        assert_eq!(gif(&s), gif(&sample(p, &clip)), "gif bytes differ between renders");
        return;
    }
    let render = |scene: &ramose_ir::Scene| {
        let png = render_png(scene, &framing, Backend::Cpu).ok();
        (png, [Profile::Vector, Profile::Svg].map(|profile| render_svg(scene, &framing, profile, scale, Backend::Cpu).ok()))
    };
    let (png, svgs) = render(scene);
    let again = evaluate(p, &job);
    let Some(scene) = &again.scene else { return };
    let (png_again, svgs_again) = render(scene);
    assert_eq!(png, png_again, "png bytes differ between renders");
    assert_eq!(svgs, svgs_again, "svg differs between renders");
}
