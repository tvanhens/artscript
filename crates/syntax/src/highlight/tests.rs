use std::fs;
use std::ops::Range;
use std::path::PathBuf;

use crate::ast::{
    Decl, DeclKind, DefItem, Draw, DrawItem, Expr, File, Item, Mask, Named, Patch, Scene,
    SceneKind, SlotFill, Span, SwatchKind,
};
use crate::lex::{self, Tok};
use crate::words::{self, Word};

use super::grammar::{self, emit};
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
    for (word, text, _) in words::rows()
        .iter()
        .filter(|row| row.2 == words::Class::Keyword)
    {
        let src = format!("{text}\n");
        let lexed = lex::lex(&src);
        let tok = &lexed.lines[0].tokens[0];
        assert!(
            matches!(&tok.kind, Tok::Kw(found) if found == word),
            "{text}"
        );
        assert_eq!(tok.start, 0);
        assert_eq!(tok.end, text.len());
        assert_painted(&src, &[(text, Class::Keyword)]);
    }

    for (_, word, class) in words::rows() {
        if *class == words::Class::Keyword {
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
        let bare_tab = byte == b'\t' && !super::indent_tab(src.as_bytes(), i);
        if (matches!(byte, b' ' | b'\n' | b'\r') || bare_tab) && !in_lex {
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
        || matches!(
            tokens.first().map(|tok| &tok.kind),
            Some(Tok::Kw(Word::Use))
        );
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
    for item in &file.items {
        match item {
            Item::Decl(decl) => walk_decl(src, &starts, decl, &mut accounted, &mut sksl_lines),
            Item::Scene(scene) => walk_scene(src, &starts, scene, &mut accounted),
        }
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

fn walk_decl(
    src: &str,
    starts: &[usize],
    decl: &Decl,
    accounted: &mut Vec<(usize, Class)>,
    sksl_lines: &mut usize,
) {
    match &decl.kind {
        DeclKind::Use { .. } => {}
        DeclKind::Palette { body, .. } => {
            for swatch in body {
                push_span(src, starts, swatch.span, Class::Verb, accounted);
                match &swatch.kind {
                    SwatchKind::Color(expr) => walk_expr(src, starts, expr, accounted),
                    SwatchKind::Ramp { pos, hue } => {
                        for expr in pos.exprs() {
                            walk_expr(src, starts, expr, accounted);
                        }
                        if let Some(hue) = hue {
                            walk_expr(src, starts, hue, accounted);
                        }
                    }
                }
            }
        }
        DeclKind::Style { named, .. }
        | DeclKind::Light { named, .. }
        | DeclKind::Brush { named, .. } => {
            walk_named(src, starts, named, accounted);
        }
        DeclKind::Finish { params, body, .. } => {
            walk_named(src, starts, params, accounted);
            for fx in body {
                walk_named(src, starts, &fx.named, accounted);
                for arg in &fx.args {
                    walk_expr(src, starts, arg, accounted);
                }
            }
        }
        DeclKind::Def { params, body, .. } => {
            walk_named(src, starts, params, accounted);
            for item in body {
                match item {
                    DefItem::Scene(scene) => walk_scene(src, starts, scene, accounted),
                    DefItem::Fx(fx) => {
                        walk_named(src, starts, &fx.named, accounted);
                        for arg in &fx.args {
                            walk_expr(src, starts, arg, accounted);
                        }
                    }
                }
            }
        }
        DeclKind::Rule { params, body, .. } => {
            walk_named(src, starts, params, accounted);
            for scene in body {
                walk_scene(src, starts, scene, accounted);
            }
        }
        DeclKind::Shader { params, sksl, .. } => {
            walk_named(src, starts, params, accounted);
            // The parser eats an indented `sksl` line. That line is not its own statement.
            if sksl.is_some() {
                *sksl_lines += 1;
            }
        }
    }
}

fn walk_scene(src: &str, starts: &[usize], scene: &Scene, accounted: &mut Vec<(usize, Class)>) {
    match &scene.kind {
        SceneKind::Let { value, .. } => walk_expr(src, starts, value, accounted),
        SceneKind::If {
            cond,
            then,
            otherwise,
        } => {
            walk_expr(src, starts, cond, accounted);
            for child in then.iter().chain(otherwise) {
                walk_scene(src, starts, child, accounted);
            }
        }
        SceneKind::Each { iter, body, .. } => {
            walk_expr(src, starts, iter, accounted);
            for child in body {
                walk_scene(src, starts, child, accounted);
            }
        }
        SceneKind::Check { expr, named, .. } => {
            walk_expr(src, starts, expr, accounted);
            walk_named(src, starts, named, accounted);
        }
        SceneKind::Slot {
            named, fallback, ..
        } => {
            walk_named(src, starts, named, accounted);
            for child in fallback {
                walk_scene(src, starts, child, accounted);
            }
        }
        SceneKind::Draw(draw) => walk_draw(src, starts, scene.span, draw, accounted),
    }
}

fn walk_draw(
    src: &str,
    starts: &[usize],
    span: Span,
    draw: &Draw,
    accounted: &mut Vec<(usize, Class)>,
) {
    push_span(src, starts, span, Class::Verb, accounted);
    for arg in &draw.args {
        walk_expr(src, starts, arg, accounted);
    }
    walk_named(src, starts, &draw.named, accounted);
    for item in draw.body.items() {
        match item {
            DrawItem::Scene(scene) => walk_scene(src, starts, scene, accounted),
            DrawItem::Fx(fx) => {
                walk_named(src, starts, &fx.named, accounted);
                for arg in &fx.args {
                    walk_expr(src, starts, arg, accounted);
                }
            }
            DrawItem::Mask { mask, .. } => match mask {
                Mask::Value(expr) => walk_expr(src, starts, expr, accounted),
                Mask::Block(body) => {
                    for scene in body {
                        walk_scene(src, starts, scene, accounted);
                    }
                }
            },
            DrawItem::SlotFill(fill) => walk_slot_fill(src, starts, fill, accounted),
            DrawItem::Patch(patch) => walk_patch(src, starts, patch, accounted),
        }
    }
}

fn walk_slot_fill(
    src: &str,
    starts: &[usize],
    fill: &SlotFill,
    accounted: &mut Vec<(usize, Class)>,
) {
    push_span(src, starts, fill.span, Class::Slot, accounted);
    for scene in &fill.body {
        walk_scene(src, starts, scene, accounted);
    }
}

fn walk_patch(src: &str, starts: &[usize], patch: &Patch, accounted: &mut Vec<(usize, Class)>) {
    push_span(src, starts, patch.span, Class::Patch, accounted);
    walk_named(src, starts, &patch.named, accounted);
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

#[test]
fn grammar_is_current() {
    let root = workspace();
    for (path, fresh) in [
        (
            root.join("editors/vscode/syntaxes/artscript.tmLanguage.json"),
            grammar::to_json(&emit()),
        ),
        (
            root.join("editors/vscode/test/scopes.json"),
            grammar::scopes_json(),
        ),
    ] {
        if std::env::var("ARTSCRIPT_BLESS").ok().as_deref() == Some("1") {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, &fresh).unwrap();
        } else {
            let have = fs::read_to_string(&path).unwrap_or_default();
            assert_eq!(
                have,
                fresh,
                "bless with ARTSCRIPT_BLESS=1 cargo test -p artscript-syntax highlight\n{}",
                path.display()
            );
        }
    }
}

#[test]
fn emit_ribbons() {
    let root = workspace();
    let out = root.join("target/art-ribbons");
    let _ = fs::remove_dir_all(&out);
    fs::create_dir_all(&out).unwrap();
    let mut manifest = Vec::new();
    for path in corpus() {
        let rel = path
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let src = fs::read_to_string(&path).unwrap();
        let ribbon_rel = format!("{rel}.ribbon");
        let ribbon_path = out.join(&ribbon_rel);
        fs::create_dir_all(ribbon_path.parent().unwrap()).unwrap();
        fs::write(&ribbon_path, ribbon(&src, &classify(&src))).unwrap();
        manifest.push(serde_json::json!({
            "source": rel,
            "ribbon": ribbon_rel,
        }));
    }
    fs::write(
        out.join("manifest.json"),
        serde_json::to_string_pretty(&manifest).unwrap() + "\n",
    )
    .unwrap();
}

#[test]
fn boundary_fixture_matches_generator() {
    let path = workspace().join("editors/vscode/test/boundaries.art");
    let fresh = boundary_source();
    if std::env::var("ARTSCRIPT_BLESS").ok().as_deref() == Some("1") {
        fs::write(&path, fresh).unwrap();
    } else {
        let have = fs::read_to_string(&path).unwrap_or_default();
        assert_eq!(have, fresh, "bless with ARTSCRIPT_BLESS=1");
    }
}

fn boundary_source() -> String {
    let mut lines = Vec::new();
    let tails = [
        "",
        "foo",
        "foo/bar",
        "foo/bar!baz",
        "foo#fff",
        "foo/bar#note",
        "foo#fffff",
        "foo/bar#fff",
        "/foo/bar",
        "foo?",
        "foo/bar?",
        "foo?bar",
    ];
    for host in ["use ", "use", "@target ", "@target"] {
        for tail in tails {
            lines.push(format!("{host}{tail}"));
        }
    }
    for indent in ["", "  ", "\t", " \t", "\t ", "\t  ", "\t \t"] {
        for body in [
            "@target out/file",
            "@a.b",
            "123",
            "# c",
            "circle 1",
            "use foo",
        ] {
            lines.push(format!("{indent}{body}"));
        }
    }
    lines.push("\t".into());
    lines.push("circle\t1".into());
    for patch in [
        ".if",
        ".head.if",
        ".head. eye",
        ".mask off",
        ".head.eye",
        ".5",
        "..",
    ] {
        lines.push(patch.into());
    }
    lines.join("\n") + "\n"
}

#[test]
fn edge_fixture_exercises_every_class() {
    let src = fs::read_to_string(workspace().join("editors/vscode/test/edge.art")).unwrap();
    span_set_equals_lex(&src);
    let painted = ribbon(&src, &classify(&src));
    for class in Class::ALL {
        assert!(
            painted.contains(class.letter()),
            "edge.art ribbon has no {:?}\n{painted}",
            class
        );
    }
}

fn workspace() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn corpus() -> Vec<PathBuf> {
    let root = workspace();
    let mut files = Vec::new();
    for dir in ["examples", "crates/syntax/tests/fixtures"] {
        collect(&root.join(dir), &mut files);
    }
    for name in ["edge.art", "boundaries.art"] {
        let path = root.join("editors/vscode/test").join(name);
        if path.exists() {
            files.push(path);
        }
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
