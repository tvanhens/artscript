use std::fs;
use std::ops::Range;
use std::path::PathBuf;

use crate::ast::{Expr, File, Named, Span, Stmt, StmtKind};
use crate::lex::{self, Kw, Tok};

use super::{Class, PATH_CHARS, Run, classify, is_path_char, ribbon};

fn range_of(src: &str, needle: &str) -> Range<usize> {
    let start = src
        .find(needle)
        .unwrap_or_else(|| panic!("missing {needle:?} in {src:?}"));
    assert_eq!(
        src.rfind(needle),
        Some(start),
        "{needle:?} is not unique in {src:?}"
    );
    start..start + needle.len()
}

fn assert_painted(src: &str, expected: &[(&str, Class)]) {
    let runs = classify(src);
    for (needle, class) in expected {
        let range = range_of(src, needle);
        let found = runs.iter().find(|run| run.range == range);
        assert_eq!(
            found.map(|run| run.class),
            Some(*class),
            "{needle:?} in {src:?}\n{}",
            dump(src, &runs)
        );
    }
    span_set_equals_lex(src);
}

fn dump(src: &str, runs: &[Run]) -> String {
    let painted = ribbon(src, runs);
    let mut out = String::new();
    for (line, paint) in src.split_inclusive('\n').zip(painted.split_inclusive('\n')) {
        out.push_str(line);
        if !line.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(paint);
        if !paint.ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

#[test]
fn letters_are_unique() {
    let mut seen = String::new();
    for class in Class::ALL {
        let letter = class.letter();
        assert!(!seen.contains(letter), "{letter} repeats");
        seen.push(letter);
    }
}

#[test]
fn path_chars_match_the_classifier() {
    for c in '\0'..='\u{7f}' {
        assert_eq!(
            is_path_char(c),
            class_contains(PATH_CHARS, c),
            "{c:?} disagrees with PATH_CHARS"
        );
    }
}

fn class_contains(class: &str, c: char) -> bool {
    let chars: Vec<char> = class.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if i + 2 < chars.len() && chars[i + 1] == '-' {
            if c >= chars[i] && c <= chars[i + 2] {
                return true;
            }
            i += 3;
        } else {
            if chars[i] == c {
                return true;
            }
            i += 1;
        }
    }
    false
}

#[test]
fn byte_range_fixtures() {
    assert_painted("#fff\n", &[("#fff", Class::Color)]);
    assert_painted("#fff,\n", &[("#fff", Class::Color), (",", Class::Operator)]);
    assert_painted("#ab\n", &[("#ab", Class::Comment)]);
    assert_painted("#fff#abc\n", &[("#fff#abc", Class::Comment)]);
    assert_painted("#ffff,\n", &[("#ffff", Class::Color)]);
    assert_painted("#12345\n", &[("#12345", Class::Comment)]);
    let brace = "{#fff}\n";
    assert_painted(brace, &[("{", Class::Punct), ("#fff}", Class::Comment)]);
    assert!(
        classify(brace)
            .iter()
            .all(|run| &brace[run.range.clone()] != "{#fff}"),
        "the brace is a token, not part of the comment"
    );

    assert_painted("90deg\n", &[("90deg", Class::Number)]);
    assert_painted("90degx\n", &[("90", Class::Number), ("degx", Class::Ident)]);
    assert_painted(".25turn\n", &[(".25turn", Class::Number)]);
    assert_painted("50%\n", &[("50%", Class::Number)]);
    assert_painted("256x256\n", &[("256x256", Class::Number)]);
    assert_painted("100%x60%\n", &[("100%x60%", Class::Number)]);
    let spaced = "256 x 256\n";
    let spaced_runs = classify(spaced);
    assert_eq!(
        spaced_runs
            .iter()
            .map(|run| (spaced[run.range.clone()].to_string(), run.class))
            .collect::<Vec<_>>(),
        vec![
            ("256".into(), Class::Number),
            ("x".into(), Class::Ident),
            ("256".into(), Class::Number),
        ]
    );
    span_set_equals_lex(spaced);
    assert_painted(
        ").5\n",
        &[
            (")", Class::Punct),
            (".", Class::Operator),
            ("5", Class::Number),
        ],
    );
    assert_painted("\"hi\\n\"\n", &[("\"hi\\n\"", Class::String)]);
    let raw = "\"\"\"ab\ncd\"\"\"\n";
    assert_painted(raw, &[("\"\"\"ab\ncd\"\"\"", Class::String)]);
    let open = "\"\"\"no end";
    let open_runs = classify(open);
    assert_eq!(open_runs.len(), 1);
    assert_eq!(open_runs[0].range, 0..open.len());
    assert_eq!(open_runs[0].class, Class::String);
    span_set_equals_lex(open);

    assert_painted(
        "14, 0\n",
        &[
            ("14", Class::Number),
            (",", Class::Operator),
            ("0", Class::Number),
        ],
    );
    assert!(crate::parse("14, 0\n").is_err());
    assert_painted(
        "let x = field\n",
        &[
            ("let", Class::Keyword),
            ("x", Class::Ident),
            ("=", Class::Operator),
            ("field", Class::Ident),
        ],
    );
    assert!(
        classify("let x = field\n").iter().all(
            |run| run.class != Class::Keyword || &"let x = field\n"[run.range.clone()] == "let"
        )
    );

    assert_painted("!\n", &[("!", Class::Invalid)]);
    assert_painted("~\n", &[("~", Class::Invalid)]);
    assert_painted("@\n", &[("@", Class::Invalid)]);
    assert_painted("@r.1.*\n", &[("@r.1.*", Class::Reference)]);
    assert_painted(
        "x café\n",
        &[
            ("x", Class::Verb),
            ("caf", Class::Ident),
            ("é", Class::Invalid),
        ],
    );
    let tab = "\tcircle 1\n";
    let tab_runs = classify(tab);
    assert_eq!(tab_runs[0].class, Class::Invalid);
    assert_eq!(&tab[tab_runs[0].range.clone()], "\t");
    span_set_equals_lex(tab);
}

#[test]
fn keyword_rows_and_reserved_words() {
    for kw in Kw::ALL {
        let src = format!("{}\n", kw.as_str());
        let lexed = lex::lex(&src);
        let tok = &lexed.lines[0].tokens[0];
        assert!(
            matches!(&tok.kind, Tok::Kw(found) if found == kw),
            "{}",
            kw.as_str()
        );
        assert_eq!(tok.start, 0);
        assert_eq!(tok.end, kw.as_str().len());
        assert_painted(&src, &[(kw.as_str(), Class::Keyword)]);
    }
    assert_eq!(Kw::ALL.len(), 24);

    for word in crate::RESERVED {
        if Kw::ALL.iter().any(|kw| kw.as_str() == *word) {
            continue;
        }
        let src = format!("{word}\n");
        let lexed = lex::lex(&src);
        assert!(
            matches!(lexed.lines[0].tokens[0].kind, Tok::Ident(_)),
            "{word} is not a keyword"
        );
        let runs = classify(&src);
        assert_eq!(runs[0].class, Class::Verb, "{word}");
        assert!(runs.iter().all(|run| run.class != Class::Keyword));
        span_set_equals_lex(&src);
    }

    let src = "circle profile:vector\n";
    assert!(crate::parse(src).is_ok(), "{:?}", crate::parse(src));
    assert_painted(
        src,
        &[
            ("circle", Class::Verb),
            ("profile", Class::Key),
            ("vector", Class::Ident),
        ],
    );
}

#[test]
fn span_set_equals_lex_on_corpus() {
    for path in corpus() {
        if path.ends_with("edge.art") {
            continue;
        }
        let src = fs::read_to_string(&path).unwrap();
        span_set_equals_lex(&src);
    }
}

fn span_set_equals_lex(src: &str) {
    let lexed = lex::lex(src);
    let runs = classify(src);
    let mut prev = 0usize;
    for (i, run) in runs.iter().enumerate() {
        assert!(run.range.end > run.range.start, "{}", dump(src, &runs));
        if i > 0 {
            assert!(run.range.start >= prev, "overlap\n{}", dump(src, &runs));
        }
        prev = run.range.end;
    }
    for (i, byte) in src.bytes().enumerate() {
        let hits = runs.iter().filter(|run| run.range.contains(&i)).count();
        let in_lex = lexed
            .comments
            .iter()
            .any(|comment| comment.start <= i && i < comment.end)
            || lexed
                .lines
                .iter()
                .any(|line| line.tokens.iter().any(|tok| tok.start <= i && i < tok.end));
        if matches!(byte, b' ' | b'\n' | b'\r') && !in_lex {
            assert_eq!(hits, 0, "whitespace painted at {i}\n{}", dump(src, &runs));
        } else {
            assert_eq!(
                hits,
                1,
                "byte {i} covered {hits} times\n{}",
                dump(src, &runs)
            );
        }
    }
    for comment in &lexed.comments {
        assert!(
            runs.iter()
                .any(|run| run.range == (comment.start..comment.end) && run.class == Class::Comment),
            "comment {}..{}\n{}",
            comment.start,
            comment.end,
            dump(src, &runs)
        );
    }
    for line in &lexed.lines {
        for (index, tok) in line.tokens.iter().enumerate() {
            let covering: Vec<_> = runs
                .iter()
                .filter(|run| run.range.start <= tok.start && tok.end <= run.range.end)
                .collect();
            assert_eq!(
                covering.len(),
                1,
                "token {}..{} {:?}\n{}",
                tok.start,
                tok.end,
                tok.kind,
                dump(src, &runs)
            );
            let run = covering[0];
            assert!(
                tokens_span(line, run),
                "run {}..{} is not a token span\n{}",
                run.range.start,
                run.range.end,
                dump(src, &runs)
            );
            match &tok.kind {
                Tok::Kw(_) => {
                    let key = index > 0 && lex::is_glued_key(&line.tokens, index);
                    let in_path = path_chunk_contains(line, index);
                    let ok = match run.class {
                        Class::Keyword => !key,
                        Class::Key => key,
                        Class::Path => in_path,
                        _ => false,
                    };
                    assert!(
                        ok,
                        "{:?} painted {:?}\n{}",
                        tok.kind,
                        run.class,
                        dump(src, &runs)
                    );
                }
                Tok::Hex(_) => assert_eq!(run.class, Class::Color),
                Tok::Str(_) => assert_eq!(run.class, Class::String),
                Tok::Num(_) | Tok::Percent(_) | Tok::Angle(_) | Tok::Size(_, _) => {
                    assert_eq!(run.class, Class::Number, "{}", dump(src, &runs));
                }
                Tok::Ref(segs) => {
                    if line.indent == 0 && index == 0 && lex::is_directive_ref(&line.tokens) {
                        assert_eq!(run.class, Class::Directive);
                    } else if segs.is_empty() || segs.iter().all(|seg| seg.is_empty()) {
                        assert_eq!(run.class, Class::Invalid);
                    } else {
                        assert_eq!(run.class, Class::Reference, "{}", dump(src, &runs));
                    }
                }
                _ => {}
            }
        }
    }
    for run in &runs {
        let comment = lexed
            .comments
            .iter()
            .any(|comment| comment.start == run.range.start && comment.end == run.range.end);
        let token_span = lexed.lines.iter().any(|line| tokens_span(line, run));
        let invalid = run.class == Class::Invalid
            && src.as_bytes()[run.range.clone()]
                .iter()
                .all(|byte| !matches!(byte, b' ' | b'\n' | b'\r'))
            && lexed.lines.iter().all(|line| {
                line.tokens
                    .iter()
                    .all(|tok| tok.end <= run.range.start || tok.start >= run.range.end)
            });
        assert!(
            comment || token_span || invalid,
            "orphan {:?} {}\n{}",
            run.class,
            &src[run.range.clone()],
            dump(src, &runs)
        );
    }
}

fn path_chunk_contains(line: &crate::lex::Line, index: usize) -> bool {
    let tokens = &line.tokens;
    let head = (line.indent == 0 && lex::is_directive_ref(tokens))
        || matches!(tokens.first().map(|tok| &tok.kind), Some(Tok::Kw(Kw::Use)));
    if !head || index == 0 || tokens.len() < 2 {
        return false;
    }
    let mut end = 2;
    while end < tokens.len() && !tokens[end].ws_before {
        end += 1;
    }
    index < end
}

fn tokens_span(line: &crate::lex::Line, run: &Run) -> bool {
    let Some(first) = line
        .tokens
        .iter()
        .position(|tok| tok.start == run.range.start)
    else {
        return false;
    };
    let Some(last) = line.tokens.iter().position(|tok| tok.end == run.range.end) else {
        return false;
    };
    if first > last {
        return false;
    }
    line.tokens[first..=last]
        .windows(2)
        .all(|pair| pair[0].end == pair[1].start)
        && line.tokens[first].start == run.range.start
        && line.tokens[last].end == run.range.end
}

#[test]
fn classifier_agrees_with_parser() {
    for path in corpus() {
        if path.ends_with("edge.art") {
            continue;
        }
        let src = fs::read_to_string(&path).unwrap();
        let Ok(file) = crate::parse(&src) else {
            continue;
        };
        agree(&path, &src, &file);
    }
}

fn agree(path: &PathBuf, src: &str, file: &File) {
    let runs = classify(src);
    let starts = line_starts(src);
    let mut accounted = Vec::new();
    let mut sksl_lines = 0usize;
    for directive in &file.directives {
        push_span(
            src,
            &starts,
            directive.span,
            Class::Directive,
            &mut accounted,
        );
        walk_named(src, &starts, &directive.named, &mut accounted);
        walk_named(src, &starts, &directive.each, &mut accounted);
        for arg in &directive.args {
            walk_expr(src, &starts, arg, &mut accounted);
        }
    }
    for stmt in &file.stmts {
        walk_stmt(src, &starts, stmt, &mut accounted, &mut sksl_lines);
    }
    for (byte, class) in &accounted {
        let run = runs.iter().find(|run| run.range.start == *byte);
        assert_eq!(
            run.map(|run| run.class),
            Some(*class),
            "{} byte {byte} {:?}\n{}",
            path.display(),
            class,
            dump(src, &runs)
        );
    }
    let mut extra_sksl = 0usize;
    for run in &runs {
        if !matches!(
            run.class,
            Class::Verb | Class::Slot | Class::Patch | Class::Directive
        ) {
            continue;
        }
        if accounted
            .iter()
            .any(|(byte, class)| *byte == run.range.start && *class == run.class)
        {
            continue;
        }
        if run.class == Class::Verb && &src[run.range.clone()] == "sksl" {
            extra_sksl += 1;
            continue;
        }
        panic!(
            "{} unaccounted {:?} `{}`\n{}",
            path.display(),
            run.class,
            &src[run.range.clone()],
            dump(src, &runs)
        );
    }
    assert_eq!(extra_sksl, sksl_lines, "{}", path.display());
}

fn walk_stmt(
    src: &str,
    starts: &[usize],
    stmt: &Stmt,
    accounted: &mut Vec<(usize, Class)>,
    sksl_lines: &mut usize,
) {
    match &stmt.kind {
        StmtKind::Node {
            args, named, body, ..
        } => {
            push_span(src, starts, stmt.span, Class::Verb, accounted);
            for arg in args {
                walk_expr(src, starts, arg, accounted);
            }
            walk_named(src, starts, named, accounted);
            for child in body {
                walk_stmt(src, starts, child, accounted, sksl_lines);
            }
        }
        StmtKind::SlotFill { body, .. } => {
            push_span(src, starts, stmt.span, Class::Slot, accounted);
            for child in body {
                walk_stmt(src, starts, child, accounted, sksl_lines);
            }
        }
        StmtKind::Patch { named, .. } => {
            push_span(src, starts, stmt.span, Class::Patch, accounted);
            walk_named(src, starts, named, accounted);
        }
        StmtKind::Palette { body, .. } => {
            for child in body {
                walk_stmt(src, starts, child, accounted, sksl_lines);
            }
        }
        StmtKind::Style { named, .. }
        | StmtKind::Light { named, .. }
        | StmtKind::Brush { named, .. } => {
            walk_named(src, starts, named, accounted);
        }
        StmtKind::Def { params, body, .. } | StmtKind::Rule { params, body, .. } => {
            walk_named(src, starts, params, accounted);
            for child in body {
                walk_stmt(src, starts, child, accounted, sksl_lines);
            }
        }
        StmtKind::Let { value, .. } => walk_expr(src, starts, value, accounted),
        StmtKind::Shader { params, sksl, .. } => {
            walk_named(src, starts, params, accounted);
            // The parser eats an indented `sksl` line. That line is not its own statement.
            if sksl.is_some() {
                *sksl_lines += 1;
            }
        }
        StmtKind::Check { expr, named, .. } => {
            walk_expr(src, starts, expr, accounted);
            walk_named(src, starts, named, accounted);
        }
        StmtKind::If {
            cond,
            then,
            otherwise,
            ..
        } => {
            walk_expr(src, starts, cond, accounted);
            for child in then.iter().chain(otherwise) {
                walk_stmt(src, starts, child, accounted, sksl_lines);
            }
        }
        StmtKind::Each { iter, body, .. } => {
            walk_expr(src, starts, iter, accounted);
            for child in body {
                walk_stmt(src, starts, child, accounted, sksl_lines);
            }
        }
        StmtKind::Use { .. } => {}
    }
}

fn walk_named(src: &str, starts: &[usize], named: &[Named], accounted: &mut Vec<(usize, Class)>) {
    for item in named {
        push_span(src, starts, item.span, Class::Key, accounted);
        walk_expr(src, starts, &item.value, accounted);
    }
}

fn walk_expr(src: &str, starts: &[usize], expr: &Expr, accounted: &mut Vec<(usize, Class)>) {
    match expr {
        Expr::Call { args, named, .. } => {
            for arg in args {
                walk_expr(src, starts, arg, accounted);
            }
            walk_named(src, starts, named, accounted);
        }
        Expr::If {
            cond,
            then,
            otherwise,
        } => {
            walk_expr(src, starts, cond, accounted);
            walk_expr(src, starts, then, accounted);
            walk_expr(src, starts, otherwise, accounted);
        }
        Expr::Each { iter, body, .. } => {
            walk_expr(src, starts, iter, accounted);
            walk_expr(src, starts, body, accounted);
        }
        Expr::Tuple(xs) | Expr::List(xs) => {
            for item in xs {
                walk_expr(src, starts, item, accounted);
            }
        }
        Expr::Binary(_, left, right) | Expr::Range(left, right) | Expr::Size(left, right) => {
            walk_expr(src, starts, left, accounted);
            walk_expr(src, starts, right, accounted);
        }
        Expr::Unary(_, inner) | Expr::Member(inner, _) => walk_expr(src, starts, inner, accounted),
        Expr::Num(_)
        | Expr::Percent(_)
        | Expr::Angle(_)
        | Expr::Hex(_)
        | Expr::Str(_)
        | Expr::Ident(_)
        | Expr::Ref(_) => {}
    }
}

fn push_span(
    src: &str,
    starts: &[usize],
    span: Span,
    class: Class,
    accounted: &mut Vec<(usize, Class)>,
) {
    accounted.push((span_byte(src, starts, span), class));
}

fn line_starts(src: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (i, byte) in src.bytes().enumerate() {
        if byte == b'\n' {
            starts.push(i + 1);
        }
    }
    starts
}

fn span_byte(src: &str, starts: &[usize], span: Span) -> usize {
    let line_start = starts[span.line as usize - 1];
    let mut col = 1u32;
    for (offset, ch) in src[line_start..].char_indices() {
        if ch == '\n' {
            break;
        }
        if col == span.col {
            return line_start + offset;
        }
        col += 1;
    }
    panic!("span {span} is not on its line");
}

fn corpus() -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    for dir in ["examples", "crates/syntax/tests/fixtures"] {
        collect(&root.join(dir), &mut files);
    }
    let edge = root.join("editors/vscode/test/edge.art");
    if edge.exists() {
        files.push(edge);
    }
    files.sort();
    files
}

fn collect(dir: &std::path::Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "art") {
            files.push(path);
        }
    }
}
