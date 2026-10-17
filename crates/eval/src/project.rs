//! The project: `artscript.toml` and every `.art` source under the root. This is the only module
//! in the evaluator that touches the filesystem; everything downstream works on [`Project`].

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use artscript_syntax::Diagnostic;
use artscript_syntax::ast::Span;

pub const MANIFEST: &str = "artscript.toml";

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

#[derive(serde::Deserialize)]
struct SpannedDoc {
    build: Option<BuildSection>,
    defaults: Option<DefaultsSection>,
}

#[derive(serde::Deserialize)]
struct BuildSection {
    backend: Option<toml::Spanned<toml::Value>>,
}

#[derive(serde::Deserialize)]
struct DefaultsSection {
    profile: Option<toml::Spanned<toml::Value>>,
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
    pub scale: Vec<f32>,
    pub bg: Option<String>,
    /// `vector | svg | raster | auto`.
    pub profile: String,
    pub backend: BackendChoice,
    pub lint: BTreeMap<String, LintLevel>,
}

impl Manifest {
    pub fn parse(src: &str) -> Result<Manifest, Diagnostic> {
        let bad_at = |span: Span, msg: String| {
            Diagnostic::error("E0200", span, msg)
                .in_file(MANIFEST)
                .with_fix("see `artscript init` for a valid manifest")
        };
        let bad = |msg: String| bad_at(Span { line: 1, col: 1 }, msg);
        let doc: toml::Table = src.parse().map_err(|e: toml::de::Error| bad(format!("invalid manifest: {e}")))?;
        let section = |name: &str| doc.get(name).and_then(|v| v.as_table()).cloned().unwrap_or_default();
        let project = section("project");
        let defaults = section("defaults");
        let s = |t: &toml::Table, k: &str| t.get(k).and_then(|v| v.as_str()).map(str::to_string);
        let lib = match project.get("lib") {
            Some(toml::Value::Array(xs)) => xs.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
            Some(toml::Value::String(one)) => vec![one.clone()],
            _ => vec!["lib".into()],
        };
        let scale = match defaults.get("scale") {
            Some(toml::Value::Array(xs)) => xs
                .iter()
                .filter_map(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)))
                .map(|v| v as f32)
                .collect(),
            Some(v) => v.as_float().or_else(|| v.as_integer().map(|i| i as f64)).map(|v| vec![v as f32]).unwrap_or_default(),
            None => vec![1.0],
        };
        let mut lint = BTreeMap::new();
        for (k, v) in section("lint") {
            let level = match v.as_str() {
                Some("off") => LintLevel::Off,
                Some("info") => LintLevel::Info,
                Some("warn") => LintLevel::Warn,
                Some("error") => LintLevel::Error,
                other => return Err(bad(format!("lint `{k}` has level {other:?}; use off, info, warn or error"))),
            };
            lint.insert(k, level);
        }
        let spanned = toml::from_str::<SpannedDoc>(src).ok();
        let backend = match spanned.as_ref().and_then(|d| d.build.as_ref()?.backend.as_ref()) {
            None => BackendChoice::default(),
            Some(v) => v
                .get_ref()
                .as_str()
                .ok_or_else(|| "backend must be a string".to_string())
                .and_then(str::parse)
                .map_err(|e| bad_at(span_at(src, v.span().start), format!("[build] {e}")))?,
        };
        let profile = match spanned.as_ref().and_then(|d| d.defaults.as_ref()?.profile.as_ref()) {
            None => "auto".to_string(),
            Some(v) => match v.get_ref().as_str() {
                Some(p @ ("vector" | "svg" | "raster" | "auto")) => p.to_string(),
                _ => {
                    return Err(bad_at(
                        span_at(src, v.span().start),
                        format!("[defaults] profile {} is not one of vector, svg, raster or auto", v.get_ref()),
                    ));
                }
            },
        };
        Ok(Manifest {
            name: s(&project, "name").unwrap_or_default(),
            out: s(&project, "out").unwrap_or_else(|| "assets".into()),
            lib,
            palette: s(&defaults, "palette"),
            scale: if scale.is_empty() { vec![1.0] } else { scale },
            bg: s(&defaults, "bg").filter(|b| b != "none"),
            profile,
            backend,
            lint,
        })
    }

    pub fn lint_level(&self, lint: &str, default: LintLevel) -> LintLevel {
        self.lint.get(lint).copied().unwrap_or(default)
    }

    /// The import path (`face/parts`) of a project-relative file under a lib root.
    pub fn lib_path_of(&self, file: &str) -> Option<String> {
        self.lib.iter().find_map(|root| {
            let root = root.trim_end_matches('/');
            file.strip_prefix(&format!("{root}/"))
                .and_then(|rest| rest.strip_suffix(".art"))
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
        Ok(Project {
            root: PathBuf::new(),
            manifest: Manifest::parse(manifest_src)?,
            manifest_src: manifest_src.to_string(),
            files,
        })
    }

    /// Resolves `use face/parts` to a project-relative file under one of the lib roots.
    pub fn resolve_use(&self, path: &str) -> Option<String> {
        self.manifest.lib.iter().find_map(|root| {
            let file = format!("{}/{path}.art", root.trim_end_matches('/'));
            self.files.contains_key(&file).then_some(file)
        })
    }

    pub fn is_lib_file(&self, file: &str) -> bool {
        self.manifest.lib_path_of(file).is_some()
    }

    pub fn library_names(&self) -> Vec<String> {
        self.files.keys().filter_map(|f| self.manifest.lib_path_of(f)).collect()
    }
}

pub fn find_root(start: &Path) -> Option<PathBuf> {
    start.ancestors().find(|d| d.join(MANIFEST).is_file()).map(Path::to_path_buf)
}

/// Reads the manifest and every `.art` file under `root`, skipping hidden directories, the output
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
            } else if name.ends_with(".art")
                && let Ok(src) = fs::read_to_string(&path)
            {
                files.insert(rel, src);
            }
        }
    }
    Ok(Project { root: root.to_path_buf(), manifest, manifest_src, files })
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
