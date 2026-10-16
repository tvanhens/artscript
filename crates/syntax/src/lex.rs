use crate::Diagnostic;
use crate::ast::Span;

#[derive(Clone, Debug)]
pub(crate) struct Comment {
    pub line: u32,
    pub text: String,
    pub trailing: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct Token {
    pub kind: Tok,
    pub span: Span,
    pub start: usize,
    pub end: usize,
    pub ws_before: bool,
}

#[derive(Clone, Debug)]
pub(crate) enum SizePart {
    Num(f64),
    Percent(f64),
}

#[derive(Clone, Debug)]
pub(crate) enum Tok {
    Ident(String),
    Kw(Kw),
    Num(f64),
    Percent(f64),
    Angle(f64),
    /// Seconds. `200ms` and `1.6s` are both this token.
    Time(f64),
    Size(SizePart, SizePart),
    Hex(String),
    Str(String),
    Ref(Vec<String>),
    Plus,
    Minus,
    Star,
    Slash,
    EqEq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
    Approx,
    Arrow,
    DotDot,
    Eq,
    Dot,
    Colon,
    Comma,
    LParen,
    RParen,
    LBrack,
    RBrack,
    LBrace,
    RBrace,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kw {
    Use,
    As,
    From,
    Palette,
    Style,
    Def,
    Rule,
    Let,
    Shader,
    Light,
    Brush,
    Assert,
    Expect,
    If,
    Then,
    Else,
    Each,
    In,
    And,
    Or,
    Not,
    True,
    False,
    None,
    Anim,
    Play,
}

impl Kw {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Kw::Use => "use",
            Kw::As => "as",
            Kw::From => "from",
            Kw::Palette => "palette",
            Kw::Style => "style",
            Kw::Def => "def",
            Kw::Rule => "rule",
            Kw::Let => "let",
            Kw::Shader => "shader",
            Kw::Light => "light",
            Kw::Brush => "brush",
            Kw::Assert => "assert",
            Kw::Expect => "expect",
            Kw::If => "if",
            Kw::Then => "then",
            Kw::Else => "else",
            Kw::Each => "each",
            Kw::In => "in",
            Kw::And => "and",
            Kw::Or => "or",
            Kw::Not => "not",
            Kw::True => "true",
            Kw::False => "false",
            Kw::None => "none",
            Kw::Anim => "anim",
            Kw::Play => "play",
        }
    }

    fn from_ident(s: &str) -> Option<Self> {
        Some(match s {
            "use" => Kw::Use,
            "as" => Kw::As,
            "from" => Kw::From,
            "palette" => Kw::Palette,
            "style" => Kw::Style,
            "def" => Kw::Def,
            "rule" => Kw::Rule,
            "let" => Kw::Let,
            "shader" => Kw::Shader,
            "light" => Kw::Light,
            "brush" => Kw::Brush,
            "assert" => Kw::Assert,
            "expect" => Kw::Expect,
            "if" => Kw::If,
            "then" => Kw::Then,
            "else" => Kw::Else,
            "each" => Kw::Each,
            "in" => Kw::In,
            "and" => Kw::And,
            "or" => Kw::Or,
            "not" => Kw::Not,
            "true" => Kw::True,
            "false" => Kw::False,
            "none" => Kw::None,
            "anim" => Kw::Anim,
            "play" => Kw::Play,
            _ => return None,
        })
    }
}

impl Tok {
    pub(crate) fn desc(&self) -> String {
        match self {
            Tok::Ident(s) => format!("name `{s}`"),
            Tok::Kw(k) => format!("keyword `{}`", k.as_str()),
            Tok::Num(n) => format!("number {n}"),
            Tok::Percent(_) => "percent".into(),
            Tok::Angle(_) => "angle".into(),
            Tok::Time(_) => "time".into(),
            Tok::Size(_, _) => "size".into(),
            Tok::Hex(h) => format!("color `#{h}`"),
            Tok::Str(_) => "string".into(),
            Tok::Ref(s) => format!("reference `@{}", s.join(".")),
            Tok::Plus => "`+`".into(),
            Tok::Minus => "`-`".into(),
            Tok::Star => "`*`".into(),
            Tok::Slash => "`/`".into(),
            Tok::EqEq => "`==`".into(),
            Tok::NotEq => "`!=`".into(),
            Tok::Lt => "`<`".into(),
            Tok::Le => "`<=`".into(),
            Tok::Gt => "`>`".into(),
            Tok::Ge => "`>=`".into(),
            Tok::Approx => "`~=`".into(),
            Tok::Arrow => "`->`".into(),
            Tok::DotDot => "`..`".into(),
            Tok::Eq => "`=`".into(),
            Tok::Dot => "`.`".into(),
            Tok::Colon => "`:`".into(),
            Tok::Comma => "`,`".into(),
            Tok::LParen => "`(`".into(),
            Tok::RParen => "`)`".into(),
            Tok::LBrack => "`[`".into(),
            Tok::RBrack => "`]`".into(),
            Tok::LBrace => "`{`".into(),
            Tok::RBrace => "`}`".into(),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Line {
    pub no: u32,
    pub indent: usize,
    pub tokens: Vec<Token>,
}

pub(crate) struct Lexed {
    pub lines: Vec<Line>,
    pub comments: Vec<Comment>,
    /// 1-based line numbers of lines that are empty or only whitespace.
    pub blanks: Vec<u32>,
    pub errors: Vec<Diagnostic>,
}

pub(crate) fn lex(src: &str) -> Lexed {
    Scanner::new(src).run()
}

struct Scanner<'a> {
    src: &'a str,
    i: usize,
    line: u32,
    col: u32,
    lines: Vec<Line>,
    comments: Vec<Comment>,
    blanks: Vec<u32>,
    errors: Vec<Diagnostic>,
}

impl<'a> Scanner<'a> {
    fn new(src: &'a str) -> Self {
        Self {
            src,
            i: 0,
            line: 1,
            col: 1,
            lines: Vec::new(),
            comments: Vec::new(),
            blanks: Vec::new(),
            errors: Vec::new(),
        }
    }

    fn run(mut self) -> Lexed {
        while self.i < self.src.len() {
            self.scan_physical_line();
        }
        Lexed {
            lines: self.lines,
            comments: self.comments,
            blanks: self.blanks,
            errors: self.errors,
        }
    }

    fn peek(&self) -> Option<char> {
        self.src[self.i..].chars().next()
    }

    fn peek_n(&self, n: usize) -> Option<char> {
        self.src[self.i..].chars().nth(n)
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.i += c.len_utf8();
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    fn span_here(&self) -> Span {
        Span {
            line: self.line,
            col: self.col,
        }
    }

    fn starts_with(&self, s: &str) -> bool {
        self.src[self.i..].starts_with(s)
    }

    fn scan_physical_line(&mut self) {
        if self.peek() == Some('\r') {
            self.bump();
        }
        if self.peek() == Some('\n') {
            self.blanks.push(self.line);
            self.bump();
            return;
        }
        let mut indent = 0usize;
        let mut tab_at: Option<Span> = None;
        loop {
            match self.peek() {
                Some(' ') => {
                    indent += 1;
                    self.bump();
                }
                Some('\t') => {
                    if tab_at.is_none() {
                        tab_at = Some(self.span_here());
                    }
                    self.bump();
                }
                _ => break,
            }
        }
        if let Some(span) = tab_at {
            self.errors.push(
                Diagnostic::error("E0102", span, "indentation contains a tab")
                    .with_fix("replace the tab with spaces"),
            );
        }
        match self.peek() {
            None => {}
            Some('\n') | Some('\r') => {
                self.blanks.push(self.line);
                if self.peek() == Some('\r') {
                    self.bump();
                }
                if self.peek() == Some('\n') {
                    self.bump();
                }
            }
            Some('#') if self.hex_color().is_none() => {
                self.scan_comment(false);
                self.consume_eol();
            }
            _ => {
                let no = self.line;
                let mut tokens = Vec::new();
                let mut ws = false;
                while let Some(c) = self.peek() {
                    if c == '\n' || c == '\r' {
                        break;
                    }
                    if c == ' ' || c == '\t' {
                        ws = true;
                        self.bump();
                        continue;
                    }
                    if c == '#' && self.hex_color().is_none() {
                        self.scan_comment(true);
                        break;
                    }
                    if let Some(tok) = self.scan_token(ws) {
                        tokens.push(tok);
                    }
                    ws = false;
                    if self
                        .errors
                        .iter()
                        .any(|e| e.code == "E0101" && e.message.contains("unterminated raw"))
                    {
                        break;
                    }
                }
                self.consume_eol();
                if !tokens.is_empty() {
                    self.lines.push(Line { no, indent, tokens });
                }
            }
        }
    }

    fn consume_eol(&mut self) {
        if self.peek() == Some('\r') {
            self.bump();
        }
        if self.peek() == Some('\n') {
            self.bump();
        }
    }

    fn scan_comment(&mut self, trailing: bool) {
        let line = self.line;
        self.bump();
        let mut text = String::new();
        while let Some(c) = self.peek() {
            if c == '\n' || c == '\r' {
                break;
            }
            text.push(c);
            self.bump();
        }
        self.comments.push(Comment {
            line,
            text: text.trim().to_string(),
            trailing,
        });
    }

    fn hex_color(&self) -> Option<String> {
        if self.peek() != Some('#') {
            return None;
        }
        let rest = &self.src[self.i + 1..];
        let hex: String = rest.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
        if !matches!(hex.len(), 3 | 4 | 6 | 8) {
            return None;
        }
        let after = rest[hex.len()..].chars().next();
        let delim = matches!(
            after,
            None | Some(' ' | '\t' | '\n' | '\r' | ',' | ')' | ']')
        );
        if delim {
            Some(hex.to_ascii_lowercase())
        } else {
            None
        }
    }

    fn scan_token(&mut self, ws_before: bool) -> Option<Token> {
        let span = self.span_here();
        let start = self.i;
        let kind = self.scan_kind()?;
        Some(Token {
            kind,
            span,
            start,
            end: self.i,
            ws_before,
        })
    }

    fn scan_kind(&mut self) -> Option<Tok> {
        let c = self.peek()?;
        if c == '#'
            && let Some(hex) = self.hex_color()
        {
            self.bump();
            for _ in 0..hex.len() {
                self.bump();
            }
            return Some(Tok::Hex(hex));
        }
        if c == '"' {
            return Some(self.scan_string());
        }
        if c == '@' {
            return Some(self.scan_ref());
        }
        if c == '.' {
            return Some(self.scan_dot());
        }
        if c.is_ascii_digit() {
            return Some(self.scan_number());
        }
        if is_ident_start(c) {
            return Some(self.scan_ident());
        }
        self.bump();
        let kind = match c {
            '+' => Tok::Plus,
            '-' => self.scan_minus(),
            '*' => Tok::Star,
            '/' => Tok::Slash,
            '=' => self.two('=', Tok::Eq, Tok::EqEq),
            '!' => self.two('=', Tok::NotEq, Tok::NotEq),
            '<' => self.two('=', Tok::Lt, Tok::Le),
            '>' => self.two('=', Tok::Gt, Tok::Ge),
            '~' => self.two('=', Tok::Approx, Tok::Approx),
            ':' => Tok::Colon,
            ',' => Tok::Comma,
            '(' => Tok::LParen,
            ')' => Tok::RParen,
            '[' => Tok::LBrack,
            ']' => Tok::RBrack,
            '{' => Tok::LBrace,
            '}' => Tok::RBrace,
            other => {
                self.errors.push(
                    Diagnostic::error(
                        "E0101",
                        Span {
                            line: self.line,
                            col: self.col.saturating_sub(1),
                        },
                        format!("unexpected character `{other}`"),
                    )
                    .with_fix(format!("remove `{other}`")),
                );
                return None;
            }
        };
        Some(kind)
    }

    fn two(&mut self, second: char, bare: Tok, both: Tok) -> Tok {
        if self.peek() == Some(second) {
            self.bump();
            both
        } else if matches!(bare, Tok::NotEq | Tok::Approx) {
            let col = self.col.saturating_sub(1);
            self.errors.push(
                Diagnostic::error(
                    "E0101",
                    Span {
                        line: self.line,
                        col,
                    },
                    format!(
                        "unexpected character `{}`",
                        if matches!(bare, Tok::NotEq) { '!' } else { '~' }
                    ),
                )
                .with_fix("write `!=` or `~=` , or remove the character"),
            );
            bare
        } else {
            bare
        }
    }

    fn scan_minus(&mut self) -> Tok {
        if self.peek() == Some('>') {
            self.bump();
            Tok::Arrow
        } else {
            Tok::Minus
        }
    }

    fn scan_dot(&mut self) -> Tok {
        let prev_member = self.prev_glues_member();
        if self.peek_n(1) == Some('.') {
            self.bump();
            self.bump();
            return Tok::DotDot;
        }
        if self.peek_n(1).is_some_and(|d| d.is_ascii_digit()) && !prev_member {
            return self.scan_number();
        }
        self.bump();
        Tok::Dot
    }

    fn prev_glues_member(&self) -> bool {
        if self.i == 0 {
            return false;
        }
        let prev = self.src[..self.i].chars().next_back();
        matches!(prev, Some(c) if is_ident_cont(c) || c == ')')
    }

    fn scan_ident(&mut self) -> Tok {
        let start = self.i;
        self.bump();
        while self.peek().is_some_and(is_ident_cont) {
            self.bump();
        }
        let word = &self.src[start..self.i];
        if let Some(kw) = Kw::from_ident(word) {
            Tok::Kw(kw)
        } else {
            Tok::Ident(word.to_string())
        }
    }

    fn scan_ref(&mut self) -> Tok {
        let at = self.span_here();
        self.bump();
        if !self.peek().is_some_and(is_ident_start) {
            self.errors.push(
                Diagnostic::error("E0101", at, "`@` must be followed by a name")
                    .with_fix("write `@name`, or remove `@`"),
            );
            return Tok::Ref(vec![]);
        }
        let mut segs = vec![self.scan_word()];
        while self.peek() == Some('.') {
            let n = self.peek_n(1);
            if n.is_some_and(is_ident_start)
                || n.is_some_and(|c| c.is_ascii_digit())
                || n == Some('*')
            {
                self.bump();
                if self.peek() == Some('*') {
                    self.bump();
                    segs.push("*".into());
                } else if self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    segs.push(self.scan_digits());
                } else {
                    segs.push(self.scan_word());
                }
            } else {
                break;
            }
        }
        Tok::Ref(segs)
    }

    fn scan_word(&mut self) -> String {
        let start = self.i;
        if self.peek().is_some_and(is_ident_start) {
            self.bump();
        }
        while self.peek().is_some_and(is_ident_cont) {
            self.bump();
        }
        self.src[start..self.i].to_string()
    }

    fn scan_digits(&mut self) -> String {
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.bump();
        }
        self.src[start..self.i].to_string()
    }

    fn scan_number(&mut self) -> Tok {
        let start = self.i;
        if self.peek() == Some('.') {
            self.bump();
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.bump();
            }
        } else {
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.bump();
            }
            if self.peek() == Some('.') && self.peek_n(1).is_some_and(|c| c.is_ascii_digit()) {
                self.bump();
                while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    self.bump();
                }
            }
        }
        let lit = &self.src[start..self.i];
        let value: f64 = lit.parse().unwrap_or(0.0);
        if self.consume_suffix("deg") {
            return Tok::Angle(value);
        }
        if self.consume_suffix("turn") {
            return Tok::Angle(value * 360.0);
        }
        if self.consume_suffix("ms") {
            return Tok::Time(value / 1000.0);
        }
        if self.consume_suffix("s") {
            return Tok::Time(value);
        }
        if self.peek() == Some('%') {
            self.bump();
            let pct = value / 100.0;
            if self.peek() == Some('x') && self.after_x_is_number() {
                self.bump();
                let second = self.scan_size_part();
                return Tok::Size(SizePart::Percent(pct), second);
            }
            return Tok::Percent(pct);
        }
        if self.peek() == Some('x') && self.after_x_is_number() {
            self.bump();
            let second = self.scan_size_part();
            return Tok::Size(SizePart::Num(value), second);
        }
        Tok::Num(value)
    }

    fn after_x_is_number(&self) -> bool {
        let rest = &self.src[self.i + 1..];
        let mut cs = rest.chars();
        match cs.next() {
            Some(c) if c.is_ascii_digit() => true,
            Some('.') => cs.next().is_some_and(|c| c.is_ascii_digit()),
            _ => false,
        }
    }

    fn scan_size_part(&mut self) -> SizePart {
        let start = self.i;
        if self.peek() == Some('.') {
            self.bump();
        }
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.bump();
        }
        if self.peek() == Some('.')
            && self.peek_n(1).is_some_and(|c| c.is_ascii_digit())
            && !self.src[start..].starts_with('.')
        {
            self.bump();
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.bump();
            }
        }
        let value: f64 = self.src[start..self.i].parse().unwrap_or(0.0);
        if self.peek() == Some('%') {
            self.bump();
            SizePart::Percent(value / 100.0)
        } else {
            SizePart::Num(value)
        }
    }

    fn consume_suffix(&mut self, suffix: &str) -> bool {
        if self.starts_with(suffix) {
            let after = self.src[self.i + suffix.len()..].chars().next();
            if after.is_some_and(is_ident_cont) {
                return false;
            }
            for _ in suffix.chars() {
                self.bump();
            }
            return true;
        }
        false
    }

    fn scan_string(&mut self) -> Tok {
        let span = self.span_here();
        if self.starts_with("\"\"\"") {
            self.bump();
            self.bump();
            self.bump();
            let start = self.i;
            if let Some(rel) = self.src[self.i..].find("\"\"\"") {
                let raw = &self.src[start..start + rel];
                self.i = start + rel;
                for c in raw.chars() {
                    if c == '\n' {
                        self.line += 1;
                        self.col = 1;
                    } else {
                        self.col += 1;
                    }
                }
                self.bump();
                self.bump();
                self.bump();
                Tok::Str(dedent_raw(raw))
            } else {
                self.errors.push(
                    Diagnostic::error("E0101", span, "unterminated raw string")
                        .with_fix("close the raw string with `\"\"\"`"),
                );
                self.i = self.src.len();
                Tok::Str(String::new())
            }
        } else {
            self.bump();
            let mut out = String::new();
            loop {
                match self.peek() {
                    None | Some('\n') | Some('\r') => {
                        self.errors.push(
                            Diagnostic::error("E0101", span, "unterminated string")
                                .with_fix("close the string with `\"`"),
                        );
                        break;
                    }
                    Some('"') => {
                        self.bump();
                        break;
                    }
                    Some('\\') => {
                        let esc_span = self.span_here();
                        self.bump();
                        match self.peek() {
                            Some('\\') => {
                                out.push('\\');
                                self.bump();
                            }
                            Some('"') => {
                                out.push('"');
                                self.bump();
                            }
                            Some('n') => {
                                out.push('\n');
                                self.bump();
                            }
                            Some(other) => {
                                self.errors.push(
                                    Diagnostic::error(
                                        "E0101",
                                        esc_span,
                                        format!("unknown string escape `\\{other}`"),
                                    )
                                    .with_fix("use `\\\\`, `\\\"`, or `\\n`"),
                                );
                                self.bump();
                            }
                            None => {
                                self.errors.push(
                                    Diagnostic::error(
                                        "E0101",
                                        esc_span,
                                        "unterminated string escape",
                                    )
                                    .with_fix("finish the escape or close the string"),
                                );
                            }
                        }
                    }
                    Some(c) => {
                        out.push(c);
                        self.bump();
                    }
                }
            }
            Tok::Str(out)
        }
    }
}

fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_ident_cont(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn dedent_raw(s: &str) -> String {
    let mut body = s;
    if let Some(rest) = body.strip_prefix('\n') {
        body = rest;
    }
    // Spaces between the last newline and the closing delimiter are the closer's indent.
    if let Some(nl) = body.rfind('\n')
        && body[nl + 1..].chars().all(|c| c == ' ')
    {
        body = &body[..nl];
    }
    if body.is_empty() {
        return String::new();
    }
    let lines: Vec<&str> = body.split('\n').collect();
    let min = lines
        .iter()
        .filter(|l| !l.is_empty())
        .map(|l| l.chars().take_while(|c| *c == ' ').count())
        .min()
        .unwrap_or(0);
    lines
        .iter()
        .map(|l| {
            if l.is_empty() {
                String::new()
            } else {
                l.chars().skip(min).collect()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
