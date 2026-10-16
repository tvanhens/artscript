use std::fs;
use std::path::{Path, PathBuf};

fn art_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            art_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "art") {
            out.push(path);
        }
    }
}

#[test]
fn every_example_parses_and_formats_idempotently() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut files = Vec::new();
    art_files(&root, &mut files);
    assert!(!files.is_empty(), "no examples found under {}", root.display());
    let mut failures = Vec::new();
    for f in &files {
        let src = fs::read_to_string(f).unwrap();
        match artscript_syntax::format_source(&src) {
            Err(diags) => failures.push(format!(
                "{}:\n{}",
                f.display(),
                diags.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n")
            )),
            Ok(once) => {
                let twice = artscript_syntax::format_source(&once).unwrap();
                if once != twice {
                    failures.push(format!("{}: fmt not idempotent", f.display()));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
