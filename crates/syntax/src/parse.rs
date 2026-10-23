use std::collections::HashSet;

use crate::Diagnostic;
use crate::ast::{
    BinOp, BodyLine, BodyLineKind, CheckKind, Decl, DeclKind, DefItem, Directive, Draw, DrawBody,
    DrawItem, Expr, File, Fx, Item, Mask, MaskLine, Named, Patch, RampPos, Scene, SceneKind,
    SecondMask, SlotFill, Span, Swatch, SwatchKind, UnOp,
};
use crate::lex::{self, Line, SizePart, Tok};
use crate::words::{self, Word};

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

enum Head {
    Decl,
    Fx,
    Mask,
    SlotFill(String),
    Patch,
    GluedColon { name: String, rest: String },
    Scene,
}

impl<'a> Parser<'a> {
    fn parse_file(&mut self) -> File {
        let mut directives = Vec::new();
        let mut items = Vec::new();
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
                if let Some(item) = self.top_line() {
                    items.push(item);
                }
            }
            self.li += 1;
        }
        File { directives, items }
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
            if self.eat_kw(Word::Each) {
                while !self.eol() {
                    if self.is_named() {
                        if !each.is_empty() && !self.cur().is_some_and(|t| t.ws_before) {
                            self.fail_here(
                                "expected whitespace between `each` axes",
                                "separate axes with a space",
                            );
                            break;
                        }
                        let item = self.parse_named();
                        if is_each_axis(&item.key) {
                            each.push(item);
                        } else {
                            named.push(item);
                        }
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
        if self.is_named() {
            return false;
        }
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

    fn line_rest(&self, from: usize) -> String {
        let tokens = &self.lines[self.li].tokens;
        match (tokens.get(from), tokens.last()) {
            (Some(a), Some(b)) => self.src[a.start..b.end].to_string(),
            _ => String::new(),
        }
    }

    fn head(&self) -> Head {
        let Some(tok) = self.cur() else {
            return Head::Scene;
        };
        let tokens = &self.lines[self.li].tokens;
        let glued_colon = tokens
            .get(self.ti + 1)
            .is_some_and(|c| matches!(c.kind, Tok::Colon) && !c.ws_before);
        let word = match &tok.kind {
            Tok::Ident(s) => Some(s.clone()),
            Tok::Kw(w) => Some(w.as_str().to_string()),
            _ => None,
        };
        if let Some(word) = word
            && glued_colon
        {
            return if self.ti + 2 >= tokens.len() {
                Head::SlotFill(word)
            } else {
                Head::GluedColon {
                    name: word,
                    rest: self.line_rest(self.ti + 2),
                }
            };
        }
        match &tok.kind {
            Tok::Kw(
                Word::Use
                | Word::Palette
                | Word::Style
                | Word::Finish
                | Word::Def
                | Word::Rule
                | Word::Light
                | Word::Anim,
            ) => Head::Decl,
            Tok::Ident(s) if words::is_decl_head(s) => Head::Decl,
            Tok::Kw(Word::Fx) => Head::Fx,
            Tok::Kw(Word::Mask) => Head::Mask,
            Tok::Dot => Head::Patch,
            _ => Head::Scene,
        }
    }

    fn reject_line_and_block(&mut self, message: impl Into<String>, fix: impl Into<String>) {
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.fail(span, message, fix);
        self.skip_block(indent);
    }

    fn skip_block(&mut self, indent: usize) {
        while self
            .lines
            .get(self.li + 1)
            .is_some_and(|l| l.indent > indent)
        {
            self.li += 1;
        }
    }

    fn top_line(&mut self) -> Option<Item> {
        match self.head() {
            Head::Decl => self.parse_decl().map(Item::Decl),
            head => self.scene_head(head, "at the top level").map(Item::Scene),
        }
    }

    fn scene_line(&mut self, error_where: &str) -> Option<Scene> {
        let head = self.head();
        self.scene_head(head, error_where)
    }

    fn scene_head(&mut self, head: Head, error_where: &str) -> Option<Scene> {
        match head {
            Head::Scene => self.parse_scene(),
            Head::Decl => {
                self.reject_line_and_block(
                    format!("a declaration cannot go {error_where}"),
                    "move it to the top level of the file",
                );
                None
            }
            Head::Fx => {
                self.reject_line_and_block(
                    format!("`fx` cannot go {error_where}"),
                    "indent the `fx` line directly under the shape, group, def, or component call it applies to",
                );
                None
            }
            Head::Mask => {
                self.reject_line_and_block(
                    format!("`mask` cannot go {error_where}"),
                    "indent `mask` directly under the draw it masks",
                );
                None
            }
            Head::SlotFill(name) => {
                self.reject_line_and_block(
                    format!("slot fill `{name}:` cannot go {error_where}"),
                    "indent it directly under the component call that has the slot",
                );
                None
            }
            Head::Patch => {
                self.reject_line_and_block(
                    format!("a patch cannot go {error_where}"),
                    "indent it directly under the component call it overrides",
                );
                None
            }
            Head::GluedColon { name, rest } => {
                self.reject_line_and_block(
                    format!("`{name}:` must end the line"),
                    format!("write `{name} {rest}`, or end the line with `{name}:` to fill a slot"),
                );
                None
            }
        }
    }

    /// `fx_branches` is false for a body that runs once per copy, where an `fx` under an `if`
    /// would have no single answer for the one node it applies to.
    fn draw_line(&mut self, fx_branches: bool) -> Option<DrawItem> {
        match self.head() {
            Head::Fx => self.parse_fx().map(DrawItem::Fx),
            Head::Mask => self
                .parse_mask()
                .map(|MaskLine { mask, span }| DrawItem::Mask { mask, span }),
            Head::SlotFill(name) => self.parse_slot_fill(name).map(DrawItem::SlotFill),
            Head::Patch => self.parse_patch().map(DrawItem::Patch),
            Head::Scene if fx_branches && self.at_kw(Word::If) => {
                self.parse_if(true).map(DrawItem::Scene)
            }
            head => self.scene_head(head, "under a draw").map(DrawItem::Scene),
        }
    }

    /// `kw` is `def` or `stamp`, the declaration whose body this line is in.
    fn def_line(&mut self, kw: &str) -> Option<DefItem> {
        match self.head() {
            Head::Fx => self.parse_fx().map(DefItem::Fx),
            Head::Scene if self.at_kw(Word::If) => self.parse_if(true).map(DefItem::Scene),
            Head::Mask => {
                self.reject_line_and_block(
                    format!("a `{kw}` body cannot hold a mask"),
                    format!("put the mask under a draw inside the {kw}, or under the call site"),
                );
                None
            }
            Head::SlotFill(name) => {
                self.reject_line_and_block(
                    format!("a `{kw}` body cannot hold slot fill `{name}:`"),
                    format!("declare it with `slot {name}`, or fill it under a component call"),
                );
                None
            }
            Head::Patch => {
                self.reject_line_and_block(
                    format!("a `{kw}` body cannot hold a patch"),
                    "write the patch under a call of the component",
                );
                None
            }
            head => self.scene_head(head, &format!("in a {kw}")).map(DefItem::Scene),
        }
    }

    fn swatch_line(&mut self) -> Option<Swatch> {
        let tokens = &self.lines[self.li].tokens;
        let first = match &tokens[self.ti].kind {
            Tok::Ident(s) => Some(s.clone()),
            _ => None,
        };
        let ramp = match first.as_deref().and_then(Word::lookup) {
            Some(Word::Ramp) => true,
            Some(Word::Color) => false,
            _ => {
                let fix = match (&first, tokens.get(self.ti + 1).map(|t| &t.kind)) {
                    (Some(name), Some(Tok::Ident(r))) if Word::lookup(r) == Some(Word::Ramp) => {
                        format!("write `ramp {name} {}`", self.line_rest(self.ti + 2))
                    }
                    (Some(name), Some(_)) => {
                        format!("write `color {name} {}`", self.line_rest(self.ti + 1))
                    }
                    _ => "start the row with `ramp <name>` or `color <name>`".to_string(),
                };
                self.reject_line_and_block("a palette row starts with `ramp` or `color`", fix);
                return None;
            }
        };
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.bump();
        let name = self.expect_word();
        let (args, named) = self.parse_items();
        self.no_block(indent);
        if self.line_failed {
            return None;
        }
        let kind = if ramp {
            let mut hue = None;
            for n in named {
                if n.key == "hue" && hue.is_none() {
                    hue = Some(n.value);
                } else {
                    self.fail(
                        n.span,
                        format!("a ramp takes no `{}:`", n.key),
                        "`hue:` is the only named argument of a ramp",
                    );
                    return None;
                }
            }
            let mut args = args.into_iter();
            let pos = match (args.next(), args.next(), args.next(), args.next()) {
                (Some(a), None, None, None) => RampPos::One(a),
                (Some(a), Some(b), None, None) => RampPos::Two(a, b),
                (Some(a), Some(b), Some(c), None) => RampPos::Three(a, b, c),
                (None, ..) => {
                    self.fail(
                        span,
                        format!("`ramp {name}` needs a color"),
                        format!("write `ramp {name} #f3a2a6 5`"),
                    );
                    return None;
                }
                _ => {
                    self.fail(
                        span,
                        "a ramp takes one or two colors and an optional stop count",
                        format!("write `ramp {name} <color> [<color>] [<count>]`"),
                    );
                    return None;
                }
            };
            SwatchKind::Ramp {
                pos: Box::new(pos),
                hue,
            }
        } else {
            match (args.len(), named.is_empty()) {
                (1, true) => SwatchKind::Color(args.into_iter().next().expect("one arg")),
                _ => {
                    self.fail(
                        span,
                        format!("`color {name}` takes one color"),
                        format!("write `color {name} #3d2632`"),
                    );
                    return None;
                }
            }
        };
        Some(Swatch { name, kind, span })
    }

    fn parse_decl(&mut self) -> Option<Decl> {
        let span = self.cur_span();
        let kind = match self.cur().map(|t| t.kind.clone()) {
            Some(Tok::Kw(Word::Use)) => self.parse_use(),
            Some(Tok::Kw(Word::Palette)) => self.parse_palette(),
            Some(Tok::Kw(Word::Style)) => self.parse_style(),
            Some(Tok::Kw(Word::Finish)) => self.parse_finish(),
            Some(Tok::Kw(Word::Def)) => self.parse_def(),
            Some(Tok::Kw(Word::Rule)) => self.parse_rule(),
            Some(Tok::Kw(Word::Light)) => self.parse_light(),
            Some(Tok::Ident(s)) if s == "param" => self.parse_param(),
            Some(Tok::Ident(s)) if s == "brush" => self.parse_brush(),
            Some(Tok::Ident(s)) if s == "stamp" => self.parse_stamp(),
            Some(Tok::Ident(s)) if s == "fn" => self.parse_fn(),
            Some(Tok::Kw(Word::Anim)) => self.parse_anim(),
            _ => None,
        }?;
        Some(Decl { kind, span })
    }

    fn parse_scene(&mut self) -> Option<Scene> {
        let kind = self.cur().map(|t| t.kind.clone())?;
        match kind {
            Tok::Kw(Word::Let) => self.parse_let(),
            Tok::Kw(Word::Assert) => self.parse_check(CheckKind::Assert),
            Tok::Kw(Word::Expect) => self.parse_check(CheckKind::Expect),
            Tok::Kw(Word::If) => self.parse_if(false),
            Tok::Kw(Word::Each) => self.parse_each(),
            Tok::Kw(Word::Slot) => self.parse_slot(),
            Tok::Kw(Word::Play) => self.parse_play(),
            Tok::Kw(Word::Else) => {
                self.reject_line_and_block(
                    "`else` must directly follow an `if` block at the same indent",
                    "place `else` on the line after an `if` block, or remove it",
                );
                None
            }
            Tok::Ident(name) => self.parse_draw(name),
            _ => {
                let desc = kind.desc();
                self.reject_line_and_block(
                    format!("expected a statement, found {desc}"),
                    "start the line with a verb, declaration, or directive",
                );
                None
            }
        }
    }

    fn parse_use(&mut self) -> Option<DeclKind> {
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.bump();
        if self.eol() {
            self.fail(
                span,
                "expected a path after `use`",
                "write `use path/to/lib`",
            );
            self.no_block(indent);
            return None;
        }
        let path = self.take_glued_chunk().unwrap_or_default();
        let mut alias = None;
        let mut items = Vec::new();
        if self.eat_kw(Word::As) {
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
        Some(DeclKind::Use { path, alias, items })
    }

    fn parse_palette(&mut self) -> Option<DeclKind> {
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.bump();
        let name = self.expect_name();
        let from = if self.eat_kw(Word::From) {
            Some(self.expect_name())
        } else {
            None
        };
        self.expect_eol("remove the extra tokens after the palette name");
        let body = self.block(indent, span.line, Self::swatch_line);
        Some(DeclKind::Palette { name, from, body })
    }

    fn parse_style(&mut self) -> Option<DeclKind> {
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.bump();
        let name = self.expect_name();
        let (args, named) = self.parse_items();
        if !args.is_empty() {
            self.fail(
                span,
                "a style takes named arguments only",
                "write `style name key:value`",
            );
        }
        self.reject_block(
            indent,
            "a style takes named keys only, not a block",
            "declare `finish <name>` with this block and write `fx <name>` under the node",
        );
        Some(DeclKind::Style { name, named })
    }

    fn parse_finish(&mut self) -> Option<DeclKind> {
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.bump();
        let name = self.expect_name();
        let (args, params) = self.parse_items();
        if !args.is_empty() {
            self.fail(
                span,
                "finish parameters are named arguments",
                "write `finish name key:value`",
            );
        }
        self.reject_param_names(&params);
        let has_child = self
            .lines
            .get(self.li + 1)
            .is_some_and(|l| l.indent > indent);
        let body = self.block(indent, span.line, Self::finish_line);
        if body.is_empty() && !has_child {
            self.errors.push(
                Diagnostic::error(
                    "E0101",
                    span,
                    format!("finish `{name}` needs an indented block of `fx` lines"),
                )
                .with_fix(format!("indent `fx` lines under `finish {name}`")),
            );
        }
        Some(DeclKind::Finish { name, params, body })
    }

    fn finish_line(&mut self) -> Option<Fx> {
        match self.head() {
            Head::Fx => {
                let mut fx = self.parse_fx()?;
                if let Some(mask) = fx.mask.take() {
                    self.fail(
                        mask.span,
                        "a finish line takes no mask",
                        "write the mask under the node's own `fx outline` line",
                    );
                }
                Some(fx)
            }
            _ => {
                self.reject_line_and_block(
                    "a finish holds only `fx` lines",
                    "move this line under the node, or write it as `fx <effect> …`",
                );
                None
            }
        }
    }

    fn parse_param(&mut self) -> Option<DeclKind> {
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.bump();
        let (args, params) = self.parse_items();
        if !args.is_empty() || params.is_empty() {
            self.fail(span, "param needs named defaults", "write `param view:one_of(normal geometry) guides:false`");
        }
        self.reject_param_names(&params);
        self.no_block(indent);
        Some(DeclKind::Param { params })
    }

    fn parse_def(&mut self) -> Option<DeclKind> {
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.bump();
        let name = self.expect_name();
        if words::is_decl_head(&name) {
            self.errors.push(
                Diagnostic::error("E0105", span, format!("a def cannot be named `{name}`"))
                    .with_note(format!("a line that starts with `{name}` declares a {name}, so no instance of this def could be written"))
                    .with_fix("rename the def"),
            );
        }
        if self.eat(&Tok::Eq) {
            let target = self.expect_name();
            let (args, named) = self.parse_items();
            if let Some(n) = named.iter().find(|n| n.key == "name") {
                self.errors.push(
                    Diagnostic::error("E0101", n.span, "a def alias cannot fix `name:`")
                        .with_fix("write `name:` on the instance"),
                );
            }
            self.reject_block(
                indent,
                "a def alias has no body",
                &format!("dedent the block, or write `def {name}` with parameters and this body"),
            );
            return Some(DeclKind::Alias {
                name,
                target,
                args,
                named,
            });
        }
        let (args, params) = self.parse_items();
        if !args.is_empty() {
            self.fail(
                span,
                "def parameters are named arguments",
                "write `def name key:value`",
            );
        }
        self.reject_param_names(&params);
        let body = self.block(indent, span.line, |p| p.def_line("def"));
        Some(DeclKind::Def { name, params, body })
    }

    /// `stamp NAME k:v...` with a body of the lines a def body takes.
    fn parse_stamp(&mut self) -> Option<DeclKind> {
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.bump();
        let name = self.expect_name();
        let (args, params) = self.parse_items();
        if !args.is_empty() {
            self.fail(
                span,
                "a stamp takes named arguments only",
                "write `stamp name key:value`",
            );
        }
        self.reject_param_names(&params);
        let body = self.block(indent, span.line, |p| p.def_line("stamp"));
        Some(DeclKind::Stamp { name, params, body })
    }

    fn parse_rule(&mut self) -> Option<DeclKind> {
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.bump();
        let name = self.expect_name();
        let (args, params) = self.parse_items();
        if !args.is_empty() {
            self.fail(
                span,
                "rule parameters are named arguments",
                "write `rule name key:value`",
            );
        }
        self.reject_param_names(&params);
        let body = self.block(indent, span.line, |p| p.scene_line("in a rule"));
        Some(DeclKind::Rule { name, params, body })
    }

    fn parse_fn(&mut self) -> Option<DeclKind> {
        let indent = self.cur_indent();
        let Some((name, params, body)) = self.fn_parts() else {
            self.no_block(indent);
            return None;
        };
        self.reject_block(
            indent,
            "a fn has no body block",
            &format!("dedent the block, or fold it into the expression after `=` in `fn {name}`"),
        );
        Some(DeclKind::Fn { name, params, body })
    }

    fn fn_parts(&mut self) -> Option<(String, Vec<String>, Expr)> {
        let span = self.cur_span();
        self.bump();
        let name = self.expect_name();
        let shown = if name.is_empty() { "name" } else { &name };
        let shape = format!("write `fn {shown}(a b) = <expr>`");
        if !self.eat(&Tok::LParen) {
            self.fail(span, "expected `(` after the name", shape);
            return None;
        }
        let mut params: Vec<String> = Vec::new();
        while !self.eat(&Tok::RParen) {
            if self.eol() {
                self.fail(span, "expected `)` after the parameters", shape);
                return None;
            }
            let pspan = self.cur_span();
            let p = self.expect_name();
            if p.is_empty() {
                return None;
            }
            if params.contains(&p) {
                self.fail(
                    pspan,
                    format!("duplicate parameter `{p}`"),
                    "give each parameter its own name",
                );
            }
            params.push(p);
        }
        if !self.eat(&Tok::Eq) {
            self.fail(span, "expected `=` after the parameters", shape);
            return None;
        }
        let body = self.parse_tuple();
        self.expect_eol("a `fn` body is one expression");
        Some((name, params, body))
    }

    fn parse_light(&mut self) -> Option<DeclKind> {
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.bump();
        let name = self.expect_name();
        let (args, named) = self.parse_items();
        if !args.is_empty() {
            self.fail(
                span,
                "a light takes named arguments only",
                "write `light name key:value`",
            );
        }
        self.no_block(indent);
        Some(DeclKind::Light { name, named })
    }

    fn parse_brush(&mut self) -> Option<DeclKind> {
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.bump();
        let name = self.expect_name();
        let (args, params) = self.parse_items();
        if !args.is_empty() {
            self.fail(
                span,
                "a brush takes named arguments only",
                "write `brush name key:value`",
            );
        }
        self.reject_param_names(&params);
        let body = self.block(indent, span.line, Self::body_line);
        Some(DeclKind::Brush { name, params, body })
    }

    fn body_line(&mut self) -> Option<BodyLine> {
        let span = self.cur_span();
        let kind = match self.cur().map(|t| t.kind.clone()) {
            Some(Tok::Kw(Word::Let)) => {
                self.bump();
                let name = self.expect_name();
                if !self.eat(&Tok::Eq) {
                    self.fail(span, "expected `=` after the name", "write `let name = <expr>`");
                    return None;
                }
                let value = self.parse_tuple();
                self.expect_eol("a `let` binding is one expression");
                BodyLineKind::Let { name, value }
            }
            Some(Tok::Ident(s)) if s == "fn" => {
                let (name, params, body) = self.fn_parts()?;
                BodyLineKind::Fn { name, params, body }
            }
            Some(Tok::Ident(s)) if s == "out" => {
                self.bump();
                let (mut args, named) = self.parse_items();
                if args.len() != 1 {
                    self.fail(span, "`out` takes one rgb value", "write `out m.paint.rgb cover:cover(m.edge)`");
                    return None;
                }
                let mut cover = None;
                for n in named {
                    if n.key == "cover" && cover.is_none() {
                        cover = Some(n);
                    } else {
                        self.fail(n.span, format!("`out` takes no `{}:`", n.key), "`cover:` is the only named argument of `out`");
                        return None;
                    }
                }
                BodyLineKind::Out { rgb: args.pop().expect("one arg"), cover }
            }
            Some(Tok::Ident(s)) if s == "sksl" => {
                self.bump();
                let Some(Tok::Raw(text)) = self.cur().map(|t| t.kind.clone()) else {
                    self.fail_here("expected a raw string after `sksl`", "write the body as `let`, `fn`, and `out` lines");
                    return None;
                };
                self.bump();
                self.expect_eol("remove the extra tokens after the raw string");
                BodyLineKind::Sksl { text }
            }
            _ => {
                self.reject_line_and_block(
                    "a brush body holds `let`, `fn`, and `out` lines",
                    "write `out m.paint.rgb cover:cover(m.edge)` as the body's last line",
                );
                return None;
            }
        };
        Some(BodyLine { kind, span })
    }

    fn parse_anim(&mut self) -> Option<DeclKind> {
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.bump();
        let name = self.expect_name();
        let mut dur = None;
        let mut loop_ = false;
        while !self.eol() {
            if self.is_named() {
                let n = self.parse_named();
                if n.key == "dur" && dur.is_none() {
                    dur = Some(n.value);
                } else {
                    self.fail(
                        n.span,
                        format!("`anim` does not take `{}:`", n.key),
                        "write `anim name dur:<time>` and an optional `loop`",
                    );
                }
            } else if self.eat_ident("loop") {
                loop_ = true;
            } else {
                self.fail_here("expected `dur:` or `loop`", "write `anim name dur:1s loop`");
                break;
            }
        }
        let dur = dur.unwrap_or_else(|| {
            self.fail(span, "`anim` needs `dur:`", "write `anim name dur:1s`");
            Expr::Time(1.0)
        });
        let has_child = self
            .lines
            .get(self.li + 1)
            .is_some_and(|l| l.indent > indent);
        let body = self.block(indent, span.line, Self::anim_line);
        if body.is_empty() && !has_child {
            self.errors.push(
                Diagnostic::error(
                    "E0341",
                    span,
                    format!("anim `{name}` needs an indented block of patch lines"),
                )
                .with_fix(format!("indent `.part key:value` under `anim {name}`")),
            );
        }
        Some(DeclKind::Anim {
            name,
            dur,
            loop_,
            body,
        })
    }

    fn anim_line(&mut self) -> Option<Patch> {
        if matches!(self.head(), Head::Patch) {
            return self.parse_patch();
        }
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.errors.push(
            Diagnostic::error("E0341", span, "an anim body may only contain patch lines")
                .with_fix("write `.part key:value` under the anim"),
        );
        self.skip_block(indent);
        self.line_failed = true;
        if self.li < self.lines.len() {
            self.ti = self.lines[self.li].tokens.len();
        }
        None
    }

    fn parse_play(&mut self) -> Option<Scene> {
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.bump();
        let name = if self.eat_kw(Word::None) {
            None
        } else if self.eol() {
            self.fail(
                span,
                "expected an anim name after `play`",
                "write `play name` or `play none`",
            );
            None
        } else {
            Some(self.expect_name())
        };
        let mut start = None;
        while !self.eol() {
            if self.is_named() {
                let n = self.parse_named();
                if n.key == "start" && start.is_none() {
                    start = Some(n.value);
                } else {
                    self.fail(
                        n.span,
                        format!("`play` does not take `{}:`", n.key),
                        "write `play name start:<time>`",
                    );
                }
            } else {
                self.fail_here(
                    "expected `start:` or the end of the line",
                    "write `play name start:0s`",
                );
                break;
            }
        }
        self.no_block(indent);
        Some(Scene {
            kind: SceneKind::Play { name, start },
            span,
        })
    }

    fn parse_let(&mut self) -> Option<Scene> {
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
            return None;
        }
        let value = self.parse_tuple();
        self.expect_eol("a `let` binding is one expression");
        self.no_block(indent);
        Some(Scene {
            kind: SceneKind::Let { name, value },
            span,
        })
    }

    fn parse_check(&mut self, kind: CheckKind) -> Option<Scene> {
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.bump();
        if self.eol() {
            self.fail(
                span,
                "expected an expression to check",
                "write `assert <expr>`",
            );
            self.no_block(indent);
            return None;
        }
        let expr = self.parse_tuple();
        let mut named = Vec::new();
        while self.is_named() {
            named.push(self.parse_named());
        }
        let message = if self
            .cur()
            .is_some_and(|t| matches!(t.kind, Tok::Str(_) | Tok::Raw(..)) && t.ws_before)
        {
            match self.bump().kind {
                Tok::Str(s) | Tok::Raw(s) => Some(s),
                _ => None,
            }
        } else {
            None
        };
        self.expect_eol("remove the extra tokens after the check");
        self.no_block(indent);
        Some(Scene {
            kind: SceneKind::Check {
                kind,
                expr,
                named,
                message,
            },
            span,
        })
    }

    fn parse_if(&mut self, fx: bool) -> Option<Scene> {
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.bump();
        self.finish_if(span, indent, fx)
    }

    fn branch_line(&mut self, fx: bool, error_where: &str) -> Option<Scene> {
        match self.head() {
            Head::Fx if fx => self.parse_fx().map(|f| Scene {
                span: f.span,
                kind: SceneKind::Fx(f),
            }),
            Head::Scene if fx && self.at_kw(Word::If) => self.parse_if(true),
            head => self.scene_head(head, error_where),
        }
    }

    fn finish_if(&mut self, span: Span, indent: usize, fx: bool) -> Option<Scene> {
        let cond = if self.eol() {
            self.fail(span, "expected a condition after `if`", "write `if <expr>`");
            None
        } else {
            Some(self.parse_tuple())
        };
        if self.at_kw(Word::Then) {
            self.fail(
                self.cur_span(),
                "unexpected keyword `then`",
                "statement `if` takes a block; remove `then`",
            );
        } else {
            self.expect_eol("the `if` condition must fit on one line");
        }
        let then = self.block(indent, span.line, |p| {
            p.branch_line(fx, "directly under `if`")
        });
        let otherwise = if self.next_is_kw(indent, Word::Else) {
            self.enter(self.li + 1);
            self.parse_else(indent, fx)
        } else {
            Vec::new()
        };
        Some(Scene {
            kind: SceneKind::If {
                cond: cond?,
                then,
                otherwise,
            },
            span,
        })
    }

    fn parse_else(&mut self, indent: usize, fx: bool) -> Vec<Scene> {
        let else_line = self.cur_line_no();
        self.bump();
        if self.at_kw(Word::If) {
            let span = self.cur_span();
            self.bump();
            self.finish_if(span, indent, fx).into_iter().collect()
        } else {
            self.expect_eol("`else` takes a block, or write `else if`");
            self.block(indent, else_line, |p| {
                p.branch_line(fx, "directly under `else`")
            })
        }
    }

    fn parse_each(&mut self) -> Option<Scene> {
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.bump();
        let (index, var) = self.loop_names();
        if !self.eat_kw(Word::In) {
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
            None
        } else {
            Some(self.parse_tuple())
        };
        self.expect_eol("the `each` header must fit on one line");
        let body = self.block(indent, span.line, |p| p.scene_line("directly under `each`"));
        Some(Scene {
            kind: SceneKind::Each {
                index,
                var,
                iter: iter?,
                body,
            },
            span,
        })
    }

    fn parse_slot(&mut self) -> Option<Scene> {
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.bump();
        let name = self.slot_name();
        let (args, named) = self.parse_items();
        if !args.is_empty() {
            self.fail(
                span,
                "a slot takes its name and named arguments only",
                "write `slot name key:value`",
            );
        }
        let fallback = self.block(indent, span.line, |p| p.scene_line("in a slot fallback"));
        if name.is_empty() {
            return None;
        }
        Some(Scene {
            kind: SceneKind::Slot {
                name,
                named,
                fallback,
            },
            span,
        })
    }

    fn parse_draw(&mut self, verb: String) -> Option<Scene> {
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.reject_reserved(&verb, span);
        self.bump();
        let name = matches!(verb.as_str(), "layer" | "group" | "anchor")
            .then(|| self.reserved_positional_name())
            .flatten();
        let (mut args, named) = self.parse_items();
        if let Some(name) = name {
            args.insert(0, Expr::Ident(name));
        }
        if let Some(m) = named.iter().find(|n| n.key == "mask") {
            self.fail(
                m.span,
                "`mask` is a child line, not a named argument",
                "move it to an indented line under the draw, such as `mask @body`",
            );
        }
        let mut body = DrawBody::default();
        let fx_branches = !matches!(verb.as_str(), "repeat" | "grid");
        for item in self.block(indent, span.line, |p| p.draw_line(fx_branches)) {
            if let Err(SecondMask(span)) = body.push(item) {
                self.errors.push(
                    Diagnostic::error("E0101", span, "a draw takes one mask")
                        .with_fix("merge the masks into one `mask` block, or remove one"),
                );
            }
        }
        Some(Scene {
            kind: SceneKind::Draw(Draw {
                verb,
                args,
                named,
                body,
            }),
            span,
        })
    }

    fn parse_fx(&mut self) -> Option<Fx> {
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.bump();
        let op = match self.cur().map(|t| t.kind.clone()) {
            Some(Tok::Ident(op)) => {
                self.bump();
                op
            }
            _ => {
                self.fail(
                    span,
                    "expected an effect name after `fx`",
                    "write `fx shadow 0,6 blur:8`",
                );
                self.no_block(indent);
                return None;
            }
        };
        let (args, named) = self.parse_items();
        let mut mask = None;
        for line in self.block(indent, span.line, |p| p.fx_line()) {
            if mask.is_some() {
                self.fail(
                    line.span,
                    "an `fx` line takes one mask",
                    "merge the masks into one `mask` block, or remove one",
                );
            } else if op != "outline" {
                self.fail(
                    line.span,
                    format!("`fx {op}` takes no mask"),
                    "only `fx outline` takes a mask; remove it, or move it under the draw",
                );
            } else {
                mask = Some(line);
            }
        }
        Some(Fx {
            op,
            args,
            named,
            span,
            mask,
        })
    }

    fn fx_line(&mut self) -> Option<MaskLine> {
        match self.head() {
            Head::Mask => self.parse_mask(),
            _ => {
                self.reject_line_and_block(
                    "only `mask` goes under an `fx` line",
                    "dedent it, or write `mask` over the shapes that keep the ring",
                );
                None
            }
        }
    }

    fn parse_mask(&mut self) -> Option<MaskLine> {
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.bump();
        let value = (!self.eol()).then(|| self.parse_tuple());
        self.expect_eol("a mask value is one expression");
        let has_block = self
            .lines
            .get(self.li + 1)
            .is_some_and(|l| l.indent > indent);
        let mask = match (value, has_block) {
            (Some(_), true) => {
                self.fail(
                    span,
                    "a mask takes a value or a block, not both",
                    "write `mask <value>` with nothing indented under it, or `mask` alone over the block",
                );
                self.skip_block(indent);
                return None;
            }
            (Some(v), false) => Mask::Value(v),
            (None, true) => {
                Mask::Block(self.block(indent, span.line, |p| p.scene_line("in a mask block")))
            }
            (None, false) => {
                self.fail(
                    span,
                    "`mask` needs a value or an indented block",
                    "write `mask @body`, `mask simplex(30) > .5`, or indent shapes under `mask`",
                );
                return None;
            }
        };
        Some(MaskLine { mask, span })
    }

    fn parse_patch(&mut self) -> Option<Patch> {
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.bump();
        let mut path = self.part_name();
        while self
            .cur()
            .is_some_and(|t| matches!(t.kind, Tok::Dot) && !t.ws_before)
        {
            self.bump();
            path.extend(self.part_name());
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
        Some(Patch {
            path,
            named,
            off,
            span,
        })
    }

    fn parse_slot_fill(&mut self, name: String) -> Option<SlotFill> {
        let span = self.cur_span();
        let indent = self.cur_indent();
        self.bump();
        self.bump();
        if !self
            .lines
            .get(self.li + 1)
            .is_some_and(|l| l.indent > indent)
        {
            self.fail(
                span,
                format!("slot `{name}:` needs an indented block"),
                format!("indent the slot body under `{name}:`"),
            );
            return None;
        }
        let body = self.block(indent, span.line, |p| p.scene_line("in a slot fill"));
        Some(SlotFill { name, body, span })
    }

    fn parse_items(&mut self) -> (Vec<Expr>, Vec<Named>) {
        let mut args = Vec::new();
        let mut named = Vec::new();
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

    fn reserved_positional_name(&mut self) -> Option<String> {
        let Some(Tok::Kw(k)) = self.cur().map(|t| t.kind.clone()) else {
            return None;
        };
        let next = self.lines[self.li].tokens.get(self.ti + 1);
        if next.is_some_and(|t| !t.ws_before) {
            return None;
        }
        self.bump();
        Some(k.as_str().to_string())
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
        if key == "name"
            && let Some(Tok::Kw(k)) = self.cur().map(|t| t.kind.clone())
        {
            self.bump();
            return Named {
                key,
                value: Expr::Ident(k.as_str().to_string()),
                span,
            };
        }
        if key == "name" && self.cur().is_some_and(|t| matches!(t.kind, Tok::LBrace)) {
            self.bump();
            let inner = self.parse_tuple();
            if !self.eat(&Tok::RBrace) {
                self.fail_here("unclosed `{`", "close it with `}`");
            }
            return Named {
                key,
                value: Expr::Tuple(vec![inner]),
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

    fn block<T>(
        &mut self,
        parent_indent: usize,
        parent_line: u32,
        mut line: impl FnMut(&mut Self) -> Option<T>,
    ) -> Vec<T> {
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
            if let Some(item) = line(self) {
                body.push(item);
            }
        }
        body
    }

    fn no_block(&mut self, indent: usize) {
        self.reject_block(
            indent,
            "this statement cannot have an indented block",
            "dedent the following lines",
        );
    }

    fn reject_block(&mut self, indent: usize, message: &str, fix: &str) {
        loop {
            let next = self.li + 1;
            if next >= self.lines.len() || self.lines[next].indent <= indent {
                break;
            }
            self.enter(next);
            let span = self.cur_span();
            self.fail(span, message, fix);
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
        if min <= PREC_IF && self.at_kw(Word::If) {
            self.bump();
            return self.parse_if_expr();
        }
        if min <= PREC_NOT && self.at_kw(Word::Not) {
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
        if !self.eat_kw(Word::Then) {
            self.fail_here(
                "expected `then`",
                "write `if <cond> then <expr> else <expr>`",
            );
            return cond;
        }
        let then_e = self.parse_tuple();
        if !self.eat_kw(Word::Else) {
            self.fail_here("expected `else`", "add `else <expr>`");
            return then_e;
        }
        let otherwise = self.parse_tuple();
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
        } else if let Some(Tok::Kw(k)) = self.cur().map(|t| t.kind.clone()) {
            self.bump();
            k.as_str().to_string()
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
                let key = self.parse_prec(0);
                if !self.eat(&Tok::RBrace) {
                    self.fail_here("expected `}` after the index", "close the index with `}`");
                }
                return Expr::Index(Box::new(base), Box::new(key));
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
            Tok::Time(n) => {
                self.bump();
                Expr::Time(n)
            }
            Tok::Size(a, b, depth) => {
                self.bump();
                Expr::Size {
                    w: Box::new(size_part(a)),
                    h: Box::new(size_part(b)),
                    depth: depth.map(|d| Box::new(size_part(d))),
                }
            }
            Tok::Hex(h) => {
                self.bump();
                Expr::Hex(h)
            }
            Tok::Str(s) | Tok::Raw(s) => {
                self.bump();
                Expr::Str(s)
            }
            Tok::Ident(s) => {
                self.bump();
                Expr::Ident(s)
            }
            Tok::Kw(Word::True) => {
                self.bump();
                Expr::Ident("true".into())
            }
            Tok::Kw(Word::False) => {
                self.bump();
                Expr::Ident("false".into())
            }
            Tok::Kw(Word::None) => {
                self.bump();
                Expr::Ident("none".into())
            }
            Tok::Kw(Word::In) => {
                self.bump();
                Expr::Ident("in".into())
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
        if self.at_kw(Word::Each) {
            return self.parse_each_expr();
        }
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

    /// `[each name in <iter> -> <expr>]`. The iterator stops before `->`.
    fn parse_each_expr(&mut self) -> Expr {
        let span = self.cur_span();
        self.bump();
        let (index, var) = self.loop_names();
        if !self.eat_kw(Word::In) {
            self.fail(
                span,
                "expected `in` after the loop name",
                "write `[each name in <expr> -> <expr>]`",
            );
        }
        let iter = if self.eol() || self.at(&Tok::Arrow) || self.at(&Tok::RBrack) {
            self.fail(
                span,
                "expected the list or range after `in`",
                "write `[each name in <expr> -> <expr>]`",
            );
            Expr::Ident(String::new())
        } else {
            self.parse_prec(PREC_MAP + 1)
        };
        if !self.eat(&Tok::Arrow) {
            self.fail_here("expected `->`", "write `[each name in <expr> -> <expr>]`");
            return Expr::Each {
                index,
                var,
                iter: Box::new(iter),
                body: Box::new(Expr::Ident(String::new())),
            };
        }
        let body = if self.eol() || self.at(&Tok::RBrack) {
            self.fail_here(
                "expected an expression after `->`",
                "write `[each name in <expr> -> <expr>]`",
            );
            Expr::Ident(String::new())
        } else {
            self.parse_tuple()
        };
        if !self.eat(&Tok::RBrack) {
            self.fail_here("unclosed `[`", "close the list with `]`");
        }
        Expr::Each {
            index,
            var,
            iter: Box::new(iter),
            body: Box::new(body),
        }
    }

    /// `x` or `i, x` after `each`. The index comes first.
    fn loop_names(&mut self) -> (Option<String>, String) {
        let first = self.expect_name();
        if !self.eat(&Tok::Comma) {
            return (None, first);
        }
        let span = self.cur_span();
        let var = self.expect_name();
        if !var.is_empty() && var == first {
            self.fail(
                span,
                format!("the loop index and the value are both named `{var}`"),
                "give the index and the value different names",
            );
        }
        (Some(first), var)
    }

    fn parse_group(&mut self) -> Expr {
        self.bump();
        let expr = self.parse_tuple();
        if !self.eat(&Tok::RParen) {
            self.fail_here("unclosed `(`", "close the group with `)`");
        }
        expr
    }

    fn slot_name(&mut self) -> String {
        self.take_word().unwrap_or_else(|| {
            self.fail_here("expected a slot name", "write `slot name`");
            String::new()
        })
    }

    fn part_name(&mut self) -> Vec<String> {
        if self.cur().is_some_and(|t| matches!(t.kind, Tok::Star)) {
            self.bump();
            return vec!["*".into()];
        }
        if let Some(tok) = self.cur()
            && matches!(tok.kind, Tok::Num(_))
        {
            let text = self.src[tok.start..tok.end].to_string();
            let span = tok.span;
            self.bump();
            let mut out = Vec::new();
            for piece in text.split('.') {
                if !piece.bytes().all(|b| b.is_ascii_digit()) || piece.is_empty() {
                    self.fail(span, "a copy index is a whole number", "write `.part.0`");
                    return out;
                }
                out.push(piece.to_string());
            }
            return out;
        }
        vec![self.take_word().unwrap_or_else(|| {
            self.fail_here("expected a part name", "write `.part` or `.head.eye`");
            String::new()
        })]
    }

    fn take_word(&mut self) -> Option<String> {
        match self.cur().map(|t| t.kind.clone()) {
            Some(Tok::Ident(s)) => {
                self.bump();
                Some(s)
            }
            Some(Tok::Kw(k)) => {
                self.bump();
                Some(k.as_str().to_string())
            }
            _ => None,
        }
    }

    fn expect_name(&mut self) -> String {
        let span = self.cur_span();
        let name = self.expect_word();
        if !name.is_empty() {
            self.reject_reserved(&name, span);
        }
        name
    }

    fn expect_word(&mut self) -> String {
        if self.eol() {
            self.fail_here("expected a name", "write a name");
            return String::new();
        }
        let span = self.cur_span();
        match self.cur().map(|t| t.kind.clone()) {
            Some(Tok::Ident(s)) => {
                self.bump();
                s
            }
            Some(Tok::Kw(k)) => {
                self.bump();
                k.as_str().to_string()
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

    fn reject_param_names(&mut self, params: &[Named]) {
        for p in params {
            self.reject_reserved(&p.key, p.span);
        }
    }

    fn reject_reserved(&mut self, name: &str, span: Span) {
        if words::is_reserved(name) {
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

    fn next_is_kw(&self, indent: usize, kw: Word) -> bool {
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

    fn at_kw(&self, kw: Word) -> bool {
        self.cur()
            .is_some_and(|t| matches!(t.kind, Tok::Kw(k) if k == kw))
    }

    fn eat(&mut self, kind: &Tok) -> bool {
        if self.at(kind) {
            self.ti += 1;
            true
        } else {
            false
        }
    }

    fn eat_kw(&mut self, kw: Word) -> bool {
        if self.at_kw(kw) {
            self.ti += 1;
            true
        } else {
            false
        }
    }
}

fn is_key(kind: &Tok) -> bool {
    matches!(kind, Tok::Ident(_) | Tok::Kw(_))
}

fn is_each_axis(key: &str) -> bool {
    !matches!(
        key,
        "bg" | "scale"
            | "as"
            | "fps"
            | "dur"
            | "t"
            | "clip"
            | "profile"
            | "pivot"
            | "palette"
            | "fit"
            | "align"
    )
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
        Tok::Kw(Word::In) => (BinOp::In, PREC_CMP),
        Tok::Arrow => (BinOp::Map, PREC_MAP),
        Tok::Kw(Word::And) => (BinOp::And, PREC_AND),
        Tok::Kw(Word::Or) => (BinOp::Or, PREC_OR),
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
            | (Tok::Arrow, Tok::Arrow)
    )
}
