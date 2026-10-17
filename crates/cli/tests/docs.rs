//! `docs` prints the language card from any directory, so a skill can read the card that matches the installed binary.

use std::process::Command;

#[test]
fn docs_prints_the_language_card_outside_a_project() {
    let out = Command::new(env!("CARGO_BIN_EXE_artscript"))
        .arg("docs")
        .current_dir(std::env::temp_dir())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        include_str!("../../../docs/language.md")
    );
}

#[test]
fn docs_rejects_arguments() {
    let out = Command::new(env!("CARGO_BIN_EXE_artscript"))
        .args(["docs", "--all"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
}
