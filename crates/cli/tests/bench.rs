//! Every task in `bench/tasks` applies its `solution.patch` to a copy of `examples/chibi`, passes
//! `artscript check`, and meets each line of its `accept` block (see `bench/README.md`).

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use skia_safe::{AlphaType, ColorType, Data, Image, ImageInfo};

#[derive(Debug, Clone, Copy)]
struct Region {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
}

#[derive(Debug)]
enum Accept {
    Assert { file: String, expr: String },
    Changed { png: String, region: Region },
    Unchanged { png: String, region: Region },
    Differs { png: String, base: String, region: Region },
    Lines { file: String, lo: usize, hi: usize },
}

struct Task {
    id: String,
    dir: PathBuf,
    accept: Vec<Accept>,
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn region(at: &str, size: &str) -> Region {
    let (x, y) = at.split_once(',').expect("region x,y");
    let (w, h) = size.split_once('x').expect("region WxH");
    Region { x: x.parse().unwrap(), y: y.parse().unwrap(), w: w.parse().unwrap(), h: h.parse().unwrap() }
}

fn parse_accept(id: &str, md: &str) -> Vec<Accept> {
    let block = md
        .split("```accept\n")
        .nth(1)
        .and_then(|rest| rest.split("```").next())
        .unwrap_or_else(|| panic!("{id}: task.md has no ```accept block"));
    block
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let words: Vec<&str> = line.split_whitespace().collect();
            match words.as_slice() {
                ["assert", file, ..] => Accept::Assert {
                    file: file.to_string(),
                    expr: line.trim_start()["assert".len()..].trim_start()[file.len()..].trim().to_string(),
                },
                ["changed", png, at, size] => Accept::Changed { png: png.to_string(), region: region(at, size) },
                ["unchanged", png, at, size] => Accept::Unchanged { png: png.to_string(), region: region(at, size) },
                ["differs", png, base, at, size] => {
                    Accept::Differs { png: png.to_string(), base: base.to_string(), region: region(at, size) }
                }
                ["lines", file, range] => {
                    let (lo, hi) = range.split_once("..").expect("lines lo..hi");
                    Accept::Lines { file: file.to_string(), lo: lo.parse().unwrap(), hi: hi.parse().unwrap() }
                }
                _ => panic!("{id}: unknown accept line `{line}`"),
            }
        })
        .collect()
}

/// Every task, or the comma-separated ids in `BENCH_TASKS`.
fn tasks() -> Vec<Task> {
    let only = std::env::var("BENCH_TASKS").ok();
    let wanted = |id: &str| only.as_deref().is_none_or(|ids| ids.split(',').any(|w| w == id));
    let mut dirs: Vec<PathBuf> = fs::read_dir(root().join("bench/tasks"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("task.md").is_file() && wanted(&p.file_name().unwrap().to_string_lossy()))
        .collect();
    dirs.sort();
    dirs.into_iter()
        .map(|dir| {
            let id = dir.file_name().unwrap().to_string_lossy().into_owned();
            let accept = parse_accept(&id, &fs::read_to_string(dir.join("task.md")).unwrap());
            Task { id, dir, accept }
        })
        .collect()
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap().flatten() {
        let name = entry.file_name();
        if [".artscript", "assets"].contains(&name.to_str().unwrap_or_default()) {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            copy_dir(&path, &to.join(&name));
        } else {
            fs::copy(&path, to.join(&name)).unwrap();
        }
    }
}

fn fixture(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("artscript-bench-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    copy_dir(&root().join("examples/chibi"), &dir);
    dir
}

fn run(dir: &Path, program: &str, args: &[&str]) -> Output {
    Command::new(program).args(args).current_dir(dir).output().unwrap()
}

fn artscript(dir: &Path, args: &[&str]) -> Output {
    run(dir, env!("CARGO_BIN_EXE_artscript"), args)
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

/// Appends each assert to its file if the file exists, and returns where each one landed.
fn append_asserts<'a>(dir: &Path, accept: &'a [Accept]) -> Vec<(&'a str, usize, &'a str)> {
    let mut placed = vec![];
    for a in accept {
        if let Accept::Assert { file, expr } = a {
            let path = dir.join(file);
            if let Ok(mut src) = fs::read_to_string(&path) {
                src.push_str(&format!("assert {expr}\n"));
                placed.push((file.as_str(), src.lines().count(), expr.as_str()));
                fs::write(&path, src).unwrap();
            }
        }
    }
    placed
}

fn render(dir: &Path, png: &str) -> (usize, Vec<u8>) {
    let out = artscript(dir, &["build", "--only", png]);
    assert_eq!(out.status.code(), Some(0), "build {png} in {}: {}", dir.display(), text(&out));
    let bytes = fs::read(dir.join("assets").join(png)).unwrap_or_else(|_| panic!("{png} was not written"));
    let image = Image::from_encoded(Data::new_copy(&bytes)).expect("png");
    let (w, h) = (image.width() as usize, image.height() as usize);
    let info = ImageInfo::new((w as i32, h as i32), ColorType::RGBA8888, AlphaType::Unpremul, None);
    let mut buf = vec![0u8; w * h * 4];
    assert!(image.read_pixels(&info, &mut buf, w * 4, (0, 0), skia_safe::image::CachingHint::Disallow));
    (w, buf)
}

/// The share of the region's pixels that moved by more than `levels` in some channel.
fn changed_share(a: &(usize, Vec<u8>), b: &(usize, Vec<u8>), r: Region, levels: u8) -> f64 {
    assert_eq!(a.0, b.0, "renders differ in size");
    let mut moved = 0;
    for y in r.y..r.y + r.h {
        for x in r.x..r.x + r.w {
            let i = (y * a.0 + x) * 4;
            if (0..4).any(|c| a.1[i + c].abs_diff(b.1[i + c]) > levels) {
                moved += 1;
            }
        }
    }
    moved as f64 / (r.w * r.h) as f64
}

const MIN_CHANGED: f64 = 0.01;
const CHANGED_LEVELS: u8 = 2;

/// Problems with one task, empty when it passes.
fn verify(task: &Task, before: &dyn Fn(&str) -> (usize, Vec<u8>)) -> Vec<String> {
    let mut problems = vec![];
    let dir = fixture(&format!("{}-before", task.id));
    let placed = append_asserts(&dir, &task.accept);
    if !placed.is_empty() {
        let out = artscript(&dir, &["check", "--json"]);
        if out.status.code() == Some(2) {
            let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
            let failed = |file: &str, line: usize| {
                json["diagnostics"].as_array().unwrap().iter().any(|d| {
                    d["code"] == "C0101" && d["file"] == file && d["span"]["line"] == line
                })
            };
            for (file, line, expr) in &placed {
                if !failed(file, *line) {
                    problems.push(format!("assert {expr} already holds before the patch"));
                }
            }
        } else if out.status.code() != Some(1) {
            problems.push(format!("the asserts already hold before the patch: {}", text(&out)));
        }
    }
    let _ = fs::remove_dir_all(&dir);

    let dir = fixture(&task.id);
    let patch = task.dir.join("solution.patch");
    let applied = run(&dir, "git", &["apply", "--whitespace=error-all", patch.to_str().unwrap()]);
    if !applied.status.success() {
        return vec![format!("solution.patch does not apply: {}", text(&applied))];
    }
    append_asserts(&dir, &task.accept);
    let check = artscript(&dir, &["check"]);
    if check.status.code() != Some(0) {
        problems.push(format!("artscript check exits {:?}: {}", check.status.code(), text(&check)));
        return problems;
    }
    for a in &task.accept {
        match a {
            Accept::Assert { .. } => {}
            Accept::Changed { png, region } => {
                let share = changed_share(&before(png), &render(&dir, png), *region, CHANGED_LEVELS);
                if share < MIN_CHANGED {
                    problems.push(format!("{png} {region:?}: {:.2}% of pixels changed", share * 100.0));
                }
            }
            Accept::Unchanged { png, region } => {
                let share = changed_share(&before(png), &render(&dir, png), *region, 0);
                if share > 0.0 {
                    problems.push(format!("{png} {region:?}: {:.2}% of pixels changed", share * 100.0));
                }
            }
            Accept::Differs { png, base, region } => {
                let share = changed_share(&render(&dir, base), &render(&dir, png), *region, CHANGED_LEVELS);
                if share < MIN_CHANGED {
                    problems.push(format!("{png} vs {base} {region:?}: {:.2}% of pixels differ", share * 100.0));
                }
            }
            Accept::Lines { file, lo, hi } => {
                let n = fs::read_to_string(dir.join(file)).map(|s| s.lines().count()).unwrap_or(0);
                if n < *lo || n > *hi {
                    problems.push(format!("{file} has {n} lines, not {lo}..{hi}"));
                }
            }
        }
    }
    let _ = fs::remove_dir_all(&dir);
    problems
}

/// `f` over `items` on one thread per core, in order.
fn par_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let workers = std::thread::available_parallelism().map_or(2, |n| n.get());
    let f = &f;
    std::thread::scope(|s| {
        let handles: Vec<_> = items
            .chunks(items.len().div_ceil(workers).max(1))
            .map(|chunk| s.spawn(move || chunk.iter().map(f).collect::<Vec<R>>()))
            .collect();
        handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
    })
}

#[test]
fn every_reference_solution_passes_its_acceptance() {
    let tasks = tasks();
    assert!(!tasks.is_empty(), "no tasks in bench/tasks");

    let pngs: Vec<String> = tasks
        .iter()
        .flat_map(|t| &t.accept)
        .filter_map(|a| match a {
            Accept::Changed { png, .. } | Accept::Unchanged { png, .. } => Some(png.clone()),
            _ => None,
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let originals = par_map(&pngs, |png| {
        let dir = fixture(&format!("original-{}", png.replace('/', "-")));
        let pixels = render(&dir, png);
        let _ = fs::remove_dir_all(&dir);
        pixels
    });
    let before = |png: &str| originals[pngs.iter().position(|p| p == png).unwrap()].clone();

    let failures: Vec<String> = par_map(&tasks, |t| verify(t, &before).into_iter().map(|p| format!("{}: {p}", t.id)).collect::<Vec<_>>())
        .into_iter()
        .flatten()
        .collect();
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
