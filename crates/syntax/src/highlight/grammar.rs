use serde_json::{Map, Value};

use super::{Class, PATH_CHARS};
use crate::lex::{ANGLE_SUFFIXES, HEX_DELIMS, HEX_LENS, Kw, STRING_ESCAPES};

const IDENT: &str = "[A-Za-z_][A-Za-z0-9_]*";
const NOT_IDENT_BEFORE: &str = "(?<![A-Za-z0-9_])";
const NOT_IDENT_AFTER: &str = "(?![A-Za-z0-9_])";
/// Spaces, then an optional tab-tainted indent in capture 1.
const HEAD: &str = "^[ ]*([ \\t]*\\t[ \\t]*)?";

const ESCAPE_SCOPE: &str = "constant.character.escape.art";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum KwGroup {
    Import,
    Declaration,
    Control,
    Logical,
    Constant,
}

/// Adding a keyword fails to compile here until it has a group.
pub(crate) fn group(kw: Kw) -> KwGroup {
    match kw {
        Kw::Use | Kw::As | Kw::From => KwGroup::Import,
        Kw::Palette
        | Kw::Style
        | Kw::Def
        | Kw::Rule
        | Kw::Let
        | Kw::Shader
        | Kw::Light
        | Kw::Brush => KwGroup::Declaration,
        Kw::Assert | Kw::Expect | Kw::If | Kw::Then | Kw::Else | Kw::Each | Kw::In => {
            KwGroup::Control
        }
        Kw::And | Kw::Or | Kw::Not => KwGroup::Logical,
        Kw::True | Kw::False | Kw::None => KwGroup::Constant,
    }
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
        name: &'static str,
        inner: Vec<Pattern>,
    },
}

pub(crate) fn emit() -> Vec<Pattern> {
    let invalid = scope(Class::Invalid);
    let mut patterns = vec![
        Pattern::Region {
            begin: "\"\"\"".into(),
            end: "\"\"\"".into(),
            name: scope(Class::String),
            inner: vec![],
        },
        Pattern::Region {
            begin: "\"".into(),
            end: "\"|$".into(),
            name: scope(Class::String),
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
        directive(),
        use_path(invalid),
    ];
    for kw_group in KwGroup::all() {
        patterns.push(keyword_head(*kw_group, invalid));
    }
    patterns.push(patch(invalid));
    patterns.push(slot(invalid));
    patterns.push(verb(invalid));
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

fn directive() -> Pattern {
    let path = path_class();
    Pattern::Match {
        re: format!(
            "^(@{ident})(?!\\.(?:[A-Za-z_0-9*]))(?:[ \\t]+({path}*[/.{{]{path}*))?",
            ident = IDENT
        ),
        name: None,
        captures: vec![(1, scope(Class::Directive)), (2, scope(Class::Path))],
    }
}

fn use_path(invalid: &'static str) -> Pattern {
    Pattern::Match {
        re: format!(
            "{head}(use){after}(?:[ \\t]+({path}+))?",
            head = HEAD,
            after = NOT_IDENT_AFTER,
            path = path_class()
        ),
        name: None,
        captures: vec![
            (1, invalid),
            (2, KwGroup::Import.scope()),
            (3, scope(Class::Path)),
        ],
    }
}

fn keyword_head(kw_group: KwGroup, invalid: &'static str) -> Pattern {
    Pattern::Match {
        re: format!(
            "{head}({alt}){after}",
            head = HEAD,
            alt = kw_alt(kw_group),
            after = NOT_IDENT_AFTER
        ),
        name: None,
        captures: vec![(1, invalid), (2, kw_group.scope())],
    }
}

fn patch(invalid: &'static str) -> Pattern {
    Pattern::Match {
        re: format!(
            "{head}(\\.{ident}(?:\\.{ident})*)",
            head = HEAD,
            ident = IDENT
        ),
        name: None,
        captures: vec![(1, invalid), (2, scope(Class::Patch))],
    }
}

fn slot(invalid: &'static str) -> Pattern {
    Pattern::Match {
        re: format!(
            "{head}({ident})(:)(?=[ \\t]*(?:\\r?$|#(?!{hex}(?=$|{delims}))))",
            head = HEAD,
            ident = IDENT,
            hex = hex_body(),
            delims = delim_class()
        ),
        name: None,
        captures: vec![
            (1, invalid),
            (2, scope(Class::Slot)),
            (3, scope(Class::Slot)),
        ],
    }
}

fn verb(invalid: &'static str) -> Pattern {
    Pattern::Match {
        re: format!("{head}({ident})", head = HEAD, ident = IDENT),
        name: None,
        captures: vec![(1, invalid), (2, scope(Class::Verb))],
    }
}

fn kw_alt(kw_group: KwGroup) -> String {
    let mut words: Vec<&str> = Kw::ALL
        .iter()
        .copied()
        .filter(|kw| group(*kw) == kw_group)
        .map(Kw::as_str)
        .collect();
    words.sort_by_key(|word| std::cmp::Reverse(word.len()));
    words.join("|")
}

fn path_class() -> String {
    format!("[{PATH_CHARS}]")
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
    for kw in Kw::ALL {
        map.insert(
            group(*kw).scope().into(),
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
            inner,
        } => {
            map.insert("begin".into(), Value::String(begin.clone()));
            map.insert("end".into(), Value::String(end.clone()));
            map.insert("name".into(), Value::String((*name).into()));
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
