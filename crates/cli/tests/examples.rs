use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn example_projects() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut dirs: Vec<PathBuf> = fs::read_dir(&root)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|d| d.join("artscript.toml").is_file())
        .collect();
    dirs.sort();
    assert!(!dirs.is_empty(), "no projects under {}", root.display());
    dirs
}

fn artscript(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_artscript")).args(args).current_dir(dir).output().unwrap()
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap().flatten() {
        let (path, dest) = (entry.path(), to.join(entry.file_name()));
        if path.is_dir() {
            copy_dir(&path, &dest);
        } else {
            fs::copy(&path, &dest).unwrap();
        }
    }
}

fn output_paths(manifest: &Path) -> Option<Vec<String>> {
    let text = match fs::read_to_string(manifest) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return None,
        Err(err) => panic!("{}: {err}", manifest.display()),
    };
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    let mut paths: Vec<String> = json["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["path"].as_str().unwrap().to_string())
        .collect();
    paths.sort();
    Some(paths)
}

#[test]
fn every_example_is_formatted() {
    for dir in example_projects() {
        let out = artscript(&dir, &["fmt", "--check"]);
        assert_eq!(out.status.code(), Some(0), "run `artscript fmt` in {}\n{}", dir.display(), text(&out));
    }
}

#[test]
fn every_example_passes_check() {
    for dir in example_projects() {
        let out = artscript(&dir, &["check"]);
        assert_eq!(out.status.code(), Some(0), "{}\n{}", dir.display(), text(&out));
    }
}

#[test]
fn every_example_builds_cold_and_makes_the_outputs_its_manifest_lists() {
    for dir in example_projects() {
        let out_dir = artscript_eval::load(&dir).unwrap_or_else(|d| panic!("{}: {d}", dir.display())).manifest.out;
        let name = dir.file_name().unwrap().to_string_lossy();
        let copy = std::env::temp_dir().join(format!("artscript-example-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&copy);
        copy_dir(&dir, &copy);
        for stale in [".artscript", out_dir.as_str()] {
            let _ = fs::remove_dir_all(copy.join(stale));
        }

        let built = artscript(&copy, &["build"]);
        assert_eq!(built.status.code(), Some(0), "{}\n{}", dir.display(), text(&built));
        if let Some(committed) = output_paths(&dir.join(&out_dir).join("art-manifest.json")) {
            assert_eq!(
                output_paths(&copy.join(&out_dir).join("art-manifest.json")),
                Some(committed),
                "{}: the committed {out_dir}/ lists other outputs than a rebuild makes",
                dir.display()
            );
        }
        let _ = fs::remove_dir_all(&copy);
    }
}
