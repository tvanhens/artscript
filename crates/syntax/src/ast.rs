//! Every line keeps the span of its first token so diagnostics can point at it.

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
    pub items: Vec<Item>,
}

impl File {
    pub fn decls(&self) -> impl Iterator<Item = &Decl> {
        self.items.iter().filter_map(|i| match i {
            Item::Decl(d) => Some(d),
            Item::Scene(_) => None,
        })
    }

    pub fn scenes(&self) -> impl Iterator<Item = &Scene> {
        self.items.iter().filter_map(|i| match i {
            Item::Scene(s) => Some(s),
            Item::Decl(_) => None,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Decl(Decl),
    Scene(Scene),
}

impl Item {
    pub fn span(&self) -> Span {
        match self {
            Item::Decl(d) => d.span,
            Item::Scene(s) => s.span,
        }
    }
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
pub struct Decl {
    pub kind: DeclKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DeclKind {
    /// `use palette/dusk as p`, `use face/eye { eye, blush }`.
    Use {
        path: String,
        alias: Option<String>,
        items: Vec<String>,
    },
    /// `palette dusk from base` with one swatch per child line.
    Palette {
        name: String,
        from: Option<String>,
        body: Vec<Swatch>,
    },
    /// `style ink stroke:p.ink,2 join:round`. Keys only; paint keys reach descendants.
    Style { name: String, named: Vec<Named> },
    /// `finish lacquer width:5` + a block of `fx` lines, kept in order. An `fx lacquer` line under
    /// a node puts those lines on that node at that position.
    Finish {
        name: String,
        params: Vec<Named>,
        body: Vec<Fx>,
    },
    /// `def eye size:10 iris:p.sky.3` + body.
    Def {
        name: String,
        params: Vec<Named>,
        body: Vec<DefItem>,
    },
    /// `rule icon_grid subject:@self size:64` + body of checks.
    Rule {
        name: String,
        params: Vec<Named>,
        body: Vec<Scene>,
    },
    /// `shader name params` + `sksl """..."""`. Parsed, not implemented in v0.1.
    Shader {
        name: String,
        params: Vec<Named>,
        sksl: Option<String>,
    },
    /// `light sun dir:-1,-1.5`. Parsed, not implemented in v0.1.
    Light { name: String, named: Vec<Named> },
    /// `brush ink stamp:@dab`. Parsed, not implemented in v0.1.
    Brush { name: String, named: Vec<Named> },
    /// `anim idle dur:2s loop` with a body of patch lines.
    Anim {
        name: String,
        dur: Expr,
        loop_: bool,
        body: Vec<Patch>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Swatch {
    pub name: String,
    pub kind: SwatchKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SwatchKind {
    Color(Expr),
    Ramp {
        pos: Box<RampPos>,
        hue: Option<Expr>,
    },
}

/// A ramp's positionals. The evaluator decides which are colors and which is the stop count,
/// because a name can be either.
#[derive(Clone, Debug, PartialEq)]
pub enum RampPos {
    One(Expr),
    Two(Expr, Expr),
    Three(Expr, Expr, Expr),
}

impl RampPos {
    pub fn exprs(&self) -> Vec<&Expr> {
        match self {
            RampPos::One(a) => vec![a],
            RampPos::Two(a, b) => vec![a, b],
            RampPos::Three(a, b, c) => vec![a, b, c],
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    pub kind: SceneKind,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckKind {
    Assert,
    Expect,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SceneKind {
    Let {
        name: String,
        value: Expr,
    },
    If {
        cond: Expr,
        then: Vec<Scene>,
        otherwise: Vec<Scene>,
    },
    Each {
        var: String,
        iter: Expr,
        body: Vec<Scene>,
    },
    Check {
        kind: CheckKind,
        expr: Expr,
        named: Vec<Named>,
        message: Option<String>,
    },
    Slot {
        name: String,
        named: Vec<Named>,
        fallback: Vec<Scene>,
    },
    /// `play idle start:1s`. `name` is `None` for `play none`.
    Play {
        name: Option<String>,
        start: Option<Expr>,
    },
    Draw(Draw),
}

/// Every drawing statement: `verb positional… key:value… [block]`. The verb is a shape, a
/// structure verb, or a component name. Trailing bare words such as `closed` stay in `args` as
/// `Expr::Ident` and the evaluator decides.
#[derive(Clone, Debug, PartialEq)]
pub struct Draw {
    pub verb: String,
    pub args: Vec<Expr>,
    pub named: Vec<Named>,
    pub body: DrawBody,
}

/// A draw's children in source order, holding at most one [`Mask`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrawBody {
    items: Vec<DrawItem>,
}

pub(crate) struct SecondMask(pub(crate) Span);

impl DrawBody {
    pub fn items(&self) -> &[DrawItem] {
        &self.items
    }

    pub fn mask(&self) -> Option<&Mask> {
        self.items.iter().find_map(|i| match i {
            DrawItem::Mask { mask, .. } => Some(mask),
            _ => None,
        })
    }

    pub fn scenes(&self) -> impl Iterator<Item = &Scene> + Clone {
        self.items.iter().filter_map(|i| match i {
            DrawItem::Scene(s) => Some(s),
            _ => None,
        })
    }

    pub fn fx(&self) -> impl Iterator<Item = &Fx> {
        self.items.iter().filter_map(|i| match i {
            DrawItem::Fx(f) => Some(f),
            _ => None,
        })
    }

    pub(crate) fn push(&mut self, item: DrawItem) -> Result<(), SecondMask> {
        if let DrawItem::Mask { span, .. } = item
            && self.mask().is_some()
        {
            return Err(SecondMask(span));
        }
        self.items.push(item);
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum DrawItem {
    Scene(Scene),
    Fx(Fx),
    Mask { mask: Mask, span: Span },
    SlotFill(SlotFill),
    Patch(Patch),
}

/// `mask @body`, `mask simplex(30) > .5`, or `mask` over an indented block.
#[derive(Clone, Debug, PartialEq)]
pub enum Mask {
    Value(Expr),
    Block(Vec<Scene>),
}

/// `fx rim 4 dir:1,1 color:p.rim a:.5 as:soft`. `as:` and `of:` stay in `named`.
/// The name is a built-in effect, or a finish.
#[derive(Clone, Debug, PartialEq)]
pub struct Fx {
    pub op: String,
    pub args: Vec<Expr>,
    pub named: Vec<Named>,
    pub span: Span,
}

/// The effect record `fx` lowering walks. Same fields as [`Fx`].
pub type FxLine = Fx;

/// `head:` followed by a block. The name may be `mask`.
#[derive(Clone, Debug, PartialEq)]
pub struct SlotFill {
    pub name: String,
    pub body: Vec<Scene>,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Patch {
    pub path: Vec<String>,
    pub named: Vec<Named>,
    pub off: bool,
    pub span: Span,
}

/// A def body line. Effects attach to the instance. A def body holds no mask, slot fill, or patch.
#[derive(Clone, Debug, PartialEq)]
pub enum DefItem {
    Scene(Scene),
    Fx(Fx),
}

impl DefItem {
    pub fn scene(&self) -> Option<&Scene> {
        match self {
            DefItem::Scene(s) => Some(s),
            DefItem::Fx(_) => None,
        }
    }

    pub fn fx(&self) -> Option<&Fx> {
        match self {
            DefItem::Fx(f) => Some(f),
            DefItem::Scene(_) => None,
        }
    }
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
    /// Seconds. `200ms` is 0.2. A bare number in a time position is also seconds.
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
