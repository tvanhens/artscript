use std::ops::Range;

use crate::lex::{self, Line, Tok, Token};
use crate::words::Word;

mod grammar;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Class {
    Comment,
    Keyword,
    Color,
    String,
    Number,
    Reference,
    Directive,
    Verb,
    Key,
    Slot,
    Patch,
    Path,
    Operator,
    Punct,
    Ident,
    Invalid,
}

impl Class {
    pub(crate) const ALL: &[Class] = &[
        Class::Comment,
        Class::Keyword,
        Class::Color,
        Class::String,
        Class::Number,
        Class::Reference,
        Class::Directive,
        Class::Verb,
        Class::Key,
        Class::Slot,
        Class::Patch,
        Class::Path,
        Class::Operator,
        Class::Punct,
        Class::Ident,
        Class::Invalid,
    ];

    pub(crate) fn letter(self) -> char {
        match self {
            Class::Comment => 'c',
            Class::Keyword => 'k',
            Class::Color => 'h',
            Class::String => 's',
            Class::Number => 'n',
            Class::Reference => 'r',
            Class::Directive => 'd',
            Class::Verb => 'v',
            Class::Key => 'y',
            Class::Slot => 'l',
            Class::Patch => 'p',
            Class::Path => 'a',
            Class::Operator => 'o',
            Class::Punct => 'b',
            Class::Ident => 'i',
            Class::Invalid => '!',
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Run {
    pub range: Range<usize>,
    pub class: Class,
}

/// The grammar's path regex is built from this same constant.
pub(crate) const PATH_CHARS: &str = "A-Za-z0-9_./{}*-";

pub(crate) fn is_path_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | '{' | '}' | '*' | '-')
}

enum HeadRole {
    Directive { path: Option<Range<usize>> },
    Use { path: Option<Range<usize>> },
    Patch { tokens: usize },
    Slot,
    Verb,
    Plain,
}

pub(crate) fn classify(src: &str) -> Vec<Run> {
    let lexed = lex::lex(src);
    let mut runs = Vec::new();
    for comment in &lexed.comments {
        runs.push(Run {
            range: comment.start..comment.end,
            class: Class::Comment,
        });
    }
    for line in &lexed.lines {
        let role = head_role(src, line);
        let mut index = 0;
        while index < line.tokens.len() {
            let (class, next) = class_at(src, line, &role, index);
            debug_assert!(next > index);
            let start = line.tokens[index].start;
            let end = line.tokens[next - 1].end;
            if end > start {
                runs.push(Run {
                    range: start..end,
                    class,
                });
            }
            index = next;
        }
    }
    fill_invalid_gaps(src, &mut runs);
    runs.sort_by_key(|run| run.range.start);
    runs
}

fn head_role(src: &str, line: &Line) -> HeadRole {
    let tokens = &line.tokens;
    let Some(first) = tokens.first() else {
        return HeadRole::Plain;
    };
    if line.indent == 0 && lex::is_directive_ref(tokens) {
        return HeadRole::Directive {
            path: directive_path(src, tokens, 1),
        };
    }
    if lex::is_slot_at(tokens, 0) {
        return HeadRole::Slot;
    }
    if matches!(first.kind, Tok::Kw(Word::Use)) {
        return HeadRole::Use {
            path: glued_path(src, tokens, 1),
        };
    }
    if matches!(first.kind, Tok::Kw(_)) {
        return HeadRole::Plain;
    }
    let patch = lex::patch_path_len(tokens);
    if patch > 0 {
        return HeadRole::Patch { tokens: patch };
    }
    if matches!(first.kind, Tok::Ident(_)) {
        return HeadRole::Verb;
    }
    HeadRole::Plain
}

fn directive_path(src: &str, tokens: &[Token], start: usize) -> Option<Range<usize>> {
    let range = glued_path(src, tokens, start)?;
    let text = &src[tokens[range.start].start..tokens[range.end - 1].end];
    text.chars()
        .any(|c| matches!(c, '/' | '.' | '{'))
        .then_some(range)
}

fn glued_path(src: &str, tokens: &[Token], start: usize) -> Option<Range<usize>> {
    if start >= tokens.len() {
        return None;
    }
    let end = glued_end(tokens, start);
    let text = &src[tokens[start].start..tokens[end - 1].end];
    text.chars().all(is_path_char).then_some(start..end)
}

fn glued_end(tokens: &[Token], start: usize) -> usize {
    let mut i = start + 1;
    while i < tokens.len() && !tokens[i].ws_before {
        i += 1;
    }
    i
}

fn class_at(src: &str, line: &Line, role: &HeadRole, index: usize) -> (Class, usize) {
    let tokens = &line.tokens;
    match role {
        HeadRole::Directive { .. } if index == 0 => return (Class::Directive, 1),
        HeadRole::Directive { path: Some(path) } if path.contains(&index) => {
            return (Class::Path, contiguous_end(tokens, index, path.end));
        }
        HeadRole::Use { path: Some(path) } if path.contains(&index) => {
            return (Class::Path, contiguous_end(tokens, index, path.end));
        }
        HeadRole::Patch { tokens: n } if index < *n => {
            return (Class::Patch, contiguous_end(tokens, index, *n));
        }
        HeadRole::Slot if index == 0 => {
            return (Class::Slot, contiguous_end(tokens, 0, tokens.len().min(2)));
        }
        HeadRole::Verb if index == 0 => return (Class::Verb, 1),
        _ => {}
    }
    if index > 0 && lex::is_glued_key(tokens, index) {
        return (Class::Key, index + 1);
    }
    (lexical(src, &tokens[index]), index + 1)
}

fn contiguous_end(tokens: &[Token], start: usize, limit: usize) -> usize {
    let mut i = start + 1;
    while i < limit && tokens[i].start == tokens[i - 1].end {
        i += 1;
    }
    i
}

fn lexical(src: &str, tok: &Token) -> Class {
    match &tok.kind {
        Tok::Kw(_) => Class::Keyword,
        Tok::Hex(_) => Class::Color,
        Tok::Str(_) => Class::String,
        Tok::Num(_) | Tok::Percent(_) | Tok::Angle(_) | Tok::Size(_, _) => Class::Number,
        Tok::Ref(segs) if segs.iter().any(|seg| !seg.is_empty()) => Class::Reference,
        Tok::Ref(_) => Class::Invalid,
        Tok::Ident(_) => Class::Ident,
        Tok::NotEq | Tok::Approx
            if &src[tok.start..tok.end] == "!" || &src[tok.start..tok.end] == "~" =>
        {
            Class::Invalid
        }
        Tok::Plus
        | Tok::Minus
        | Tok::Star
        | Tok::Slash
        | Tok::EqEq
        | Tok::NotEq
        | Tok::Lt
        | Tok::Le
        | Tok::Gt
        | Tok::Ge
        | Tok::Approx
        | Tok::Arrow
        | Tok::DotDot
        | Tok::Eq
        | Tok::Dot
        | Tok::Colon
        | Tok::Comma => Class::Operator,
        Tok::LParen | Tok::RParen | Tok::LBrack | Tok::RBrack | Tok::LBrace | Tok::RBrace => {
            Class::Punct
        }
    }
}

fn fill_invalid_gaps(src: &str, runs: &mut Vec<Run>) {
    let mut covered = vec![false; src.len()];
    for run in runs.iter() {
        for byte in run.range.clone() {
            covered[byte] = true;
        }
    }
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if covered[i] || is_gap(bytes[i]) || (bytes[i] == b'\t' && !indent_tab(bytes, i)) {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len()
            && !covered[i]
            && !is_gap(bytes[i])
            && !(bytes[i] == b'\t' && !indent_tab(bytes, i))
        {
            i += 1;
        }
        runs.push(Run {
            range: start..i,
            class: Class::Invalid,
        });
    }
}

fn is_gap(byte: u8) -> bool {
    matches!(byte, b' ' | b'\n' | b'\r')
}

pub(crate) fn indent_tab(bytes: &[u8], index: usize) -> bool {
    if bytes.get(index) != Some(&b'\t') {
        return false;
    }
    let mut i = index;
    while i > 0 && bytes[i - 1] != b'\n' && bytes[i - 1] != b'\r' {
        i -= 1;
        if bytes[i] != b' ' && bytes[i] != b'\t' {
            return false;
        }
    }
    true
}

pub(crate) fn ribbon(src: &str, runs: &[Run]) -> String {
    let mut lines = Vec::new();
    let mut cur = String::new();
    let bytes = src.as_bytes();
    for (byte, ch) in src.char_indices() {
        if ch == '\r' {
            if bytes.get(byte + 1) != Some(&b'\n') {
                lines.push(std::mem::take(&mut cur));
            }
            continue;
        }
        if ch == '\n' {
            lines.push(std::mem::take(&mut cur));
            continue;
        }
        let letter = runs
            .iter()
            .find(|run| run.range.contains(&byte))
            .map(|run| run.class.letter())
            .unwrap_or(' ');
        for _ in 0..ch.len_utf16() {
            cur.push(letter);
        }
    }
    if !src.ends_with('\n') {
        lines.push(cur);
    }
    let mut out = lines.join("\n");
    if src.ends_with('\n') {
        out.push('\n');
    }
    out
}
