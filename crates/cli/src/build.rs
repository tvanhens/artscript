//! `artscript build`: evaluate and render targets in parallel, keep the incremental cache, write
//! `art-manifest.json`, and delete outputs a previous build listed but this one no longer makes.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use artscript_eval::{
    BackendChoice, Format, OutputSpec, Project, TargetJob, dependencies, evaluate, fnv1a, plan,
};
use artscript_ir::Scene;
use artscript_render::Backend;
use artscript_syntax::ast::Span;
use artscript_syntax::{Diagnostic, Severity};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};

use crate::{Selector, exit_code, report, unmatched};

pub const MANIFEST_FILE: &str = "art-manifest.json";
const CACHE_DIR: &str = ".artscript/cache";

#[derive(Default)]
pub struct Options {
    /// A target is built when any glob matches it. Empty builds every target.
    pub only: Vec<Selector>,
    pub allow_warnings: bool,
    /// Overrides the manifest's `[build] backend`.
    pub backend: Option<BackendChoice>,
}

/// What one target produced: output bytes' hashes plus its diagnostics.
struct Outcome {
    job: TargetJob,
    /// `(path relative to out, hash)` for each output written or verified.
    outputs: Vec<(String, String)>,
    diagnostics: Vec<Diagnostic>,
}

/// `Diagnostic` only serializes; the cache stores this mirror so warnings replay on a hit.
#[derive(Serialize, Deserialize)]
struct CachedDiag {
    severity: String,
    code: String,
    file: String,
    line: u32,
    col: u32,
    node: String,
    target: String,
    message: String,
    notes: Vec<String>,
    fix: Option<String>,
}

impl From<&Diagnostic> for CachedDiag {
    fn from(d: &Diagnostic) -> Self {
        let severity = match d.severity {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
        };
        CachedDiag {
            severity: severity.into(),
            code: d.code.clone(),
            file: d.file.clone(),
            line: d.span.line,
            col: d.span.col,
            node: d.node.clone(),
            target: d.target.clone(),
            message: d.message.clone(),
            notes: d.notes.clone(),
            fix: d.fix.clone(),
        }
    }
}

impl From<CachedDiag> for Diagnostic {
    fn from(c: CachedDiag) -> Self {
        let severity = match c.severity.as_str() {
            "info" => Severity::Info,
            "warning" => Severity::Warning,
            _ => Severity::Error,
        };
        Diagnostic {
            severity,
            code: c.code,
            file: c.file,
            span: Span {
                line: c.line,
                col: c.col,
            },
            node: c.node,
            target: c.target,
            message: c.message,
            notes: c.notes,
            fix: c.fix,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct CacheEntry {
    key: String,
    outputs: Vec<(String, String)>,
    diagnostics: Vec<CachedDiag>,
}

fn hex(bytes: &[u8]) -> String {
    format!("{:016x}", fnv1a(bytes))
}

fn cache_key(project: &Project, job: &TargetJob, backend: Backend) -> String {
    let mut buf = Vec::new();
    for dep in dependencies(project, &job.source) {
        buf.extend_from_slice(dep.as_bytes());
        buf.push(0);
        buf.extend_from_slice(
            project
                .files
                .get(&dep)
                .map(String::as_bytes)
                .unwrap_or_default(),
        );
        buf.push(0);
    }
    buf.extend_from_slice(project.manifest_src.as_bytes());
    buf.push(0);
    buf.extend_from_slice(renderer_id().as_bytes());
    buf.push(0);
    buf.extend_from_slice(format!("{backend:?}").as_bytes());
    buf.push(0);
    buf.extend_from_slice(job.key().as_bytes());
    hex(&buf)
}

/// The crate version stays put while the renderer changes, so outputs are keyed on the executable itself.
fn renderer_id() -> &'static str {
    static ID: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    ID.get_or_init(|| {
        std::env::current_exe()
            .and_then(fs::read)
            .map(|bytes| hex(&bytes))
            .unwrap_or_else(|_| artscript_eval::VERSION.to_string())
    })
}

fn cache_path(root: &Path, job: &TargetJob) -> std::path::PathBuf {
    root.join(CACHE_DIR)
        .join(format!("{}.json", hex(job.key().as_bytes())))
}

/// The cached outcome when the key matches and every output on disk still has its hash.
fn cache_hit(project: &Project, out_dir: &Path, job: &TargetJob, key: &str) -> Option<Outcome> {
    let entry: CacheEntry =
        serde_json::from_str(&fs::read_to_string(cache_path(&project.root, job)).ok()?).ok()?;
    if entry.key != key || entry.outputs.len() != job.outputs.len() {
        return None;
    }
    for (path, hash) in &entry.outputs {
        if &hex(&fs::read(out_dir.join(path)).ok()?) != hash {
            return None;
        }
    }
    Some(Outcome {
        job: job.clone(),
        outputs: entry.outputs,
        diagnostics: entry
            .diagnostics
            .into_iter()
            .map(Diagnostic::from)
            .collect(),
    })
}

fn render_error(job: &TargetJob, e: artscript_render::RenderError) -> Diagnostic {
    let span = job.span;
    match e {
        artscript_render::RenderError::NotVector(list) => {
            let mut d = Diagnostic::error(
                "E0209",
                span,
                "`profile:vector` target uses raster-only features",
            )
            .in_file(&job.source);
            for u in list {
                d = d.with_note(format!("{}: {}", u.node, u.feature));
            }
            d.with_fix("use `profile:svg` to bake those parts into embedded PNG tiles, or remove the raster-only features")
        }
        artscript_render::RenderError::Skia(msg) => {
            Diagnostic::error("E0400", span, format!("render failed: {msg}")).in_file(&job.source)
        }
        artscript_render::RenderError::TransparentGif => {
            Diagnostic::error("E0206", span, "a gif needs an opaque `bg:`")
                .in_file(&job.source)
                .with_fix("set `bg:` to an opaque color; transparent gifs come later")
        }
        artscript_render::RenderError::GifTooLarge => {
            let mib = artscript_render::RASTER_BUDGET / (1024 * 1024);
            Diagnostic::error(
                "E0206",
                span,
                format!("a gif raster is larger than {mib}MiB"),
            )
            .in_file(&job.source)
            .with_fix("lower `fps`, shorten `dur`, or shrink the canvas")
        }
    }
}

fn run_job(project: &Project, out_dir: &Path, job: &TargetJob, backend: Backend) -> Outcome {
    let key = cache_key(project, job, backend);
    if let Some(hit) = cache_hit(project, out_dir, job, &key) {
        return hit;
    }
    if job.outputs.iter().any(|o| o.format == Format::Gif) {
        return run_gif(project, out_dir, job, key, backend);
    }
    let ev = evaluate(project, job);
    let mut diagnostics = ev.diagnostics;
    let mut outputs = Vec::new();
    let compile_error = diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error && d.code.starts_with('E'));
    if let (Some(scene), false) = (ev.scene, compile_error) {
        let mut scenes = vec![(job.bake_scale, scene)];
        for out in &job.outputs {
            if out.format == Format::Png && scenes.iter().all(|(s, _)| *s != out.scale) {
                let at = TargetJob {
                    bake_scale: out.scale,
                    ..job.clone()
                };
                scenes.extend(evaluate(project, &at).scene.map(|sc| (out.scale, sc)));
            }
        }
        let render = |out: &OutputSpec, scene: &Scene| match out.format {
            Format::Png => artscript_render::render_png(scene, out.scale, backend),
            Format::Svg => artscript_render::render_svg(scene, job.profile, job.bake_scale, backend)
                .map(String::into_bytes),
            Format::Gif => Err(artscript_render::RenderError::Skia(
                "gif is rendered through sample".into(),
            )),
        };
        // A scene's paths are not `Sync`, so each thread takes its own copy.
        let rendered: Vec<_> = std::thread::scope(|s| {
            let threads: Vec<_> = job
                .outputs
                .iter()
                .map(|out| {
                    let sample = match out.format {
                        Format::Png => out.scale,
                        Format::Svg | Format::Gif => job.bake_scale,
                    };
                    let scene = scenes.iter().find(|(s, _)| *s == sample).map(|(_, sc)| sc.clone());
                    s.spawn(move || scene.map(|scene| render(out, &scene)))
                })
                .collect();
            threads.into_iter().map(|t| t.join().expect("render thread panicked")).collect()
        });
        for (out, bytes) in job.outputs.iter().zip(rendered) {
            let Some(bytes) = bytes else {
                continue;
            };
            let bytes = match bytes {
                Ok(b) => b,
                Err(e) => {
                    diagnostics.push(render_error(job, e));
                    continue;
                }
            };
            let path = out_dir.join(&out.path);
            let written = path
                .parent()
                .map_or(Ok(()), fs::create_dir_all)
                .and_then(|_| fs::write(&path, &bytes));
            match written {
                Ok(()) => outputs.push((out.path.clone(), hex(&bytes))),
                Err(e) => diagnostics.push(
                    Diagnostic::error("E0400", job.span, format!("cannot write {}: {e}", out.path))
                        .in_file(&job.source),
                ),
            }
        }
    }
    if outputs.len() == job.outputs.len() {
        let entry = CacheEntry {
            key,
            outputs: outputs.clone(),
            diagnostics: diagnostics.iter().map(CachedDiag::from).collect(),
        };
        let path = cache_path(&project.root, job);
        let _ = path
            .parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|_| fs::write(&path, serde_json::to_string(&entry).unwrap_or_default()));
    }
    Outcome {
        job: job.clone(),
        outputs,
        diagnostics,
    }
}

fn run_gif(
    project: &Project,
    out_dir: &Path,
    job: &TargetJob,
    key: String,
    backend: Backend,
) -> Outcome {
    use artscript_eval::{frame_delays, sample};
    let sampled = sample(project, job);
    let mut diagnostics = sampled.diagnostics;
    let mut outputs = Vec::new();
    let compile_error = diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error && d.code.starts_with('E'));
    if !compile_error
        && let Ok(delays) = frame_delays(sampled.frames.len(), sampled.dur)
        && !delays.is_empty()
    {
        let scenes: Vec<_> = sampled.frames.iter().map(|f| &f.scene).collect();
        for out in &job.outputs {
            let bytes = artscript_render::render_gif(&scenes, out.scale, &delays, sampled.loops, backend);
            let bytes = match bytes {
                Ok(b) => b,
                Err(artscript_render::RenderError::TransparentGif) => {
                    diagnostics.push(
                        Diagnostic::error("E0206", job.span, "a gif needs an opaque `bg:`")
                            .in_file(&job.source)
                            .with_fix("set `bg:` to an opaque color; transparent gifs come later"),
                    );
                    continue;
                }
                Err(e) => {
                    diagnostics.push(render_error(job, e));
                    continue;
                }
            };
            let path = out_dir.join(&out.path);
            let written = path
                .parent()
                .map_or(Ok(()), fs::create_dir_all)
                .and_then(|_| fs::write(&path, &bytes));
            match written {
                Ok(()) => outputs.push((out.path.clone(), hex(&bytes))),
                Err(e) => diagnostics.push(
                    Diagnostic::error("E0400", job.span, format!("cannot write {}: {e}", out.path))
                        .in_file(&job.source),
                ),
            }
        }
    }
    if outputs.len() == job.outputs.len() && !job.outputs.is_empty() {
        let entry = CacheEntry {
            key,
            outputs: outputs.clone(),
            diagnostics: diagnostics.iter().map(CachedDiag::from).collect(),
        };
        let path = cache_path(&project.root, job);
        let _ = path
            .parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|_| fs::write(&path, serde_json::to_string(&entry).unwrap_or_default()));
    }
    Outcome {
        job: job.clone(),
        outputs,
        diagnostics,
    }
}

fn status(diags: &[Diagnostic]) -> &'static str {
    match exit_code(diags, false) {
        0 => "pass",
        3 => "warn",
        _ => "fail",
    }
}

fn manifest_entry(o: &Outcome, path: &str, hash: &str) -> Json {
    let spec = o.job.outputs.iter().find(|s| s.path == path);
    let matrix: serde_json::Map<String, Json> = o
        .job
        .matrix
        .iter()
        .map(|(k, v)| (k.clone(), Json::String(v.text())))
        .collect();
    let checks: Vec<String> = o
        .diagnostics
        .iter()
        .filter(|d| d.code.starts_with('C'))
        .map(|d| {
            if d.node.is_empty() {
                format!("{}: {}", d.code, d.message)
            } else {
                format!("{}: {} ({})", d.code, d.message, d.node)
            }
        })
        .collect();
    json!({
        "path": path,
        "source": o.job.source,
        "line": o.job.line,
        "target": o.job.label,
        "matrix": matrix,
        "scale": spec.map(|s| s.scale),
        "hash": hash,
        "status": status(&o.diagnostics),
        "checks": checks,
    })
}

fn read_manifest(path: &Path) -> BTreeMap<String, Json> {
    let Ok(src) = fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    let Ok(v) = serde_json::from_str::<Json>(&src) else {
        return BTreeMap::new();
    };
    v["outputs"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|o| Some((o["path"].as_str()?.to_string(), o.clone())))
        .collect()
}

/// `Auto` falls back to the CPU. `None` is a forced `gpu` with no GPU, never a silent fallback.
fn resolve_backend(choice: BackendChoice, gpu: bool) -> Option<Backend> {
    match (choice, gpu) {
        (BackendChoice::Cpu, _) | (BackendChoice::Auto, false) => Some(Backend::Cpu),
        (BackendChoice::Gpu | BackendChoice::Auto, true) => Some(Backend::Gpu),
        (BackendChoice::Gpu, false) => None,
    }
}

pub fn build(project: &Project, opts: &Options) -> u8 {
    let plan = plan(project);
    let choice = opts.backend.unwrap_or(project.manifest.backend);
    let Some(backend) = resolve_backend(choice, artscript_render::gpu_available()) else {
        let mut diagnostics = plan.diagnostics;
        diagnostics.push(
            Diagnostic::error(
                "E0400",
                Span::default(),
                "the gpu backend is not available on this machine",
            )
            .with_fix("use `--backend auto` or `--backend cpu`, or set `[build] backend` in artscript.toml"),
        );
        return report(diagnostics, opts.allow_warnings, false);
    };
    let unmatched = unmatched(&plan, &opts.only);
    if !unmatched.is_empty() {
        let mut diagnostics = plan.diagnostics;
        diagnostics.extend(unmatched);
        return report(diagnostics, opts.allow_warnings, false);
    }
    let out_dir = project.root.join(&project.manifest.out);
    let jobs: Vec<&TargetJob> = plan
        .jobs
        .iter()
        .filter(|j| opts.only.is_empty() || opts.only.iter().any(|g| g.matches(j)))
        .collect();

    let workers = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .min(jobs.len().max(1));
    let next = std::sync::atomic::AtomicUsize::new(0);
    let mut outcomes: Vec<(usize, Outcome)> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                s.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(job) = jobs.get(i) else { break };
                        done.push((i, run_job(project, &out_dir, job, backend)));
                    }
                    done
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_default())
            .collect()
    });
    outcomes.sort_by_key(|(i, _)| *i);

    let manifest_path = out_dir.join(MANIFEST_FILE);
    let old = read_manifest(&manifest_path);
    let mut new: BTreeMap<String, Json> = BTreeMap::new();
    if !opts.only.is_empty() {
        let rebuilt: BTreeSet<&str> = jobs.iter().map(|j| j.source.as_str()).collect();
        for (path, entry) in &old {
            if !entry["source"]
                .as_str()
                .is_some_and(|s| rebuilt.contains(s))
            {
                new.insert(path.clone(), entry.clone());
            }
        }
    }
    let mut diagnostics = plan.diagnostics;
    for (_, o) in &outcomes {
        for (path, hash) in &o.outputs {
            new.insert(path.clone(), manifest_entry(o, path, hash));
        }
        for spec in o
            .job
            .outputs
            .iter()
            .filter(|s| !o.outputs.iter().any(|(p, _)| p == &s.path))
        {
            if let Some(prev) = old.get(&spec.path) {
                let mut kept = prev.clone();
                kept["status"] = json!("fail");
                new.insert(spec.path.clone(), kept);
            }
        }
        diagnostics.extend(o.diagnostics.iter().cloned());
    }
    for path in old.keys().filter(|p| !new.contains_key(*p)) {
        let _ = fs::remove_file(out_dir.join(path));
        outln!("removed stale {path}");
    }
    let manifest = json!({ "version": 1, "outputs": new.into_values().collect::<Vec<_>>() });
    let text = serde_json::to_string_pretty(&manifest).unwrap_or_default() + "\n";
    if fs::read_to_string(&manifest_path).ok().as_deref() != Some(text.as_str()) {
        let _ = fs::create_dir_all(&out_dir).and_then(|_| fs::write(&manifest_path, text));
    }
    let written: usize = outcomes.iter().map(|(_, o)| o.outputs.len()).sum();
    outln!(
        "built {} target(s), {written} output(s) in {}",
        outcomes.len(),
        project.manifest.out
    );
    report(diagnostics, opts.allow_warnings, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_resolution() {
        assert_eq!(resolve_backend(BackendChoice::Auto, true), Some(Backend::Gpu));
        assert_eq!(resolve_backend(BackendChoice::Auto, false), Some(Backend::Cpu));
        assert_eq!(resolve_backend(BackendChoice::Gpu, true), Some(Backend::Gpu));
        assert_eq!(resolve_backend(BackendChoice::Gpu, false), None, "forced gpu never falls back");
        assert_eq!(resolve_backend(BackendChoice::Cpu, true), Some(Backend::Cpu));
    }
}
