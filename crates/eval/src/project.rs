//! The project: `ramose.toml` and every `.ramo` source under the root. This is the only module
//! in the evaluator that touches the filesystem; everything downstream works on [`Project`].

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use ramose_syntax::Diagnostic;
use ramose_syntax::ast::Span;

pub const MANIFEST: &str = "ramose.toml";

/// The bundled standard library, by `use` path. `use std/brushes` reads this table and never a
/// file, so a project cannot shadow it and `fmt` and the lints never see it.
pub const STD: &[(&str, &str)] = &[
    ("std/brushes", include_str!("../std/brushes.ramo")),
    ("std/stamps", include_str!("../std/stamps.ramo")),
];

/// The project-relative name a bundled library is known by in diagnostics and dependencies.
pub fn std_file(path: &str) -> String {
    format!("{path}.ramo")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LintLevel {
    Off,
    Info,
    Warn,
    Error,
}

/// The backend a build asks for. `Auto` resolves to the GPU when one is available, else the CPU.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BackendChoice {
    #[default]
    Auto,
    Gpu,
    Cpu,
}

impl std::str::FromStr for BackendChoice {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "auto" => Ok(Self::Auto),
            "gpu" => Ok(Self::Gpu),
            "cpu" => Ok(Self::Cpu),
            other => Err(format!("backend `{other}` is not one of auto, gpu or cpu")),
        }
    }
}

/// The lints a `[lint]` table may set, with the level each has when the table leaves it out.
pub const LINTS: &[(&str, LintLevel)] = &[
    ("raw-color", LintLevel::Warn),
    ("off-artboard", LintLevel::Warn),
    ("unused", LintLevel::Warn),
    ("positional-seed", LintLevel::Warn),
    ("hidden-child", LintLevel::Warn),
    ("unknown-target", LintLevel::Warn),
    ("unknown-word", LintLevel::Warn),
    ("warp-cut", LintLevel::Warn),
];

const PROJECT_KEYS: &[&str] = &["name", "out", "lib", "std"];
const DEFAULTS_KEYS: &[&str] = &["palette", "scale", "bg", "profile"];

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestDoc {
    #[serde(default)]
    project: BTreeMap<String, toml::Spanned<toml::Value>>,
    #[serde(default)]
    defaults: BTreeMap<String, toml::Spanned<toml::Value>>,
    #[serde(default)]
    lint: BTreeMap<String, toml::Spanned<toml::Value>>,
    #[serde(default)]
    build: BuildSection,
}

#[derive(Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BuildSection {
    backend: Option<toml::Spanned<toml::Value>>,
}

fn span_at(src: &str, offset: usize) -> Span {
    let before = &src[..offset.min(src.len())];
    let line = before.matches('\n').count() as u32 + 1;
    let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) as u32 + 1;
    Span { line, col }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Manifest {
    pub name: String,
    pub out: String,
    pub lib: Vec<String>,
    pub palette: Option<String>,
    /// Where `[defaults] palette` is written, for the error when no lib root holds it.
    pub palette_span: Span,
    pub scale: Vec<f32>,
    pub bg: Option<String>,
    /// `vector | svg | raster | auto`.
    pub profile: String,
    pub backend: BackendChoice,
    pub lint: BTreeMap<String, LintLevel>,
}

impl Manifest {
    pub fn parse(src: &str) -> Result<Manifest, Diagnostic> {
        let bad_at = |offset: usize, msg: String| {
            Diagnostic::error("E0200", span_at(src, offset), msg)
                .in_file(MANIFEST)
                .with_fix("see `ramose init` for a valid manifest")
        };
        let ManifestDoc { project, defaults, lint: levels, build } = toml::from_str(src).map_err(|e: toml::de::Error| {
            bad_at(e.span().map_or(0, |r| r.start), format!("invalid manifest: {}", e.message().trim().replace('\n', "; ")))
        })?;
        for (table, keys, known) in [("project", &project, PROJECT_KEYS), ("defaults", &defaults, DEFAULTS_KEYS)] {
            if let Some((k, v)) = keys.iter().find(|(k, _)| !known.contains(&k.as_str())) {
                let msg = format!("[{table}] has no key `{k}`; use {}", known.join(", "));
                return Err(bad_at(v.span().start, msg));
            }
        }
        let string = |table: &str, map: &BTreeMap<String, toml::Spanned<toml::Value>>, k: &str| match map.get(k) {
            None => Ok(None),
            Some(v) => match v.get_ref().as_str() {
                Some(s) => Ok(Some(s.to_string())),
                None => Err(bad_at(v.span().start, format!("[{table}] {k} must be a string"))),
            },
        };
        let lib = match project.get("lib") {
            None => vec!["lib".into()],
            Some(v) => match v.get_ref() {
                toml::Value::String(one) => Some(vec![one.clone()]),
                toml::Value::Array(xs) => xs.iter().map(|x| x.as_str().map(str::to_string)).collect(),
                _ => None,
            }
            .ok_or_else(|| bad_at(v.span().start, "[project] lib must be a path or a list of paths".into()))?,
        };
        let number = |v: &toml::Value| {
            v.as_float().or_else(|| v.as_integer().map(|i| i as f64)).filter(|v| *v > 0.0).map(|v| v as f32)
        };
        let scale = match defaults.get("scale") {
            None => vec![1.0],
            Some(v) => match v.get_ref() {
                toml::Value::Array(xs) => xs.iter().map(number).collect::<Option<Vec<_>>>(),
                one => number(one).map(|s| vec![s]),
            }
            .filter(|xs| !xs.is_empty())
            .ok_or_else(|| bad_at(v.span().start, "[defaults] scale must be a positive number or a list of them".into()))?,
        };
        let bg = match defaults.get("bg") {
            None => None,
            Some(v) => match v.get_ref().as_str() {
                Some("none") => None,
                Some(hex) if crate::color::Rgba::hex(hex).is_some() => Some(hex.to_string()),
                _ => {
                    let msg = format!("[defaults] bg {} is not \"none\" or a hex color", v.get_ref());
                    return Err(bad_at(v.span().start, msg));
                }
            },
        };
        let mut lint = BTreeMap::new();
        for (k, v) in levels {
            if !LINTS.iter().any(|(name, _)| *name == k) {
                if k == "off-canvas" {
                    return Err(bad_at(v.span().start, "[lint] `off-canvas` is now `off-artboard`".to_string()));
                }
                let names: Vec<_> = LINTS.iter().map(|(name, _)| *name).collect();
                return Err(bad_at(v.span().start, format!("[lint] has no lint `{k}`; use {}", names.join(", "))));
            }
            let level = match v.get_ref().as_str() {
                Some("off") => LintLevel::Off,
                Some("info") => LintLevel::Info,
                Some("warn") => LintLevel::Warn,
                Some("error") => LintLevel::Error,
                _ => {
                    let msg = format!("lint `{k}` has level {}; use off, info, warn or error", v.get_ref());
                    return Err(bad_at(v.span().start, msg));
                }
            };
            lint.insert(k, level);
        }
        let backend = match build.backend {
            None => BackendChoice::default(),
            Some(v) => v
                .get_ref()
                .as_str()
                .ok_or_else(|| "backend must be a string".to_string())
                .and_then(str::parse)
                .map_err(|e| bad_at(v.span().start, format!("[build] {e}")))?,
        };
        let profile = match defaults.get("profile") {
            None => "auto".to_string(),
            Some(v) => match v.get_ref().as_str() {
                Some(p @ ("vector" | "svg" | "raster" | "auto")) => p.to_string(),
                _ => {
                    let msg = format!("[defaults] profile {} is not one of vector, svg, raster or auto", v.get_ref());
                    return Err(bad_at(v.span().start, msg));
                }
            },
        };
        string("project", &project, "std")?;
        Ok(Manifest {
            name: string("project", &project, "name")?.unwrap_or_default(),
            out: string("project", &project, "out")?.unwrap_or_else(|| "assets".into()),
            lib,
            palette: string("defaults", &defaults, "palette")?,
            palette_span: defaults.get("palette").map_or(Span::default(), |v| span_at(src, v.span().start)),
            scale,
            bg,
            profile,
            backend,
            lint,
        })
    }

    pub fn lint_level(&self, lint: &str) -> LintLevel {
        let default = LINTS.iter().find(|(name, _)| *name == lint).map(|(_, level)| *level);
        self.lint.get(lint).copied().or(default).expect("every lint is listed in LINTS")
    }

    /// The import path (`face/parts`) of a project-relative file under a lib root.
    pub fn lib_path_of(&self, file: &str) -> Option<String> {
        self.lib.iter().find_map(|root| {
            let root = root.trim_end_matches('/');
            file.strip_prefix(&format!("{root}/"))
                .and_then(|rest| rest.strip_suffix(".ramo"))
                .map(str::to_string)
        })
    }
}

/// Everything the evaluator reads, keyed by project-relative path with `/` separators.
#[derive(Clone, Debug)]
pub struct Project {
    pub root: PathBuf,
    pub manifest: Manifest,
    pub manifest_src: String,
    pub files: BTreeMap<String, String>,
}

impl Project {
    pub fn from_sources(manifest_src: &str, files: BTreeMap<String, String>) -> Result<Project, Diagnostic> {
        Project {
            root: PathBuf::new(),
            manifest: Manifest::parse(manifest_src)?,
            manifest_src: manifest_src.to_string(),
            files,
        }
        .without_std_shadow()?
        .with_palette()
    }

    /// A file that `use std/...` could reach, through a lib root or the project-relative
    /// fallback, would shadow the bundled library, so it is refused.
    fn without_std_shadow(self) -> Result<Project, Diagnostic> {
        let shadow = self.files.keys().find(|f| {
            f.starts_with("std/") || self.manifest.lib_path_of(f).is_some_and(|p| p.starts_with("std/"))
        });
        match shadow {
            Some(f) => Err(Diagnostic::error(
                "E0213",
                Span::default(),
                format!("`{f}` would shadow the bundled `std/` library"),
            )
            .in_file(f)
            .with_fix("move it out of a folder named `std`; `ramose std <name>` prints a bundled file to fork")),
            None => Ok(self),
        }
    }

    fn with_palette(self) -> Result<Project, Diagnostic> {
        match &self.manifest.palette {
            Some(pal) if self.resolve_use(pal).is_none() => {
                let msg = format!("[defaults] palette `{pal}` is in no lib root ({})", self.manifest.lib.join(", "));
                Err(Diagnostic::error("E0200", self.manifest.palette_span, msg)
                    .in_file(MANIFEST)
                    .with_fix(format!("add `{pal}.ramo` under a lib root, or remove the palette key")))
            }
            _ => Ok(self),
        }
    }

    /// Resolves `use face/parts` to a project-relative file under one of the lib roots. A `std/`
    /// path resolves only against [`STD`].
    pub fn resolve_use(&self, path: &str) -> Option<String> {
        if path.starts_with("std/") {
            return STD.iter().any(|(p, _)| *p == path).then(|| std_file(path));
        }
        self.manifest.lib.iter().find_map(|root| {
            let file = format!("{}/{path}.ramo", root.trim_end_matches('/'));
            self.files.contains_key(&file).then_some(file)
        })
    }

    /// The text of a project file or a bundled library. The one reader of `.ramo` source for
    /// evaluation, dependencies and cache keys. `fmt` and `plan` keep iterating `files`.
    pub fn source(&self, file: &str) -> Option<&str> {
        if let Some(src) = self.files.get(file) {
            return Some(src);
        }
        STD.iter().find(|(p, _)| std_file(p) == file).map(|(_, src)| *src)
    }

    pub fn is_bundled(&self, file: &str) -> bool {
        STD.iter().any(|(p, _)| std_file(p) == file)
    }

    pub fn is_lib_file(&self, file: &str) -> bool {
        self.manifest.lib_path_of(file).is_some()
    }

    pub fn library_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.files.keys().filter_map(|f| self.manifest.lib_path_of(f)).collect();
        names.extend(STD.iter().map(|(p, _)| p.to_string()));
        names
    }
}

pub fn find_root(start: &Path) -> Option<PathBuf> {
    start.ancestors().find(|d| d.join(MANIFEST).is_file()).map(Path::to_path_buf)
}

/// Reads the manifest and every `.ramo` file under `root`, skipping hidden directories, the output
/// directory and simple `.gitignore` entries.
pub fn load(root: &Path) -> Result<Project, Diagnostic> {
    let manifest_src = fs::read_to_string(root.join(MANIFEST)).map_err(|e| {
        Diagnostic::error("E0200", Span::default(), format!("cannot read {MANIFEST}: {e}")).in_file(MANIFEST)
    })?;
    let manifest = Manifest::parse(&manifest_src)?;
    let ignored = gitignore(root);
    let out_dir = manifest.out.trim_end_matches('/').to_string();
    let mut files = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let rel = rel_path(root, &path);
            if name.starts_with('.') || ignored.iter().any(|g| g == &name || g == &rel) {
                continue;
            }
            if path.is_dir() {
                if rel != out_dir && name != "target" {
                    stack.push(path);
                }
            } else if name.ends_with(".ramo")
                && let Ok(src) = fs::read_to_string(&path)
            {
                files.insert(rel, src);
            }
        }
    }
    Project { root: root.to_path_buf(), manifest, manifest_src, files }.without_std_shadow()?.with_palette()
}

pub fn rel_path(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    rel.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect::<Vec<_>>().join("/")
}

fn gitignore(root: &Path) -> Vec<String> {
    fs::read_to_string(root.join(".gitignore"))
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().trim_start_matches('/').trim_end_matches('/').to_string())
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.contains('*'))
        .collect()
}
