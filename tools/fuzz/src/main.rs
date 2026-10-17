//! Spike: mutation fuzzer for artscript properties. See tools/fuzz/README.md.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use artscript_eval::{Format, Project, evaluate, frame_delays, plan, sample};
use artscript_ir::{Node, NodeKind, Scene};
use artscript_render::Backend;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
    fn chance(&mut self, p: f64) -> bool {
        (self.next() % 10_000) as f64 / 10_000.0 < p
    }
    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }
}

struct Corpus {
    manifest: String,
    libs: BTreeMap<String, String>,
    entries: Vec<(String, String)>,
}

impl Corpus {
    fn load(root: &Path) -> Corpus {
        let project = artscript_eval::load(root).expect("load corpus project");
        let mut libs = BTreeMap::new();
        let mut entries = Vec::new();
        for (path, src) in &project.files {
            if project.is_lib_file(path) {
                libs.insert(path.clone(), src.clone());
            } else {
                // The examples' 512px, 24fps GIFs cost a minute of CPU per sample; keep cases fast.
                let small: String = src.lines().filter(|l| !(l.starts_with("@target") && l.contains(".gif"))).map(|l| format!("{l}\n")).collect();
                entries.push((path.clone(), small));
            }
        }
        Corpus { manifest: project.manifest_src.clone(), libs, entries }
    }

    fn project(&self, manifest: &str, entry: &str, src: &str) -> Option<Project> {
        let mut files = self.libs.clone();
        files.insert(entry.to_string(), src.to_string());
        Project::from_sources(manifest, files).ok()
    }
}

// ---------- line model ----------

fn indent(l: &str) -> usize {
    l.len() - l.trim_start().len()
}

fn is_blank(l: &str) -> bool {
    l.trim().is_empty() || l.trim_start().starts_with('#')
}

/// One past the last line of the block that starts at `i` (the line and its deeper children).
fn block_end(lines: &[String], i: usize) -> usize {
    let d = indent(&lines[i]);
    let mut j = i + 1;
    while j < lines.len() && (is_blank(&lines[j]) || indent(&lines[j]) > d) {
        j += 1;
    }
    while j > i + 1 && is_blank(&lines[j - 1]) {
        j -= 1;
    }
    j
}

fn first_word(l: &str) -> &str {
    l.split_whitespace().next().unwrap_or("")
}

const NON_DRAW: &[&str] = &[
    "use", "palette", "style", "finish", "def", "rule", "let", "shader", "light", "brush", "anim", "if", "else",
    "each", "assert", "expect", "slot", "ramp", "color", "fx", "mask", "anchor", "play",
];

fn is_draw(l: &str) -> bool {
    let w = first_word(l);
    !w.is_empty() && !w.starts_with('@') && !w.starts_with('.') && !w.starts_with('#') && !w.ends_with(':')
        && !NON_DRAW.contains(&w)
}

fn body_start(lines: &[String]) -> usize {
    lines.iter().position(|l| !l.starts_with('@')).unwrap_or(lines.len())
}

fn reindent(block: &[String], from: usize, to: usize) -> Vec<String> {
    block
        .iter()
        .map(|l| if is_blank(l) { l.clone() } else { format!("{}{}", " ".repeat(to), &l[from.min(indent(l))..]) })
        .collect()
}

// ---------- mutation ----------

const SNIPPETS: &[(&str, bool)] = &[
    ("circle rand(4 20) at:rand(100 900),rand(100 900) fill:#a0c0e0", false),
    ("let fz_v = rand(0 1)\ncircle 8+fz_v*10 at:200,200 fill:#203040", false),
    ("if rand(0 1) < .5\n  circle 12 at:300,300 fill:#405060\nelse\n  rect 20x20 at:300,300 fill:#506070", false),
    ("each fz_k in 0..3\n  circle 6+rand(0 4) at:100+fz_k*40,120 fill:#333 name:fz_dot", false),
    ("rect 40x20 r:4 at:50%,50% fill:pick([#f00 #0f0 #00f])", false),
    ("repeat 4 rot:90\n  ellipse 10,4 at:0,20 fill:#777", false),
    ("mirror x\n  circle 9 at:30,0 fill:#899", false),
    ("blob 30 seed:4 wobble:.2 at:600,200 fill:#9ab", false),
    ("group\n  circle 14 at:700,700 fill:#a33 name:fz_g", false),
    ("layer fz_node pivot:512,512\n  circle 30 at:512,512 fill:#c84 name:fz_disc", true),
    ("anim fzwig dur:500ms loop\n  .fz_node rot:key(-10 10 -10)", true),
    ("anim fzone dur:300ms\n  .fz_node at:key(0,0 20,0)", true),
    ("play fzwig", true),
    ("play fzone start:100ms", true),
    ("play fzwig start:rand(0 .2)", true),
    ("let fz_on = true\nif fz_on\n  play fzone", true),
    (ANIM_KIT, true),
    (ANIM_KIT, true),
    (ANIM_KIT, true),
];

const ANIM_KIT: &str = "layer fz_node pivot:512,512\n  circle 30 at:512,512 fill:#c84 name:fz_disc\nanim fzwig dur:500ms loop\n  .fz_node rot:key(-10 10 -10)\nanim fzone dur:300ms\n  .fz_node at:key(0,0 20,0)\nplay fzwig\nplay fzone start:100ms";

const GIF_TARGETS: &[&str] = &[
    "@target fz/clip.gif 256x256 bg:#ffffff fps:8 dur:500ms",
    "@target fz/clip.gif 256x256 bg:#ffffff fps:12",
    "@target fz/clip.gif 128x128 bg:#f0f0f0 fps:10 dur:300ms clip:idle",
];

fn tweak_number(rng: &mut Rng, l: &str) -> String {
    let b: Vec<char> = l.chars().collect();
    let runs: Vec<(usize, usize)> = {
        let mut v = Vec::new();
        let mut i = 0;
        while i < b.len() {
            if b[i].is_ascii_digit() && (i == 0 || !b[i - 1].is_alphanumeric() && b[i - 1] != '#' && b[i - 1] != '_') {
                let s = i;
                while i < b.len() && (b[i].is_ascii_digit() || b[i] == '.') {
                    i += 1;
                }
                if i >= b.len() || !b[i].is_alphanumeric() || "sxm%d".contains(b[i]) {
                    v.push((s, i));
                }
            } else {
                i += 1;
            }
        }
        v
    };
    if runs.is_empty() {
        return l.to_string();
    }
    let (s, e) = *rng.pick(&runs);
    let old: f64 = b[s..e].iter().collect::<String>().parse().unwrap_or(1.0);
    let new = match rng.below(6) {
        0 => "0".to_string(),
        1 => "1".to_string(),
        2 => format!("{}", (old * 2.0).round()),
        3 => format!("{}", (old / 2.0).max(0.0)),
        4 => format!("{}", old + 1.0),
        _ => ".5".to_string(),
    };
    format!("{}{}{}", b[..s].iter().collect::<String>(), new, b[e..].iter().collect::<String>())
}

fn mutate(rng: &mut Rng, corpus: &Corpus, src: &str) -> String {
    let mut lines: Vec<String> = src.lines().map(String::from).collect();
    let rounds = 1 + rng.below(4);
    for _ in 0..rounds {
        let start = body_start(&lines);
        if start >= lines.len() {
            break;
        }
        let cand: Vec<usize> = (start..lines.len()).filter(|&i| !is_blank(&lines[i])).collect();
        let i = *rng.pick(&cand);
        let d = indent(&lines[i]);
        let e = block_end(&lines, i);
        match rng.below(9) {
            0 if e - i < lines.len() / 3 => {
                lines.drain(i..e);
            }
            1 => {
                let copy: Vec<String> = lines[i..e].to_vec();
                lines.splice(e..e, copy);
            }
            2 => {
                if e < lines.len() && indent(&lines[e]) == d && !is_blank(&lines[e]) {
                    let e2 = block_end(&lines, e);
                    let a: Vec<String> = lines[i..e].to_vec();
                    let bb: Vec<String> = lines[e..e2].to_vec();
                    lines.splice(i..e2, bb.into_iter().chain(a));
                }
            }
            3 | 4 => lines[i] = tweak_number(rng, &lines[i]),
            5 => {
                let (_, other) = rng.pick(&corpus.entries);
                let ol: Vec<String> = other.lines().map(String::from).collect();
                let os = body_start(&ol);
                let oc: Vec<usize> = (os..ol.len()).filter(|&k| is_draw(&ol[k])).collect();
                if !oc.is_empty() && is_draw(&lines[i]) {
                    let k = *rng.pick(&oc);
                    let ke = block_end(&ol, k);
                    if ke - k < 30 {
                        let block = reindent(&ol[k..ke], indent(&ol[k]), d);
                        lines.splice(i..i, block);
                    }
                }
            }
            6 | 7 => {
                let (snip, top) = *rng.pick(SNIPPETS);
                let (at, ind) = if top {
                    let tops: Vec<usize> = (start..=lines.len())
                        .filter(|&k| k == lines.len() || (indent(&lines[k]) == 0 && !is_blank(&lines[k])))
                        .collect();
                    (*rng.pick(&tops), 0)
                } else {
                    (i, d)
                };
                let block: Vec<String> = snip.lines().map(|l| format!("{}{}", " ".repeat(ind), l)).collect();
                lines.splice(at..at, block);
            }
            _ => {
                if is_draw(&lines[i]) || first_word(&lines[i]) == "play" {
                    let head = match rng.below(3) {
                        0 => "if true",
                        1 => "group",
                        _ => "if 1 < 2",
                    };
                    let body = reindent(&lines[i..e], d, d + 2);
                    let mut block = vec![format!("{}{}", " ".repeat(d), head)];
                    block.extend(body);
                    lines.splice(i..e, block);
                }
            }
        }
    }
    if rng.chance(0.4) && !lines.iter().any(|l| l.contains(".gif")) {
        lines.insert(0, rng.pick(GIF_TARGETS).to_string());
    }
    if rng.chance(0.35) {
        let targets: Vec<usize> = (0..lines.len()).filter(|&k| lines[k].starts_with("@target")).collect();
        if !targets.is_empty() {
            let k = *rng.pick(&targets);
            lines[k] = mutate_target(rng, &lines[k]);
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

// ---------- observation ----------

#[derive(Debug, PartialEq, Clone)]
struct Diag {
    sev: String,
    code: String,
    file: String,
    line: u32,
    node: String,
    msg: String,
}

#[derive(Debug, PartialEq, Clone)]
struct JobObs {
    key: String,
    gif: bool,
    frames: Vec<String>,
    diags: Vec<Diag>,
    nodes: Vec<BTreeMap<String, String>>,
}

#[derive(Debug, PartialEq, Clone)]
struct Obs {
    plan_diags: Vec<Diag>,
    jobs: Vec<JobObs>,
}

fn diag(d: &artscript_syntax::Diagnostic) -> Diag {
    Diag {
        sev: format!("{:?}", d.severity),
        code: d.code.clone(),
        file: d.file.clone(),
        line: d.span.line,
        node: d.node.clone(),
        msg: d.message.clone(),
    }
}

fn walk(nodes: &[Node], out: &mut BTreeMap<String, String>) {
    for n in nodes {
        let mut n2 = n.clone();
        n2.origin.clear();
        out.insert(n.path.clone(), strip_gen(&format!("{:?}", n2.transform)));
        match &n.kind {
            NodeKind::Group { children, .. } => walk(children, out),
            NodeKind::Shape(s) => walk(&s.children, out),
        }
    }
}

/// Scene text without `origin` (file:line), which legitimately moves when lines move.
fn scene_text(s: &Scene) -> String {
    fn strip(nodes: &mut [Node]) {
        for n in nodes {
            n.origin.clear();
            match &mut n.kind {
                NodeKind::Group { children, .. } => strip(children),
                NodeKind::Shape(s) => strip(&mut s.children),
            }
        }
    }
    let mut s = s.clone();
    strip(&mut s.children);
    strip_origin(&strip_gen(&format!("{s:?}")))
}

fn strip_origin(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("origin: \"") {
        out.push_str(&rest[..i]);
        let tail = &rest[i + 9..];
        rest = &tail[tail.find('"').map_or(tail.len(), |j| j + 1)..];
    }
    out.push_str(rest);
    out
}

/// Skia's `generation_id` counts paths built in the process, so it differs between two equal scenes.
fn strip_gen(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("generation_id: ") {
        out.push_str(&rest[..i]);
        let tail = &rest[i + 15..];
        rest = &tail[tail.find(|c: char| !c.is_ascii_digit()).unwrap_or(tail.len())..];
    }
    out.push_str(rest);
    out
}

fn observe(p: &Project, entry: &str) -> Obs {
    let pl = plan(p);
    let jobs = pl
        .jobs
        .iter()
        .filter(|j| j.source == entry)
        .map(|j| {
            let gif = j.outputs.iter().any(|o| o.format == Format::Gif);
            if gif {
                let s = sample(p, j);
                JobObs {
                    key: j.key(),
                    gif,
                    frames: s.frames.iter().map(|f| scene_text(&f.scene)).collect(),
                    nodes: s
                        .frames
                        .iter()
                        .map(|f| {
                            let mut m = BTreeMap::new();
                            walk(&f.scene.children, &mut m);
                            m
                        })
                        .collect(),
                    diags: s.diagnostics.iter().map(diag).collect(),
                }
            } else {
                let ev = evaluate(p, j);
                JobObs {
                    key: j.key(),
                    gif,
                    frames: ev.scene.iter().map(scene_text).collect(),
                    nodes: Vec::new(),
                    diags: ev.diagnostics.iter().map(diag).collect(),
                }
            }
        })
        .collect();
    Obs { plan_diags: pl.diagnostics.iter().map(diag).collect(), jobs }
}

/// Bytes of every output of every job for `entry`, rendered the way `build` does (small PNG scale).
fn render_all(p: &Project, entry: &str) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for j in plan(p).jobs.iter().filter(|j| j.source == entry) {
        if j.outputs.iter().any(|o| o.format == Format::Gif) {
            let s = sample(p, j);
            if s.diagnostics.iter().any(|d| d.code.starts_with('E')) {
                continue;
            }
            if let Ok(delays) = frame_delays(s.frames.len(), s.dur)
                && !delays.is_empty()
            {
                let scenes: Vec<&Scene> = s.frames.iter().map(|f| &f.scene).collect();
                if let Ok(b) = artscript_render::render_gif(&scenes, 1.0, &delays, s.loops, Backend::Cpu) {
                    out.push((j.outputs[0].path.clone(), b));
                }
            }
            continue;
        }
        let ev = evaluate(p, j);
        if ev.diagnostics.iter().any(|d| d.code.starts_with('E')) {
            continue;
        }
        let Some(scene) = ev.scene else { continue };
        for o in &j.outputs {
            let b = match o.format {
                Format::Png => artscript_render::render_png(&scene, o.scale * 0.125, Backend::Cpu).ok(),
                Format::Svg => artscript_render::render_svg(&scene, j.profile, j.bake_scale, Backend::Cpu).ok().map(String::into_bytes),
                Format::Gif => None,
            };
            if let Some(b) = b {
                out.push((o.path.clone(), b));
            }
        }
    }
    out
}

/// First difference between two observations, ignoring diagnostic lines when `lines` is false.
fn obs_diff(a: &Obs, b: &Obs, lines: bool) -> Option<String> {
    let norm = |d: &[Diag]| -> Vec<Diag> {
        d.iter()
            .map(|d| Diag { line: if lines { d.line } else { 0 }, ..d.clone() })
            .collect()
    };
    if norm(&a.plan_diags) != norm(&b.plan_diags) {
        return Some(format!("plan diagnostics differ:\n  {:?}\n  {:?}", a.plan_diags, b.plan_diags));
    }
    if a.jobs.len() != b.jobs.len() {
        return Some(format!("job count {} vs {}", a.jobs.len(), b.jobs.len()));
    }
    for (x, y) in a.jobs.iter().zip(&b.jobs) {
        if x.frames.len() != y.frames.len() {
            return Some(format!("{}: frame count {} vs {}", x.key, x.frames.len(), y.frames.len()));
        }
        for (i, (f, g)) in x.frames.iter().zip(&y.frames).enumerate() {
            if f != g {
                let at = f.bytes().zip(g.bytes()).position(|(p, q)| p != q).unwrap_or(0);
                let lo = at.saturating_sub(120);
                return Some(format!(
                    "{}: frame {i} scene differs near byte {at}:\n  A …{}…\n  B …{}…",
                    x.key,
                    &f[lo..(at + 120).min(f.len())],
                    &g[lo..(at + 120).min(g.len())]
                ));
            }
        }
        let (dx, dy) = (norm(&x.diags), norm(&y.diags));
        if dx != dy {
            let only_a: Vec<_> = dx.iter().filter(|d| !dy.contains(d)).collect();
            let only_b: Vec<_> = dy.iter().filter(|d| !dx.contains(d)).collect();
            let codes: Vec<&str> = only_a.iter().chain(&only_b).map(|d| d.code.as_str()).collect();
            return Some(format!(
                "{}: diagnostics differ {codes:?}\n  only A: {only_a:?}\n  only B: {only_b:?}",
                x.key
            ));
        }
    }
    None
}

// ---------- properties ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Prop {
    Crash,
    FmtIdempotent,
    FmtKeepsOutput,
    Deterministic,
    CommentInert,
    WrapInert,
    ScaleIsolated,
    StaticAcrossFrames,
    DiagSane,
    UniquePaths,
    LastPlayWins,
    GifTiming,
}

const MARK: &str = "# fzmark";
const WRAP: &str = "# fzwrap";
const EARLY: &str = "# fzearly";

impl Prop {
    fn name(self) -> &'static str {
        match self {
            Prop::Crash => "crash",
            Prop::FmtIdempotent => "fmt-idempotent",
            Prop::FmtKeepsOutput => "fmt-keeps-output",
            Prop::Deterministic => "deterministic",
            Prop::CommentInert => "comment-inert",
            Prop::WrapInert => "wrap-inert",
            Prop::ScaleIsolated => "scale-isolated",
            Prop::StaticAcrossFrames => "static-across-frames",
            Prop::DiagSane => "diag-sane",
            Prop::UniquePaths => "unique-paths",
            Prop::LastPlayWins => "last-play-wins",
            Prop::GifTiming => "gif-timing",
        }
    }
    fn all() -> &'static [Prop] {
        &[
            Prop::FmtIdempotent,
            Prop::FmtKeepsOutput,
            Prop::Deterministic,
            Prop::CommentInert,
            Prop::WrapInert,
            Prop::ScaleIsolated,
            Prop::StaticAcrossFrames,
            Prop::DiagSane,
            Prop::UniquePaths,
            Prop::LastPlayWins,
            Prop::GifTiming,
        ]
    }
    fn parse(s: &str) -> Option<Prop> {
        Prop::all().iter().copied().chain([Prop::Crash]).find(|p| p.name() == s)
    }
}

struct Ctx<'a> {
    corpus: &'a Corpus,
    entry: String,
}

fn unmark(src: &str) -> String {
    let mut s: String = src.lines().filter(|l| l.trim() != MARK).collect::<Vec<_>>().join("\n");
    s.push('\n');
    s
}

fn unwrap_marked(src: &str) -> Option<String> {
    let lines: Vec<String> = src.lines().map(String::from).collect();
    let i = lines.iter().position(|l| l.ends_with(WRAP))?;
    let e = block_end(&lines, i);
    let d = indent(&lines[i]);
    let mut out = lines[..i].to_vec();
    out.push(format!("{}# unwrapped", " ".repeat(d)));
    out.extend(reindent(&lines[i + 1..e], d + 2, d));
    out.extend(lines[e..].iter().cloned());
    let mut s = out.join("\n");
    s.push('\n');
    Some(s)
}

const STATIC_SNIPPET: &str = "if t < .2\n  let fz_a = rand(0 1)\nelse\n  let fz_b = 1\nlayer fz_static\n  circle rand(5 30) at:rand(100 900),rand(100 900) fill:#123456\nanim fz_still dur:rand(1 2) loop\n  .fz_static at:rand(0 40),rand(0 40)\nplay fz_still start:rand(0 .01)\nlayer fz_lin\n  circle 9 at:100,900 fill:#654321\nanim fz_slide dur:rand(4 5)\n  .fz_lin at:key(0,0 400,0 ease:linear)\nplay fz_slide start:rand(-.2 -.1)\n";

fn check(ctx: &Ctx, prop: Prop, src: &str) -> Option<String> {
    let c = ctx.corpus;
    let man = &c.manifest;
    match prop {
        Prop::Crash => None,
        Prop::FmtIdempotent => {
            let f1 = artscript_syntax::format_source(src).ok()?;
            match artscript_syntax::format_source(&f1) {
                Err(e) => Some(format!("formatted output no longer parses: {:?}", e.first().map(|d| &d.message))),
                Ok(f2) if f2 != f1 => Some(format!("fmt not idempotent:\n--- once\n{f1}--- twice\n{f2}")),
                Ok(_) => None,
            }
        }
        Prop::FmtKeepsOutput => {
            let f1 = artscript_syntax::format_source(src).ok()?;
            if f1 == src {
                return None;
            }
            let a = observe(&c.project(man, &ctx.entry, src)?, &ctx.entry);
            let b = observe(&c.project(man, &ctx.entry, &f1)?, &ctx.entry);
            obs_diff(&a, &b, false).map(|d| format!("fmt changed the output: {d}"))
        }
        Prop::Deterministic => {
            let p = c.project(man, &ctx.entry, src)?;
            let a = observe(&p, &ctx.entry);
            let b = observe(&c.project(man, &ctx.entry, src)?, &ctx.entry);
            if let Some(d) = obs_diff(&a, &b, true) {
                return Some(format!("two evaluations differ: {d}"));
            }
            let ra = render_all(&p, &ctx.entry);
            let rb = render_all(&p, &ctx.entry);
            for ((ka, ba), (_, bb)) in ra.iter().zip(&rb) {
                if ba != bb {
                    return Some(format!("{ka}: two renders differ"));
                }
            }
            (ra.len() != rb.len()).then(|| "render count differs".into())
        }
        Prop::CommentInert => {
            if !src.contains(MARK) {
                return None;
            }
            let a = observe(&c.project(man, &ctx.entry, &unmark(src))?, &ctx.entry);
            let b = observe(&c.project(man, &ctx.entry, src)?, &ctx.entry);
            let seeded = src.split(MARK).nth(1).is_some_and(|after| after.contains("rand(") || after.contains("pick("));
            let why = if seeded { "rand below the comment" } else { "no rand below" };
            obs_diff(&a, &b, false).map(|d| format!("a comment line changed the output ({why}): {d}"))
        }
        Prop::WrapInert => {
            let base = unwrap_marked(src)?;
            let a = observe(&c.project(man, &ctx.entry, &base)?, &ctx.entry);
            if a.jobs.iter().any(|j| j.diags.iter().any(|d| d.code.starts_with('E'))) {
                return None;
            }
            let b = observe(&c.project(man, &ctx.entry, src)?, &ctx.entry);
            let errors: Vec<&str> =
                b.jobs.iter().flat_map(|j| &j.diags).filter(|d| d.code.starts_with('E')).map(|d| d.code.as_str()).collect();
            if !errors.is_empty() {
                let head = src.lines().find(|l| l.ends_with(WRAP)).unwrap_or("");
                if head.contains('@') || head.trim_start().starts_with("group") {
                    return None;
                }
                return Some(format!("a wrapper that reads nothing introduced errors {errors:?}"));
            }
            obs_diff(&a, &b, false).map(|d| format!("wrapping a line in an always-true block changed the output: {d}"))
        }
        Prop::ScaleIsolated => {
            let p1 = c.project(man, &ctx.entry, src)?;
            let man2 = man.replace("scale   = [1]", "scale   = [1, 2]");
            let p2 = c.project(&man2, &ctx.entry, src)?;
            let a: BTreeMap<_, _> = render_all(&p1, &ctx.entry).into_iter().collect();
            let b: BTreeMap<_, _> = render_all(&p2, &ctx.entry).into_iter().collect();
            for (k, v) in &a {
                if k.ends_with(".svg") {
                    continue;
                }
                if let Some(w) = b.get(k)
                    && v != w
                {
                    let ext = k.rsplit('.').next().unwrap_or("");
                    return Some(format!("{k}: {ext} bytes change when the manifest adds scale 2"));
                }
            }
            None
        }
        Prop::StaticAcrossFrames => {
            if !src.contains("fz_static") || !src.contains(".gif") {
                return None;
            }
            let o = observe(&c.project(man, &ctx.entry, src)?, &ctx.entry);
            for j in o.jobs.iter().filter(|j| j.gif) {
                if j.diags.iter().any(|d| d.code.starts_with('E')) {
                    continue;
                }
                let vals: Vec<Option<&String>> = j.nodes.iter().map(|m| m.get("fz_static")).collect();
                if vals.windows(2).any(|w| w[0] != w[1]) {
                    return Some(format!("{}: fz_static moves between frames: {vals:?}", j.key));
                }
                let xs: Vec<f64> = j.nodes.iter().filter_map(|m| m.get("fz_lin").and_then(|t| tx(t))).collect();
                let steps: Vec<f64> = xs.windows(2).map(|w| w[1] - w[0]).collect();
                if steps.windows(2).any(|w| (w[0] - w[1]).abs() > 1e-3) {
                    return Some(format!("{}: a linear key moves by unequal steps: {xs:?}", j.key));
                }
            }
            None
        }
        Prop::UniquePaths => {
            let p = c.project(man, &ctx.entry, src)?;
            for (key, scenes) in scenes_of(&p, &ctx.entry) {
                for (i, scene) in scenes.iter().enumerate() {
                    let mut paths = Vec::new();
                    collect_paths(&scene.children, &mut paths);
                    paths.sort();
                    if let Some(w) = paths.windows(2).find(|w| w[0] == w[1]) {
                        return Some(format!("{key}: frame {i}: two nodes share the path {}", w[0]));
                    }
                }
            }
            None
        }
        Prop::LastPlayWins => {
            if !src.contains(EARLY) {
                return None;
            }
            let base: String = src
                .lines()
                .map(|l| if l.ends_with(EARLY) { format!("{}play none", " ".repeat(indent(l))) } else { l.to_string() })
                .map(|l| l + "\n")
                .collect();
            let a = observe(&c.project(man, &ctx.entry, &base)?, &ctx.entry);
            if a.jobs.iter().any(|j| j.diags.iter().any(|d| d.code.starts_with('E'))) {
                return None;
            }
            let b = observe(&c.project(man, &ctx.entry, src)?, &ctx.entry);
            let errors: Vec<&str> =
                b.jobs.iter().flat_map(|j| &j.diags).filter(|d| d.code.starts_with('E')).map(|d| d.code.as_str()).collect();
            if !errors.is_empty() {
                return Some(format!("an earlier play that a later one overrides introduced errors {errors:?}"));
            }
            obs_diff(&a, &b, false).map(|d| format!("an earlier play showed through a later play of the same channel: {d}"))
        }
        Prop::GifTiming => {
            let p = c.project(man, &ctx.entry, src)?;
            for j in plan(&p).jobs.iter().filter(|j| j.source == ctx.entry) {
                if !j.outputs.iter().any(|o| o.format == Format::Gif) {
                    continue;
                }
                let s = sample(&p, j);
                if s.diagnostics.iter().any(|d| d.code.starts_with('E')) {
                    continue;
                }
                let key = j.key();
                let product = s.dur * s.fps;
                let want = if (product - product.round()).abs() < 1e-8 { product.round() } else { product.ceil() };
                if s.frames.len() as f64 != want {
                    return Some(format!("{key}: {} frames for dur {} at fps {}, want {want}", s.frames.len(), s.dur, s.fps));
                }
                if let Some(f) = s.frames.iter().find(|f| f.t >= s.dur) {
                    return Some(format!("{key}: a frame at t={} is not before dur {}", f.t, s.dur));
                }
                match frame_delays(s.frames.len(), s.dur) {
                    Err(e) => return Some(format!("{key}: sample reported no error but the delays fail: {e}")),
                    Ok(d) if d.contains(&0) => return Some(format!("{key}: a zero delay {d:?}")),
                    Ok(d) => {
                        let total: u64 = d.iter().map(|&x| x as u64).sum();
                        let want = (s.dur * 100.0).round() as u64;
                        if total != want {
                            return Some(format!("{key}: delays sum to {total}cs, want round(dur*100) = {want}cs"));
                        }
                    }
                }
            }
            None
        }
        Prop::DiagSane => {
            let p = c.project(man, &ctx.entry, src)?;
            let o = observe(&p, &ctx.entry);
            for j in &o.jobs {
                for d in &j.diags {
                    if d.file.is_empty() {
                        return Some(format!("{}: diagnostic with no file {d:?}", j.key));
                    }
                    if let Some(text) = p.files.get(&d.file) {
                        let n = text.lines().count() as u32;
                        if d.line == 0 || d.line > n {
                            return Some(format!("{}: line {} outside {} ({n} lines) {d:?}", j.key, d.line, d.file));
                        }
                    } else if d.file != "artscript.toml" {
                        return Some(format!("{}: diagnostic names unknown file {d:?}", j.key));
                    }
                }
            }
            None
        }
    }
}

/// Every scene each job for `entry` produces when it compiles: one for a still, every frame for a GIF.
fn scenes_of(p: &Project, entry: &str) -> Vec<(String, Vec<Scene>)> {
    plan(p)
        .jobs
        .iter()
        .filter(|j| j.source == entry)
        .map(|j| {
            let compiled = |diags: &[artscript_syntax::Diagnostic]| !diags.iter().any(|d| d.code.starts_with('E'));
            let scenes = if j.outputs.iter().any(|o| o.format == Format::Gif) {
                let s = sample(p, j);
                if compiled(&s.diagnostics) { s.frames.into_iter().map(|f| f.scene).collect() } else { Vec::new() }
            } else {
                let ev = evaluate(p, j);
                if compiled(&ev.diagnostics) { ev.scene.into_iter().collect() } else { Vec::new() }
            };
            (j.key(), scenes)
        })
        .collect()
}

fn collect_paths(nodes: &[Node], out: &mut Vec<String>) {
    for n in nodes {
        out.push(n.path.clone());
        match &n.kind {
            NodeKind::Group { children, .. } => collect_paths(children, out),
            NodeKind::Shape(s) => collect_paths(&s.children, out),
        }
    }
}

/// Two plays that set one channel, the earlier marked `EARLY`. The later one must win wherever the earlier one sits.
fn play_order_snippet(rng: &mut Rng) -> String {
    let (early, late) = *rng.pick(&[
        ("at:abs(11,0)", "at:abs(77,0)"),
        ("rot:abs(10)", "rot:abs(40)"),
        ("fill:#ff0000", "fill:#00ff00"),
        ("a:.3", "a:.8"),
    ]);
    let host = *rng.pick(&["", "group fz_g", "layer fz_g"]);
    let (inst, path) = if host.is_empty() {
        ("fz_pbox name:fz_bx".to_string(), "fz_bx.part")
    } else {
        (format!("{host}\n  fz_pbox name:fz_bx"), "fz_g.fz_bx.part")
    };
    let inst_indent = if host.is_empty() { 2 } else { 4 };
    let wrap = *rng.pick(&["", "if true", "each fz_once in [0]"]);
    let play_at = |ind: usize, anim: &str| -> String {
        let pad = " ".repeat(ind);
        if wrap.is_empty() {
            format!("{pad}play {anim} {EARLY}")
        } else {
            format!("{pad}{wrap}\n{pad}  play {anim} {EARLY}")
        }
    };
    let mut out = String::from("def fz_pbox\n  rect 20x20 at:512,512 fill:#335577 name:part\n");
    let under_instance = rng.chance(0.5);
    let early_patch = if under_instance { ".part".to_string() } else { format!(".{path}") };
    out += &format!("anim fz_early dur:1s loop\n  {early_patch} {early}\nanim fz_late dur:1s loop\n  .{path} {late}\n");
    out += &inst;
    out.push('\n');
    if under_instance {
        out += &play_at(inst_indent, "fz_early");
    } else {
        out += &play_at(0, "fz_early");
    }
    out.push('\n');
    if rng.chance(0.3) {
        out += "if true\n  play fz_late\n";
    } else {
        out += "play fz_late\n";
    }
    out
}

fn set_key(line: &str, key: &str, val: &str) -> String {
    let prefix = format!("{key}:");
    let mut found = false;
    let mut toks: Vec<String> = line
        .split(' ')
        .map(|t| {
            if t.starts_with(&prefix) {
                found = true;
                format!("{prefix}{val}")
            } else {
                t.to_string()
            }
        })
        .collect();
    if !found {
        toks.push(format!("{prefix}{val}"));
    }
    toks.join(" ")
}

fn mutate_target(rng: &mut Rng, line: &str) -> String {
    match rng.below(7) {
        0 if line.contains(" bg:") => line.split(' ').filter(|t| !t.starts_with("bg:")).collect::<Vec<_>>().join(" "),
        0 => set_key(line, "bg", "#ffffff"),
        1 => set_key(line, "fps", rng.pick(&["1", "7", "10", "24", "30", "100", "120"])),
        2 => set_key(line, "dur", rng.pick(&["100ms", "250ms", "300ms", ".25", "1s", "1.5s", "100ms+200ms"])),
        3 => set_key(&set_key(line, "fps", ".01"), "dur", "700s"),
        4 => {
            let mut toks: Vec<&str> = line.split(' ').collect();
            let at = 3.min(toks.len());
            let clip = *rng.pick(&["clip:idle", "clip:[idle blink]", "clip:none"]);
            toks.insert(at, clip);
            toks.join(" ")
        }
        5 if !line.contains('{') => {
            let mut toks: Vec<String> = line.split(' ').map(String::from).collect();
            if let Some((stem, ext)) = toks.get(1).and_then(|t| t.rsplit_once('.')) {
                toks[1] = format!("{stem}_{{k}}.{ext}");
            }
            toks.push(rng.pick(&["each k:[a b]", "each k:oops", "each k:[a b] j:[x]"]).to_string());
            toks.join(" ")
        }
        _ => set_key(line, "scale", rng.pick(&["1,2", "2", "0"])),
    }
}

fn tx(matrix: &str) -> Option<f64> {
    let inner = matrix.split('[').nth(1)?;
    inner.split(',').nth(2)?.trim().parse().ok()
}

fn guarded(ctx: &Ctx, prop: Prop, src: &str) -> Option<String> {
    let deadline = Instant::now() + Duration::from_secs(400);
    WATCH.store(deadline_secs(deadline), Ordering::SeqCst);
    let r = catch_unwind(AssertUnwindSafe(|| check(ctx, prop, src)));
    WATCH.store(0, Ordering::SeqCst);
    match r {
        Ok(v) => v,
        Err(e) => {
            let msg = e
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            Some(format!("PANIC in {}: {msg}", prop.name()))
        }
    }
}

static WATCH: AtomicU64 = AtomicU64::new(0);
static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
static CURRENT: std::sync::Mutex<Option<(PathBuf, String)>> = std::sync::Mutex::new(None);

fn deadline_secs(d: Instant) -> u64 {
    d.duration_since(*START.get().unwrap()).as_secs().max(1)
}

fn spawn_watchdog() {
    START.get_or_init(Instant::now);
    std::thread::spawn(|| loop {
        std::thread::sleep(Duration::from_secs(1));
        let w = WATCH.load(Ordering::SeqCst);
        if w != 0 && START.get().unwrap().elapsed().as_secs() > w {
            if let Some((dir, src)) = CURRENT.lock().unwrap().clone() {
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::fs::write(dir.join("hang.art"), src);
                eprintln!("HANG: wrote {}", dir.join("hang.art").display());
            }
            std::process::exit(99);
        }
    });
}

/// Variants of one mutated source, each carrying the markers its property reads.
fn variants(rng: &mut Rng, src: &str) -> Vec<(Prop, String)> {
    let lines: Vec<String> = src.lines().map(String::from).collect();
    let start = body_start(&lines);
    let mut v = Vec::new();
    for p in [Prop::FmtIdempotent, Prop::FmtKeepsOutput, Prop::Deterministic, Prop::DiagSane, Prop::UniquePaths] {
        v.push((p, src.to_string()));
    }
    if src.contains(".gif") {
        v.push((Prop::GifTiming, src.to_string()));
    }
    if rng.chance(0.4) {
        let mut s = src.to_string();
        if !s.contains(".gif") {
            s = format!("{}\n{s}", GIF_TARGETS[0]);
        }
        s.push_str(&play_order_snippet(rng));
        v.push((Prop::LastPlayWins, s));
    }
    if rng.chance(0.3) {
        v.push((Prop::ScaleIsolated, src.to_string()));
    }
    if start < lines.len() {
        let i = start + rng.below(lines.len() - start);
        let mut l2 = lines.clone();
        let ind = if is_blank(&lines[i]) { 0 } else { indent(&lines[i]) };
        l2.insert(i, format!("{}{MARK}", " ".repeat(ind)));
        v.push((Prop::CommentInert, l2.join("\n") + "\n"));

        let cand: Vec<usize> =
            (start..lines.len()).filter(|&k| is_draw(&lines[k]) || first_word(&lines[k]) == "play").collect();
        let plays: Vec<usize> = cand.iter().copied().filter(|&k| first_word(&lines[k]) == "play").collect();
        let pool = if !plays.is_empty() && rng.chance(0.6) { plays } else { cand };
        if !pool.is_empty() {
            let k = *rng.pick(&pool);
            let e = block_end(&lines, k);
            let d = indent(&lines[k]);
            let named: Vec<String> = lines[start..]
                .iter()
                .filter_map(|l| l.split_whitespace().find_map(|t| t.strip_prefix("name:")).map(String::from))
                .collect();
            let mut heads = vec!["if true".to_string(), "each fz_once in [0]".to_string(), "group".to_string()];
            if !named.is_empty() {
                let n = rng.pick(&named).clone();
                heads.push(format!("if @{n}.w > -1"));
                heads.push(format!("each fz_once in [@{n}.w]"));
            }
            let head = rng.pick(&heads).clone();
            if first_word(&lines[k]) == "play" || !(head == "group" || head.starts_with("each")) {
                let mut l3 = lines[..k].to_vec();
                l3.push(format!("{}{head} {WRAP}", " ".repeat(d)));
                l3.extend(reindent(&lines[k..e], d, d + 2));
                l3.extend(lines[e..].iter().cloned());
                v.push((Prop::WrapInert, l3.join("\n") + "\n"));
            }
        }
    }
    if rng.chance(0.3) {
        let mut s = src.to_string();
        if !s.contains(".gif") {
            s = format!("{}\n{s}", GIF_TARGETS[0]);
        }
        s.push_str(STATIC_SNIPPET);
        v.push((Prop::StaticAcrossFrames, s));
    }
    v
}

// ---------- minimization ----------

fn minimize(ctx: &Ctx, prop: Prop, src: &str, want: &dyn Fn(&str) -> bool) -> String {
    let keep = |l: &str| {
        l.starts_with('@') || l.contains(MARK) || l.ends_with(WRAP) || l.contains("fz_static") || l.ends_with(EARLY)
    };
    let mut lines: Vec<String> = src.lines().map(String::from).collect();
    if let Some(w) = lines.iter().position(|l| l.ends_with(WRAP))
        && w + 1 < lines.len()
    {
        lines[w + 1].push_str(" # fzbody");
    }
    let keep = |l: &str| keep(l) || l.ends_with("# fzbody");
    let still = |ls: &[String]| {
        let s = ls.join("\n") + "\n";
        guarded(ctx, prop, &s).is_some_and(|d| want(&d))
    };
    let mut chunk = lines.len() / 2;
    while chunk >= 1 {
        let mut i = 0;
        let mut progressed = false;
        while i < lines.len() {
            let end = (i + chunk).min(lines.len());
            if lines[i..end].iter().any(|l| keep(l)) {
                i += 1;
                continue;
            }
            let mut cand = lines.clone();
            cand.drain(i..end);
            if still(&cand) {
                lines = cand;
                progressed = true;
            } else {
                i += chunk;
            }
        }
        if !progressed {
            chunk /= 2;
        }
    }
    let mut i = 0;
    while i < lines.len() {
        let mut cand = lines.clone();
        let e = if is_blank(&lines[i]) { i + 1 } else { block_end(&lines, i) };
        cand.drain(i..e);
        if !cand.is_empty() && still(&cand) && !keep(&lines[i]) {
            lines = cand;
        } else {
            i += 1;
        }
    }
    lines.join("\n") + "\n"
}

// ---------- driver ----------

struct Args {
    root: PathBuf,
    out: PathBuf,
    iters: u64,
    seed: u64,
    secs: u64,
    only: Option<Prop>,
    repro: Vec<PathBuf>,
    generate: Option<u64>,
}

fn args() -> Args {
    let mut a = Args {
        root: PathBuf::from("examples/chibi"),
        out: PathBuf::from("target/fuzz-findings"),
        iters: 200,
        seed: 1,
        secs: 0,
        only: None,
        repro: Vec::new(),
        generate: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(k) = it.next() {
        match k.as_str() {
            "--root" => a.root = it.next().unwrap().into(),
            "--out" => a.out = it.next().unwrap().into(),
            "--iters" => a.iters = it.next().unwrap().parse().unwrap(),
            "--seed" => a.seed = it.next().unwrap().parse().unwrap(),
            "--secs" => a.secs = it.next().unwrap().parse().unwrap(),
            "--prop" => a.only = Prop::parse(&it.next().unwrap()),
            "gen" => a.generate = Some(it.next().unwrap().parse().unwrap()),
            "repro" => a.repro = it.by_ref().map(PathBuf::from).collect(),
            _ => panic!("unknown arg {k}"),
        }
    }
    a
}

/// A short signature used to bucket findings so one bug is reported once.
fn bucket(prop: Prop, detail: &str) -> String {
    let first = detail.lines().next().unwrap_or("");
    let cut: String = first
        .split(": ")
        .filter(|seg| !seg.contains("art/") && !seg.contains(".png") && !seg.contains(".svg") && !seg.contains(".gif"))
        .collect::<Vec<_>>()
        .join(": ")
        .chars()
        .filter(|c| !c.is_ascii_digit())
        .take(90)
        .collect();
    format!("{}|{cut}", prop.name())
}

fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    spawn_watchdog();
    let a = args();
    let corpus = Corpus::load(&a.root);

    if let Some(n) = a.generate {
        let mut rng = Rng(a.seed);
        for k in 0..n {
            let (entry, base) = rng.pick(&corpus.entries).clone();
            let src = mutate(&mut rng, &corpus, &base);
            let dir = a.out.join(format!("{k:04}"));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("entry"), &entry).unwrap();
            std::fs::write(dir.join("case.art"), src).unwrap();
        }
        return;
    }

    if !a.repro.is_empty() {
        let mut bad = 0;
        for f in &a.repro {
            let src = std::fs::read_to_string(f).unwrap();
            let entry = src
                .lines()
                .find_map(|l| l.strip_prefix("# entry: "))
                .unwrap_or("art/fuzz/case.art")
                .to_string();
            let ctx = Ctx { corpus: &corpus, entry };
            let props: Vec<Prop> = match src.lines().find_map(|l| l.strip_prefix("# prop: ")) {
                Some(p) => Prop::parse(p).into_iter().collect(),
                None => Prop::all().to_vec(),
            };
            let issue = src.lines().find_map(|l| l.strip_prefix("# issue: "));
            for p in props {
                match (guarded(&ctx, p, &src), issue) {
                    (Some(_), Some(n)) => println!("known  {} [{}] #{n}", f.display(), p.name()),
                    (None, Some(n)) => {
                        bad += 1;
                        println!("FIXED  {} [{}] passes now: move it to seeds/ and close #{n}", f.display(), p.name());
                    }
                    (Some(d), None) => {
                        bad += 1;
                        println!("FAIL   {} [{}]\n{d}\n", f.display(), p.name());
                    }
                    (None, None) => {}
                }
            }
        }
        println!("{bad} failing");
        std::process::exit(if bad > 0 { 1 } else { 0 });
    }

    let mut rng = Rng(a.seed);
    let mut buckets: BTreeMap<String, (u64, PathBuf)> = BTreeMap::new();
    let mut known: BTreeMap<String, (u32, u64)> = BTreeMap::new();
    let mut tried: BTreeMap<&'static str, u64> = BTreeMap::new();
    let t0 = Instant::now();
    let _ = std::fs::create_dir_all(&a.out);
    let mut it = 0u64;
    while it < a.iters && (a.secs == 0 || t0.elapsed().as_secs() < a.secs) {
        it += 1;
        let (entry, base) = rng.pick(&corpus.entries).clone();
        let entry = entry.replace("art/", "art/fuzz/");
        let src = mutate(&mut rng, &corpus, &base);
        let ctx = Ctx { corpus: &corpus, entry: entry.clone() };
        *CURRENT.lock().unwrap() = Some((a.out.join(format!("hang-{it}")), src.clone()));
        for (prop, text) in variants(&mut rng, &src) {
            if a.only.is_some_and(|o| o != prop) {
                continue;
            }
            *tried.entry(prop.name()).or_default() += 1;
            *CURRENT.lock().unwrap() = Some((a.out.join(format!("hang-{it}")), text.clone()));
            let _ = std::fs::write(a.out.join("last.art"), format!("# prop: {}\n# entry: {entry}\n{text}", prop.name()));
            let Some(detail) = guarded(&ctx, prop, &text) else { continue };
            let prop = if detail.starts_with("PANIC") { Prop::Crash } else { prop };
            let key = bucket(prop, &detail);
            if let Some(n) = known_issue(prop, &text, &detail) {
                let k = known.entry(key.clone()).or_insert((n, 0));
                if k.1 == 0 {
                    println!("[{it}] KNOWN #{n} {key}");
                }
                k.1 += 1;
                continue;
            }
            if let Some(b) = buckets.get_mut(&key) {
                b.0 += 1;
                continue;
            }
            let want_key = key.clone();
            let check_prop = if prop == Prop::Crash { prop_of_panic(&detail) } else { prop };
            let min = minimize(&ctx, check_prop, &text, &|d| {
                let p = if d.starts_with("PANIC") { Prop::Crash } else { check_prop };
                bucket(p, d) == want_key
            });
            let final_detail = guarded(&ctx, check_prop, &min).unwrap_or_else(|| detail.clone());
            let n = buckets.len();
            let file = a.out.join(format!("{n:02}-{}.art", prop.name()));
            let body = format!(
                "# prop: {}\n# entry: {entry}\n{}\n# ---- detail\n{}\n",
                check_prop.name(),
                min.trim_end(),
                final_detail.lines().map(|l| format!("# {l}")).collect::<Vec<_>>().join("\n")
            );
            std::fs::write(&file, body).unwrap();
            println!("[{it}] NEW {key}\n  -> {}", file.display());
            buckets.insert(key, (1, file));
        }
    }
    println!("\n{it} cases in {:.1}s; property checks run: {tried:?}", t0.elapsed().as_secs_f64());
    for (k, (issue, n)) in &known {
        println!("{n:5}  #{issue} {k}");
    }
    for (k, (n, f)) in &buckets {
        println!("{n:5}  NEW {k}\n       {}", f.display());
    }
    if !buckets.is_empty() {
        println!("\n{} failure(s) match no filed issue", buckets.len());
        std::process::exit(1);
    }
}

/// The filed issue a failure belongs to. CI passes on these and fails on any other failure.
fn known_issue(prop: Prop, text: &str, detail: &str) -> Option<u32> {
    let line_seeded = || {
        artscript_syntax::format_source(text).is_ok_and(|f| f.lines().count() != text.lines().count())
            && (text.contains("rand(") || text.contains("pick("))
    };
    let guard_reads_scene = text.lines().any(|l| l.ends_with(WRAP) && l.contains('@'));
    match prop {
        Prop::CommentInert if detail.contains("rand below the comment") => Some(39),
        Prop::FmtKeepsOutput if line_seeded() => Some(39),
        Prop::ScaleIsolated if detail.contains("png bytes") => Some(40),
        Prop::WrapInert if guard_reads_scene && detail.contains("changed the output") => Some(41),
        _ => None,
    }
}

fn prop_of_panic(detail: &str) -> Prop {
    let name = detail.trim_start_matches("PANIC in ").split(':').next().unwrap_or("");
    Prop::parse(name).unwrap_or(Prop::Deterministic)
}
