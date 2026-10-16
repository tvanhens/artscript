//! Untyped syntax tree. The parser produces it, `fmt` prints it, the evaluator consumes it.
//! Every statement keeps the span of its first token so diagnostics can point at it.

use std::fmt;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize)]
pub struct Span {
    /// 1-based line.
    pub line: u32,
    /// 1-based column, counted in chars.
    pub col: u32,
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.col)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct File {
    pub directives: Vec<Directive>,
    pub stmts: Vec<Stmt>,
}

/// `@target chars/hero.png 256x256 scale:1,2 each team:[red, blue]`.
/// `path` is the raw first token (it may contain `/`, `.`, `{name}`).
#[derive(Clone, Debug, PartialEq)]
pub struct Directive {
    pub name: String,
    pub path: Option<String>,
    pub args: Vec<Expr>,
    pub named: Vec<Named>,
    /// `each a:[..] b:[..]` matrix axes, in source order.
    pub each: Vec<Named>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Named {
    pub key: String,
    pub value: Expr,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckKind {
    Assert,
    Expect,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StmtKind {
    /// `use palette/dusk as p`, `use face/eye { eye, blush }`.
    Use {
        path: String,
        alias: Option<String>,
        items: Vec<String>,
    },
    /// `palette dusk from base` with one entry per child line (each entry is a `Node`).
    Palette {
        name: String,
        from: Option<String>,
        body: Vec<Stmt>,
    },
    /// `style ink stroke:p.ink,2 join:round`.
    Style {
        name: String,
        named: Vec<Named>,
    },
    /// `def eye size:10 iris:p.sky.3` + body.
    Def {
        name: String,
        params: Vec<Named>,
        body: Vec<Stmt>,
    },
    /// `rule icon_grid subject:@self size:64` + body of checks.
    Rule {
        name: String,
        params: Vec<Named>,
        body: Vec<Stmt>,
    },
    Let {
        name: String,
        value: Expr,
    },
    /// `shader name params` + `sksl """..."""`. Parsed, not implemented in v0.1.
    Shader {
        name: String,
        params: Vec<Named>,
        sksl: Option<String>,
    },
    /// `light sun dir:-1,-1.5`. Parsed, not implemented in v0.1.
    Light {
        name: String,
        named: Vec<Named>,
    },
    /// `brush ink stamp:@dab`. Parsed, not implemented in v0.1.
    Brush {
        name: String,
        named: Vec<Named>,
    },
    Check {
        kind: CheckKind,
        expr: Expr,
        named: Vec<Named>,
        message: Option<String>,
    },
    If {
        cond: Expr,
        then: Vec<Stmt>,
        otherwise: Vec<Stmt>,
    },
    Each {
        var: String,
        iter: Expr,
        body: Vec<Stmt>,
    },
    /// Every drawing statement: `verb positional… key:value… [block]`.
    /// `flags` are trailing bare words that the parser cannot distinguish from
    /// positional identifiers; they stay in `args` as `Expr::Ident` and the evaluator decides.
    Node {
        verb: String,
        args: Vec<Expr>,
        named: Vec<Named>,
        body: Vec<Stmt>,
    },
    /// `head:` followed by a block.
    SlotFill {
        name: String,
        body: Vec<Stmt>,
    },
    /// `.arm_r rot:-20`, `.cape off`, `.head.eye_l iris:p.accent`.
    /// A path segment may be `*`.
    Patch {
        path: Vec<String>,
        named: Vec<Named>,
        off: bool,
    },
    Anim {
        name: String,
        dur: Expr,
        loop_: bool,
        body: Vec<Stmt>,
    },
    /// `play idle start:1s`. `name` is `None` for `play none`.
    Play {
        name: Option<String>,
        start: Option<Expr>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    /// `~=`, approximate equality; tolerance comes from the check's `tol:`.
    Approx,
    And,
    Or,
    /// `a in lo..hi`.
    In,
    /// `field -> ramp`.
    Map,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Num(f64),
    /// `50%` stored as 0.5.
    Percent(f64),
    /// Always degrees. `.25turn` parses to 90.
    Angle(f64),
    Time(f64),
    /// `256x256`, `100%x60%`.
    Size(Box<Expr>, Box<Expr>),
    /// Hex without `#`, lowercased: `ff5e5b`, `fff`.
    Hex(String),
    Str(String),
    Ident(String),
    /// `@body`, `@hero.head.eye_l`, `@self`, `@body.neck`. First segment is the name.
    Ref(Vec<String>),
    /// `p.sky.3`: the base and each member segment (numbers are kept as text).
    Member(Box<Expr>, String),
    /// Tight comma tuple without spaces: `14,0`, `p.ink,1.5`, `.4,1`.
    Tuple(Vec<Expr>),
    /// `[a b]`.
    List(Vec<Expr>),
    /// `[each k in 0..n -> expr]`. Evaluates to a list.
    Each {
        var: String,
        iter: Box<Expr>,
        body: Box<Expr>,
    },
    /// `lo..hi`.
    Range(Box<Expr>, Box<Expr>),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    /// `grad(down a b)`, `shade(c -2)`, `radial(a b at:.3,.3)`, `oklch(.78 .07 55)`.
    Call {
        func: String,
        args: Vec<Expr>,
        named: Vec<Named>,
    },
    If {
        cond: Box<Expr>,
        then: Box<Expr>,
        otherwise: Box<Expr>,
    },
}
