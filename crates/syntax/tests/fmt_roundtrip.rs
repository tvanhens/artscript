//! `fmt` must reprint a tree that parses back to the same tree, and do it once.

use std::fs;
use std::path::{Path, PathBuf};

use ramose_syntax::{format_source, parse};

fn art_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            art_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "ramo") {
            out.push(path);
        }
    }
}

/// Debug with `Span { ... }` removed, so line shifts from blank lines do not count.
fn spine(src: &str) -> Result<String, String> {
    let file = parse(src).map_err(|errs| {
        errs.iter()
            .map(|d| d.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    })?;
    let raw = format!("{file:?}");
    let mut out = String::new();
    let mut rest = raw.as_str();
    while let Some(i) = rest.find("Span {") {
        out.push_str(&rest[..i]);
        let after = &rest[i..];
        let end = after
            .find('}')
            .ok_or_else(|| format!("unclosed span in {raw}"))?
            + 1;
        rest = &after[end..];
    }
    out.push_str(rest);
    Ok(out)
}

fn assert_stable(src: &str) {
    let once = format_source(src).unwrap_or_else(|errs| panic!("fmt failed\n{src}\n{errs:?}"));
    let twice = format_source(&once).unwrap_or_else(|errs| panic!("second pass\n{once}\n{errs:?}"));
    assert_eq!(once, twice, "fmt is not idempotent");
    let before = spine(src).unwrap_or_else(|e| panic!("parse src\n{src}\n{e}"));
    let after = spine(&once).unwrap_or_else(|e| panic!("parse fmt\n{once}\n{e}"));
    assert_eq!(before, after, "fmt changed the tree\n{src}=>\n{once}");
}

#[test]
fn fmt_tight_tuple_is_canonical() {
    // Glyph `+` and `-` are tight, same as `*` and `/`. Parentheses stay only
    // where the tree would change without them.
    let cases = [
        ("curve cx-40,884\n", "curve cx-40,884\n"),
        ("curve cx - 40,884\n", "curve cx-40,884\n"),
        ("curve (cx - 40),884\n", "curve cx-40,884\n"),
        (
            "ellipse 22.4,34.4 at:look.x*8.8,4+look.y*5.6\n",
            "ellipse 22.4,34.4 at:look.x*8.8,4+look.y*5.6\n",
        ),
        (
            "ellipse at:look.x*8.8,4 + look.y*5.6\n",
            "ellipse at:look.x*8.8,4+look.y*5.6\n",
        ),
        (
            "curve x-w*.05,h*.18 x+w*.05,-h*.18\n",
            "curve x-w*.05,h*.18 x+w*.05,-h*.18\n",
        ),
        (
            "let x = (k - (lines - 1)/2)*w*.22\n",
            "let x = (k-(lines-1)/2)*w*.22\n",
        ),
        ("let v = (a + b)*c\n", "let v = (a+b)*c\n"),
        ("let v = a - (b - c)\n", "let v = a-(b-c)\n"),
        ("let v = a + (b,c)\n", "let v = a+(b,c)\n"),
        ("let v = (a,b) + c\n", "let v = (a,b)+c\n"),
        ("draw a -b\n", "draw a -b\n"),
        ("let v = shade(c -2)\n", "let v = shade(c -2)\n"),
        ("let v = shade(c - 2)\n", "let v = shade(c-2)\n"),
        (
            "let v = if k < 2 then (3,4) else (5,6)\n",
            "let v = if k < 2 then 3,4 else 5,6\n",
        ),
        (
            "let v = (if k < 2 then a else b),6\n",
            "let v = (if k < 2 then a else b),6\n",
        ),
        (
            "let v = a,(if k < 2 then b else c)\n",
            "let v = a,if k < 2 then b else c\n",
        ),
        ("let v = (a,b),c\n", "let v = (a,b),c\n"),
        (
            "let v = if (if a then b else c) then d else e\n",
            "let v = if (if a then b else c) then d else e\n",
        ),
        ("rect 40x20x10\n", "rect 40x20x10\n"),
        ("rect 40 x 20\n", "rect 40 x 20\n"),
        ("box 100%x50%x8 at:14,0,8\n", "box 100%x50%x8 at:14,0,8\n"),
    ];
    for (src, expected) in cases {
        let once = format_source(src).unwrap_or_else(|errs| panic!("{src}\n{errs:?}"));
        assert_eq!(once, expected, "src {src:?}");
        assert_stable(src);
    }
}

#[test]
fn fmt_keeps_single_blank_lines() {
    let cases = [
        ("let a = 1\n\n\nlet b = 2\n", "let a = 1\n\nlet b = 2\n"),
        ("let a = 1\nlet b = 2\n", "let a = 1\nlet b = 2\n"),
        (
            "let a = 1\n\n# note\nlet b = 2\n",
            "let a = 1\n\n# note\nlet b = 2\n",
        ),
        ("let a = 1\n   \nlet b = 2\n", "let a = 1\n\nlet b = 2\n"),
    ];
    for (src, expected) in cases {
        let once = format_source(src).unwrap_or_else(|errs| panic!("{src:?}\n{errs:?}"));
        assert_eq!(once, expected);
        assert_stable(src);
    }
}

#[test]
fn fmt_round_trip_preserves_ast() {
    let mut files = Vec::new();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    art_files(&root.join("../../examples"), &mut files);
    assert!(!files.is_empty(), "no examples under {}", root.display());

    let mut inline = vec![
        "curve cx-40,884\n".to_string(),
        "curve (cx - 40),884\n".to_string(),
        "ellipse 22.4,34.4 at:look.x*8.8,4+look.y*5.6\n".to_string(),
        "curve x-w*.05,h*.18 x+w*.05,-h*.18\n".to_string(),
        "let x = (k - (lines - 1)/2)*w*.22\n".to_string(),
        "let a = 1\n\n\nlet b = 2\n".to_string(),
        "rect 40x24 warp:[cylinder(40 axis:x) corners(84,22 140,22 150,54 74,54)] name:side\n".to_string(),
    ];
    for f in &files {
        inline.push(fs::read_to_string(f).unwrap_or_else(|e| panic!("{}: {e}", f.display())));
    }
    let mut failures = Vec::new();
    for src in &inline {
        let once = match format_source(src) {
            Ok(s) => s,
            Err(errs) => {
                failures.push(format!(
                    "fmt error: {}",
                    errs.iter()
                        .map(|d| d.to_string())
                        .collect::<Vec<_>>()
                        .join("\n")
                ));
                continue;
            }
        };
        let twice = format_source(&once).unwrap_or_else(|errs| panic!("{errs:?}\n{once}"));
        if once != twice {
            failures.push(format!("not idempotent:\n{once}"));
            continue;
        }
        match (spine(src), spine(&once)) {
            (Ok(a), Ok(b)) if a == b => {}
            (Ok(_), Ok(_)) => failures.push(format!("tree changed:\n{src}=>\n{once}")),
            (Err(e), _) => failures.push(format!("parse: {e}\n{src}")),
            (_, Err(e)) => failures.push(format!("reparse: {e}\n{once}")),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn entry_parameters_round_trip() {
    assert_stable("@artboard 20x20\n@target t.png\nparam view:one_of(normal geometry) guides:false\nparam width:8 height:width\nrect width,height\n");
}
