use serde_json::{Map, Value};

use super::{Class, PATH_CHARS};
use crate::lex::{ANGLE_SUFFIXES, HEX_DELIMS, HEX_LENS, STRING_ESCAPES};
use crate::words::{self, Word};

const IDENT: &str = "[A-Za-z_][A-Za-z0-9_]*";
/// A digit may sit in front of a word. `5degx` is a number and then a name.
const NOT_IDENT_BEFORE: &str = "(?<![A-Za-z_])";
const NOT_IDENT_AFTER: &str = "(?![A-Za-z0-9_])";
const AT_STATEMENT: &str = "(?<=^[^#\"@A-Za-z0-9_./{}*+=!<>~:,()\\[\\]-]*)";

const ESCAPE_SCOPE: &str = "constant.character.escape.art";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum KwGroup {
    Import,
    Declaration,
    Control,
    Logical,
    Constant,
}

pub(crate) fn group(word: Word) -> Option<KwGroup> {
    Some(match word {
        Word::Use | Word::As | Word::From => KwGroup::Import,
        Word::Palette
        | Word::Style
        | Word::Finish
        | Word::Def
        | Word::Rule
        | Word::Let
        | Word::Shader
        | Word::Light
        | Word::Brush => KwGroup::Declaration,
        Word::Assert
        | Word::Expect
        | Word::If
        | Word::Then
        | Word::Else
        | Word::Each
        | Word::In
        | Word::Fx
        | Word::Mask
        | Word::Slot => KwGroup::Control,
        Word::And | Word::Or | Word::Not => KwGroup::Logical,
        Word::True | Word::False | Word::None => KwGroup::Constant,
        Word::Ramp
        | Word::Color
        | Word::Sksl
        | Word::Field
        | Word::Warp
        | Word::Backdrop
        | Word::Profile
        | Word::Pose
        | Word::Joint
        | Word::Ease
        | Word::T
        | Word::Frames
        | Word::Anim
        | Word::Key => return None,
    })
}

impl KwGroup {
    pub(crate) fn scope(self) -> &'static str {
        match self {
            KwGroup::Import => "keyword.control.import.art",
            KwGroup::Declaration => "storage.type.art",
            KwGroup::Control => "keyword.control.art",
            KwGroup::Logical => "keyword.operator.logical.art",
            KwGroup::Constant => "constant.language.art",
        }
    }

    fn all() -> &'static [KwGroup] {
        &[
            KwGroup::Import,
            KwGroup::Declaration,
            KwGroup::Control,
            KwGroup::Logical,
            KwGroup::Constant,
        ]
    }
}

pub(crate) fn scope(class: Class) -> &'static str {
    match class {
        Class::Comment => "comment.line.number-sign.art",
        Class::Keyword => unreachable!("keywords are scoped by group"),
        Class::Color => "constant.other.color.rgb-value.hex.art",
        Class::String => "string.quoted.art",
        Class::Number => "constant.numeric.art",
        Class::Reference => "variable.other.reference.art",
        Class::Directive => "keyword.other.directive.art",
        Class::Verb => "entity.name.function.verb.art",
        Class::Key => "entity.other.attribute-name.key.art",
        Class::Slot => "entity.name.tag.slot.art",
        Class::Patch => "variable.other.property.patch.art",
        Class::Path => "string.unquoted.path.art",
        Class::Operator => "keyword.operator.art",
        Class::Punct => "punctuation.art",
        Class::Ident => unreachable!("identifiers are unscoped"),
        Class::Invalid => "invalid.illegal.art",
    }
}

pub(crate) enum Pattern {
    Match {
        re: String,
        name: Option<&'static str>,
        captures: Vec<(u8, &'static str)>,
    },
    Region {
        begin: String,
        end: String,
        name: Option<&'static str>,
        begin_captures: Vec<(u8, &'static str)>,
        end_captures: Vec<(u8, &'static str)>,
        inner: Vec<Pattern>,
    },
}

pub(crate) fn emit() -> Vec<Pattern> {
    let invalid = scope(Class::Invalid);
    let mut patterns = vec![
        Pattern::Region {
            begin: "\"\"\"".into(),
            end: "\"\"\"".into(),
            name: Some(scope(Class::String)),
            begin_captures: vec![],
            end_captures: vec![],
            inner: vec![],
        },
        Pattern::Region {
            begin: "\"".into(),
            end: "\"|(?<!\\\\)(?:\\\\\\\\)*\\n".into(),
            name: Some(scope(Class::String)),
            begin_captures: vec![],
            end_captures: vec![],
            inner: vec![
                Pattern::Match {
                    re: escape_re(),
                    name: Some(ESCAPE_SCOPE),
                    captures: vec![],
                },
                Pattern::Match {
                    re: "\\\\.".into(),
                    name: Some(invalid),
                    captures: vec![],
                },
            ],
        },
        Pattern::Match {
            re: hex_re(),
            name: Some(scope(Class::Color)),
            captures: vec![],
        },
        Pattern::Match {
            re: "#.*$".into(),
            name: Some(scope(Class::Comment)),
            captures: vec![],
        },
        indent_tab(invalid),
        directive(invalid),
        keyword_slot(),
        keyword_slot_erased(),
        use_path(invalid),
    ];
    for kw_group in KwGroup::all() {
        patterns.push(keyword_head(*kw_group));
    }
    patterns.push(patch());
    patterns.push(slot());
    patterns.push(slot_erased());
    patterns.push(verb());
    patterns.push(Pattern::Match {
        re: format!("@{ident}(?:\\.(?:{ident}|[0-9]+|\\*))*", ident = IDENT),
        name: Some(scope(Class::Reference)),
        captures: vec![],
    });
    patterns.push(Pattern::Match {
        re: "@(?![A-Za-z_])".into(),
        name: Some(invalid),
        captures: vec![],
    });
    patterns.push(Pattern::Match {
        re: "\\.\\.".into(),
        name: Some(scope(Class::Operator)),
        captures: vec![],
    });
    patterns.push(Pattern::Match {
        re: number_re(),
        name: Some(scope(Class::Number)),
        captures: vec![],
    });
    patterns.push(key_erased());
    patterns.push(Pattern::Match {
        re: format!(
            "{before}({ident})(:)",
            before = NOT_IDENT_BEFORE,
            ident = IDENT
        ),
        name: None,
        captures: vec![(1, scope(Class::Key)), (2, scope(Class::Operator))],
    });
    for kw_group in KwGroup::all() {
        patterns.push(Pattern::Match {
            re: format!(
                "{before}(?:{alt}){after}",
                before = NOT_IDENT_BEFORE,
                alt = kw_alt(*kw_group),
                after = NOT_IDENT_AFTER
            ),
            name: Some(kw_group.scope()),
            captures: vec![],
        });
    }
    patterns.push(Pattern::Match {
        re: format!(
            "{before}{ident}{after}",
            before = NOT_IDENT_BEFORE,
            ident = IDENT,
            after = NOT_IDENT_AFTER
        ),
        name: None,
        captures: vec![],
    });
    patterns.push(Pattern::Match {
        re: "==|!=|<=|>=|~=|->|[-+*/=<>:,]|\\.".into(),
        name: Some(scope(Class::Operator)),
        captures: vec![],
    });
    patterns.push(Pattern::Match {
        re: "\\(|\\)|\\[|\\]|\\{|\\}".into(),
        name: Some(scope(Class::Punct)),
        captures: vec![],
    });
    patterns.push(Pattern::Match {
        re: "[^\\n\\r\\t ]".into(),
        name: Some(invalid),
        captures: vec![],
    });
    patterns
}

fn indent_tab(invalid: &'static str) -> Pattern {
    Pattern::Match {
        re: "(?<=^[ \\t]*)\\t".into(),
        name: Some(invalid),
        captures: vec![],
    }
}

fn directive(invalid: &'static str) -> Pattern {
    Pattern::Region {
        begin: format!(
            "{at}(@{ident}){after}(?!\\.(?:[A-Za-z_0-9*]))",
            at = at_directive(),
            ident = IDENT,
            after = NOT_IDENT_AFTER
        ),
        end: format!(
            "({path}*[/.{{]{path}*){chunk}|{stop}",
            path = path_class(),
            chunk = path_chunk_end(),
            stop = glue_stop()
        ),
        name: None,
        begin_captures: vec![(1, scope(Class::Directive))],
        end_captures: vec![(1, scope(Class::Path))],
        inner: vec![invalid_dropped(invalid)],
    }
}

fn use_path(invalid: &'static str) -> Pattern {
    Pattern::Region {
        begin: format!(
            "{at}(use){after}",
            at = AT_STATEMENT,
            after = NOT_IDENT_AFTER
        ),
        end: format!(
            "({path}+){chunk}|{stop}",
            path = path_class(),
            chunk = path_chunk_end(),
            stop = glue_stop()
        ),
        name: None,
        begin_captures: vec![(1, KwGroup::Import.scope())],
        end_captures: vec![(1, scope(Class::Path))],
        inner: vec![invalid_dropped(invalid)],
    }
}

fn keyword_head(kw_group: KwGroup) -> Pattern {
    Pattern::Match {
        re: format!(
            "{at}(?:{alt}){after}",
            at = AT_STATEMENT,
            alt = kw_alt(kw_group),
            after = NOT_IDENT_AFTER
        ),
        name: Some(kw_group.scope()),
        captures: vec![],
    }
}

fn patch() -> Pattern {
    let patch = scope(Class::Patch);
    Pattern::Region {
        begin: format!("{at}(\\.)(?!\\.|[0-9])", at = AT_STATEMENT),
        end: format!(
            "(?<=[A-Za-z0-9_])|(?![ \\t]|{dropped})(?!{name})(?!(?<![ \\t])\\.(?!\\.|[0-9])(?=(?:[ \\t]|{dropped})*{name}))",
            dropped = dropped_class(),
            name = non_keyword_name()
        ),
        name: None,
        begin_captures: vec![(1, patch)],
        end_captures: vec![],
        inner: vec![
            invalid_dropped(scope(Class::Invalid)),
            patch_segment(patch),
            patch_name(patch),
        ],
    }
}

fn patch_name(patch: &'static str) -> Pattern {
    Pattern::Region {
        begin: format!("({name})", name = non_keyword_name()),
        end: format!(
            "(?!\\.(?!\\.|[0-9])(?=(?:[ \\t]|{dropped})*{name}))(?!(?:[ \\t]|{dropped})*{dropped}\\.(?!\\.|[0-9])(?=(?:[ \\t]|{dropped})*{name}))",
            dropped = dropped_class(),
            name = non_keyword_name()
        ),
        name: None,
        begin_captures: vec![(1, patch)],
        end_captures: vec![],
        inner: vec![invalid_dropped(scope(Class::Invalid)), patch_segment(patch)],
    }
}

fn patch_segment(patch: &'static str) -> Pattern {
    Pattern::Region {
        begin: format!(
            "(\\.)(?!\\.|[0-9])(?=(?:[ \\t]|{dropped})*{name})",
            dropped = dropped_class(),
            name = non_keyword_name()
        ),
        end: format!(
            "({name})|{stop}",
            name = non_keyword_name(),
            stop = glue_stop()
        ),
        name: None,
        begin_captures: vec![(1, patch)],
        end_captures: vec![(1, patch)],
        inner: vec![invalid_dropped(scope(Class::Invalid))],
    }
}

fn non_keyword_name() -> String {
    format!("{not_kw}{ident}", not_kw = not_keyword(), ident = IDENT)
}

fn not_keyword() -> String {
    format!(
        "(?!(?:{alts}){after})",
        alts = keyword_alts(),
        after = NOT_IDENT_AFTER
    )
}

fn keyword_alts() -> String {
    let mut words: Vec<&str> = words::rows()
        .iter()
        .filter(|row| row.2 == words::Class::Keyword)
        .map(|row| row.1)
        .collect();
    words.sort_by_key(|word| std::cmp::Reverse(word.len()));
    words.join("|")
}

fn slot_tail() -> String {
    format!("(?={after})", after = slot_after())
}

fn slot() -> Pattern {
    Pattern::Match {
        re: format!(
            "{at}({ident})(:){tail}",
            at = AT_STATEMENT,
            ident = IDENT,
            tail = slot_tail()
        ),
        name: None,
        captures: vec![(1, scope(Class::Slot)), (2, scope(Class::Slot))],
    }
}

fn verb() -> Pattern {
    Pattern::Match {
        re: format!("{at}{ident}", at = AT_STATEMENT, ident = IDENT),
        name: Some(scope(Class::Verb)),
        captures: vec![],
    }
}

fn kw_alt(kw_group: KwGroup) -> String {
    let mut words: Vec<&str> = words::rows()
        .iter()
        .filter(|row| group(row.0) == Some(kw_group))
        .map(|row| row.1)
        .collect();
    words.sort_by_key(|word| std::cmp::Reverse(word.len()));
    words.join("|")
}

fn keyword_slot() -> Pattern {
    Pattern::Match {
        re: format!(
            "{at}({alts})(:){tail}",
            at = AT_STATEMENT,
            alts = keyword_alts(),
            tail = slot_tail()
        ),
        name: None,
        captures: vec![(1, scope(Class::Slot)), (2, scope(Class::Slot))],
    }
}

fn keyword_slot_erased() -> Pattern {
    colon_erased_region(
        format!("{at}({alts})", at = AT_STATEMENT, alts = keyword_alts()),
        scope(Class::Slot),
    )
}

fn slot_erased() -> Pattern {
    colon_erased_region(
        format!("{at}({ident})", at = AT_STATEMENT, ident = IDENT),
        scope(Class::Slot),
    )
}

fn key_erased() -> Pattern {
    Pattern::Region {
        begin: format!(
            "{before}({ident})(?={erased}:)",
            before = NOT_IDENT_BEFORE,
            ident = IDENT,
            erased = erased_before_colon()
        ),
        end: format!("(:)|{stop}", stop = glue_stop()),
        name: None,
        begin_captures: vec![(1, scope(Class::Key))],
        end_captures: vec![(1, scope(Class::Operator))],
        inner: vec![invalid_dropped(scope(Class::Invalid))],
    }
}

fn colon_erased_region(head: String, head_scope: &'static str) -> Pattern {
    Pattern::Region {
        begin: format!(
            "{head}(?={erased}:{after})",
            erased = erased_before_colon(),
            after = slot_after()
        ),
        end: format!("(:)|{stop}", stop = glue_stop()),
        name: None,
        begin_captures: vec![(1, head_scope)],
        end_captures: vec![(1, scope(Class::Operator))],
        inner: vec![invalid_dropped(scope(Class::Invalid))],
    }
}

fn erased_before_colon() -> String {
    format!("(?:{dropped}|[ \\t])*{dropped}", dropped = dropped_class())
}

fn slot_after() -> String {
    format!(
        "(?:[ \\t]|{dropped})*(?:\\r?$|#(?!(?:{hex})(?=$|{delims})))",
        dropped = dropped_class(),
        hex = hex_body(),
        delims = delim_class()
    )
}

fn path_class() -> String {
    format!("[{PATH_CHARS}]")
}

fn glue_stop() -> String {
    format!("(?![ \\t]|{})", dropped_class())
}

fn invalid_dropped(invalid: &'static str) -> Pattern {
    Pattern::Match {
        re: dropped_class().into(),
        name: Some(invalid),
        captures: vec![],
    }
}

fn at_directive() -> String {
    format!(
        "(?<=^\\t*(?:{dropped}+[ \\t]*)*)",
        dropped = dropped_class()
    )
}

fn path_chunk_end() -> String {
    let gap = "(?:$|[ \\t]|#)";
    let dropped = dropped_class();
    let color = format!(
        "#(?:{hex})(?=$|{delims})",
        hex = hex_body(),
        delims = delim_class()
    );
    let glued = format!(
        "(?:{dropped}|[ \\t])*{dropped}(?:[@\"A-Za-z0-9_./{{}}*+=!<>~:,()\\[\\]-]|{color})"
    );
    format!(
        "(?!{glued})(?!(?:{dropped})*{color})(?={gap}|(?!(?<=(?:^|[^A-Za-z_0-9_.])(?:[0-9]+(?:\\.[0-9]+)?|\\.[0-9]+))%)(?:{dropped})+{gap})"
    )
}

fn dropped_class() -> &'static str {
    "[^ \\t\\r\\n#\"@A-Za-z0-9_./{}*+=!<>~:,()\\[\\]-]"
}

fn hex_body() -> String {
    let mut lens = HEX_LENS.to_vec();
    lens.sort_by(|a, b| b.cmp(a));
    lens.iter()
        .map(|len| format!("[0-9a-fA-F]{{{len}}}"))
        .collect::<Vec<_>>()
        .join("|")
}

fn hex_re() -> String {
    format!("#(?:{})(?=$|{})", hex_body(), delim_class())
}

fn delim_class() -> String {
    let mut out = String::from("[");
    for c in HEX_DELIMS {
        match c {
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            ']' | '\\' | '^' | '-' => {
                out.push('\\');
                out.push(*c);
            }
            other => out.push(*other),
        }
    }
    out.push(']');
    out
}

fn escape_re() -> String {
    let mut alts = Vec::new();
    for (letter, _) in STRING_ESCAPES {
        alts.push(regex_escape(&letter.to_string()));
    }
    format!("\\\\(?:{})", alts.join("|"))
}

fn number_re() -> String {
    let angles = ANGLE_SUFFIXES
        .iter()
        .map(|(suffix, _)| regex_escape(suffix))
        .collect::<Vec<_>>()
        .join("|");
    let body = "(?:[0-9]+(?:\\.[0-9]+)?|(?<![A-Za-z0-9_)])\\.[0-9]+)";
    let part = "(?:\\.[0-9]+|[0-9]+(?:\\.[0-9]+)?)%?";
    format!(
        "{body}(?:(?:{angles}){after}|%(?:x{part})?|x{part})?",
        after = NOT_IDENT_AFTER
    )
}

fn regex_escape(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if matches!(
            c,
            '\\' | '.' | '+' | '*' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '^' | '$'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

pub(crate) fn to_json(patterns: &[Pattern]) -> String {
    let mut root = Map::new();
    root.insert("name".into(), Value::String("artscript".into()));
    root.insert("scopeName".into(), Value::String("source.art".into()));
    root.insert("fileTypes".into(), Value::Array(vec![json_str("art")]));
    root.insert(
        "patterns".into(),
        Value::Array(patterns.iter().map(pattern_value).collect()),
    );
    root.insert(
        "comment".into(),
        Value::String(
            "Generated from crates/syntax/src/highlight/grammar.rs. Bless with ARTSCRIPT_BLESS=1 cargo test -p artscript-syntax highlight"
                .into(),
        ),
    );
    serde_json::to_string_pretty(&Value::Object(root)).unwrap() + "\n"
}

pub(crate) fn scopes_json() -> String {
    let mut map = Map::new();
    for class in Class::ALL {
        if matches!(class, Class::Keyword | Class::Ident) {
            continue;
        }
        map.insert(scope(*class).into(), json_str(&class.letter().to_string()));
    }
    for (word, _, _) in words::rows() {
        let Some(group) = group(*word) else {
            continue;
        };
        map.insert(
            group.scope().into(),
            json_str(&Class::Keyword.letter().to_string()),
        );
    }
    map.insert(ESCAPE_SCOPE.into(), json_str("s"));
    serde_json::to_string_pretty(&Value::Object(map)).unwrap() + "\n"
}

fn pattern_value(pattern: &Pattern) -> Value {
    let mut map = Map::new();
    match pattern {
        Pattern::Match { re, name, captures } => {
            map.insert("match".into(), Value::String(re.clone()));
            if let Some(name) = name {
                map.insert("name".into(), Value::String((*name).into()));
            }
            if !captures.is_empty() {
                map.insert("captures".into(), captures_value(captures));
            }
        }
        Pattern::Region {
            begin,
            end,
            name,
            begin_captures,
            end_captures,
            inner,
        } => {
            map.insert("begin".into(), Value::String(begin.clone()));
            map.insert("end".into(), Value::String(end.clone()));
            if let Some(name) = name {
                map.insert("name".into(), Value::String((*name).into()));
            }
            if !begin_captures.is_empty() {
                map.insert("beginCaptures".into(), captures_value(begin_captures));
            }
            if !end_captures.is_empty() {
                map.insert("endCaptures".into(), captures_value(end_captures));
            }
            if !inner.is_empty() {
                map.insert(
                    "patterns".into(),
                    Value::Array(inner.iter().map(pattern_value).collect()),
                );
            }
        }
    }
    Value::Object(map)
}

fn captures_value(captures: &[(u8, &str)]) -> Value {
    let mut map = Map::new();
    for (group, name) in captures {
        let mut item = Map::new();
        item.insert("name".into(), Value::String((*name).into()));
        map.insert(group.to_string(), Value::Object(item));
    }
    Value::Object(map)
}

fn json_str(text: &str) -> Value {
    Value::String(text.into())
}
