//! `docs` prints the core card and a topic index from any directory, and `docs <topic>...` prints the card and topic files, so a skill reads the docs that match the installed binary.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn docs_dir() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs"))
}

fn artscript(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_artscript"))
        .args(args)
        .current_dir(std::env::temp_dir())
        .output()
        .unwrap()
}

fn topic(name: &str) -> String {
    fs::read_to_string(docs_dir().join("topics").join(format!("{name}.md"))).unwrap()
}

#[test]
fn docs_prints_the_card_then_an_index_of_every_topic_file() {
    let out = artscript(&["docs"]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8(out.stdout).unwrap();
    let card = fs::read_to_string(docs_dir().join("language.md")).unwrap();
    let index = stdout
        .strip_prefix(card.as_str())
        .expect("output starts with docs/language.md");
    assert!(index.trim_start().starts_with("## Topics\n"), "{index}");
    assert!(index.contains("artscript docs <topic> [topic...]"), "{index}");

    let mut names: Vec<String> = fs::read_dir(docs_dir().join("topics"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .map(|p| p.file_stem().unwrap().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert!(!names.is_empty());
    for name in &names {
        let body = topic(name);
        let summary = body
            .lines()
            .next()
            .and_then(|l| l.strip_prefix(&format!("# {name}: ")))
            .unwrap_or_else(|| panic!("{name}.md starts with `# {name}: <summary>`"));
        assert!(
            index.lines().any(|l| l.split_once(' ').is_some_and(
                |(n, rest)| n == name && rest.trim_start() == summary
            )),
            "index lists `{name}  {summary}`:\n{index}"
        );
    }
}

#[test]
fn docs_prints_the_card_then_named_topics_in_order() {
    let out = artscript(&["docs", "color", "anim"]);
    assert_eq!(out.status.code(), Some(0));
    let card = fs::read_to_string(docs_dir().join("language.md")).unwrap();
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        format!("{card}\n{}\n{}", topic("color"), topic("anim"))
    );
}

#[test]
fn the_skill_lists_every_topic_with_its_summary() {
    let skill = fs::read_to_string(docs_dir().join("../skills/artscript/SKILL.md")).unwrap();
    for entry in fs::read_dir(docs_dir().join("topics")).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let first = fs::read_to_string(&path).unwrap().lines().next().unwrap().to_owned();
        let summary = first.strip_prefix(&format!("# {name}: ")).unwrap();
        let line = format!("- `{name}` {summary}");
        assert!(skill.lines().any(|l| l.trim_start() == line), "SKILL.md lists `{line}`");
    }
}

#[test]
fn docs_rejects_an_unknown_topic() {
    let out = artscript(&["docs", "color", "nope"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("unknown docs topic `nope`"), "{stderr}");
    assert!(stderr.contains("color"), "{stderr}");
}

#[test]
fn docs_rejects_flags() {
    let out = artscript(&["docs", "--all"]);
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn std_lists_the_bundled_libraries_and_prints_one_to_fork() {
    let list = artscript(&["std"]);
    assert_eq!(list.status.code(), Some(0));
    assert_eq!(String::from_utf8(list.stdout).unwrap(), "std/brushes\n");
    for name in ["brushes", "std/brushes"] {
        let out = artscript(&["std", name]);
        assert_eq!(out.status.code(), Some(0), "{name}");
        let text = String::from_utf8(out.stdout).unwrap();
        assert!(text.starts_with("# std/brushes:"), "{name}: {text}");
        assert!(text.contains("\nbrush dry ") && text.contains("\nbrush chip "), "{name}");
    }
    let missing = artscript(&["std", "nope"]);
    assert_eq!(missing.status.code(), Some(1));
    assert!(String::from_utf8(missing.stderr).unwrap().contains("bundled: std/brushes"));
}
