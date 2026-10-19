//! `artscript compare`: measure how far a render is from a reference image.
//!
//! Colour distance is Euclidean in OKLab times 100. The report covers the whole canvas, each named
//! node's box, and a 16x16 grid for the regions no node names. A sheet PNG puts the reference, the
//! render, a 50% onion overlay and a distance heatmap side by side.

use std::fs;
use std::path::Path;

use artscript_eval::{Format, OutputSpec, Project, TargetJob, evaluate_outputs, plan, project::rel_path};
use artscript_ir::{Matrix, Node, NodeKind, Scene};
use artscript_syntax::ast::Span;
use artscript_syntax::{Diagnostic, Severity};
use serde_json::json;
use skia_safe::{AlphaType, ColorType, Data, EncodedImageFormat, FilterMode, ImageInfo, MipmapMode, Rect, SamplingOptions};

use crate::{build, glob_match, report, usage_error};

pub struct Options {
    pub file: String,
    pub reference: String,
    pub target: Option<String>,
    pub out: Option<String>,
    pub top: usize,
    pub json: bool,
}

pub fn parse(args: &[String]) -> Result<Options, String> {
    let mut file = None;
    let (mut reference, mut target, mut out, mut top, mut json) = (None, None, None, 12usize, false);
    let mut it = args.iter();
    let value = |flag: &str, it: &mut std::slice::Iter<String>| -> Result<String, String> {
        it.next().cloned().ok_or(format!("`{flag}` needs a value"))
    };
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => json = true,
            "--ref" => reference = Some(value("--ref", &mut it)?),
            "--target" => target = Some(value("--target", &mut it)?),
            "--out" => out = Some(value("--out", &mut it)?),
            "--top" => {
                top = value("--top", &mut it)?.parse().map_err(|_| "`--top` needs a whole number".to_string())?;
            }
            s if s.starts_with('-') => return Err(format!("unknown flag `{s}`")),
            s if file.is_none() => file = Some(s.to_string()),
            s => return Err(format!("unexpected argument `{s}`")),
        }
    }
    Ok(Options {
        file: file.ok_or("`compare` needs a file")?,
        reference: reference.ok_or("`compare` needs `--ref <image>`")?,
        target,
        out,
        top,
        json,
    })
}

/// OKLab colour, `l` in 0..1, `a` and `b` roughly -0.4..0.4.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Lab {
    l: f64,
    a: f64,
    b: f64,
}

fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn linear_to_srgb(c: f64) -> f64 {
    if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

fn lab_from_linear(r: f64, g: f64, b: f64) -> Lab {
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    Lab {
        l: 0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        a: 1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        b: 0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    }
}

fn lab_to_srgb(c: Lab) -> [u8; 3] {
    let l = (c.l + 0.3963377774 * c.a + 0.2158037573 * c.b).powi(3);
    let m = (c.l - 0.1055613458 * c.a - 0.0638541728 * c.b).powi(3);
    let s = (c.l - 0.0894841775 * c.a - 1.2914855480 * c.b).powi(3);
    let lin = [
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
    ];
    lin.map(|v| (linear_to_srgb(v.clamp(0.0, 1.0)) * 255.0).round() as u8)
}

fn hex(c: Lab) -> String {
    let [r, g, b] = lab_to_srgb(c);
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn delta_e(a: Lab, b: Lab) -> f64 {
    100.0 * ((a.l - b.l).powi(2) + (a.a - b.a).powi(2) + (a.b - b.b).powi(2)).sqrt()
}

/// Opaque RGB pixels, row-major.
struct Raster {
    w: usize,
    h: usize,
    rgb: Vec<[u8; 3]>,
}

fn lab_table() -> [f64; 256] {
    std::array::from_fn(|i| srgb_to_linear(i as f64 / 255.0))
}

fn lab_of(table: &[f64; 256], p: [u8; 3]) -> Lab {
    lab_from_linear(table[p[0] as usize], table[p[1] as usize], table[p[2] as usize])
}

/// Per-pixel distance plus summed-area tables, so any box's mean distance and mean colours cost
/// four lookups.
struct Measure {
    w: usize,
    h: usize,
    de: Vec<f32>,
    /// `[de, render l, a, b, ref l, a, b]` summed over the rectangle from the origin to `(x, y)` exclusive.
    sat: Vec<[f64; 7]>,
}

#[derive(Clone, Copy, Debug)]
struct Region {
    de: f64,
    render: Lab,
    reference: Lab,
}

impl Measure {
    fn new(render: &Raster, reference: &Raster) -> Measure {
        let (w, h) = (render.w, render.h);
        let table = lab_table();
        let mut de = Vec::with_capacity(w * h);
        let mut sat = vec![[0.0f64; 7]; (w + 1) * (h + 1)];
        for y in 0..h {
            let mut row = [0.0f64; 7];
            for x in 0..w {
                let (a, b) = (lab_of(&table, render.rgb[y * w + x]), lab_of(&table, reference.rgb[y * w + x]));
                let d = delta_e(a, b);
                de.push(d as f32);
                for (acc, v) in row.iter_mut().zip([d, a.l, a.a, a.b, b.l, b.a, b.b]) {
                    *acc += v;
                }
                for k in 0..7 {
                    sat[(y + 1) * (w + 1) + x + 1][k] = sat[y * (w + 1) + x + 1][k] + row[k];
                }
            }
        }
        Measure { w, h, de, sat }
    }

    /// Mean over the pixel box `[x0, x1) x [y0, y1)`, or `None` when it holds no pixel.
    fn region(&self, x0: usize, y0: usize, x1: usize, y1: usize) -> Option<Region> {
        let (x1, y1) = (x1.min(self.w), y1.min(self.h));
        if x0 >= x1 || y0 >= y1 {
            return None;
        }
        let at = |x: usize, y: usize| &self.sat[y * (self.w + 1) + x];
        let n = ((x1 - x0) * (y1 - y0)) as f64;
        let sum: [f64; 7] = std::array::from_fn(|k| at(x1, y1)[k] - at(x0, y1)[k] - at(x1, y0)[k] + at(x0, y0)[k]);
        let m = |k: usize| sum[k] / n;
        Some(Region {
            de: m(0),
            render: Lab { l: m(1), a: m(2), b: m(3) },
            reference: Lab { l: m(4), a: m(5), b: m(6) },
        })
    }
}

const HUES: [(&str, f64); 8] = [
    ("red", 29.0),
    ("orange", 55.0),
    ("yellow", 110.0),
    ("green", 142.0),
    ("cyan", 195.0),
    ("blue", 264.0),
    ("purple", 305.0),
    ("pink", 350.0),
];

fn hue_of(c: Lab) -> f64 {
    c.b.atan2(c.a).to_degrees().rem_euclid(360.0)
}

/// Signed hue distance from `from` to `to`, in -180..180.
fn hue_delta(from: f64, to: f64) -> f64 {
    (to - from + 540.0).rem_euclid(360.0) - 180.0
}

fn nearest_hue(h: f64) -> usize {
    (0..HUES.len())
        .min_by(|&i, &j| hue_delta(h, HUES[i].1).abs().total_cmp(&hue_delta(h, HUES[j].1).abs()))
        .unwrap_or(0)
}

/// `.04` for 0.04, in the language of the hint.
fn short(v: f64) -> String {
    let s = format!("{v:.2}");
    s.strip_prefix("0.").map_or(s.clone(), |t| format!(".{t}"))
}

/// What to change in the render so it moves toward the reference.
fn hint(render: Lab, reference: Lab) -> String {
    const LIGHT: f64 = 0.012;
    const CHROMA: f64 = 0.012;
    const HUE_MIN_CHROMA: f64 = 0.03;
    const HUE_MIN_DEG: f64 = 8.0;
    let mut parts = Vec::new();
    let dl = reference.l - render.l;
    if dl.abs() >= LIGHT {
        parts.push(format!("{} by ΔL {}", if dl > 0.0 { "lighter" } else { "darker" }, short(dl.abs())));
    }
    let (cr, cf) = (render.a.hypot(render.b), reference.a.hypot(reference.b));
    if (cf - cr).abs() >= CHROMA {
        parts.push(format!("{} saturated by {}", if cf > cr { "more" } else { "less" }, short((cf - cr).abs())));
    }
    if cr > HUE_MIN_CHROMA && cf > HUE_MIN_CHROMA {
        let (hr, hf) = (hue_of(render), hue_of(reference));
        let travel = hue_delta(hr, hf);
        if travel.abs() > HUE_MIN_DEG {
            let mut name = nearest_hue(hf);
            if name == nearest_hue(hr) {
                let step = if travel > 0.0 { 1 } else { HUES.len() - 1 };
                name = (name + step) % HUES.len();
            }
            parts.push(format!("hue toward {} {:.0}°", HUES[name].0, travel.abs()));
        }
    }
    if parts.is_empty() { "close".to_string() } else { parts.join(", ") }
}

/// A node with a name an author wrote. An unnamed node's path ends in `<verb>~<n>`; a loop copy
/// or stamp adds a numeric index after it, which is not a name.
fn is_named(path: &str) -> bool {
    path.rsplit('.').find(|seg| !seg.chars().all(|c| c.is_ascii_digit())).is_some_and(|seg| !seg.contains('~'))
}

/// Every named node with its device-space box.
fn named_boxes(scene: &Scene, scale: f32) -> Vec<(String, Rect)> {
    fn walk(n: &Node, parent: &Matrix, out: &mut Vec<(String, Rect)>) {
        let world = *parent * n.transform;
        if is_named(&n.path)
            && let Some(local) = n.local_bounds()
        {
            out.push((n.path.clone(), world.map_rect(local).0));
        }
        if let NodeKind::Shape(s) = &n.kind {
            for c in &s.stamps {
                walk(c, &world, out);
            }
        }
        for c in n.descendants().skip(match &n.kind {
            NodeKind::Shape(s) => s.stamps.len(),
            NodeKind::Group { .. } => 0,
        }) {
            walk(c, &world, out);
        }
    }
    let mut out = Vec::new();
    let root = Matrix::scale((scale, scale));
    for c in &scene.children {
        walk(c, &root, &mut out);
    }
    out
}

struct NodeRow {
    path: String,
    region: Region,
    share: f64,
    box_px: (usize, usize, usize, usize),
}

struct CellRow {
    rect: (f64, f64, f64, f64),
    region: Region,
}

fn node_rows(m: &Measure, boxes: &[(String, Rect)]) -> Vec<NodeRow> {
    let mut rows: Vec<NodeRow> = boxes
        .iter()
        .filter_map(|(path, r)| {
            let clip = |v: f32, hi: usize| (v.max(0.0) as usize).min(hi);
            let (x0, y0) = (clip(r.left.floor(), m.w), clip(r.top.floor(), m.h));
            let (x1, y1) = (clip(r.right.ceil(), m.w), clip(r.bottom.ceil(), m.h));
            let region = m.region(x0, y0, x1, y1)?;
            let share = ((x1 - x0) * (y1 - y0)) as f64 / (m.w * m.h) as f64;
            Some(NodeRow { path: path.clone(), region, share, box_px: (x0, y0, x1, y1) })
        })
        .collect();
    rows.sort_by(|a, b| (b.region.de * b.share).total_cmp(&(a.region.de * a.share)).then_with(|| a.path.cmp(&b.path)));
    rows
}

const GRID: usize = 16;

fn grid_rows(m: &Measure, scale: f64) -> Vec<CellRow> {
    let mut cells = Vec::new();
    for gy in 0..GRID {
        for gx in 0..GRID {
            let (x0, x1) = (gx * m.w / GRID, (gx + 1) * m.w / GRID);
            let (y0, y1) = (gy * m.h / GRID, (gy + 1) * m.h / GRID);
            if let Some(region) = m.region(x0, y0, x1, y1) {
                cells.push(CellRow {
                    rect: (x0 as f64 / scale, y0 as f64 / scale, (x1 - x0) as f64 / scale, (y1 - y0) as f64 / scale),
                    region,
                });
            }
        }
    }
    cells.sort_by(|a, b| b.region.de.total_cmp(&a.region.de).then(a.rect.1.total_cmp(&b.rect.1)).then(a.rect.0.total_cmp(&b.rect.0)));
    cells
}

struct Summary {
    mean: f64,
    p95: f64,
    indistinguishable: f64,
}

fn summarize(de: &[f32]) -> Summary {
    let mut sorted = de.to_vec();
    sorted.sort_by(f32::total_cmp);
    let n = sorted.len().max(1);
    Summary {
        mean: de.iter().map(|&d| d as f64).sum::<f64>() / n as f64,
        p95: sorted.get(((n as f64 * 0.95).ceil() as usize).saturating_sub(1)).copied().unwrap_or(0.0) as f64,
        indistinguishable: 100.0 * de.iter().filter(|&&d| d < 2.0).count() as f64 / n as f64,
    }
}

fn decode_reference(path: &Path, w: usize, h: usize) -> Result<Raster, String> {
    let bytes = fs::read(path).map_err(|e| format!("cannot read `{}`: {e}", path.display()))?;
    let image = skia_safe::Image::from_encoded(Data::new_copy(&bytes))
        .ok_or_else(|| format!("`{}` is not a PNG or JPEG image", path.display()))?;
    let mut surface = skia_safe::surfaces::raster_n32_premul((w as i32, h as i32)).ok_or("cannot allocate a surface")?;
    let canvas = surface.canvas();
    canvas.clear(skia_safe::Color::WHITE);
    let sampling = SamplingOptions::new(FilterMode::Linear, MipmapMode::Linear);
    canvas.draw_image_rect_with_sampling_options(
        &image,
        None,
        Rect::from_wh(w as f32, h as f32),
        sampling,
        &skia_safe::Paint::default(),
    );
    let info = ImageInfo::new((w as i32, h as i32), ColorType::RGBA8888, AlphaType::Premul, None);
    let mut buf = vec![0u8; w * h * 4];
    if !surface.image_snapshot().read_pixels(&info, &mut buf, w * 4, (0, 0), skia_safe::image::CachingHint::Disallow) {
        return Err("cannot read the reference pixels".into());
    }
    Ok(Raster { w, h, rgb: buf.as_chunks::<4>().0.iter().map(|p| [p[0], p[1], p[2]]).collect() })
}

/// The render over the scene's background, or white when the scene has none.
fn flatten(w: usize, h: usize, rgba: &[u8], bg: Option<artscript_ir::Color>) -> Raster {
    let bg = bg.map_or([1.0; 3], |c| [c.r, c.g, c.b].map(|v| v as f64)).map(|v| v.clamp(0.0, 1.0));
    let over = |c: u8, a: f64, back: f64| ((c as f64 * a + back * 255.0 * (1.0 - a)).round()) as u8;
    let rgb = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| {
            let a = p[3] as f64 / 255.0;
            [over(p[0], a, bg[0]), over(p[1], a, bg[1]), over(p[2], a, bg[2])]
        })
        .collect();
    Raster { w, h, rgb }
}

fn heat(de: f32) -> [u8; 3] {
    let t = (de / 20.0).clamp(0.0, 1.0);
    if t < 0.5 {
        [(t * 2.0 * 255.0).round() as u8, 0, 0]
    } else {
        [255, ((t - 0.5) * 2.0 * 255.0).round() as u8, 0]
    }
}

/// Reference, render, onion and heatmap panels with a 4px gap, as PNG bytes.
fn sheet(render: &Raster, reference: &Raster, de: &[f32]) -> Result<Vec<u8>, String> {
    const GAP: usize = 4;
    let (w, h) = (render.w, render.h);
    let width = w * 4 + GAP * 3;
    let mut buf = vec![255u8; width * h * 4];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let (r, f) = (render.rgb[i], reference.rgb[i]);
            let onion = [0, 1, 2].map(|k| (r[k] as u16 + f[k] as u16).div_ceil(2) as u8);
            for (panel, px) in [f, r, onion, heat(de[i])].into_iter().enumerate() {
                let o = (y * width + panel * (w + GAP) + x) * 4;
                buf[o..o + 3].copy_from_slice(&px);
            }
        }
    }
    let info = ImageInfo::new((width as i32, h as i32), ColorType::RGBA8888, AlphaType::Opaque, None);
    let image = skia_safe::images::raster_from_data(&info, Data::new_copy(&buf), width * 4).ok_or("cannot build the sheet image")?;
    let png = image.encode(None, EncodedImageFormat::PNG, None).ok_or("cannot encode the sheet")?;
    Ok(png.as_bytes().to_vec())
}

fn pick_output<'a>(job: &'a TargetJob, target: Option<&str>) -> Result<&'a OutputSpec, String> {
    let pngs: Vec<&OutputSpec> = job.outputs.iter().filter(|o| o.format == Format::Png).collect();
    let hit = |o: &&OutputSpec| target.is_none_or(|t| o.path == t || glob_match(t, &o.path));
    let mut matching: Vec<&OutputSpec> = pngs.iter().copied().filter(hit).collect();
    matching.sort_by_key(|o| o.scale != 1.0);
    matching.first().copied().ok_or_else(|| {
        let paths: Vec<&str> = pngs.iter().map(|o| o.path.as_str()).collect();
        match target {
            Some(t) => format!("`--target {t}` matches no png output of this file; its png outputs are {}", paths.join(", ")),
            None => "this file has no png target to compare".to_string(),
        }
    })
}

fn error_diag(msg: impl Into<String>) -> Vec<Diagnostic> {
    vec![Diagnostic::error("E0200", Span::default(), msg)]
}

pub fn compare(project: &Project, cwd: &Path, opts: &Options) -> u8 {
    let file = fs::canonicalize(cwd.join(&opts.file)).unwrap_or_else(|_| cwd.join(&opts.file));
    let root = fs::canonicalize(&project.root).unwrap_or_else(|_| project.root.clone());
    let rel = rel_path(&root, &file);
    let plan = plan(project);
    let Some(job) = plan.jobs.iter().find(|j| j.source == rel) else {
        let own: Vec<Diagnostic> = plan.diagnostics.iter().filter(|d| d.file == rel).cloned().collect();
        return if own.is_empty() {
            report(error_diag(format!("`{rel}` has no @target to compare")), false, false)
        } else {
            report(own, false, false)
        };
    };
    let out = match pick_output(job, opts.target.as_deref()) {
        Ok(o) => o,
        Err(msg) => return usage_error(&msg),
    };
    let scale = job.sample_scale(out);
    let evaluated = evaluate_outputs(project, job);
    if evaluated.diagnostics.iter().any(|d| d.severity == Severity::Error && d.code.starts_with('E')) {
        return report(evaluated.diagnostics, false, false);
    }
    let Some(at) = evaluated.at(scale) else {
        return report(evaluated.diagnostics, false, false);
    };
    let Some(backend) = build::resolve_backend(project.manifest.backend, artscript_render::gpu_available()) else {
        return report(error_diag("the gpu backend is not available on this machine"), false, false);
    };
    let (w, h, rgba) = match artscript_render::render_rgba(&at.scene, scale, backend) {
        Ok(r) => r,
        Err(e) => return report(error_diag(format!("render failed: {e:?}")), false, false),
    };
    let (w, h) = (w as usize, h as usize);
    let render = flatten(w, h, &rgba, at.scene.background);
    let reference = match decode_reference(&cwd.join(&opts.reference), w, h) {
        Ok(r) => r,
        Err(msg) => return usage_error(&msg),
    };
    let measure = Measure::new(&render, &reference);
    let summary = summarize(&measure.de);
    let nodes = node_rows(&measure, &named_boxes(&at.scene, scale));
    let cells = grid_rows(&measure, scale as f64);

    let sheet_path = match &opts.out {
        Some(p) => cwd.join(p),
        None => {
            let stem = Path::new(&out.path).file_stem().map_or("target".into(), |s| s.to_string_lossy().into_owned());
            project.root.join(&project.manifest.out).join("compare").join(format!("{stem}.compare.png"))
        }
    };
    let written = sheet(&render, &reference, &measure.de).and_then(|png| {
        sheet_path
            .parent()
            .map_or(Ok(()), fs::create_dir_all)
            .and_then(|_| fs::write(&sheet_path, png))
            .map_err(|e| format!("cannot write `{}`: {e}", sheet_path.display()))
    });
    if let Err(msg) = written {
        return usage_error(&msg);
    }

    let top = &nodes[..opts.top.min(nodes.len())];
    let cell_top = &cells[..8.min(cells.len())];
    if opts.json {
        print_json(&out.path, (w, h), &summary, top, cell_top, &sheet_path);
    } else {
        print_text(&out.path, (w, h), &summary, top, cell_top, &sheet_path);
    }
    0
}

fn print_json(target: &str, size: (usize, usize), s: &Summary, nodes: &[NodeRow], cells: &[CellRow], sheet: &Path) {
    let region = |r: &Region| json!({ "de": r.de, "render": hex(r.render), "ref": hex(r.reference), "hint": hint(r.render, r.reference) });
    let nodes: Vec<_> = nodes
        .iter()
        .map(|n| {
            let mut v = region(&n.region);
            v["node"] = json!(n.path);
            v["share"] = json!(n.share);
            v["box_px"] = json!([n.box_px.0, n.box_px.1, n.box_px.2, n.box_px.3]);
            v
        })
        .collect();
    let cells: Vec<_> = cells
        .iter()
        .map(|c| {
            let mut v = region(&c.region);
            v["rect"] = json!([c.rect.0, c.rect.1, c.rect.2, c.rect.3]);
            v
        })
        .collect();
    let doc = json!({
        "target": target,
        "size": [size.0, size.1],
        "mean_de": s.mean,
        "p95_de": s.p95,
        "indistinguishable_pct": s.indistinguishable,
        "nodes": nodes,
        "cells": cells,
        "sheet": sheet.display().to_string(),
    });
    outln!("{doc}");
}

fn print_text(target: &str, size: (usize, usize), s: &Summary, nodes: &[NodeRow], cells: &[CellRow], sheet: &Path) {
    outln!("{target}  {}x{}  mean ΔE {:.1}  p95 ΔE {:.1}  {:.1}% of pixels under ΔE 2", size.0, size.1, s.mean, s.p95, s.indistinguishable);
    outln!();
    outln!("worst named nodes, by mean ΔE x share of canvas");
    let width = nodes.iter().map(|n| n.path.len() + 1).max().unwrap_or(0);
    for n in nodes {
        let r = &n.region;
        outln!(
            "{:<width$}  ΔE {:>4.1}  ({:.1}% of canvas)  render {}  ref {}  {}",
            format!("@{}", n.path),
            r.de,
            n.share * 100.0,
            hex(r.render),
            hex(r.reference),
            hint(r.render, r.reference)
        );
    }
    outln!();
    outln!("worst cells of a {GRID}x{GRID} grid, in canvas units (x,y wxh)");
    let fmt = |v: f64| {
        let t = format!("{v:.1}");
        t.strip_suffix(".0").map_or(t.clone(), str::to_string)
    };
    for c in cells {
        let r = &c.region;
        outln!(
            "{:>7},{:<7} {}x{}  ΔE {:>4.1}  render {}  ref {}  {}",
            fmt(c.rect.0),
            fmt(c.rect.1),
            fmt(c.rect.2),
            fmt(c.rect.3),
            r.de,
            hex(r.render),
            hex(r.reference),
            hint(r.render, r.reference)
        );
    }
    outln!();
    outln!("sheet {}", sheet.display());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lab(rgb: [u8; 3]) -> Lab {
        lab_of(&lab_table(), rgb)
    }

    #[test]
    fn identical_colors_are_zero_apart_and_black_to_white_is_a_hundred() {
        assert_eq!(delta_e(lab([90, 40, 140]), lab([90, 40, 140])), 0.0);
        let de = delta_e(lab([0, 0, 0]), lab([255, 255, 255]));
        assert!((de - 100.0).abs() < 0.1, "{de}");
    }

    #[test]
    fn oklab_round_trips_through_hex() {
        for rgb in [[0u8, 0, 0], [255, 255, 255], [192, 136, 80], [42, 74, 154]] {
            assert_eq!(hex(lab(rgb)), format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]));
        }
    }

    #[test]
    fn the_hint_says_what_to_change_in_the_render() {
        let purple = lab([0x6a, 0x2a, 0x8f]);
        let bluer = lab([0x5b, 0x2f, 0x95]);
        assert_eq!(hint(purple, purple), "close");
        assert_eq!(hint(purple, bluer), "hue toward blue 11°");
        assert_eq!(hint(purple, lab([0x40, 0x1a, 0x60])), "darker by ΔL .11, less saturated by .04");
        assert!(hint(lab([40, 40, 40]), lab([120, 120, 120])).starts_with("lighter by ΔL"));
        assert!(hint(lab([200, 60, 60]), lab([150, 110, 110])).contains("less saturated"));
        assert!(hint(lab([150, 110, 110]), lab([200, 60, 60])).contains("more saturated"));
    }

    #[test]
    fn a_grey_pair_gets_no_hue_hint() {
        assert!(!hint(lab([100, 100, 100]), lab([110, 108, 100])).contains("hue"));
    }

    #[test]
    fn only_author_names_count_as_named() {
        assert!(is_named("spine_3"));
        assert!(is_named("books.spine_3"));
        assert!(is_named("bit.2"));
        assert!(is_named("rect~1.child"));
        assert!(!is_named("books.rect~7"));
        assert!(!is_named("circle~1.0"));
    }

    #[test]
    fn box_means_come_from_the_summed_area_table() {
        let px = |c: [u8; 3]| vec![c; 4];
        let render = Raster { w: 4, h: 4, rgb: [px([255, 0, 0]), px([255, 0, 0]), px([0, 0, 255]), px([0, 0, 255])].concat() };
        let reference = Raster { w: 4, h: 4, rgb: [px([255, 0, 0]), px([255, 0, 0]), px([255, 0, 0]), px([255, 0, 0])].concat() };
        let m = Measure::new(&render, &reference);
        assert!(m.region(0, 0, 4, 2).unwrap().de < 1e-9);
        let bottom = m.region(0, 2, 4, 4).unwrap();
        assert!(bottom.de > 40.0);
        assert_eq!(hex(bottom.render), "#0000ff");
        assert_eq!(hex(bottom.reference), "#ff0000");
        assert!(m.region(2, 2, 2, 4).is_none());
        let s = summarize(&m.de);
        assert_eq!(s.indistinguishable, 50.0);
    }
}
