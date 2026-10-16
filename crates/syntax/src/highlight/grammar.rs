use serde_json::{Map, Value};

use super::{Class, PATH_CHARS};
use crate::lex::{ANGLE_SUFFIXES, HEX_DELIMS, HEX_LENS, STRING_ESCAPES};
use crate::words::{self, Word};

const IDENT: &str = "[A-Za-z_][A-Za-z0-9_]*";
/// A digit may sit in front of a word. `5degx` is a number and then a name.
const NOT_IDENT_BEFORE: &str = "(?<![A-Za-z_])";
const NOT_IDENT_AFTER: &str = "(?![A-Za-z0-9_])";
const AT_STATEMENT: &str = "(?<=^[^#\"@A-Za-z0-9_./{}*+=!<>~:,()\\[\\]-]*)";
const AT_DIRECTIVE: &str = "(?<=^[^ #\\r\\n\"@A-Za-z0-9_./{}*+=!<>~:,()\\[\\]-]*)";

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
            inner: vec![],
        },
        Pattern::Region {
            begin: "\"".into(),
            end: "\"|$".into(),
            name: Some(scope(Class::String)),
            begin_captures: vec![],
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
        directive(),
        keyword_slot(),
        use_path(),
    ];
    for kw_group in KwGroup::all() {
        patterns.push(keyword_head(*kw_group));
    }
    patterns.push(patch());
    patterns.push(slot());
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
        re: "\\S".into(),
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

fn directive() -> Pattern {
    let path = path_class();
    Pattern::Match {
        re: format!(
            "{at}(@{ident})(?!\\.(?:[A-Za-z_0-9*]))(?:[ \\t]*({path}*[/.{{]{path}*){end})?",
            at = AT_DIRECTIVE,
            ident = IDENT,
            end = path_chunk_end()
        ),
        name: None,
        captures: vec![(1, scope(Class::Directive)), (2, scope(Class::Path))],
    }
}

fn use_path() -> Pattern {
    Pattern::Match {
        re: format!(
            "{at}(use){after}(?:[ \\t]*({path}+){end})?",
            at = AT_STATEMENT,
            after = NOT_IDENT_AFTER,
            path = path_class(),
            end = path_chunk_end()
        ),
        name: None,
        captures: vec![(1, KwGroup::Import.scope()), (2, scope(Class::Path))],
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
    let name = format!("{not_kw}({ident})", not_kw = not_keyword(), ident = IDENT);
    let segment = format!("(\\.)([ \\t]*){name}");
    let optional_non_keyword_name = format!(
        "(?:{not_kw}({ident}))?",
        not_kw = not_keyword(),
        ident = IDENT
    );
    Pattern::Region {
        begin: format!(
            "{at}(\\.)(?!\\.|[0-9])([ \\t]*){optional_non_keyword_name}",
            at = AT_STATEMENT,
            optional_non_keyword_name = optional_non_keyword_name
        ),
        end: format!("(?!\\.[ \\t]*{name})"),
        name: None,
        begin_captures: vec![(1, patch), (3, patch)],
        inner: vec![Pattern::Match {
            re: segment,
            name: None,
            captures: vec![(1, patch), (3, patch)],
        }],
    }
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

fn slot() -> Pattern {
    Pattern::Match {
        re: format!(
            "{at}({ident})(:)(?=[ \\t]*(?:\\r?$|#(?!(?:{hex})(?=$|{delims}))))",
            at = AT_STATEMENT,
            ident = IDENT,
            hex = hex_body(),
            delims = delim_class()
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
            "{at}({alts})(:)(?=[ \\t]*(?:\\r?$|#(?!(?:{hex})(?=$|{delims}))))",
            at = AT_STATEMENT,
            alts = keyword_alts(),
            hex = hex_body(),
            delims = delim_class()
        ),
        name: None,
        captures: vec![(1, scope(Class::Slot)), (2, scope(Class::Slot))],
    }
}

fn path_class() -> String {
    format!("[{PATH_CHARS}]")
}

fn path_chunk_end() -> String {
    let gap = "(?:$|[ \\t]|#)";
    let dropped = dropped_class();
    let color = format!(
        "#(?:{hex})(?=$|{delims})",
        hex = hex_body(),
        delims = delim_class()
    );
    format!("(?!(?:{dropped})*{color})(?={gap}|(?:{dropped})+{gap})")
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
