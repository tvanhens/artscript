//! `fmt` file arguments, `--check`, and unknown flags. Uses a tiny temp project.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn ramose(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ramose"))
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap()
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn project(tag: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ramose-fmt-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("art")).unwrap();
    fs::write(
        dir.join("ramose.toml"),
        "[project]\nname = \"fmt\"\nout = \"assets\"\nlib = [\"lib\"]\nstd = \"0.1\"\n",
    )
    .unwrap();
    for (rel, body) in files {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, body).unwrap();
    }
    dir
}

#[test]
fn unknown_flags_are_usage_errors_and_write_nothing() {
    let dir = project(
        "flags",
        &[("art/a.ramo", "let n = 0.50\n\nlet b = cx-40,884\n")],
    );
    let before = fs::read(dir.join("art/a.ramo")).unwrap();
    for args in [
        vec!["fmt", "--help"],
        vec!["fmt", "--check", "--bogus"],
        vec!["check", "--bogus"],
        vec!["check", "--json", "--extra"],
        vec!["explain", "--help"],
        vec!["build", "--help"],
        vec!["fmt", "-h"],
    ] {
        let out = ramose(&dir, &args);
        assert_eq!(out.status.code(), Some(1), "{args:?}\n{}", text(&out));
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("usage:"), "{args:?}\n{err}");
        assert!(
            err.contains("unknown flag") || err.contains("unknown build flag"),
            "{args:?}\n{err}"
        );
        assert_eq!(
            fs::read(dir.join("art/a.ramo")).unwrap(),
            before,
            "{args:?} wrote the file"
        );
    }

    let empty = std::env::temp_dir().join(format!("ramose-fmt-init-{}", std::process::id()));
    let _ = fs::remove_dir_all(&empty);
    fs::create_dir_all(&empty).unwrap();
    let out = ramose(&empty, &["init", "--help"]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    assert!(String::from_utf8_lossy(&out.stderr).contains("usage:"));
    assert!(
        !empty.join("ramose.toml").exists(),
        "init --help created a project"
    );
}

#[test]
fn fmt_check_lists_files_and_file_args_limit_the_write() {
    let dir = project(
        "check",
        &[
            ("art/a.ramo", "let n = 0.50\n"),
            ("art/b.ramo", "let n = 1\n"),
        ],
    );
    let b_before = fs::read(dir.join("art/b.ramo")).unwrap();

    let check = ramose(&dir, &["fmt", "--check"]);
    assert_eq!(check.status.code(), Some(1), "{}", text(&check));
    assert_eq!(String::from_utf8_lossy(&check.stdout), "art/a.ramo\n");
    assert_eq!(
        fs::read_to_string(dir.join("art/a.ramo")).unwrap(),
        "let n = 0.50\n"
    );
    assert_eq!(fs::read(dir.join("art/b.ramo")).unwrap(), b_before);

    let one = ramose(&dir, &["fmt", "--check", "art/b.ramo"]);
    assert_eq!(one.status.code(), Some(0), "{}", text(&one));
    assert!(one.stdout.is_empty(), "{}", text(&one));

    let write = ramose(&dir, &["fmt", "art/a.ramo"]);
    assert_eq!(write.status.code(), Some(0), "{}", text(&write));
    assert_eq!(
        fs::read_to_string(dir.join("art/a.ramo")).unwrap(),
        "let n = .5\n"
    );
    assert_eq!(fs::read(dir.join("art/b.ramo")).unwrap(), b_before);

    let again = ramose(&dir, &["fmt", "--check"]);
    assert_eq!(again.status.code(), Some(0), "{}", text(&again));
    assert!(again.stdout.is_empty(), "{}", text(&again));
}
