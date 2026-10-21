use std::fs;
use std::path::{Component, Path, PathBuf};

use ramose_eval::{BackendChoice, OutputSpec, Parameters, Project, TargetJob};
use ramose_ir::Rect;
use ramose_render::Backend;
use ramose_syntax::{Diagnostic, Severity};
use serde_json::json;

#[derive(Default)]
pub struct Options {
    file: String,
    target: Option<String>,
    parameters: Parameters,
    crop: Crop,
    pad: f32,
    zoom: u32,
    background: Background,
    frames: Frames,
    clip: Option<String>,
    columns: Option<usize>,
    out: PathBuf,
    backend: Option<BackendChoice>,
}

#[derive(Default)]
enum Crop {
    #[default]
    Canvas,
    Rect(Rect),
    Node(String),
}

#[derive(Default)]
enum Frames {
    #[default]
    Target,
    Times(Vec<f64>),
    Count(usize),
}

#[derive(Default)]
enum Background {
    #[default]
    Transparent,
    Checker,
    Color([u8; 4]),
}

struct SelectedTarget {
    job: TargetJob,
    output: OutputSpec,
}

pub fn parse(args: &[String]) -> Result<Options, String> {
    let mut opts = Options {
        zoom: 1,
        ..Options::default()
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        if !arg.starts_with('-') {
            if !opts.file.is_empty() {
                return Err("preview takes one entry file".into());
            }
            opts.file = arg.clone();
            continue;
        }
        let value = it.next().ok_or_else(|| format!("{arg} needs a value"))?;
        match arg.as_str() {
            "--target" => opts.target = Some(value.clone()),
            "--set" => {
                let (name, value) = ramose_eval::parse_parameter(value)?;
                opts.parameters.insert(name, value);
            }
            "--crop" => {
                opts.crop = if value.contains(',') {
                    let values: Vec<f32> = value
                        .split(',')
                        .map(|v| v.parse::<f32>().map_err(|_| "--crop needs x,y,w,h"))
                        .collect::<Result<_, _>>()?;
                    if values.len() != 4
                        || values.iter().any(|n| !n.is_finite())
                        || values[2] <= 0.0
                        || values[3] <= 0.0
                    {
                        return Err(
                            "--crop needs finite x,y,w,h with positive width and height".into()
                        );
                    }
                    Crop::Rect(Rect::from_xywh(values[0], values[1], values[2], values[3]))
                } else {
                    Crop::Node(value.clone())
                };
            }
            "--pad" => {
                opts.pad = value
                    .parse()
                    .map_err(|_| "--pad needs a nonnegative number")?;
                if !opts.pad.is_finite() || opts.pad < 0.0 {
                    return Err("--pad needs a nonnegative finite number".into());
                }
            }
            "--zoom" => {
                opts.zoom = value
                    .parse()
                    .map_err(|_| "--zoom needs a positive integer")?;
                if opts.zoom == 0 {
                    return Err("--zoom needs a positive integer".into());
                }
            }
            "--bg" => {
                opts.background = match value.as_str() {
                    "none" | "transparent" => Background::Transparent,
                    "checker" => Background::Checker,
                    "light" => Background::Color([255, 255, 255, 255]),
                    "dark" => Background::Color([32, 32, 32, 255]),
                    color => {
                        if !color.is_ascii() {
                            return Err("--bg needs a hex color".into());
                        }
                        let rgba = ramose_eval::color::Rgba::hex(color)
                            .ok_or("--bg needs checker, light, dark, none, or a hex color")?;
                        Background::Color(
                            [rgba.r, rgba.g, rgba.b, rgba.a].map(|c| (c * 255.0).round() as u8),
                        )
                    }
                }
            }
            "--time" | "--times" | "--frames" => {
                if !matches!(opts.frames, Frames::Target) {
                    return Err("choose only one of --time, --times, and --frames".into());
                }
                opts.frames = if arg == "--frames" {
                    let n = value
                        .parse::<usize>()
                        .map_err(|_| "--frames needs a positive integer")?;
                    if n == 0 {
                        return Err("--frames needs a positive integer".into());
                    }
                    Frames::Count(n)
                } else {
                    let times = value
                        .split(',')
                        .map(|s| s.parse::<f64>().map_err(|_| "times must be seconds"))
                        .collect::<Result<Vec<_>, _>>()?;
                    if times.is_empty()
                        || times.iter().any(|t| !t.is_finite() || *t < 0.0)
                        || (arg == "--time" && times.len() != 1)
                    {
                        return Err(
                            "times must be finite nonnegative seconds; --time takes one value"
                                .into(),
                        );
                    }
                    Frames::Times(times)
                };
            }
            "--clip" => opts.clip = Some(value.clone()),
            "--columns" => {
                let n = value
                    .parse::<usize>()
                    .map_err(|_| "--columns needs a positive integer")?;
                if n == 0 {
                    return Err("--columns needs a positive integer".into());
                }
                opts.columns = Some(n);
            }
            "--out" => opts.out = PathBuf::from(value),
            "--backend" => opts.backend = Some(value.parse()?),
            _ => return Err(format!("unknown preview option `{arg}`")),
        }
    }
    if opts.file.is_empty() || opts.out.as_os_str().is_empty() {
        return Err("preview needs an entry file and --out PATH.png".into());
    }
    if opts.out.extension().and_then(|s| s.to_str()) != Some("png") {
        return Err("preview --out must end in .png".into());
    }
    Ok(opts)
}

pub fn run(project: &Project, cwd: &Path, opts: &Options) -> u8 {
    match preview(project, cwd, opts) {
        Ok(receipt) => {
            outln!("{}", serde_json::to_string_pretty(&receipt).unwrap());
            0
        }
        Err(message) => {
            errln!("error: {message}");
            1
        }
    }
}

fn preview(project: &Project, cwd: &Path, opts: &Options) -> Result<serde_json::Value, String> {
    let source = crate::resolve_art(project, cwd, &opts.file)?;
    let plan = ramose_eval::plan(project);
    let deps = ramose_eval::dependencies(project, &source);
    reject_errors(
        plan.diagnostics
            .iter()
            .filter(|d| deps.contains(&d.file))
            .cloned()
            .collect(),
    )?;
    let candidates: Vec<_> = plan
        .jobs
        .iter()
        .filter(|j| j.source == source)
        .flat_map(|job| job.outputs.iter().map(move |output| (job, output)))
        .collect();
    let matches: Vec<_> = candidates
        .iter()
        .filter(|(j, o)| {
            opts.target.as_ref().is_none_or(|target| target == &o.path)
                && opts
                    .clip
                    .as_ref()
                    .is_none_or(|clip| j.clip.as_ref() == Some(clip))
        })
        .collect();
    if matches.len() != 1 {
        let paths = candidates
            .iter()
            .map(|(_, o)| o.path.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "preview needs exactly one target output; matched {}. Use --target with one of: {paths}",
            matches.len()
        ));
    }
    let (job, output) = *matches[0];
    let selected = SelectedTarget {
        job: job.clone(),
        output: output.clone(),
    };
    let destination = resolved_path(&cwd.join(&opts.out))?;
    let shipping = resolved_path(&project.root.join(&project.manifest.out))?;
    let cache = resolved_path(&project.root.join(".ramose"))?;
    if destination.starts_with(&shipping) || destination.starts_with(&cache) {
        return Err(
            "preview destination must be outside the production output and .ramose directories"
                .into(),
        );
    }
    for path in project
        .files
        .keys()
        .map(|p| project.root.join(p))
        .chain([project.root.join("ramose.toml")])
    {
        if destination == resolved_path(&path)? {
            return Err("preview cannot overwrite project source".into());
        }
    }
    let backend = match opts.backend.unwrap_or(project.manifest.backend) {
        BackendChoice::Cpu => Backend::Cpu,
        BackendChoice::Auto if !ramose_render::gpu_available() => Backend::Cpu,
        _ if ramose_render::gpu_available() => Backend::Gpu,
        _ => return Err("gpu backend is unavailable; use --backend cpu".into()),
    };
    let mut job = selected.job.clone();
    job.bake_scale = job.sample_scale(&selected.output);
    job.parameters = opts.parameters.clone();
    let mut probe_job = job.clone();
    probe_job.frame_t = match &opts.frames {
        Frames::Times(times) => times[0],
        Frames::Count(_) => 0.0,
        Frames::Target => job.frame_t,
    };
    let probe = ramose_eval::evaluate(project, &probe_job);
    reject_errors(probe.diagnostics)?;
    let duration = job
        .dur
        .unwrap_or_else(|| probe.plays.iter().map(|p| p.end).fold(0.0, f64::max));
    let times = match &opts.frames {
        Frames::Target => vec![job.frame_t],
        Frames::Times(times) => times.clone(),
        Frames::Count(n) => {
            if !duration.is_finite() || duration <= 0.0 {
                return Err("--frames needs a target duration or an active animation clip".into());
            }
            (0..*n).map(|i| i as f64 * duration / *n as f64).collect()
        }
    };
    let mut altered = false;
    let mut images = Vec::new();
    let mut crop: Option<Rect> = None;
    let mut parameters = Vec::new();
    for time in &times {
        job.frame_t = *time;
        let evaluated = ramose_eval::evaluate(project, &job);
        reject_errors(evaluated.diagnostics)?;
        if !opts.parameters.is_empty() {
            let mut normal_job = job.clone();
            normal_job.parameters.clear();
            altered |=
                ramose_eval::evaluate(project, &normal_job).parameters != evaluated.parameters;
        }
        let scene = evaluated.scene.ok_or("target produced no scene")?;
        let bounds = match &opts.crop {
            Crop::Canvas => Rect::from_xywh(0.0, 0.0, scene.width, scene.height),
            Crop::Rect(rect) => *rect,
            Crop::Node(path) => ramose_eval::node_bounds(&scene, path)
                .ok_or_else(|| format!("crop node `{path}` has no ink bounds at t={time}"))?,
        };
        if let Some(union) = &mut crop {
            union.join(bounds);
        } else {
            crop = Some(bounds);
        }
        parameters.push(evaluated.parameters);
        images.push(
            ramose_render::render_rgba(&scene, selected.output.scale, backend)
                .map_err(|e| format!("cannot render preview: {e:?}"))?,
        );
    }
    let bounds = crop
        .ok_or("preview has no frames")?
        .with_outset((opts.pad, opts.pad));
    let scale = f64::from(selected.output.scale);
    let coordinates = [
        f64::from(bounds.left) * scale,
        f64::from(bounds.top) * scale,
        f64::from(bounds.right) * scale,
        f64::from(bounds.bottom) * scale,
    ];
    if coordinates
        .iter()
        .any(|v| !v.is_finite() || v.abs() > i32::MAX as f64)
    {
        return Err("crop coordinates exceed raster dimensions".into());
    }
    let [left, top, right, bottom] = [
        coordinates[0].floor() as i64,
        coordinates[1].floor() as i64,
        coordinates[2].ceil() as i64,
        coordinates[3].ceil() as i64,
    ];
    let width = u32::try_from(right - left)
        .ok()
        .filter(|n| *n > 0)
        .ok_or("crop width must be positive")?;
    let height = u32::try_from(bottom - top)
        .ok()
        .filter(|n| *n > 0)
        .ok_or("crop height must be positive")?;
    let columns = opts.columns.unwrap_or(times.len()).min(times.len());
    let rows = times.len().div_ceil(columns);
    let cell_w = width
        .checked_mul(opts.zoom)
        .ok_or("preview width overflow")?;
    let cell_h = height
        .checked_mul(opts.zoom)
        .ok_or("preview height overflow")?;
    let sheet_w = cell_w
        .checked_mul(u32::try_from(columns).map_err(|_| "too many columns")?)
        .ok_or("preview width overflow")?;
    let sheet_h = cell_h
        .checked_mul(u32::try_from(rows).map_err(|_| "too many rows")?)
        .ok_or("preview height overflow")?;
    let bytes = (sheet_w as usize)
        .checked_mul(sheet_h as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or("preview dimensions overflow")?;
    let mut sheet = Vec::new();
    sheet
        .try_reserve_exact(bytes)
        .map_err(|e| format!("cannot allocate preview: {e}"))?;
    sheet.resize(bytes, 0);
    for (frame, (image_w, image_h, pixels)) in images.iter().enumerate() {
        for y in 0..cell_h {
            for x in 0..cell_w {
                let px = left + i64::from(x / opts.zoom);
                let py = top + i64::from(y / opts.zoom);
                let rgba =
                    if px >= 0 && py >= 0 && px < i64::from(*image_w) && py < i64::from(*image_h) {
                        let offset = (py as usize * *image_w as usize + px as usize) * 4;
                        pixels[offset..offset + 4].try_into().unwrap()
                    } else {
                        [0; 4]
                    };
                let bg = match opts.background {
                    Background::Transparent => [0; 4],
                    Background::Color(color) => color,
                    Background::Checker => {
                        if (x / opts.zoom / 8 + y / opts.zoom / 8).is_multiple_of(2) {
                            [224, 224, 224, 255]
                        } else {
                            [176, 176, 176, 255]
                        }
                    }
                };
                let sx = (frame % columns) as u32 * cell_w + x;
                let sy = (frame / columns) as u32 * cell_h + y;
                let offset = (sy as usize * sheet_w as usize + sx as usize) * 4;
                sheet[offset..offset + 4].copy_from_slice(&composite(rgba, bg));
            }
        }
    }
    let mut encoded = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut encoded, sheet_w, sheet_h);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .map_err(|e| e.to_string())?
            .write_image_data(&sheet)
            .map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(destination.parent().ok_or("destination has no parent")?)
        .map_err(|e| format!("cannot create preview directory: {e}"))?;
    let temporary = destination.with_extension(format!("{}.tmp", std::process::id()));
    use std::io::Write;
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&temporary)
        .map_err(|e| format!("cannot create preview temporary file: {e}"))?;
    let result = file.write_all(&encoded).and_then(|_| fs::rename(&temporary, &destination));
    if let Err(e) = result {
        let _ = fs::remove_file(&temporary);
        return Err(format!(
            "cannot write preview {}: {e}",
            destination.display()
        ));
    }
    Ok(json!({
        "source": source, "target": selected.output.path, "target_line": job.line,
        "matrix": job.matrix.iter().map(|(key, value)| (key.clone(), value.text())).collect::<std::collections::BTreeMap<_, _>>(),
        "scale": selected.output.scale, "backend": format!("{backend:?}").to_lowercase(),
        "parameters": parameters, "parameter_altered": altered, "clip": job.clip,
        "times": times, "duration": duration, "sampling": "even samples exclude loop end; explicit times include it",
        "crop_canvas": [bounds.left, bounds.top, bounds.width(), bounds.height()],
        "crop_pixels": [left, top, width as i64, height as i64], "zoom": opts.zoom,
        "background": match opts.background { Background::Transparent => json!("none"), Background::Checker => json!("checker"), Background::Color(color) => json!(color) },
        "columns": columns, "rows": rows, "size": [sheet_w, sheet_h], "paths": [destination],
    }))
}

fn reject_errors(diagnostics: Vec<Diagnostic>) -> Result<(), String> {
    let errors = diagnostics
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.to_string())
        .collect::<Vec<_>>();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}

fn composite(fg: [u8; 4], bg: [u8; 4]) -> [u8; 4] {
    if bg[3] == 0 || fg[3] == 255 {
        return fg;
    }
    let a = u32::from(fg[3]);
    let b = u32::from(bg[3]) * (255 - a);
    let total = a * 255 + b;
    if total == 0 {
        return [0; 4];
    }
    let mut out = [0; 4];
    for i in 0..3 {
        out[i] = ((u32::from(fg[i]) * a * 255 + u32::from(bg[i]) * b + total / 2) / total) as u8;
    }
    out[3] = ((total + 127) / 255) as u8;
    out
}

fn resolved_path(path: &Path) -> Result<PathBuf, String> {
    let mut resolved = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                resolved.pop();
            }
            Component::CurDir => {}
            other => {
                resolved.push(other);
                if resolved.exists() {
                    resolved = fs::canonicalize(&resolved)
                        .map_err(|e| format!("cannot resolve {}: {e}", resolved.display()))?;
                }
            }
        }
    }
    Ok(resolved)
}
