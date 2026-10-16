use std::collections::HashSet;

use crate::Diagnostic;
use crate::ast::{BinOp, CheckKind, Directive, Expr, File, Named, Span, Stmt, StmtKind, UnOp};
use crate::lex::{self, Kw, Line, SizePart, Tok};

const PREC_IF: u8 = 1;
const PREC_OR: u8 = 2;
const PREC_AND: u8 = 3;
const PREC_NOT: u8 = 4;
const PREC_CMP: u8 = 5;
const PREC_MAP: u8 = 6;
const PREC_RANGE: u8 = 7;
const PREC_ADD: u8 = 8;
const PREC_MUL: u8 = 9;
const PREC_UNARY: u8 = 10;

pub fn parse_file(src: &str) -> Result<File, Vec<Diagnostic>> {
    let lexed = lex::lex(src);
    let mut p = Parser {
        src,
        lines: lexed.lines,
        li: 0,
        ti: 0,
        line_failed: false,
        errors: lexed.errors,
        dedent_reported: HashSet::new(),
    };
    let file = p.parse_file();
    if p.errors.is_empty() {
        Ok(file)
    } else {
        p.errors.sort_by(|a, b| {
            (a.span.line, a.span.col, a.code.clone()).cmp(&(
                b.span.line,
                b.span.col,
                b.code.clone(),
            ))
        });
        p.errors.dedup();
        Err(p.errors)
    }
}

struct Parser<'a> {
    src: &'a str,
    lines: Vec<Line>,
    li: usize,
    ti: usize,
    line_failed: bool,
    errors: Vec<Diagnostic>,
    dedent_reported: HashSet<u32>,
}

impl<'a> Parser<'a> {
    fn parse_file(&mut self) -> File {
        let mut directives = Vec::new();
        let mut stmts = Vec::new();
        let mut seen_stmt = false;
        while self.li < self.lines.len() {
            self.ti = 0;
            self.line_failed = false;
            let ind = self.lines[self.li].indent;
            if ind != 0 {
                self.report_dedent(self.li, 0, 0);
                self.lines[self.li].indent = 0;
            }
            if self.starts_directive() {
                if seen_stmt {
                    let span = self.cur_span();
                    self.errors.push(
                        Diagnostic::error(
                            "E0101",
                            span,
                            "a directive must come before other statements",
                        )
                        .with_fix("move this `@` line above the first statement"),
                    );
                }
                directives.push(self.parse_directive());
            } else {
                seen_stmt = true;
                stmts.push(self.parse_stmt());
            }
            self.li += 1;
        }
        File { directives, stmts }
    }

    fn starts_directive(&self) -> bool {
        matches!(self.cur().map(|t| &t.kind), Some(Tok::Ref(segs)) if segs.len() == 1 && !segs[0].is_empty())
    }

    fn parse_directive(&mut self) -> Directive {
        let span = self.cur_span();
        let indent = self.cur_indent();
        let name = match self.bump().kind {
            Tok::Ref(segs) => segs.into_iter().next().unwrap_or_default(),
            _ => String::new(),
        };
        let mut path = None;
        let mut args = Vec::new();
        let mut named = Vec::new();
        let mut each = Vec::new();
        if self.looks_like_path() {
            path = self.take_glued_chunk();
        }
        while !self.eol() {
            if self.eat_kw(Kw::Each) {
                while !self.eol() {
                    if self.is_named() {
                        if !each.is_empty() && !self.cur().is_some_and(|t| t.ws_before) {
                            self.fail_here(
                                "expected whitespace between `each` axes",
                                "separate axes with a space",
                            );
                            break;
                        }
                        each.push(self.parse_named());
                    } else {
                        self.fail_here(
                            "expected a named axis after `each`",
                            "write `each name:[a, b]`",
                        );
                        break;
                    }
                }
                break;
            }
            self.push_item(&mut args, &mut named);
        }
        self.no_block(indent);
        Directive {
            name,
            path,
            args,
            named,
            each,
            span,
        }
    }

    fn looks_like_path(&self) -> bool {
        let Some(chunk) = self.peek_glued_source() else {
            return false;
        };
        chunk.contains('/') || chunk.contains('.') || chunk.contains('{')
    }

    fn peek_glued_source(&self) -> Option<String> {
        let first = self.cur()?;
        if self.line_failed {
            return None;
        }
        let start = first.start;
        let mut end = first.end;
        let mut i = self.ti + 1;
        let tokens = &self.lines[self.li].tokens;
        while i < tokens.len() && !tokens[i].ws_before {
            end = tokens[i].end;
            i += 1;
        }
        Some(self.src[start..end].to_string())
    }

    fn take_glued_chunk(&mut self) -> Option<String> {
        let text = self.peek_glued_source()?;
        let tokens = &self.lines[self.li].tokens;
        self.ti += 1;
        while self.ti < tokens.len() && !self.lines[self.li].tokens[self.ti].ws_before {
            self.ti += 1;
        }
        Some(text)
    }

    fn parse_stmt(&mut self) -> Stmt {
        let Some(kind) = self.cur().map(|t| t.kind.clone()) else {
            let span = Span {
                line: self.cur_line_no(),
                col: 1,
            };
            return self.dummy(span);
        };
        match kind {
            Tok::Kw(Kw::Use) => self.parse_use(),
            Tok::Kw(Kw::Palette) => self.parse_palette(),
            Tok::Kw(Kw::Style) => self.parse_style(),
            Tok::Kw(Kw::Def) => self.parse_def(),
            Tok::Kw(Kw::Rule) => self.parse_rule(),
            Tok::Kw(Kw::Let) => self.parse_let(),
            Tok::Kw(Kw::Shader) => self.parse_shader(),
            Tok::Kw(Kw::Light) => self.parse_light(),
            Tok::Kw(Kw::Brush) => self.parse_brush(),
            Tok::Kw(Kw::Assert) => self.parse_check(CheckKind::Assert),
            Tok::Kw(Kw::Expect) => self.parse_check(CheckKind::Expect),
            Tok::Kw(Kw::If) => self.parse_if(),
            Tok::Kw(Kw::Each) => self.parse_each(),
            Tok::Kw(Kw::Else) => {
                let span = self.cur_span();
                self.fail(
                    span,
                    "`else` must directly follow an `if` block at the same indent",
                    "place `else` on the line after an `if` block, or remove it",
                );
                self.dummy(span)
            }
            Tok::Dot => self.parse_patch(),
            Tok::Ident(name) => {
                if self.is_slot() {
                    self.parse_slot(name)
                } else {
                    self.parse_node(name)
                }
            }
            _ => {
                let span = self.cur_span();
                let desc = kind.desc();
                self.fail(
                    span,
                    format!("expected a statement, found {desc}"),
                    "start the line with a verb, declaration, or directive",
                );
                self.dummy(span)
            }
        }
    }

    fn parse_use(&mut self) -> Stmt {
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.bump();
        if self.eol() {
            self.fail(
                span,
                "expected a path after `use`",
                "write `use path/to/lib`",
            );
            return self.dummy(span);
        }
        let path = self.take_glued_chunk().unwrap_or_default();
        let mut alias = None;
        let mut items = Vec::new();
        if self.eat_kw(Kw::As) {
            alias = Some(self.expect_name());
        }
        if self.eat(&Tok::LBrace) {
            while !self.eol() && !self.eat(&Tok::RBrace) {
                if self.at(&Tok::Comma) {
                    self.fail_here(
                        "imported names are separated by spaces, not commas",
                        "remove the comma: write `use lib { a b }`",
                    );
                    break;
                }
                items.push(self.expect_name());
            }
        }
        self.expect_eol("remove the extra tokens after `use`");
        self.no_block(indent);
        Stmt {
            kind: StmtKind::Use { path, alias, items },
            span,
        }
    }

    fn parse_palette(&mut self) -> Stmt {
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.bump();
        let name = self.expect_name();
        let from = if self.eat_kw(Kw::From) {
            Some(self.expect_name())
        } else {
            None
        };
        self.expect_eol("remove the extra tokens after the palette name");
        let body = self.parse_block(indent, span.line);
        Stmt {
            kind: StmtKind::Palette { name, from, body },
            span,
        }
    }

    fn parse_style(&mut self) -> Stmt {
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.bump();
        let name = self.expect_name();
        let (args, named) = self.parse_items(false);
        if !args.is_empty() {
            self.fail(
                span,
                "a style takes named arguments only",
                "write `style name key:value`",
            );
        }
        self.no_block(indent);
        Stmt {
            kind: StmtKind::Style { name, named },
            span,
        }
    }

    fn parse_def(&mut self) -> Stmt {
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.bump();
        let name = self.expect_name();
        let (args, params) = self.parse_items(false);
        if !args.is_empty() {
            self.fail(
                span,
                "def parameters are named arguments",
                "write `def name key:value`",
            );
        }
        let body = self.parse_block(indent, span.line);
        Stmt {
            kind: StmtKind::Def { name, params, body },
            span,
        }
    }

    fn parse_rule(&mut self) -> Stmt {
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.bump();
        let name = self.expect_name();
        let (args, params) = self.parse_items(false);
        if !args.is_empty() {
            self.fail(
                span,
                "rule parameters are named arguments",
                "write `rule name key:value`",
            );
        }
        let body = self.parse_block(indent, span.line);
        Stmt {
            kind: StmtKind::Rule { name, params, body },
            span,
        }
    }

    fn parse_let(&mut self) -> Stmt {
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.bump();
        let name = self.expect_name();
        if !self.eat(&Tok::Eq) {
            self.fail(
                span,
                "expected `=` after the name",
                "write `let name = <expr>`",
            );
            self.no_block(indent);
            return Stmt {
                kind: StmtKind::Let {
                    name,
                    value: Expr::Ident(String::new()),
                },
                span,
            };
        }
        let value = self.parse_tuple();
        self.expect_eol("a `let` binding is one expression");
        self.no_block(indent);
        Stmt {
            kind: StmtKind::Let { name, value },
            span,
        }
    }

    fn parse_shader(&mut self) -> Stmt {
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.bump();
        let name = self.expect_name();
        let (args, params) = self.parse_items(false);
        if !args.is_empty() {
            self.fail(
                span,
                "shader parameters are named arguments",
                "write `shader name key:value`",
            );
        }
        let mut sksl = None;
        let mut child_indent: Option<usize> = None;
        loop {
            let next = self.li + 1;
            if next >= self.lines.len() {
                break;
            }
            let ind = self.lines[next].indent;
            if ind <= indent {
                break;
            }
            match child_indent {
                None => child_indent = Some(ind),
                Some(ci) if ind != ci => {
                    self.report_dedent(next, indent, span.line);
                    self.lines[next].indent = indent;
                    break;
                }
                _ => {}
            }
            self.enter(next);
            if self.eat_ident("sksl") {
                if let Some(Tok::Str(s)) = self.cur().map(|t| t.kind.clone()) {
                    self.bump();
                    if sksl.is_some() {
                        self.fail_here(
                            "a shader has one `sksl` block",
                            "keep a single `sksl \"\"\"...\"\"\"` line",
                        );
                    } else {
                        sksl = Some(s);
                    }
                } else {
                    self.fail_here(
                        "expected a raw string after `sksl`",
                        "write `sksl \"\"\"...\"\"\"`",
                    );
                }
            } else {
                self.fail_here(
                    "expected `sksl` under the shader",
                    "indent `sksl \"\"\"...\"\"\"` under the shader",
                );
            }
            self.expect_eol("remove the extra tokens after `sksl`");
        }
        Stmt {
            kind: StmtKind::Shader { name, params, sksl },
            span,
        }
    }

    fn parse_light(&mut self) -> Stmt {
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.bump();
        let name = self.expect_name();
        let (args, named) = self.parse_items(false);
        if !args.is_empty() {
            self.fail(
                span,
                "a light takes named arguments only",
                "write `light name key:value`",
            );
        }
        self.no_block(indent);
        Stmt {
            kind: StmtKind::Light { name, named },
            span,
        }
    }

    fn parse_brush(&mut self) -> Stmt {
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.bump();
        let name = self.expect_name();
        let (args, named) = self.parse_items(false);
        if !args.is_empty() {
            self.fail(
                span,
                "a brush takes named arguments only",
                "write `brush name key:value`",
            );
        }
        self.no_block(indent);
        Stmt {
            kind: StmtKind::Brush { name, named },
            span,
        }
    }

    fn parse_check(&mut self, kind: CheckKind) -> Stmt {
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.bump();
        let expr = if self.eol() {
            self.fail(
                span,
                "expected an expression to check",
                "write `assert <expr>`",
            );
            Expr::Ident(String::new())
        } else {
            self.parse_tuple()
        };
        let mut named = Vec::new();
        while self.is_named() {
            named.push(self.parse_named());
        }
        let message = if self
            .cur()
            .is_some_and(|t| matches!(t.kind, Tok::Str(_)) && t.ws_before)
        {
            match self.bump().kind {
                Tok::Str(s) => Some(s),
                _ => None,
            }
        } else {
            None
        };
        self.expect_eol("remove the extra tokens after the check");
        self.no_block(indent);
        Stmt {
            kind: StmtKind::Check {
                kind,
                expr,
                named,
                message,
            },
            span,
        }
    }

    fn parse_if(&mut self) -> Stmt {
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.bump();
        self.finish_if(span, indent)
    }

    fn finish_if(&mut self, span: Span, indent: usize) -> Stmt {
        let cond = if self.eol() {
            self.fail(span, "expected a condition after `if`", "write `if <expr>`");
            Expr::Ident(String::new())
        } else {
            self.parse_tuple()
        };
        self.expect_eol("the `if` condition must fit on one line");
        let then = self.parse_block(indent, span.line);
        let otherwise = if self.next_is_kw(indent, Kw::Else) {
            self.enter(self.li + 1);
            self.parse_else(indent)
        } else {
            Vec::new()
        };
        Stmt {
            kind: StmtKind::If {
                cond,
                then,
                otherwise,
            },
            span,
        }
    }

    fn parse_else(&mut self, indent: usize) -> Vec<Stmt> {
        let else_line = self.cur_line_no();
        self.bump();
        if self.at_kw(Kw::If) {
            let span = self.cur_span();
            self.bump();
            vec![self.finish_if(span, indent)]
        } else {
            self.expect_eol("`else` takes a block, or write `else if`");
            self.parse_block(indent, else_line)
        }
    }

    fn parse_each(&mut self) -> Stmt {
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.bump();
        let var = self.expect_name();
        if !self.eat_kw(Kw::In) {
            self.fail(
                span,
                "expected `in` after the loop name",
                "write `each name in <expr>`",
            );
        }
        let iter = if self.eol() {
            self.fail(
                span,
                "expected the list or range after `in`",
                "write `each name in <expr>`",
            );
            Expr::Ident(String::new())
        } else {
            self.parse_tuple()
        };
        self.expect_eol("the `each` header must fit on one line");
        let body = self.parse_block(indent, span.line);
        Stmt {
            kind: StmtKind::Each { var, iter, body },
            span,
        }
    }

    fn parse_patch(&mut self) -> Stmt {
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.bump();
        let mut path = vec![self.expect_name()];
        while self
            .cur()
            .is_some_and(|t| matches!(t.kind, Tok::Dot) && !t.ws_before)
        {
            self.bump();
            path.push(self.expect_name());
        }
        let mut named = Vec::new();
        let mut off = false;
        while !self.eol() {
            if self.is_named() {
                named.push(self.parse_named());
            } else if self
                .cur()
                .is_some_and(|t| matches!(&t.kind, Tok::Ident(s) if s == "off"))
            {
                off = true;
                self.bump();
            } else {
                self.fail_here(
                    "expected a named argument or `off`",
                    "write `key:value` or a trailing `off`",
                );
                break;
            }
        }
        self.no_block(indent);
        Stmt {
            kind: StmtKind::Patch { path, named, off },
            span,
        }
    }

    fn parse_slot(&mut self, name: String) -> Stmt {
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.reject_reserved(&name, span);
        self.bump();
        self.bump();
        self.expect_eol("a slot fill is `name:` with an indented block");
        let body = self.parse_block(indent, span.line);
        if body.is_empty() && !self.line_failed {
            self.errors.push(
                Diagnostic::error(
                    "E0101",
                    span,
                    format!("slot `{name}:` needs an indented block"),
                )
                .with_fix(format!("indent the slot body under `{name}:`")),
            );
        }
        Stmt {
            kind: StmtKind::SlotFill { name, body },
            span,
        }
    }

    fn is_slot(&self) -> bool {
        let Some(colon) = self
            .lines
            .get(self.li)
            .and_then(|l| l.tokens.get(self.ti + 1))
        else {
            return false;
        };
        matches!(colon.kind, Tok::Colon)
            && !colon.ws_before
            && self.ti + 2 >= self.lines[self.li].tokens.len()
    }

    fn parse_node(&mut self, verb: String) -> Stmt {
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.reject_reserved(&verb, span);
        self.bump();
        let (args, named) = self.parse_items(true);
        let body = self.parse_block(indent, span.line);
        Stmt {
            kind: StmtKind::Node {
                verb,
                args,
                named,
                body,
            },
            span,
        }
    }

    fn parse_items(&mut self, colon_value: bool) -> (Vec<Expr>, Vec<Named>) {
        let mut args = Vec::new();
        let mut named = Vec::new();
        if colon_value
            && self
                .cur()
                .is_some_and(|t| matches!(t.kind, Tok::Colon) && !t.ws_before)
        {
            self.bump();
            if self.eol() || self.cur().is_some_and(|t| t.ws_before) {
                self.fail_here(
                    "expected a value glued to `:`",
                    "remove the space after `:`, or end the line to open a slot",
                );
            } else {
                args.push(self.parse_tuple());
            }
        }
        while !self.eol() {
            self.push_item(&mut args, &mut named);
        }
        (args, named)
    }

    fn push_item(&mut self, args: &mut Vec<Expr>, named: &mut Vec<Named>) {
        if (!args.is_empty() || !named.is_empty()) && !self.cur().is_some_and(|t| t.ws_before) {
            self.fail_here(
                "expected whitespace between items",
                "separate items with a single space",
            );
            return;
        }
        if self.is_loose_comma() {
            self.fail_here(
                "a comma here is not a tuple",
                "remove the spaces around `,` to make a tuple, or drop the comma",
            );
            return;
        }
        if self.is_named() {
            named.push(self.parse_named());
        } else {
            args.push(self.parse_tuple());
        }
    }

    fn is_named(&self) -> bool {
        let Some(tok) = self.cur() else { return false };
        if !is_key(&tok.kind) {
            return false;
        }
        let Some(colon) = self.lines[self.li].tokens.get(self.ti + 1) else {
            return false;
        };
        matches!(colon.kind, Tok::Colon) && !colon.ws_before
    }

    fn parse_named(&mut self) -> Named {
        let span = self.cur_span();
        let key = self.take_key();
        if !self.eat(&Tok::Colon) {
            self.fail(
                span,
                format!("expected `:` after `{key}`"),
                format!("write `{key}:value`"),
            );
            return Named {
                key,
                value: Expr::Ident(String::new()),
                span,
            };
        }
        if self.eol() || self.cur().is_some_and(|t| t.ws_before) {
            self.fail(
                span,
                format!("expected a value glued to `{key}:`"),
                format!("remove the space after `{key}:`"),
            );
            return Named {
                key,
                value: Expr::Ident(String::new()),
                span,
            };
        }
        let value = self.parse_tuple();
        Named { key, value, span }
    }

    fn take_key(&mut self) -> String {
        match self.bump().kind {
            Tok::Ident(s) => s,
            Tok::Kw(k) => k.as_str().to_string(),
            _ => String::new(),
        }
    }

    fn parse_block(&mut self, parent_indent: usize, parent_line: u32) -> Vec<Stmt> {
        let mut body = Vec::new();
        let mut child_indent: Option<usize> = None;
        loop {
            let next = self.li + 1;
            if next >= self.lines.len() {
                break;
            }
            let ind = self.lines[next].indent;
            if ind <= parent_indent {
                break;
            }
            match child_indent {
                None => child_indent = Some(ind),
                Some(ci) if ind == ci => {}
                Some(ci) if ind > ci => {
                    self.enter(next);
                    let span = self.cur_span();
                    self.fail(
                        span,
                        "this line is indented under a statement that does not take a block",
                        "dedent it to the enclosing block, or indent it under a shape, def, or if",
                    );
                    continue;
                }
                Some(_) => {
                    self.report_dedent(next, parent_indent, parent_line);
                    self.lines[next].indent = parent_indent;
                    break;
                }
            }
            self.enter(next);
            body.push(self.parse_stmt());
        }
        body
    }

    fn no_block(&mut self, indent: usize) {
        loop {
            let next = self.li + 1;
            if next >= self.lines.len() || self.lines[next].indent <= indent {
                break;
            }
            self.enter(next);
            let span = self.cur_span();
            self.fail(
                span,
                "this statement cannot have an indented block",
                "dedent the following lines",
            );
            self.ti = self.lines[self.li].tokens.len();
        }
    }

    fn parse_tuple(&mut self) -> Expr {
        let first = self.parse_prec(0);
        if !self.is_tight_comma() {
            return first;
        }
        let mut xs = vec![first];
        while self.is_tight_comma() {
            self.bump();
            if self.eol() {
                self.fail_here(
                    "expected a value after `,`",
                    "write the next tuple element, or remove `,`",
                );
                break;
            }
            xs.push(self.parse_prec(0));
        }
        Expr::Tuple(xs)
    }

    fn is_tight_comma(&self) -> bool {
        let Some(c) = self.cur() else { return false };
        if !matches!(c.kind, Tok::Comma) || c.ws_before {
            return false;
        }
        self.lines[self.li]
            .tokens
            .get(self.ti + 1)
            .is_some_and(|n| !n.ws_before)
    }

    fn is_loose_comma(&self) -> bool {
        let Some(c) = self.cur() else { return false };
        if !matches!(c.kind, Tok::Comma) {
            return false;
        }
        if c.ws_before {
            return true;
        }
        self.lines[self.li]
            .tokens
            .get(self.ti + 1)
            .is_none_or(|n| n.ws_before)
    }

    fn parse_prec(&mut self, min: u8) -> Expr {
        let mut left = self.parse_prefix(min);
        loop {
            if PREC_RANGE >= min
                && self.cur().is_some_and(|t| matches!(t.kind, Tok::DotDot))
                && self.binop_spacing()
            {
                self.bump();
                let right = self.parse_prec(PREC_RANGE + 1);
                left = Expr::Range(Box::new(left), Box::new(right));
                continue;
            }
            let Some((op, prec)) = self.peek_binop(min) else {
                break;
            };
            self.bump();
            let right = self.parse_prec(prec + 1);
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        left
    }

    fn parse_prefix(&mut self, min: u8) -> Expr {
        if min <= PREC_IF && self.at_kw(Kw::If) {
            self.bump();
            return self.parse_if_expr();
        }
        if min <= PREC_NOT && self.at_kw(Kw::Not) {
            self.bump();
            let operand = self.parse_prec(PREC_NOT + 1);
            return Expr::Unary(UnOp::Not, Box::new(operand));
        }
        if min <= PREC_UNARY && self.cur().is_some_and(|t| matches!(t.kind, Tok::Minus)) {
            self.bump();
            let operand = self.parse_prec(PREC_UNARY + 1);
            return Expr::Unary(UnOp::Neg, Box::new(operand));
        }
        self.parse_postfix()
    }

    fn parse_if_expr(&mut self) -> Expr {
        let cond = self.parse_prec(PREC_IF + 1);
        if !self.eat_kw(Kw::Then) {
            self.fail_here(
                "expected `then`",
                "write `if <cond> then <expr> else <expr>`",
            );
            return cond;
        }
        let then_e = self.parse_prec(PREC_IF);
        if !self.eat_kw(Kw::Else) {
            self.fail_here("expected `else`", "add `else <expr>`");
            return then_e;
        }
        let otherwise = self.parse_prec(PREC_IF);
        Expr::If {
            cond: Box::new(cond),
            then: Box::new(then_e),
            otherwise: Box::new(otherwise),
        }
    }

    fn peek_binop(&self, min: u8) -> Option<(BinOp, u8)> {
        let tok = self.cur()?;
        let (op, prec) = binop_of(&tok.kind)?;
        if prec < min || !self.binop_spacing() {
            return None;
        }
        Some((op, prec))
    }

    /// Symmetric spacing keeps one expression. A minus with space on only one side starts a new item.
    fn binop_spacing(&self) -> bool {
        let Some(tok) = self.cur() else { return false };
        let after = self.lines[self.li]
            .tokens
            .get(self.ti + 1)
            .is_some_and(|t| t.ws_before);
        tok.ws_before == after
    }

    fn parse_postfix(&mut self) -> Expr {
        let mut expr = self.parse_primary();
        loop {
            if self
                .cur()
                .is_some_and(|t| matches!(t.kind, Tok::Dot) && !t.ws_before)
            {
                expr = self.parse_member(expr);
                continue;
            }
            if self
                .cur()
                .is_some_and(|t| matches!(t.kind, Tok::LParen) && !t.ws_before)
            {
                expr = self.parse_call(expr);
                continue;
            }
            break;
        }
        expr
    }

    fn parse_member(&mut self, base: Expr) -> Expr {
        self.bump();
        let seg = if self.cur().is_some_and(|t| t.ws_before) {
            self.fail_here(
                "member access must be glued to `.`",
                "remove the space after `.`",
            );
            return base;
        } else if let Some(Tok::Ident(s)) = self.cur().map(|t| t.kind.clone()) {
            self.bump();
            s
        } else if let Some(tok) = self.cur() {
            if matches!(tok.kind, Tok::Num(_)) {
                let text = self.src[tok.start..tok.end].to_string();
                self.bump();
                text
            } else if matches!(tok.kind, Tok::Star) {
                self.bump();
                "*".into()
            } else if matches!(tok.kind, Tok::LBrace) {
                self.bump();
                let name = match self.cur().map(|t| t.kind.clone()) {
                    Some(Tok::Ident(s)) => {
                        self.bump();
                        s
                    }
                    _ => {
                        self.fail_here("expected a name inside `{}`", "write `.{name}`");
                        String::new()
                    }
                };
                if !self.eat(&Tok::RBrace) {
                    self.fail_here("expected `}` after the name", "close the segment with `}`");
                }
                format!("{{{name}}}")
            } else {
                self.fail_here(
                    "expected a member name after `.`",
                    "write a name, number, or `*` after `.`",
                );
                return base;
            }
        } else {
            self.fail_here("expected a member name after `.`", "write a name after `.`");
            return base;
        };
        Expr::Member(Box::new(base), seg)
    }

    fn parse_call(&mut self, func: Expr) -> Expr {
        let Expr::Ident(name) = func else {
            self.fail_here(
                "only a name can be called",
                "call a name, or add a space before `(` to group",
            );
            self.bump();
            self.skip_until(&Tok::RParen);
            return func;
        };
        self.bump();
        let (args, named) = self.parse_call_args();
        Expr::Call {
            func: name,
            args,
            named,
        }
    }

    fn parse_call_args(&mut self) -> (Vec<Expr>, Vec<Named>) {
        let mut args = Vec::new();
        let mut named = Vec::new();
        if self.eat(&Tok::RParen) {
            return (args, named);
        }
        loop {
            if self.eol() {
                self.fail_here("unclosed `(`", "close the call with `)`");
                break;
            }
            if self.is_named() {
                named.push(self.parse_named());
            } else {
                args.push(self.parse_tuple());
            }
            if self.eat(&Tok::RParen) {
                break;
            }
            if self.is_arg_comma() {
                self.fail_here(
                    "arguments are separated by spaces, not commas",
                    "remove the comma: write `f(a b)`, not `f(a, b)`",
                );
                break;
            }
            if self.cur().is_some_and(|t| t.ws_before) {
                continue;
            }
            self.fail_here(
                "expected `)` or a space between arguments",
                "separate arguments with a space",
            );
            break;
        }
        (args, named)
    }

    fn is_arg_comma(&self) -> bool {
        let Some(c) = self.cur() else { return false };
        if !matches!(c.kind, Tok::Comma) {
            return false;
        }
        c.ws_before
            || self.lines[self.li]
                .tokens
                .get(self.ti + 1)
                .is_none_or(|n| n.ws_before)
    }

    fn parse_primary(&mut self) -> Expr {
        let Some(tok) = self.cur().cloned() else {
            self.fail_here("expected an expression", "write a value");
            return Expr::Ident(String::new());
        };
        match tok.kind {
            Tok::Num(n) => {
                self.bump();
                Expr::Num(n)
            }
            Tok::Percent(n) => {
                self.bump();
                Expr::Percent(n)
            }
            Tok::Angle(n) => {
                self.bump();
                Expr::Angle(n)
            }
            Tok::Size(a, b) => {
                self.bump();
                Expr::Size(Box::new(size_part(a)), Box::new(size_part(b)))
            }
            Tok::Hex(h) => {
                self.bump();
                Expr::Hex(h)
            }
            Tok::Str(s) => {
                self.bump();
                Expr::Str(s)
            }
            Tok::Ident(s) => {
                self.bump();
                self.reject_reserved(&s, tok.span);
                Expr::Ident(s)
            }
            Tok::Kw(Kw::True) => {
                self.bump();
                Expr::Ident("true".into())
            }
            Tok::Kw(Kw::False) => {
                self.bump();
                Expr::Ident("false".into())
            }
            Tok::Kw(Kw::None) => {
                self.bump();
                Expr::Ident("none".into())
            }
            Tok::Ref(segs) => {
                self.bump();
                if segs.is_empty() {
                    Expr::Ident(String::new())
                } else {
                    Expr::Ref(segs)
                }
            }
            Tok::LBrack => self.parse_list(),
            Tok::LParen => self.parse_group(),
            other => {
                let desc = other.desc();
                self.fail(
                    tok.span,
                    format!("expected an expression, found {desc}"),
                    "write a value here",
                );
                Expr::Ident(String::new())
            }
        }
    }

    fn parse_list(&mut self) -> Expr {
        self.bump();
        let mut items = Vec::new();
        if self.eat(&Tok::RBrack) {
            return Expr::List(items);
        }
        loop {
            if self.eol() {
                self.fail_here("unclosed `[`", "close the list with `]`");
                break;
            }
            items.push(self.parse_tuple());
            if self.eat(&Tok::RBrack) {
                break;
            }
            if self.at(&Tok::Comma) {
                self.fail_here(
                    "list items are separated by spaces, not commas",
                    "remove the comma: write `[a b]`, not `[a, b]`",
                );
                break;
            }
            if self.cur().is_some_and(|t| t.ws_before) {
                continue;
            }
            self.fail_here(
                "expected `]` or a space in the list",
                "separate list items with a space",
            );
            break;
        }
        Expr::List(items)
    }

    fn parse_group(&mut self) -> Expr {
        self.bump();
        let expr = self.parse_tuple();
        if !self.eat(&Tok::RParen) {
            self.fail_here("unclosed `(`", "close the group with `)`");
        }
        expr
    }

    fn skip_until(&mut self, kind: &Tok) {
        while !self.eol() && !self.same_kind(kind) {
            self.ti += 1;
        }
        if self.same_kind(kind) {
            self.ti += 1;
        }
    }

    fn expect_name(&mut self) -> String {
        if self.eol() {
            self.fail_here("expected a name", "write a name");
            return String::new();
        }
        let span = self.cur_span();
        match self.cur().map(|t| t.kind.clone()) {
            Some(Tok::Ident(s)) => {
                self.bump();
                self.reject_reserved(&s, span);
                s
            }
            Some(Tok::Kw(k)) => {
                self.bump();
                let name = k.as_str();
                if crate::RESERVED.contains(&name) {
                    self.reject_reserved(name, span);
                    name.to_string()
                } else {
                    self.fail(
                        span,
                        format!("expected a name, found keyword `{name}`"),
                        format!("choose a name other than `{name}`"),
                    );
                    String::new()
                }
            }
            Some(other) => {
                let desc = other.desc();
                self.fail(
                    span,
                    format!("expected a name, found {desc}"),
                    "write a name",
                );
                String::new()
            }
            None => String::new(),
        }
    }

    fn reject_reserved(&mut self, name: &str, span: Span) {
        if crate::RESERVED.contains(&name) {
            self.errors.push(
                Diagnostic::error("E0105", span, format!("`{name}` is reserved"))
                    .with_fix(format!("rename `{name}` to a word that is not reserved")),
            );
        }
    }

    fn eat_ident(&mut self, name: &str) -> bool {
        if self
            .cur()
            .is_some_and(|t| matches!(&t.kind, Tok::Ident(s) if s == name))
        {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect_eol(&mut self, fix: &str) {
        if !self.eol() {
            let span = self.cur_span();
            let desc = self
                .cur()
                .map(|t| t.kind.desc())
                .unwrap_or_else(|| "more tokens".into());
            self.fail(span, format!("unexpected {desc}"), fix);
        }
    }

    fn fail_here(&mut self, message: &str, fix: &str) {
        let span = if let Some(tok) = self.cur() {
            tok.span
        } else {
            Span {
                line: self.cur_line_no(),
                col: 1,
            }
        };
        self.fail(span, message, fix);
    }

    fn fail(&mut self, span: Span, message: impl Into<String>, fix: impl Into<String>) {
        if self.line_failed {
            return;
        }
        self.line_failed = true;
        self.errors
            .push(Diagnostic::error("E0101", span, message).with_fix(fix));
        if self.li < self.lines.len() {
            self.ti = self.lines[self.li].tokens.len();
        }
    }

    fn report_dedent(&mut self, line_idx: usize, parent_indent: usize, parent_line: u32) {
        let line_no = self.lines[line_idx].no;
        if !self.dedent_reported.insert(line_no) {
            return;
        }
        let span = self.lines[line_idx]
            .tokens
            .first()
            .map(|t| t.span)
            .unwrap_or(Span {
                line: line_no,
                col: 1,
            });
        let (note, fix) = if parent_line == 0 {
            (
                "nearest valid parent is the start of the file".to_string(),
                "dedent this line to column 0".to_string(),
            )
        } else {
            (
                format!("nearest valid parent is line {parent_line}"),
                format!("indent to {parent_indent} spaces to align with line {parent_line}"),
            )
        };
        self.errors.push(
            Diagnostic::error(
                "E0103",
                span,
                "indentation does not match an enclosing block",
            )
            .with_note(note)
            .with_fix(fix),
        );
    }

    fn next_is_kw(&self, indent: usize, kw: Kw) -> bool {
        let next = self.li + 1;
        let Some(line) = self.lines.get(next) else {
            return false;
        };
        line.indent == indent
            && matches!(line.tokens.first().map(|t| &t.kind), Some(Tok::Kw(k)) if *k == kw)
    }

    fn enter(&mut self, li: usize) {
        self.li = li;
        self.ti = 0;
        self.line_failed = false;
    }

    fn cur(&self) -> Option<&lex::Token> {
        if self.line_failed {
            return None;
        }
        self.lines.get(self.li)?.tokens.get(self.ti)
    }

    fn cur_span(&self) -> Span {
        self.cur().map(|t| t.span).unwrap_or(Span {
            line: self.cur_line_no(),
            col: 1,
        })
    }

    fn cur_indent(&self) -> usize {
        self.lines.get(self.li).map(|l| l.indent).unwrap_or(0)
    }

    fn cur_line_no(&self) -> u32 {
        self.lines.get(self.li).map(|l| l.no).unwrap_or(1)
    }

    fn eol(&self) -> bool {
        self.line_failed
            || self.li >= self.lines.len()
            || self.ti >= self.lines[self.li].tokens.len()
    }

    fn bump(&mut self) -> lex::Token {
        let tok = self.lines[self.li].tokens[self.ti].clone();
        self.ti += 1;
        tok
    }

    fn at(&self, kind: &Tok) -> bool {
        self.cur().is_some_and(|t| tok_eq(&t.kind, kind))
    }

    fn at_kw(&self, kw: Kw) -> bool {
        self.cur()
            .is_some_and(|t| matches!(t.kind, Tok::Kw(k) if k == kw))
    }

    fn same_kind(&self, kind: &Tok) -> bool {
        self.cur().is_some_and(|t| tok_eq(&t.kind, kind))
    }

    fn eat(&mut self, kind: &Tok) -> bool {
        if self.at(kind) {
            self.ti += 1;
            true
        } else {
            false
        }
    }

    fn eat_kw(&mut self, kw: Kw) -> bool {
        if self.at_kw(kw) {
            self.ti += 1;
            true
        } else {
            false
        }
    }

    fn dummy(&self, span: Span) -> Stmt {
        Stmt {
            kind: StmtKind::Node {
                verb: String::new(),
                args: Vec::new(),
                named: Vec::new(),
                body: Vec::new(),
            },
            span,
        }
    }
}

fn is_key(kind: &Tok) -> bool {
    matches!(kind, Tok::Ident(_) | Tok::Kw(_))
}

fn binop_of(kind: &Tok) -> Option<(BinOp, u8)> {
    Some(match kind {
        Tok::Plus => (BinOp::Add, PREC_ADD),
        Tok::Minus => (BinOp::Sub, PREC_ADD),
        Tok::Star => (BinOp::Mul, PREC_MUL),
        Tok::Slash => (BinOp::Div, PREC_MUL),
        Tok::EqEq => (BinOp::Eq, PREC_CMP),
        Tok::NotEq => (BinOp::Ne, PREC_CMP),
        Tok::Lt => (BinOp::Lt, PREC_CMP),
        Tok::Le => (BinOp::Le, PREC_CMP),
        Tok::Gt => (BinOp::Gt, PREC_CMP),
        Tok::Ge => (BinOp::Ge, PREC_CMP),
        Tok::Approx => (BinOp::Approx, PREC_CMP),
        Tok::Kw(Kw::In) => (BinOp::In, PREC_CMP),
        Tok::Arrow => (BinOp::Map, PREC_MAP),
        Tok::Kw(Kw::And) => (BinOp::And, PREC_AND),
        Tok::Kw(Kw::Or) => (BinOp::Or, PREC_OR),
        _ => return None,
    })
}

fn size_part(part: SizePart) -> Expr {
    match part {
        SizePart::Num(n) => Expr::Num(n),
        SizePart::Percent(n) => Expr::Percent(n),
    }
}

fn tok_eq(a: &Tok, b: &Tok) -> bool {
    matches!(
        (a, b),
        (Tok::LParen, Tok::LParen)
            | (Tok::RParen, Tok::RParen)
            | (Tok::LBrack, Tok::LBrack)
            | (Tok::RBrack, Tok::RBrack)
            | (Tok::LBrace, Tok::LBrace)
            | (Tok::RBrace, Tok::RBrace)
            | (Tok::Colon, Tok::Colon)
            | (Tok::Comma, Tok::Comma)
            | (Tok::Eq, Tok::Eq)
            | (Tok::Dot, Tok::Dot)
    )
}
