use crate::ast::{
    BinOp, CheckKind, Decl, DeclKind, DefItem, Draw, DrawItem, Expr, File, Fx, Item, Mask, Named,
    Patch, Scene, SceneKind, Span, Swatch, SwatchKind, UnOp,
};
use crate::lex::{self, Comment};

pub fn format_file(file: &File) -> String {
    format_file_with_comments(file, &[], &[])
}

pub(crate) fn format_file_with_comments(
    file: &File,
    comments: &[Comment],
    blanks: &[u32],
) -> String {
    let mut p = Printer {
        comments,
        ci: 0,
        out: String::new(),
        blanks,
        prev_line: 0,
    };
    for d in &file.directives {
        let mut parts = vec![format!("@{}", d.name)];
        let args = d.args.iter().map(fmt_expr);
        let named = d.named.iter().filter(|n| n.key != "clip").map(fmt_named);
        if let Some(path) = &d.path {
            parts.push(path.clone());
            parts.extend(args.chain(named));
        } else {
            parts.extend(named.chain(args));
        }
        if !d.each.is_empty() {
            parts.push("each".into());
            for axis in &d.each {
                parts.push(fmt_named(axis));
            }
        }
        for named in d.named.iter().filter(|n| n.key == "clip") {
            parts.push(fmt_named(named));
        }
        p.emit(d.span.line, 0, &parts.join(" "));
    }
    for item in &file.items {
        p.item(item);
    }
    p.finish();
    p.out
}

struct Printer<'a> {
    comments: &'a [Comment],
    ci: usize,
    out: String,
    blanks: &'a [u32],
    prev_line: u32,
}

impl<'a> Printer<'a> {
    fn item(&mut self, item: &Item) {
        match item {
            Item::Decl(d) => self.decl(d),
            Item::Scene(s) => self.scene(s, 0),
        }
    }

    fn decl(&mut self, decl: &Decl) {
        let line = decl.span.line;
        match &decl.kind {
            DeclKind::Use { path, alias, items } => {
                let mut text = format!("use {path}");
                if let Some(alias) = alias {
                    text.push_str(" as ");
                    text.push_str(alias);
                }
                if !items.is_empty() {
                    text.push_str(" { ");
                    text.push_str(&items.join(" "));
                    text.push_str(" }");
                }
                self.emit(line, 0, &text);
            }
            DeclKind::Palette { name, from, body } => {
                let mut text = format!("palette {name}");
                if let Some(from) = from {
                    text.push_str(" from ");
                    text.push_str(from);
                }
                self.emit(line, 0, &text);
                for swatch in body {
                    self.swatch(swatch);
                }
            }
            DeclKind::Style { name, named } => {
                self.emit(line, 0, &join_head("style", name, &[], named));
            }
            DeclKind::Finish { name, params, body } => {
                self.emit(line, 0, &join_head("finish", name, &[], params));
                for line in body {
                    self.fx(line, 1);
                }
            }
            DeclKind::Def { name, params, body } => {
                self.emit(line, 0, &join_head("def", name, &[], params));
                self.def_body(body);
            }
            DeclKind::Alias {
                name,
                target,
                args,
                named,
            } => {
                let call = join_head(target, "", args, named);
                self.emit(line, 0, &format!("def {name} = {call}"));
            }
            DeclKind::Stamp { name, params, body } => {
                self.emit(line, 0, &join_head("stamp", name, &[], params));
                self.def_body(body);
            }
            DeclKind::Rule { name, params, body } => {
                self.emit(line, 0, &join_head("rule", name, &[], params));
                self.scenes(body, 1);
            }
            DeclKind::Fn { name, params, body } => {
                let head = format!("fn {name}({})", params.join(" "));
                self.emit(line, 0, &format!("{head} = {}", fmt_expr(body)));
            }
            DeclKind::Light { name, named } => {
                self.emit(line, 0, &join_head("light", name, &[], named));
            }
            DeclKind::Brush { name, params, sksl } => {
                self.emit(line, 0, &join_head("brush", name, &[], params));
                if let Some(block) = sksl {
                    self.raw_block(1, &block.text);
                }
            }
            DeclKind::Anim {
                name,
                dur,
                loop_,
                body,
            } => {
                let mut text = format!("anim {name} dur:{}", fmt_expr(dur));
                if *loop_ {
                    text.push_str(" loop");
                }
                self.emit(line, 0, &text);
                for p in body {
                    self.patch(p, 1);
                }
            }
        }
    }

    fn swatch(&mut self, swatch: &Swatch) {
        let text = match &swatch.kind {
            SwatchKind::Color(e) => format!("color {} {}", swatch.name, fmt_expr(e)),
            SwatchKind::Ramp { pos, hue } => {
                let mut parts = vec![format!("ramp {}", swatch.name)];
                parts.extend(pos.exprs().into_iter().map(fmt_expr));
                if let Some(hue) = hue {
                    parts.push(format!("hue:{}", fmt_expr(hue)));
                }
                parts.join(" ")
            }
        };
        self.emit(swatch.span.line, 1, &text);
    }

    fn scenes(&mut self, scenes: &[Scene], indent: usize) {
        for s in scenes {
            self.scene(s, indent);
        }
    }

    fn def_body(&mut self, body: &[DefItem]) {
        for item in body {
            match item {
                DefItem::Scene(s) => self.scene(s, 1),
                DefItem::Fx(f) => self.fx(f, 1),
            }
        }
    }

    fn scene(&mut self, scene: &Scene, indent: usize) {
        let line = scene.span.line;
        match &scene.kind {
            SceneKind::Let { name, value } => {
                self.emit(line, indent, &format!("let {name} = {}", fmt_expr(value)));
            }
            SceneKind::Check {
                kind,
                expr,
                named,
                message,
            } => {
                let kw = match kind {
                    CheckKind::Assert => "assert",
                    CheckKind::Expect => "expect",
                };
                let mut parts = vec![kw.to_string(), fmt_expr(expr)];
                for n in named {
                    parts.push(fmt_named(n));
                }
                if let Some(message) = message {
                    parts.push(fmt_string(message));
                }
                self.emit(line, indent, &parts.join(" "));
            }
            SceneKind::If {
                cond,
                then,
                otherwise,
            } => {
                self.stmt_if(line, cond, then, otherwise, indent, false);
            }
            SceneKind::Each {
                index,
                var,
                iter,
                body,
            } => {
                let names = loop_names(index, var);
                self.emit(line, indent, &format!("each {names} in {}", fmt_expr(iter)));
                self.scenes(body, indent + 1);
            }
            SceneKind::Slot {
                name,
                named,
                fallback,
            } => {
                self.emit(line, indent, &join_head("slot", name, &[], named));
                self.scenes(fallback, indent + 1);
            }
            SceneKind::Play { name, start } => {
                let mut text = format!("play {}", name.as_deref().unwrap_or("none"));
                if let Some(start) = start {
                    text.push_str(" start:");
                    text.push_str(&fmt_expr(start));
                }
                self.emit(line, indent, &text);
            }
            SceneKind::Draw(Draw {
                verb,
                args,
                named,
                body,
            }) => {
                self.emit(line, indent, &join_head("", verb, args, named));
                for item in body.items() {
                    self.draw_item(item, indent + 1);
                }
            }
            SceneKind::Fx(f) => self.fx(f, indent),
        }
    }

    fn draw_item(&mut self, item: &DrawItem, indent: usize) {
        match item {
            DrawItem::Scene(s) => self.scene(s, indent),
            DrawItem::Fx(f) => self.fx(f, indent),
            DrawItem::Mask { mask, span } => self.mask(mask, *span, indent),
            DrawItem::SlotFill(fill) => {
                self.emit(fill.span.line, indent, &format!("{}:", fill.name));
                self.scenes(&fill.body, indent + 1);
            }
            DrawItem::Patch(p) => self.patch(p, indent),
        }
    }

    fn patch(&mut self, p: &Patch, indent: usize) {
        let mut text = format!(".{}", p.path.join("."));
        for n in &p.named {
            text.push(' ');
            text.push_str(&fmt_named(n));
        }
        if p.off {
            text.push_str(" off");
        }
        self.emit(p.span.line, indent, &text);
    }

    fn fx(&mut self, fx: &Fx, indent: usize) {
        self.emit(
            fx.span.line,
            indent,
            &join_head("fx", &fx.op, &fx.args, &fx.named),
        );
        if let Some(line) = &fx.mask {
            self.mask(&line.mask, line.span, indent + 1);
        }
    }

    fn mask(&mut self, mask: &Mask, span: Span, indent: usize) {
        match mask {
            Mask::Value(e) => self.emit(span.line, indent, &format!("mask {}", fmt_expr(e))),
            Mask::Block(body) => {
                self.emit(span.line, indent, "mask");
                self.scenes(body, indent + 1);
            }
        }
    }

    fn stmt_if(
        &mut self,
        line: u32,
        cond: &Expr,
        then: &[Scene],
        otherwise: &[Scene],
        indent: usize,
        else_if: bool,
    ) {
        let kw = if else_if { "else if" } else { "if" };
        self.emit(line, indent, &format!("{kw} {}", fmt_expr(cond)));
        self.scenes(then, indent + 1);
        match otherwise {
            [
                Scene {
                    kind:
                        SceneKind::If {
                            cond,
                            then,
                            otherwise,
                        },
                    span,
                },
            ] => {
                self.stmt_if(span.line, cond, then, otherwise, indent, true);
            }
            [] => {}
            body => {
                let anchor = body.first().map(|s| s.span.line).unwrap_or(line);
                self.emit_leading(anchor, indent);
                self.gap_before(anchor);
                self.out.push_str(&" ".repeat(indent * 2));
                self.out.push_str("else\n");
                self.scenes(body, indent + 1);
            }
        }
    }

    fn raw_block(&mut self, indent: usize, content: &str) {
        let pad = " ".repeat(indent * 2);
        self.out.push_str(&pad);
        self.out.push_str("sksl \"\"\"\n");
        if !content.is_empty() {
            let mut lines: Vec<&str> = content.split('\n').collect();
            if content.ends_with('\n') {
                lines.pop();
            }
            for line in lines {
                if line.is_empty() {
                    self.out.push('\n');
                } else {
                    self.out.push_str(&pad);
                    self.out.push_str(line);
                    self.out.push('\n');
                }
            }
        }
        self.out.push_str(&pad);
        self.out.push_str("\"\"\"\n");
    }

    fn emit(&mut self, source_line: u32, indent: usize, text: &str) {
        self.emit_leading(source_line, indent);
        self.gap_before(source_line);
        self.out.push_str(&" ".repeat(indent * 2));
        self.out.push_str(text);
        self.emit_trailing(source_line);
        self.out.push('\n');
    }

    /// A run of blank lines between two source lines becomes one blank line.
    fn gap_before(&mut self, line: u32) {
        if self.prev_line > 0 && line > self.prev_line {
            let prev = self.prev_line;
            if self.blanks.iter().any(|&b| b > prev && b < line) && !self.out.ends_with("\n\n") {
                self.out.push('\n');
            }
        }
        if line > self.prev_line {
            self.prev_line = line;
        }
    }

    fn emit_leading(&mut self, before_line: u32, indent: usize) {
        while self.ci < self.comments.len() {
            let c = &self.comments[self.ci];
            if c.line >= before_line {
                break;
            }
            if c.trailing {
                self.ci += 1;
                continue;
            }
            let text = c.text.clone();
            let line = c.line;
            self.ci += 1;
            self.write_comment(indent, &text, line);
        }
    }

    fn emit_trailing(&mut self, line: u32) {
        if self.ci >= self.comments.len() {
            return;
        }
        let c = &self.comments[self.ci];
        if c.trailing && c.line == line {
            let text = c.text.clone();
            self.ci += 1;
            self.out.push(' ');
            self.push_hash(&text);
        }
    }

    fn write_comment(&mut self, indent: usize, text: &str, line: u32) {
        self.gap_before(line);
        self.out.push_str(&" ".repeat(indent * 2));
        self.push_hash(text);
        self.out.push('\n');
    }

    fn push_hash(&mut self, text: &str) {
        if text.is_empty() {
            self.out.push('#');
        } else {
            self.out.push_str("# ");
            self.out.push_str(text);
        }
    }

    fn finish(&mut self) {
        while self.ci < self.comments.len() {
            let c = &self.comments[self.ci];
            if !c.trailing {
                let text = c.text.clone();
                let line = c.line;
                self.write_comment(0, &text, line);
            }
            self.ci += 1;
        }
    }
}

fn join_head(kw: &str, name: &str, args: &[Expr], named: &[Named]) -> String {
    let mut parts = Vec::new();
    if kw.is_empty() {
        if !name.is_empty() {
            parts.push(name.to_string());
        }
    } else if name.is_empty() {
        parts.push(kw.to_string());
    } else {
        parts.push(format!("{kw} {name}"));
    }
    for arg in args {
        parts.push(fmt_expr(arg));
    }
    for n in named {
        parts.push(fmt_named(n));
    }
    parts.join(" ")
}

fn fmt_named(n: &Named) -> String {
    match &n.value {
        Expr::Tuple(xs) if n.key == "name" && xs.len() == 1 => format!("name:{{{}}}", fmt_expr(&xs[0])),
        value => format!("{}:{}", n.key, fmt_expr(value)),
    }
}

pub(crate) fn fmt_expr(e: &Expr) -> String {
    fmt_at(e, 0, false)
}

fn fmt_at(e: &Expr, parent: u8, right: bool) -> String {
    let prec = prec_of(e);
    let body = fmt_body(e);
    if prec < parent || (right && prec == parent && nests(e)) {
        format!("({body})")
    } else {
        body
    }
}

fn nests(e: &Expr) -> bool {
    matches!(
        e,
        Expr::Binary(_, _, _) | Expr::Range(_, _) | Expr::If { .. } | Expr::Unary(_, _)
    )
}

fn prec_of(e: &Expr) -> u8 {
    match e {
        Expr::Tuple(_) => 0,
        Expr::If { .. } => 1,
        Expr::Binary(op, _, _) => match op {
            BinOp::Or => 2,
            BinOp::And => 3,
            BinOp::Eq
            | BinOp::Ne
            | BinOp::Lt
            | BinOp::Le
            | BinOp::Gt
            | BinOp::Ge
            | BinOp::Approx
            | BinOp::In => 5,
            BinOp::Map => 6,
            BinOp::Add | BinOp::Sub => 8,
            BinOp::Mul | BinOp::Div => 9,
        },
        Expr::Range(_, _) => 7,
        Expr::Unary(UnOp::Not, _) => 4,
        Expr::Unary(UnOp::Neg, _) => 10,
        Expr::Member(_, _) | Expr::Index(_, _) | Expr::Call { .. } => 11,
        _ => 12,
    }
}

fn loop_names(index: &Option<String>, var: &str) -> String {
    match index {
        Some(i) => format!("{i}, {var}"),
        None => var.to_string(),
    }
}

fn fmt_body(e: &Expr) -> String {
    match e {
        Expr::Num(n) => fmt_number(*n),
        Expr::Percent(n) => format!("{}%", fmt_number(round_scaled(*n, 100.0))),
        Expr::Angle(n) => format!("{}deg", fmt_number(*n)),
        Expr::Time(n) => fmt_time(*n),
        Expr::Size(w, h) => format!("{}x{}", fmt_expr(w), fmt_expr(h)),
        Expr::Hex(h) => format!("#{h}"),
        Expr::Str(s) => fmt_string(s),
        Expr::Ident(s) => s.clone(),
        Expr::Ref(segs) => format!("@{}", segs.join(".")),
        Expr::Member(base, seg) if matches!(**base, Expr::Num(_)) => format!("({}).{seg}", fmt_body(base)),
        Expr::Member(base, seg) => format!("{}.{}", fmt_at(base, 11, false), seg),
        Expr::Index(base, key) if matches!(**base, Expr::Num(_)) => {
            format!("({}).{{{}}}", fmt_body(base), fmt_expr(key))
        }
        Expr::Index(base, key) => format!("{}.{{{}}}", fmt_at(base, 11, false), fmt_expr(key)),
        Expr::Tuple(xs) => {
            // An `else` branch takes the rest of the tuple, so only the last element may be a bare `if`.
            let last = xs.len().saturating_sub(1);
            xs.iter()
                .enumerate()
                .map(|(i, x)| fmt_at(x, if i < last { 2 } else { 1 }, false))
                .collect::<Vec<_>>()
                .join(",")
        }
        Expr::List(xs) => format!(
            "[{}]",
            xs.iter().map(fmt_expr).collect::<Vec<_>>().join(" ")
        ),
        Expr::Each {
            index,
            var,
            iter,
            body,
        } => {
            let names = loop_names(index, var);
            format!("[each {names} in {} -> {}]", fmt_expr(iter), fmt_expr(body))
        }
        Expr::Range(lo, hi) => format!("{}..{}", fmt_at(lo, 7, false), fmt_at(hi, 7, true)),
        Expr::Unary(UnOp::Neg, inner) => format!("-{}", fmt_at(inner, 10, true)),
        Expr::Unary(UnOp::Not, inner) => format!("not {}", fmt_at(inner, 4, true)),
        Expr::Binary(op, l, r) => {
            let prec = prec_of(e);
            let ls = fmt_at(l, prec, false);
            let rs = fmt_at(r, prec, true);
            let sym = op_sym(*op);
            if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div) {
                format!("{ls}{sym}{rs}")
            } else {
                format!("{ls} {sym} {rs}")
            }
        }
        Expr::Call { func, args, named } => {
            let mut parts: Vec<String> = args.iter().map(fmt_expr).collect();
            for n in named {
                parts.push(fmt_named(n));
            }
            if parts.is_empty() {
                format!("{func}()")
            } else {
                format!("{func}({})", parts.join(" "))
            }
        }
        Expr::If {
            cond,
            then,
            otherwise,
        } => {
            format!(
                "if {} then {} else {}",
                fmt_at(cond, 2, false),
                fmt_expr(then),
                fmt_expr(otherwise)
            )
        }
    }
}

fn op_sym(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Eq => "==",
        BinOp::Ne => "!=",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Gt => ">",
        BinOp::Ge => ">=",
        BinOp::Approx => "~=",
        BinOp::And => "and",
        BinOp::Or => "or",
        BinOp::In => "in",
        BinOp::Map => "->",
    }
}

fn fmt_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn round_scaled(n: f64, scale: f64) -> f64 {
    let scaled = n * scale;
    (scaled * 1e8).round() / 1e8
}

fn fmt_time(seconds: f64) -> String {
    if !seconds.is_finite() {
        return "0s".into();
    }
    let ms = seconds * 1000.0;
    let whole_ms = (ms - ms.round()).abs() < 1e-6;
    if seconds > 0.0 && seconds < 1.0 && whole_ms {
        format!("{}ms", fmt_number(ms.round()))
    } else {
        format!("{}s", fmt_number(seconds))
    }
}

fn fmt_number(n: f64) -> String {
    if !n.is_finite() {
        return "0".into();
    }
    let neg = n.is_sign_negative() && n != 0.0;
    let n = n.abs();
    let rounded = (n * 1e10).round() / 1e10;
    let raw = format!("{rounded:.10}");
    let trimmed = trim_float(&raw);
    let body = match trimmed.strip_prefix("0.") {
        Some(frac) => format!(".{frac}"),
        None => trimmed,
    };
    if neg { format!("-{body}") } else { body }
}

fn trim_float(s: &str) -> String {
    if !s.contains('.') {
        return s.to_string();
    }
    let t = s.trim_end_matches('0').trim_end_matches('.');
    if t.is_empty() || t == "-" {
        "0".into()
    } else {
        t.to_string()
    }
}

pub(crate) fn source_notes(src: &str) -> (Vec<Comment>, Vec<u32>) {
    let lexed = lex::lex(src);
    (lexed.comments, lexed.blanks)
}
