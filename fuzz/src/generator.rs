use std::fmt::Write as _;

use libfuzzer_sys::arbitrary::Unstructured;

enum Naming {
    Auto,
    Fixed(String),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Base {
    Num,
    Vec,
    Size,
    Color,
    Bool,
    Word,
    Field,
    Pts,
    Comp,
    List,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ty {
    Num,
    Len,
    Coord,
    Unit,
    Small,
    Count,
    Sides,
    Strength,
    Signed,
    Shift,
    Hue,
    Angle,
    Time,
    Radius,
    Tension,
    Vec,
    At,
    Offset,
    DirVec,
    Scale,
    Skew,
    Jitter,
    Dash,
    Taper,
    Radii,
    Corners,
    Warp,
    Size,
    Color,
    Paint,
    Stroke,
    Field,
    Bool,
    Pts,
    PathData,
    Blend,
    Order,
    Cap,
    Join,
    Axis,
    Edge,
    StyleRef,
    Ref,
    Flag,
}

impl Ty {
    fn base(self) -> Base {
        match self {
            Ty::Num
            | Ty::Len
            | Ty::Coord
            | Ty::Unit
            | Ty::Small
            | Ty::Count
            | Ty::Sides
            | Ty::Strength
            | Ty::Signed
            | Ty::Shift
            | Ty::Hue
            | Ty::Angle
            | Ty::Time
            | Ty::Radius
            | Ty::Tension => Base::Num,
            Ty::Vec
            | Ty::At
            | Ty::Offset
            | Ty::DirVec
            | Ty::Scale
            | Ty::Skew
            | Ty::Jitter
            | Ty::Dash
            | Ty::Taper
            | Ty::Radii => Base::Vec,
            Ty::Size => Base::Size,
            Ty::Color | Ty::Paint | Ty::Stroke => Base::Color,
            Ty::Field => Base::Field,
            Ty::Bool => Base::Bool,
            Ty::Pts => Base::Pts,
            _ => Base::Word,
        }
    }
}

type Key = (&'static str, Ty, usize);

struct Shape {
    verb: &'static str,
    pos: &'static [Ty],
    keys: &'static [Key],
}

const SHAPES: &[Shape] = &[
    Shape {
        verb: "circle",
        pos: &[Ty::Radius],
        keys: &[],
    },
    Shape {
        verb: "rect",
        pos: &[Ty::Size],
        keys: &[("r", Ty::Corners, 5)],
    },
    Shape {
        verb: "ellipse",
        pos: &[Ty::Radii],
        keys: &[],
    },
    Shape {
        verb: "poly",
        pos: &[Ty::Sides],
        keys: &[("r", Ty::Len, 12), ("round", Ty::Small, 4)],
    },
    Shape {
        verb: "star",
        pos: &[Ty::Sides],
        keys: &[("r", Ty::Len, 12), ("inner", Ty::Unit, 8), ("round", Ty::Small, 2)],
    },
    Shape {
        verb: "curve",
        pos: &[Ty::Pts],
        keys: &[
            ("closed", Ty::Flag, 6),
            ("tension", Ty::Tension, 5),
            ("round", Ty::Small, 2),
        ],
    },
    Shape {
        verb: "blob",
        pos: &[Ty::Radius],
        keys: &[
            ("seed", Ty::Count, 6),
            ("wobble", Ty::Unit, 8),
            ("points", Ty::Sides, 3),
        ],
    },
    Shape {
        verb: "path",
        pos: &[Ty::PathData],
        keys: &[],
    },
];

const COMMON: &[Key] = &[
    ("at", Ty::At, 10),
    ("rot", Ty::Angle, 3),
    ("scale", Ty::Scale, 3),
    ("skew", Ty::Skew, 1),
    ("pivot", Ty::Vec, 1),
    ("warp", Ty::Warp, 1),
    ("a", Ty::Unit, 4),
    ("blend", Ty::Blend, 2),
    ("order", Ty::Order, 2),
];

const STYLE: &[Key] = &[("style", Ty::StyleRef, 2)];

const PAINT: &[Key] = &[
    ("fill", Ty::Paint, 9),
    ("stroke", Ty::Stroke, 5),
    ("blur", Ty::Small, 2),
    ("jitter", Ty::Jitter, 1),
    ("cap", Ty::Cap, 1),
    ("join", Ty::Join, 1),
    ("dash", Ty::Dash, 1),
];

const XFORM: &[Key] = &[
    ("at", Ty::Offset, 8),
    ("rot", Ty::Angle, 3),
    ("scale", Ty::Scale, 2),
    ("a", Ty::Unit, 4),
    ("blend", Ty::Blend, 1),
];

const SOFT_CLIP: &[Key] = &[("feather", Ty::Count, 6)];

const PATCH: &[Key] = &[
    ("at", Ty::Offset, 8),
    ("rot", Ty::Angle, 3),
    ("scale", Ty::Scale, 2),
    ("a", Ty::Unit, 4),
    ("fill", Ty::Paint, 4),
    ("blur", Ty::Small, 2),
    ("blend", Ty::Blend, 1),
];

struct Fx {
    op: &'static str,
    pos: &'static [Ty],
    keys: &'static [Key],
}

const COLOR_A: &[Key] = &[("color", Ty::Color, 6), ("a", Ty::Unit, 5)];

const FXS: &[Fx] = &[
    Fx {
        op: "blur",
        pos: &[Ty::Small],
        keys: &[],
    },
    Fx {
        op: "shadow",
        pos: &[Ty::Offset],
        keys: &[("blur", Ty::Small, 8), ("color", Ty::Color, 6), ("a", Ty::Unit, 5)],
    },
    Fx {
        op: "inner",
        pos: &[Ty::Offset],
        keys: &[("blur", Ty::Small, 8), ("color", Ty::Color, 6), ("a", Ty::Unit, 5)],
    },
    Fx {
        op: "glow",
        pos: &[Ty::Small],
        keys: COLOR_A,
    },
    Fx {
        op: "rim",
        pos: &[Ty::Small],
        keys: &[("dir", Ty::DirVec, 10), ("color", Ty::Color, 6), ("a", Ty::Unit, 5)],
    },
    Fx {
        op: "adjust",
        pos: &[],
        keys: &[("hue", Ty::Hue, 10), ("sat", Ty::Signed, 10), ("light", Ty::Signed, 10)],
    },
    Fx {
        op: "outline",
        pos: &[Ty::Small],
        keys: COLOR_A,
    },
    Fx {
        op: "grain",
        pos: &[Ty::Strength],
        keys: &[("scale", Ty::Small, 8), ("seed", Ty::Count, 2)],
    },
];

struct Fun {
    name: &'static str,
    ret: Base,
    args: &'static [Ty],
}

const FUNS: &[Fun] = &[
    Fun {
        name: "min",
        ret: Base::Num,
        args: &[Ty::Num, Ty::Num],
    },
    Fun {
        name: "max",
        ret: Base::Num,
        args: &[Ty::Num, Ty::Num],
    },
    Fun {
        name: "clamp",
        ret: Base::Num,
        args: &[Ty::Num, Ty::Small, Ty::Len],
    },
    Fun {
        name: "lerp",
        ret: Base::Num,
        args: &[Ty::Num, Ty::Num, Ty::Unit],
    },
    Fun {
        name: "sqrt",
        ret: Base::Num,
        args: &[Ty::Len],
    },
    Fun {
        name: "floor",
        ret: Base::Num,
        args: &[Ty::Num],
    },
    Fun {
        name: "round",
        ret: Base::Num,
        args: &[Ty::Num],
    },
    Fun {
        name: "sin",
        ret: Base::Num,
        args: &[Ty::Angle],
    },
    Fun {
        name: "cos",
        ret: Base::Num,
        args: &[Ty::Angle],
    },
    Fun {
        name: "atan2",
        ret: Base::Num,
        args: &[Ty::Num, Ty::Num],
    },
    Fun {
        name: "mod",
        ret: Base::Num,
        args: &[Ty::Num, Ty::Count],
    },
    Fun {
        name: "rand",
        ret: Base::Num,
        args: &[Ty::Small, Ty::Len],
    },
    Fun {
        name: "polar",
        ret: Base::Vec,
        args: &[Ty::Len, Ty::Angle],
    },
    Fun {
        name: "abs",
        ret: Base::Vec,
        args: &[Ty::Vec],
    },
    Fun {
        name: "abs",
        ret: Base::Num,
        args: &[Ty::Num],
    },
    Fun {
        name: "lerp",
        ret: Base::Vec,
        args: &[Ty::Vec, Ty::Vec, Ty::Unit],
    },
    Fun {
        name: "shade",
        ret: Base::Color,
        args: &[Ty::Color, Ty::Shift],
    },
    Fun {
        name: "mix",
        ret: Base::Color,
        args: &[Ty::Color, Ty::Color, Ty::Unit],
    },
    Fun {
        name: "fade",
        ret: Base::Color,
        args: &[Ty::Color, Ty::Unit],
    },
    Fun {
        name: "line",
        ret: Base::Color,
        args: &[Ty::Color],
    },
    Fun {
        name: "ramp",
        ret: Base::Color,
        args: &[Ty::Color, Ty::Count],
    },
    Fun {
        name: "lerp",
        ret: Base::Color,
        args: &[Ty::Color, Ty::Color, Ty::Unit],
    },
];

const NOISES: &[&str] = &["simplex", "perlin", "worley", "value"];
const BLENDS: &[&str] = &[
    "normal",
    "multiply",
    "screen",
    "overlay",
    "darken",
    "lighten",
    "dodge",
    "burn",
    "hard_light",
    "soft_light",
    "difference",
    "exclusion",
    "hue",
    "saturation",
    "color",
    "luminosity",
    "add",
    "erase",
];
const EASES: &[&str] = &["linear", "in", "out", "in_out", "hold"];
const EDGES: &[&str] = &["top", "bottom", "left", "right", "cx", "cy"];
const MEASURES: &[&str] = &[
    "x", "y", "w", "h", "cx", "cy", "left", "right", "top", "bottom", "area", "geom.w", "geom.h",
];
const DIRS: &[&str] = &["down", "up", "left", "right"];
const PAL_NAMES: &[&str] = &[
    "ink", "sky", "leaf", "stone", "skin", "rim", "glint", "shade", "peach", "moss", "dusk", "ember",
];
const PART_NAMES: &[&str] = &[
    "body", "head", "eye", "leaf", "cap", "arm", "tail", "dot", "mark", "petal", "stem", "wing",
];
const WORDS: &[&str] = &["idle", "blink", "wide", "slim", "hot", "cold"];
const HOUSE: &[&str] = &[
    "ink",
    "glint",
    "eye_glint",
    "rim",
    "shadow",
    "sclera",
    "sclera_shade",
    "mouth",
    "tongue",
    "blush",
    "dew",
    "iris",
];
const TARGET_JUNK: &[&str] = &[
    "fps:1s",
    "profile:",
    "profile:bogus",
    "clip:\"s\"",
    "dur:",
    "scale:0",
    "t:x",
    "bg:nope",
    "as:",
    "each k:[]",
    "fps:0",
];

const OFF_CANVAS: &str = "-999,-999";

const KEY_JUNK: &[&str] = &["\"str\"", "simplex(9)", "50%", "12x8", "[1 2]", "none", "@nope"];
const KEY_TAILS: &[&str] = &[
    " at:[0 1]",
    " at:[1 .5 0]",
    " at:[0 2 -1]",
    " at:.5",
    " ease:3",
    " ease:[in]",
    " ease:[linear linear linear linear linear]",
    " at:[0 .5]",
];

const TYPED_JUNK: &[(Ty, &[&str])] = &[
    (Ty::Jitter, &["1,0", "2", "1,-5"]),
    (Ty::Dash, &["red", "0,0", "[]"]),
    (Ty::PathData, &["\"bad\"", "\"M\"", "\"M0 0 L\"", "\"\"", "\"Z Z\""]),
    (Ty::Size, &["10x", "x5", "0x0", "-4x4", "50%x"]),
    (Ty::Sides, &["2", "0", "1.5"]),
    (Ty::Corners, &["[1 2 3]", "1,2,3", "[]"]),
    (Ty::Pts, &["[1,2]", "[]", "[1]"]),
    (Ty::Radius, &["0", "-3"]),
];

const UNIMPLEMENTED_FX: &[&str] = &["bevel", "displace", "halftone", "posterize"];

const NOTES: &[&str] = &[
    "\"note\"",
    "\"a \\\"quoted\\\" word\"",
    "\"back\\\\slash\"",
    "\"two\\nlines\"",
    "\"\"",
];

// `{}` is replaced by a real path.
const BAD_PATCHES: &[&str] = &["{}.zz", "{}.9", "{}.*.x", "{}.-1", "0", "*.x", "*.*", "{}.{}"];

const JUNK: &[&str] = &[
    "nope",
    "#zzz",
    "\"s\"",
    "[1 2]",
    "0..3",
    "@nope",
    "p.nope",
    "1/0",
    "none",
    "(1,2)",
    "true",
    "-",
    "p",
    "oklch(1)",
    "grad(nope)",
    "key(1)",
    "pick([])",
    "pick(3)",
    "rand(1 2 3)",
    "contrast(@nope)",
    "visible(@nope)",
    "mod(3 0)",
    "[]",
    "200%",
];
const JUNK_STMTS: &[&str] = &[
    "shader s",
    "light l at:1,2",
    "brush b",
    "brush b size:4\n  circle 3",
    "artboard fill:",
    "artboard",
    "fx blur 3",
    "slot orphan",
    "play missing",
    "mask @nope",
    ".part off",
    "bogus 1 2",
    "circle",
    "rect 10",
    "let fx = 1",
    "assert",
    "else",
    "each",
    "if",
    "use nope/zzz",
    "palette",
    "def",
    "key(0 1)",
    "star 5 r:20 inner:",
    "copy @nope",
    "inside @nope",
    "mirror z",
    "stroke:3",
    "assert 1 +",
];

struct Chk {
    name: &'static str,
    refs: usize,
    keys: &'static [Key],
    number: bool,
}

const TOL: Key = ("tol", Ty::Small, 5);

const CHECKS: &[Chk] = &[
    Chk {
        name: "level",
        refs: 2,
        keys: &[TOL],
        number: false,
    },
    Chk {
        name: "aligned",
        refs: 2,
        keys: &[("edge", Ty::Edge, 12), TOL],
        number: false,
    },
    Chk {
        name: "symmetric",
        refs: 2,
        keys: &[("about", Ty::Coord, 8), TOL],
        number: false,
    },
    Chk {
        name: "inside",
        refs: 2,
        keys: &[("margin", Ty::Small, 8)],
        number: false,
    },
    Chk {
        name: "overlaps",
        refs: 2,
        keys: &[],
        number: false,
    },
    Chk {
        name: "touches",
        refs: 2,
        keys: &[TOL],
        number: false,
    },
    Chk {
        name: "gap",
        refs: 2,
        keys: &[],
        number: true,
    },
    Chk {
        name: "dist",
        refs: 2,
        keys: &[],
        number: true,
    },
    Chk {
        name: "centered",
        refs: 1,
        keys: &[("in", Ty::Ref, 16), TOL],
        number: false,
    },
    Chk {
        name: "aspect",
        refs: 1,
        keys: &[],
        number: true,
    },
    Chk {
        name: "on_grid",
        refs: 1,
        keys: &[],
        number: false,
    },
    Chk {
        name: "same_size",
        refs: 3,
        keys: &[TOL],
        number: false,
    },
    Chk {
        name: "stroke_widths",
        refs: 1,
        keys: &[],
        number: false,
    },
];

#[derive(Clone)]
struct Bind {
    name: String,
    base: Base,
}

#[derive(Clone)]
struct Node {
    name: String,
    parts: Vec<Node>,
    is_shape: bool,
    /// How many copies a loop makes of this name, or 0 for one node.
    copies: usize,
}

#[derive(Clone)]
struct Addr {
    text: String,
    geom: bool,
    has_children: bool,
    /// A bare family name, which reaches every copy.
    many: bool,
}

impl Addr {
    fn collect(nodes: &[Node], prefix: &str, depth: usize, out: &mut Vec<Addr>) {
        for n in nodes {
            let base = format!("{prefix}{}", n.name);
            let has_children = n.parts.iter().any(|p| p.copies == 0);
            let one = |text: String| Addr {
                text,
                geom: n.is_shape,
                has_children,
                many: false,
            };
            if n.copies == 0 {
                out.push(one(base.clone()));
            } else {
                out.push(Addr {
                    many: true,
                    ..one(base.clone())
                });
                out.push(one(format!("{base}.0")));
                if n.copies > 1 {
                    out.push(one(format!("{base}.{}", n.copies - 1)));
                }
            }
            if depth < 2 {
                let inner = if n.copies > 0 { format!("{base}.0.") } else { format!("{base}.") };
                Self::collect(&n.parts, &inner, depth + 1, out);
            }
        }
    }
}

#[derive(Clone)]
struct Param {
    name: String,
    ty: Ty,
    default: String,
}

#[derive(Clone)]
struct Sig {
    name: String,
    params: Vec<Param>,
    slots: Vec<String>,
    parts: Vec<Node>,
    anims: Vec<String>,
    declares_names: bool,
    needs: Option<String>,
}

impl Sig {
    fn needs_call_name(&self) -> bool {
        self.declares_names || !self.slots.is_empty()
    }
}

#[derive(Clone)]
struct Callable {
    name: String,
    params: Vec<Param>,
}

#[derive(Clone)]
struct Pal {
    name: String,
    entries: Vec<(String, Option<usize>)>,
}

#[derive(Clone, Default)]
struct Module {
    colors: Vec<String>,
    ramps: Vec<String>,
    defs: Vec<Sig>,
    styles: Vec<String>,
    finishes: Vec<Callable>,
    rules: Vec<Callable>,
    anims: Vec<String>,
    pals: Vec<Pal>,
}

impl Module {
    fn merge(&mut self, o: &Module) {
        self.colors.extend(o.colors.iter().cloned());
        self.ramps.extend(o.ramps.iter().cloned());
        self.defs.extend(o.defs.iter().cloned());
        self.styles.extend(o.styles.iter().cloned());
        self.finishes.extend(o.finishes.iter().cloned());
        self.rules.extend(o.rules.iter().cloned());
        self.anims.extend(o.anims.iter().cloned());
        self.pals.extend(o.pals.iter().cloned());
    }

    fn add_pal(&mut self, prefix: &str, pal: &Pal) {
        for (name, stops) in &pal.entries {
            self.colors.push(format!("{prefix}.{name}"));
            if let Some(n) = stops {
                self.ramps.push(format!("{prefix}.{name}"));
                self.colors.extend((0..*n).map(|k| format!("{prefix}.{name}.{k}")));
                self.colors.push(format!("{prefix}.{name}.line"));
            }
        }
    }

    fn only(&self, names: &[String]) -> Module {
        let keep = |n: &String| names.contains(n);
        Module {
            colors: vec![],
            ramps: vec![],
            defs: self.defs.iter().filter(|d| keep(&d.name)).cloned().collect(),
            styles: self.styles.iter().filter(|s| keep(s)).cloned().collect(),
            finishes: self.finishes.iter().filter(|f| keep(&f.name)).cloned().collect(),
            rules: self.rules.iter().filter(|r| keep(&r.name)).cloned().collect(),
            anims: self.anims.iter().filter(|a| keep(a)).cloned().collect(),
            pals: vec![],
        }
    }

    fn export_names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.defs.iter().map(|d| d.name.clone()).collect();
        v.extend(self.styles.iter().cloned());
        v.extend(self.finishes.iter().map(|f| f.name.clone()));
        v.extend(self.rules.iter().map(|r| r.name.clone()));
        v.extend(self.anims.iter().cloned());
        v
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    Own,
    Copies(usize),
    Conditional,
}

#[derive(Clone, Copy)]
struct Ctx {
    ind: usize,
    mult: usize,
    framed: bool,
    scope: Scope,
    direct: bool,
}

impl Ctx {
    fn child(self) -> Ctx {
        Ctx {
            ind: self.ind + 1,
            direct: false,
            ..self
        }
    }

    fn looped(self, times: usize) -> Ctx {
        let scope = match self.scope {
            Scope::Own => Scope::Copies(times.max(1)),
            _ => Scope::Conditional,
        };
        Ctx {
            mult: self.mult * times.max(1),
            scope,
            ..self
        }
    }

    fn hidden(self) -> Ctx {
        Ctx {
            scope: Scope::Conditional,
            ..self
        }
    }
}

struct Gen<'u, 'd> {
    u: &'u mut Unstructured<'d>,
    wrong: usize,
    cw: i32,
    ch: i32,
    lines: Vec<String>,
    env: Vec<Bind>,
    names: Vec<Node>,
    parts: Vec<Node>,
    scope: Module,
    own: Module,
    counter: u32,
    budget: u32,
    framed: bool,
    in_anim: bool,
    in_finish: bool,
    junk_ok: bool,
    declared: u32,
    force_fx: bool,
    lists: Vec<(String, usize)>,
    filled: Vec<Node>,
    hosts: Vec<Vec<String>>,
}

pub fn generate(u: &mut Unstructured<'_>) -> Vec<u8> {
    Gen::new(u).project()
}

pub fn show(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut out = String::new();
    for (i, part) in text.split('\0').enumerate() {
        if i == 0 {
            let _ = write!(out, "==== art/t.ramo ====\n{part}");
        } else {
            let (path, src) = part.split_once('\n').unwrap_or((part, ""));
            let _ = write!(out, "==== {path} ====\n{src}");
        }
    }
    out
}

impl<'u, 'd> Gen<'u, 'd> {
    fn new(u: &'u mut Unstructured<'d>) -> Self {
        let mut g = Gen {
            u,
            wrong: 0,
            cw: 64,
            ch: 64,
            lines: vec![],
            env: vec![],
            names: vec![],
            parts: vec![],
            scope: Module::default(),
            own: Module::default(),
            counter: 0,
            budget: 0,
            framed: true,
            in_anim: false,
            in_finish: false,
            junk_ok: false,
            declared: 0,
            force_fx: false,
            lists: vec![],
            filled: vec![],
            hosts: vec![],
        };
        g.wrong = [0, 0, 0, 0, 0, 0, 6, 14][g.n(0, 7)];
        g.cw = *g.pick(&[32, 48, 64, 96, 128, 200, 256]);
        g.ch = if g.chance(10) {
            g.cw
        } else {
            *g.pick(&[32, 48, 64, 96, 128, 200])
        };
        g
    }

    fn n(&mut self, lo: usize, hi: usize) -> usize {
        self.u.int_in_range(lo..=hi).unwrap_or(lo)
    }

    fn chance(&mut self, k: usize) -> bool {
        self.n(0, 15) >= 16usize.saturating_sub(k)
    }

    fn bad(&mut self) -> bool {
        self.wrong > 0 && self.n(0, 255) >= 256 - self.wrong
    }

    fn pick<'t, T>(&mut self, xs: &'t [T]) -> &'t T {
        let i = self.n(0, xs.len() - 1);
        &xs[i]
    }

    fn weighted<T: Copy>(&mut self, xs: &[(usize, T)]) -> T {
        let total: usize = xs.iter().map(|x| x.0).sum();
        let mut at = if total == 0 { 0 } else { self.n(0, total - 1) };
        for &(w, v) in xs {
            if at < w {
                return v;
            }
            at -= w;
        }
        xs[0].1
    }

    fn fresh(&mut self, prefix: &str) -> String {
        self.counter += 1;
        format!("{prefix}{}", self.counter)
    }

    fn line(&mut self, ind: usize, s: impl AsRef<str>) {
        self.lines.push(format!("{}{}", "  ".repeat(ind), s.as_ref()));
    }

    fn lit(&mut self, lo: i32, hi: i32) -> String {
        let v = self.u.int_in_range(lo..=hi).unwrap_or(lo);
        match self.n(0, 9) {
            0 if v < hi => format!("{v}.5"),
            1 if v < hi => format!("{v}.25"),
            _ => v.to_string(),
        }
    }

    fn unit(&mut self) -> String {
        self.pick(&["0", ".5", ".25", ".75", "1", ".1", ".9", ".35", ".6"])
            .to_string()
    }

    fn junk(&mut self, ty: Ty) -> String {
        match TYPED_JUNK.iter().find(|(t, _)| *t == ty) {
            Some((_, own)) if self.chance(10) => self.pick(own).to_string(),
            _ => self.pick(JUNK).to_string(),
        }
    }

    fn vars(&self, base: Base) -> Vec<String> {
        self.env
            .iter()
            .filter(|b| b.base == base)
            .map(|b| b.name.clone())
            .collect()
    }

    fn atom(s: String) -> String {
        let mut depth = 0;
        let mut closes_at_end = s.starts_with('(');
        for (i, c) in s.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 && i + 1 < s.len() {
                        closes_at_end = false;
                    }
                }
                _ => {}
            }
        }
        if closes_at_end || (!s.contains([' ', ',']) && !s.starts_with('-')) {
            s
        } else {
            format!("({s})")
        }
    }

    fn value(&mut self, ty: Ty, d: u8) -> String {
        if self.bad() {
            return self.junk(ty);
        }
        match ty {
            Ty::Bool => {
                let c = self.cond(d);
                if c.starts_with('@') {
                    "true".into()
                } else {
                    Self::atom(c)
                }
            }
            Ty::Color => self.color(d),
            Ty::Paint => self.paint(d),
            Ty::Field => self.field(d),
            Ty::Pts => self.points_list(),
            Ty::Stroke => self.stroke(d),
            Ty::Size => self.size(),
            Ty::Radii => {
                let (a, b) = (self.radius_part(), self.radius_part());
                format!("{a},{b}")
            }
            Ty::Radius => self.radius_part(),
            Ty::Corners => self.corners(),
            Ty::Warp => self.warp(),
            Ty::Time => self.num_lit(Ty::Time),
            t if t.base() == Base::Num => self.number(t, d),
            t if t.base() == Base::Vec => self.vector(t, d),
            t => self.word(t),
        }
    }

    fn warp(&mut self) -> String {
        let stage = |g: &mut Self| match g.n(0, 2) {
            0 => format!("cylinder({})", g.lit(4, 120)),
            1 => format!("cylinder({} axis:{})", g.lit(4, 120), g.word(Ty::Axis)),
            _ => {
                let pts: Vec<String> = (0..4).map(|_| format!("{},{}", g.lit(-20, 120), g.lit(-20, 120))).collect();
                format!("corners({})", pts.join(" "))
            }
        };
        match self.n(0, 3) {
            0 => "none".into(),
            1 => {
                let n = self.n(1, 3);
                let stages: Vec<String> = (0..n).map(|_| stage(self)).collect();
                format!("[{}]", stages.join(" "))
            }
            _ => stage(self),
        }
    }

    fn corners(&mut self) -> String {
        if self.chance(11) {
            return self.number(Ty::Small, 1);
        }
        let four: Vec<String> = (0..4).map(|_| self.lit(0, 6)).collect();
        match self.n(0, 1) {
            0 => format!("[{}]", four.join(" ")),
            _ => four.join(","),
        }
    }

    fn radius_part(&mut self) -> String {
        if self.framed && self.chance(2) {
            let p = self.lit(5, 60);
            return format!("{p}%");
        }
        self.number(Ty::Len, 1)
    }

    fn size(&mut self) -> String {
        if self.framed && self.chance(3) {
            let (a, b) = (self.lit(10, 100), self.lit(10, 100));
            return format!("{a}%x{b}%");
        }
        let (w, h) = (self.lit(2, self.cw / 3), self.lit(2, self.ch / 3));
        format!("{w}x{h}")
    }

    fn number(&mut self, ty: Ty, d: u8) -> String {
        #[derive(Clone, Copy)]
        enum N {
            Lit,
            Var,
            Bin,
            Fun,
            Cond,
            Meas,
            Time,
            Key,
            Idx,
        }
        let vars = self.vars(Base::Num);
        let lists = self.vars(Base::List);
        let open = d > 0;
        let measurable = matches!(ty, Ty::Num | Ty::Coord | Ty::Len) && self.names.iter().any(|n| n.is_shape);
        let pick = self.weighted(&[
            (9, N::Lit),
            (if vars.is_empty() { 0 } else { 4 }, N::Var),
            (if open { 3 } else { 0 }, N::Bin),
            (if open { 2 } else { 0 }, N::Fun),
            (if open { 1 } else { 0 }, N::Cond),
            (if measurable { 1 } else { 0 }, N::Meas),
            (
                if matches!(ty, Ty::Num | Ty::Coord | Ty::Angle | Ty::Unit | Ty::Signed | Ty::Hue) {
                    2
                } else {
                    0
                },
                N::Time,
            ),
            (if open { 1 } else { 0 }, N::Key),
            (if lists.is_empty() { 0 } else { 2 }, N::Idx),
        ]);
        let text = match pick {
            N::Lit => self.num_lit(ty),
            N::Var => {
                let v = self.pick(&vars).clone();
                if self.chance(1) && !v.contains('.') {
                    format!("-{v}")
                } else {
                    v
                }
            }
            N::Bin => {
                let l = self.number(ty, d - 1);
                let r = self.number(ty, d - 1);
                let op = *self.pick(&["+", "-", "*", "/"]);
                let (l, r) = (Self::atom(l), if op == "/" { self.lit(2, 4) } else { Self::atom(r) });
                if !l.contains(' ') && !r.contains(' ') && self.chance(6) {
                    format!("{l}{op}{r}")
                } else {
                    format!("({l} {op} {r})")
                }
            }
            N::Fun => {
                let funs: Vec<&Fun> = FUNS.iter().filter(|f| f.ret == Base::Num).collect();
                let f = *self.pick(&funs);
                self.call(f, d - 1)
            }
            N::Cond => {
                let c = self.cond(d - 1);
                let (a, b) = (self.number(ty, d - 1), self.number(ty, d - 1));
                format!("(if {c} then {a} else {b})")
            }
            N::Meas => {
                let r = self.word(Ty::Ref);
                let m = if self.bad() {
                    *self.pick(&["geom.nope", "nope", "geom"])
                } else {
                    *self.pick(MEASURES)
                };
                format!("{r}.{m}")
            }
            N::Idx => {
                let name = self.pick(&lists).clone();
                let len = self.lists.iter().find(|l| l.0 == name).map_or(1, |l| l.1);
                let at = if self.bad() { 9 } else { self.n(0, len - 1) };
                format!("{name}.{at}")
            }
            N::Time => match self.n(0, 3) {
                0 => "t".into(),
                1 => format!("(t * {})", self.lit(10, 60)),
                2 => format!("sin(t * {})", self.lit(30, 360)),
                _ => format!("(t + {})", self.lit(1, 20)),
            },
            N::Key => self.key_call(Base::Num, d - 1),
        };
        if matches!(pick, N::Lit) {
            text
        } else {
            self.bounded(ty, text)
        }
    }

    fn bounded(&self, ty: Ty, text: String) -> String {
        let (cw, ch) = (self.cw, self.ch);
        let half = cw.min(ch) / 2;
        let (lo, hi) = match ty {
            Ty::Len | Ty::Radius => ("1".to_string(), half.to_string()),
            Ty::Sides => ("3".into(), "10".into()),
            Ty::Count => ("1".into(), "6".into()),
            Ty::Small => ("0".into(), "12".into()),
            Ty::Unit | Ty::Tension => ("0".into(), "1".into()),
            Ty::Strength => ("0".into(), ".2".into()),
            Ty::Coord => ((-cw / 2).to_string(), (cw / 2).to_string()),
            Ty::Angle => ("-360".into(), "360".into()),
            Ty::Hue => ("-90".into(), "90".into()),
            Ty::Signed => ("-.5".into(), ".5".into()),
            Ty::Shift => ("-3".into(), "3".into()),
            _ => return text,
        };
        format!("clamp({text} {lo} {hi})")
    }

    fn num_lit(&mut self, ty: Ty) -> String {
        let (cw, ch) = (self.cw, self.ch);
        match ty {
            Ty::Len => self.lit(2, cw.min(ch) / 3),
            Ty::Coord => self.lit(-cw / 3, cw / 3),
            Ty::Unit | Ty::Tension => self.unit(),
            Ty::Small => self.lit(0, 8),
            Ty::Count => self.lit(1, 6).split('.').next().unwrap_or("1").to_string(),
            Ty::Sides => self.n(3, 8).to_string(),
            Ty::Strength => self.pick(&[".02", ".04", ".06", ".1", "0"]).to_string(),
            Ty::Signed => self.pick(&["-.3", "-.1", ".1", ".3", "0", ".05"]).to_string(),
            Ty::Shift => self.lit(-3, 3).split('.').next().unwrap_or("0").to_string(),
            Ty::Hue => self.lit(-90, 90),
            Ty::Angle => {
                let v = self.lit(-180, 180);
                match self.n(0, 5) {
                    0 => format!("{v}deg"),
                    1 => ".25turn".into(),
                    _ => v,
                }
            }
            Ty::Time => {
                let v = self.lit(1, 9);
                match self.n(0, 3) {
                    0 => format!("{}ms", self.n(1, 9) * 100),
                    1 => format!("{v}s"),
                    2 => format!(".{}s", self.n(1, 9)),
                    _ => v,
                }
            }
            _ => self.lit(-20, 60),
        }
    }

    fn vector(&mut self, ty: Ty, d: u8) -> String {
        let pair = |g: &mut Self, a: Ty, b: Ty| {
            let (x, y) = (g.number(a, d.min(1)), g.number(b, d.min(1)));
            format!("{x},{y}")
        };
        match ty {
            Ty::Offset => pair(self, Ty::Coord, Ty::Small),
            Ty::DirVec => self.pick(&["1,1", "-1,-1", "1,0", "0,1", "-1,1", "0,-1"]).to_string(),
            Ty::Scale => match self.n(0, 3) {
                0 => "-1,1".into(),
                1 => self.pick(&[".5,.5", "1,.8", ".7,1.2", "1,-1", "2,1"]).to_string(),
                _ => self.pick(&["1.5", ".5", "2", ".8", "1.2"]).to_string(),
            },
            Ty::Skew => format!("{},{}", self.lit(-40, 40), self.lit(-20, 20)),
            Ty::Jitter => {
                let (a, w) = (self.lit(0, 3), self.lit(20, 80));
                format!("{a},{w}")
            }
            Ty::Dash if self.chance(4) => {
                let items: Vec<String> = (0..self.n(1, 4)).map(|_| self.lit(1, 6)).collect();
                format!("[{}]", items.join(" "))
            }
            Ty::Dash => pair(self, Ty::Count, Ty::Count),
            Ty::Taper => match self.n(0, 2) {
                0 => "0,.6".into(),
                _ => self.unit(),
            },
            _ => {
                let vars = self.vars(Base::Vec);
                #[derive(Clone, Copy)]
                enum V {
                    Lit,
                    Pct,
                    Var,
                    Fun,
                    Bin,
                    Key,
                    Cond,
                }
                let open = d > 0;
                let v = self.weighted(&[
                    (8, V::Lit),
                    (if self.framed && matches!(ty, Ty::At) { 2 } else { 0 }, V::Pct),
                    (if vars.is_empty() { 0 } else { 3 }, V::Var),
                    (if open { 1 } else { 0 }, V::Fun),
                    (if open { 2 } else { 0 }, V::Bin),
                    (if open { 1 } else { 0 }, V::Key),
                    (if open { 1 } else { 0 }, V::Cond),
                ]);
                match v {
                    V::Lit => {
                        if matches!(ty, Ty::At) && self.chance(1) {
                            OFF_CANVAS.to_string()
                        } else if matches!(ty, Ty::At) {
                            let (x, y) = (self.lit(0, self.cw), self.lit(0, self.ch));
                            format!("{x},{y}")
                        } else {
                            pair(self, Ty::Coord, Ty::Coord)
                        }
                    }
                    V::Pct => {
                        let (x, y) = (self.lit(0, 100), self.lit(0, 100));
                        let sign = if self.chance(3) { "-" } else { "" };
                        format!("{sign}{x}%,{y}%")
                    }
                    V::Var => self.pick(&vars).clone(),
                    V::Fun => {
                        let funs: Vec<&Fun> = FUNS.iter().filter(|f| f.ret == Base::Vec).collect();
                        let f = *self.pick(&funs);
                        self.call(f, d - 1)
                    }
                    V::Bin => {
                        let (a, b) = (self.vector(Ty::Vec, d - 1), self.vector(Ty::Vec, d - 1));
                        match self.n(0, 3) {
                            0 => format!("({} + {})", Self::atom(a), Self::atom(b)),
                            1 => format!("({} - {})", Self::atom(a), Self::atom(b)),
                            2 => format!("({} * {})", Self::atom(a), self.lit(1, 3)),
                            _ => format!("({} / 2)", Self::atom(a)),
                        }
                    }
                    V::Key => self.key_call(Base::Vec, d - 1),
                    V::Cond => {
                        let c = self.cond(d - 1);
                        let (a, b) = (self.vector(ty, d - 1), self.vector(ty, d - 1));
                        format!("(if {c} then {} else {})", Self::atom(a), Self::atom(b))
                    }
                }
            }
        }
    }

    fn call(&mut self, f: &Fun, d: u8) -> String {
        let mut args: Vec<String> = f.args.iter().map(|&t| self.value(t, d)).collect();
        if f.name == "rand" && self.chance(3) {
            args.clear();
        }
        if f.name == "rand" && self.chance(4) {
            let s = self.n(0, 5);
            args.push(format!("seed:{s}"));
        }
        format!("{}({})", f.name, args.join(" "))
    }

    fn cond(&mut self, d: u8) -> String {
        #[derive(Clone, Copy)]
        enum B {
            Cmp,
            Logic,
            Not,
            Var,
            In,
            Node,
            Word,
            Lit,
        }
        let vars = self.vars(Base::Bool);
        let open = d > 0;
        let b = self.weighted(&[
            (6, B::Cmp),
            (if open { 2 } else { 0 }, B::Logic),
            (if open { 1 } else { 0 }, B::Not),
            (if vars.is_empty() { 0 } else { 2 }, B::Var),
            (1, B::In),
            (if self.names.is_empty() { 0 } else { 1 }, B::Node),
            (2, B::Word),
            (1, B::Lit),
        ]);
        match b {
            B::Cmp => {
                let (a, c) = (self.number(Ty::Num, d.min(1)), self.number(Ty::Num, d.min(1)));
                let op = *self.pick(&["<", "<=", ">", ">=", "==", "!=", "~="]);
                format!("{} {op} {}", Self::atom(a), Self::atom(c))
            }
            B::Logic => {
                let (a, c) = (self.cond(d - 1), self.cond(d - 1));
                let op = *self.pick(&["and", "or"]);
                format!("{a} {op} {c}")
            }
            B::Not => format!("not {}", Self::atom(self.cond(d - 1))),
            B::Var => self.pick(&vars).clone(),
            B::In => {
                let (a, lo, hi) = (self.number(Ty::Num, 0), self.lit(0, 5), self.lit(6, 20));
                format!("{} in {lo}..{hi}", Self::atom(a))
            }
            B::Node => {
                let n = self.pick(&self.names.clone()).name.clone();
                format!("@{n}")
            }
            B::Word => {
                let w = *self.pick(WORDS);
                let who = *self.pick(&["clip", "target"]);
                format!("{who} == {w}")
            }
            B::Lit => self.pick(&["true", "false"]).to_string(),
        }
    }

    fn hex(&mut self) -> String {
        let digits = *self.pick(&[3, 6, 6, 6, 8]);
        let mut s = String::from("#");
        for _ in 0..digits {
            let d = self.n(0, 15);
            let _ = write!(s, "{d:x}");
        }
        s
    }

    fn color(&mut self, d: u8) -> String {
        #[derive(Clone, Copy)]
        enum C {
            Pal,
            Hex,
            Var,
            Fun,
            Pick,
            Cond,
            Oklch,
        }
        let vars = self.vars(Base::Color);
        let open = d > 0;
        let pal = !self.scope.colors.is_empty();
        let c = self.weighted(&[
            (if pal { 8 } else { 0 }, C::Pal),
            (3, C::Hex),
            (if vars.is_empty() { 0 } else { 3 }, C::Var),
            (if open { 4 } else { 0 }, C::Fun),
            (if open { 1 } else { 0 }, C::Pick),
            (if open { 1 } else { 0 }, C::Cond),
            (1, C::Oklch),
        ]);
        match c {
            C::Pal => {
                let c = self.pick(&self.scope.colors.clone()).clone();
                if self.chance(1) && !c.ends_with(".line") && c.matches('.').count() == 1 {
                    format!("{c}.line")
                } else if self.scope.ramps.contains(&c) && self.chance(1) {
                    format!("{c}.{}", self.pick(&["2.5", "9", "0.5"]))
                } else {
                    c
                }
            }
            C::Hex => self.hex(),
            C::Var => self.pick(&vars).clone(),
            C::Fun => {
                let funs: Vec<&Fun> = FUNS.iter().filter(|f| f.ret == Base::Color).collect();
                let f = *self.pick(&funs);
                self.call(f, d - 1)
            }
            C::Pick => {
                let items: Vec<String> = (0..self.n(1, 3)).map(|_| self.color(0)).collect();
                format!("pick([{}])", items.join(" "))
            }
            C::Cond => {
                let c = self.cond(d - 1);
                let (a, b) = (self.color(d - 1), self.color(d - 1));
                format!("(if {c} then {a} else {b})")
            }
            C::Oklch => {
                let l = if self.chance(4) {
                    format!("{}%", self.n(10, 95))
                } else {
                    self.unit()
                };
                let (c, h) = (
                    self.pick(&[".02", ".07", ".12", ".2"]).to_string(),
                    self.lit(0, 360),
                );
                format!("oklch({l} {c} {h})")
            }
        }
    }

    fn stops(&mut self, lo: usize, hi: usize) -> String {
        let count = if lo > 1 && self.chance(1) { 1 } else { self.n(lo, hi) };
        let mut out = Vec::new();
        for i in 0..count {
            let c = self.color(1);
            out.push(if i > 0 && i + 1 < count && self.chance(6) {
                format!("{c},{}", self.unit())
            } else {
                c
            });
        }
        out.join(" ")
    }

    fn paint(&mut self, d: u8) -> String {
        #[derive(Clone, Copy)]
        enum P {
            Color,
            Grad,
            Radial,
            Conic,
            Field,
            None,
        }
        let p = self.weighted(&[
            (10, P::Color),
            (if d > 0 { 3 } else { 0 }, P::Grad),
            (if d > 0 { 1 } else { 0 }, P::Radial),
            (if d > 0 { 1 } else { 0 }, P::Conic),
            (if d > 0 { 1 } else { 0 }, P::Field),
            (1, P::None),
        ]);
        match p {
            P::Color => self.color(d),
            P::Grad => {
                let stops = self.stops(2, 4);
                match self.n(0, 4) {
                    0 => {
                        let (cw, ch) = (self.cw, self.ch);
                        let a = format!("{},{}", self.lit(0, cw), self.lit(0, ch));
                        let b = if self.bad() {
                            self.unit()
                        } else {
                            format!("{},{}", self.lit(0, cw), self.lit(0, ch))
                        };
                        format!("grad({a} {b} {stops})")
                    }
                    1 => format!("grad({} {stops})", self.number(Ty::Angle, 0)),
                    2 => format!("grad({stops})"),
                    _ => format!("grad({} {stops})", self.pick(DIRS)),
                }
            }
            P::Radial => {
                let stops = self.stops(2, 3);
                let mut extra = String::new();
                if self.chance(8) {
                    let _ = write!(extra, " at:{},{}", self.unit(), self.unit());
                }
                if self.chance(6) {
                    let _ = write!(extra, " r:{}", self.lit(5, 100));
                }
                format!("radial({stops}{extra})")
            }
            P::Conic => {
                let stops = self.stops(2, 4);
                let extra = if self.chance(6) {
                    format!(" from:{}", self.lit(0, 180))
                } else {
                    String::new()
                };
                format!("conic({stops}{extra})")
            }
            P::Field => {
                let f = self.field(d.min(2));
                let target = if self.bad() {
                    self.pick(&["[]", "[#fff]", "none"]).to_string()
                } else if !self.scope.ramps.is_empty() && self.chance(10) {
                    self.pick(&self.scope.ramps.clone()).clone()
                } else {
                    let (a, b, c) = (self.color(0), self.color(0), self.color(0));
                    format!("[{a} {b} {c}]")
                };
                format!("{f} -> {target}")
            }
            P::None => "none".into(),
        }
    }

    fn field(&mut self, d: u8) -> String {
        #[derive(Clone, Copy)]
        enum F {
            Noise,
            Wrap,
            Arith,
            Unary,
            Neg,
            Pair,
            Var,
        }
        let vars = self.vars(Base::Field);
        let open = d > 0;
        let f = self.weighted(&[
            (6, F::Noise),
            (if open { 2 } else { 0 }, F::Wrap),
            (if open { 2 } else { 0 }, F::Arith),
            (if open { 1 } else { 0 }, F::Unary),
            (if open { 1 } else { 0 }, F::Neg),
            (if open { 1 } else { 0 }, F::Pair),
            (if vars.is_empty() { 0 } else { 2 }, F::Var),
        ]);
        match f {
            F::Noise => {
                let kind = *self.pick(NOISES);
                let mut extra = String::new();
                if self.chance(5) {
                    let _ = write!(extra, " seed:{}", self.n(0, 9));
                }
                if self.chance(5) {
                    let _ = write!(extra, " oct:{}", self.n(1, 4));
                }
                if self.chance(5) {
                    let _ = write!(extra, " wrap:{}", self.n(8, 96));
                    if self.chance(2) {
                        let _ = write!(extra, ",{}", self.n(8, 96));
                    }
                }
                format!("{kind}({}{extra})", self.lit(4, 60))
            }
            F::Wrap => {
                let inner = if self.chance(1) { self.lit(2, 9) } else { self.field(d - 1) };
                let wrap = *self.pick(&["fbm", "ridged"]);
                let extra = if self.chance(4) {
                    format!(" oct:{}", self.n(1, 5))
                } else {
                    String::new()
                };
                format!("{wrap}({inner}{extra})")
            }
            F::Arith => {
                let a = self.field(d - 1);
                let op = *self.pick(&["+", "-", "*", "/"]);
                let b = match op {
                    "/" => self.lit(2, 4),
                    _ if self.chance(8) => self.field(d - 1),
                    _ => self.unit(),
                };
                if op != "/" && self.chance(3) {
                    format!("({b} {op} {a})")
                } else {
                    format!("({a} {op} {b})")
                }
            }
            F::Unary => {
                let inner = self.field(d - 1);
                format!("{}({inner})", self.pick(&["abs", "clamp"]))
            }
            F::Neg => format!("-{}", Self::atom(self.field(d - 1))),
            F::Pair => {
                let a = self.field(d - 1);
                let b = if self.chance(3) { self.lit(1, 3) } else { self.field(d - 1) };
                format!("{}({a} {b})", self.pick(&["min", "max"]))
            }
            F::Var => self.pick(&vars).clone(),
        }
    }

    fn stroke(&mut self, d: u8) -> String {
        let c = self.color(d.min(1));
        match self.n(0, 4) {
            0 => self.lit(1, 6),
            1 => {
                let n = self.n(2, 5);
                let widths: Vec<String> = (0..n).map(|_| self.lit(1, 9)).collect();
                format!("{c},[{}]", widths.join(" "))
            }
            _ => format!("{c},{}", self.number(Ty::Len, 1)),
        }
    }

    fn points_list(&mut self) -> String {
        let vars = self.vars(Base::Pts);
        if !vars.is_empty() && self.chance(8) {
            return self.pick(&vars).clone();
        }
        if self.chance(5) {
            let n = self.n(3, 8);
            let (a, b) = (self.lit(2, 20), self.lit(2, 20));
            return format!("[each k in 0..{n} -> (k*{a}),(sin(k*{b})*10)]");
        }
        let pts = self.curve_points();
        format!("[{}]", pts.join(" "))
    }

    fn curve_points(&mut self) -> Vec<String> {
        let n = if self.bad() { 1 } else { self.n(2, 6) };
        (0..n).map(|_| self.vector(Ty::Vec, 1)).collect()
    }

    fn word(&mut self, ty: Ty) -> String {
        match ty {
            Ty::Blend => self.pick(BLENDS).to_string(),
            Ty::Order => self.pick(&["behind", "front", "1", "-1", "3"]).to_string(),
            Ty::Cap => self.pick(&["round", "butt", "square"]).to_string(),
            Ty::Join => self.pick(&["round", "miter", "bevel"]).to_string(),
            Ty::Axis => self.pick(&["x", "y"]).to_string(),
            Ty::Edge => self.pick(EDGES).to_string(),
            Ty::StyleRef => self.pick(&self.scope.styles.clone()).clone(),
            Ty::Ref => {
                let shapes: Vec<Node> = self.names.iter().filter(|n| n.is_shape).cloned().collect();
                let from = if shapes.is_empty() || self.chance(1) {
                    self.names.clone()
                } else {
                    shapes
                };
                if from.is_empty() {
                    return "@nope".into();
                }
                let n = self.pick(&from).clone();
                let mut r = format!("@{}", n.name);
                if !n.parts.is_empty() && self.chance(5) {
                    let mut addrs = Vec::new();
                    Addr::collect(&n.parts, "", 0, &mut addrs);
                    addrs.retain(|a| !a.many);
                    if !addrs.is_empty() {
                        let _ = write!(r, ".{}", self.pick(&addrs).text);
                    }
                }
                if self.bad() {
                    r.push_str(self.pick(&[".*.x", ".*", ".0", ".zz", ".*.*"]));
                }
                r
            }
            Ty::PathData => self.path_data(),
            _ => self.pick(WORDS).to_string(),
        }
    }

    fn path_data(&mut self) -> String {
        let mut parts = vec![format!("M{} {}", self.lit(0, 30), self.lit(0, 30))];
        for _ in 0..self.n(1, 4) {
            let n = |g: &mut Self| g.lit(-40, 40);
            let seg = match self.n(0, 5) {
                0 => format!("L{} {}", n(self), n(self)),
                1 => format!(
                    "C{} {} {} {} {} {}",
                    n(self),
                    n(self),
                    n(self),
                    n(self),
                    n(self),
                    n(self)
                ),
                2 => format!("Q{} {} {} {}", n(self), n(self), n(self), n(self)),
                3 => format!("l{} {}", n(self), n(self)),
                4 => format!(
                    "c{} {} {} {} {} {}",
                    n(self),
                    n(self),
                    n(self),
                    n(self),
                    n(self),
                    n(self)
                ),
                _ => format!("H{}", n(self)),
            };
            parts.push(seg);
        }
        if self.chance(10) {
            parts.push("Z".into());
        }
        format!("\"{}\"", parts.join(" "))
    }

    fn key_call(&mut self, base: Base, d: u8) -> String {
        let n = self.n(2, 4);
        let ty = match base {
            Base::Color => Ty::Color,
            Base::Vec => Ty::Offset,
            _ => Ty::Coord,
        };
        let mut vals: Vec<String> = (0..n).map(|_| self.value(ty, d.min(1))).collect();
        for v in &mut vals {
            if self.scope.ramps.contains(v) {
                v.push_str(".0");
            } else if v.starts_with("ramp(") {
                *v = format!("line({v})");
            }
        }
        let mut wrong_tail = "";
        if self.bad() {
            match self.n(0, 2) {
                0 => vals.clear(),
                1 => vals[0] = self.pick(KEY_JUNK).to_string(),
                _ => wrong_tail = self.pick(KEY_TAILS),
            }
        }
        let mut s = format!("key({}", vals.join(" "));
        s.push_str(wrong_tail);
        if self.chance(2) {
            s.push_str(" loop");
        }
        if self.chance(5) {
            let mut at = vec!["0".to_string()];
            for i in 1..n - 1 {
                at.push(format!(".{}", (i * 10 / n).max(1) * 10 / 10 + i));
            }
            at.push("1".into());
            let _ = write!(s, " at:[{}]", at.join(" "));
        }
        if self.chance(6) {
            let e = if self.chance(4) {
                let mut es: Vec<&str> = (0..n - 1).map(|_| *self.pick(EASES)).collect();
                for e in es.iter_mut().skip(1).filter(|e| **e == "in") {
                    *e = "out";
                }
                format!("[{}]", es.join(" "))
            } else {
                self.pick(EASES).to_string()
            };
            let _ = write!(s, " ease:{e}");
        }
        if !self.in_anim || self.chance(2) {
            let _ = write!(s, " dur:{}", self.pick(&["1s", "2s", "500ms", "3"]));
        }
        s.push(')');
        s
    }

    fn available(&self, ty: Ty) -> bool {
        match ty {
            Ty::StyleRef => !self.scope.styles.is_empty(),
            Ty::Ref => !self.names.is_empty(),
            _ => true,
        }
    }

    fn keys(&mut self, tables: &[&[Key]], out: &mut String) {
        for table in tables {
            for &(key, ty, p) in *table {
                if !self.chance(p) || !self.available(ty) {
                    continue;
                }
                if ty == Ty::Flag {
                    let _ = write!(out, " {key}");
                    continue;
                }
                let v = self.value(ty, 2);
                let tapered = key == "stroke" && v.contains('[');
                let _ = write!(out, " {key}:{v}");
                if tapered && self.chance(10) {
                    let t = self.value(Ty::Taper, 0);
                    let _ = write!(out, " taper:{t}");
                }
            }
        }
    }

    fn fx_line(&mut self, named: &mut Vec<String>) -> String {
        let finishes = self.scope.finishes.clone();
        if !self.in_finish && !finishes.is_empty() && self.chance(5) {
            let f = self.pick(&finishes).clone();
            let mut s = format!("fx {}", f.name);
            for p in &f.params {
                if self.chance(8) {
                    let v = self.value(p.ty, 1);
                    let _ = write!(s, " {}:{v}", p.name);
                }
            }
            if self.bad() {
                s.push_str(self.pick(&[" zz:1", " 2", " as:x", " of:self"]));
            }
            return s;
        }
        if !self.in_finish && named.len() >= 2 && self.chance(5) {
            let n = named.len().min(3);
            let mut s = format!("fx merge {}", named[..n].join(" "));
            if self.chance(10) {
                let alphas: Vec<String> = (0..n).map(|_| self.unit()).collect();
                let _ = write!(s, " a:{}", alphas.join(","));
            }
            return s;
        }
        if self.bad() {
            let word = match self.scope.styles.is_empty() {
                true => self.pick(UNIMPLEMENTED_FX).to_string(),
                false => self.pick(&self.scope.styles.clone()).clone(),
            };
            return format!("fx {word} 3");
        }
        let fx = self.pick(FXS);
        let mut s = format!("fx {}", fx.op);
        for &t in fx.pos {
            if self.chance(11) {
                let v = self.value(t, 1);
                let _ = write!(s, " {v}");
            } else {
                break;
            }
        }
        self.keys(&[fx.keys], &mut s);
        if !self.in_finish {
            if self.chance(3) {
                let name = self.fresh("fxn");
                let _ = write!(s, " as:{name}");
                named.push(name);
            }
            if self.chance(2) {
                let of = if self.bad() {
                    "3".into()
                } else if !named.is_empty() && self.chance(8) {
                    self.pick(&named.clone()).clone()
                } else if !self.names.is_empty() && self.chance(6) {
                    self.word(Ty::Ref)
                } else {
                    "self".into()
                };
                let _ = write!(s, " of:{of}");
            }
        }
        s
    }

    /// Effects each cost a layer, so a group or a node repeated by a loop mostly takes none.
    fn fx_lines(&mut self, ind: usize, heavy: bool) {
        let count = if std::mem::take(&mut self.force_fx) {
            self.n(1, 2)
        } else if heavy {
            self.weighted(&[(15, 0), (1, 1)])
        } else {
            self.weighted(&[(13, 0), (4, 1), (2, 2), (1, 3)])
        };
        let mut named = Vec::new();
        for _ in 0..count {
            let l = self.fx_line(&mut named);
            self.line(ind, l);
        }
    }

    fn mask(&mut self, c: Ctx) {
        #[derive(Clone, Copy)]
        enum M {
            Field,
            Node,
            Block,
        }
        let m = self.weighted(&[
            (4, M::Field),
            (if self.names.is_empty() { 0 } else { 3 }, M::Node),
            (2, M::Block),
        ]);
        match m {
            M::Field => {
                let f = self.field(1);
                let op = *self.pick(&[">", ">=", "<", "<="]);
                let t = self.unit();
                if self.chance(3) {
                    self.line(c.ind, format!("mask {f}"));
                } else if self.chance(4) {
                    self.line(c.ind, format!("mask {t} {op} {f}"));
                } else {
                    self.line(c.ind, format!("mask {f} {op} {t}"));
                }
            }
            M::Node => {
                let r = self.word(Ty::Ref);
                self.line(c.ind, format!("mask {r}"));
            }
            M::Block => {
                self.line(c.ind, "mask");
                self.block(c.child().hidden(), 1, 2, true);
            }
        }
    }

    fn block(&mut self, c: Ctx, lo: usize, hi: usize, shapes_only: bool) {
        let mark = self.env.len();
        for _ in 0..self.n(lo, hi) {
            if shapes_only {
                self.shape(c, false);
            } else {
                self.stmt(c);
            }
        }
        self.env.truncate(mark);
    }

    fn draw(
        &mut self,
        c: Ctx,
        mut head: String,
        info: Option<&Sig>,
        naming: Naming,
        body: impl FnOnce(&mut Self, Ctx),
    ) {
        let verb = head.split(' ').next().unwrap_or_default();
        let plain = info.is_none() && (SHAPES.iter().any(|s| s.verb == verb) || verb == "copy");
        let geom = plain || (info.is_none() && matches!(verb, "subtract" | "union" | "intersect" | "xor"));
        let soft_clip = matches!(verb, "inside" | "outside");
        let heavy = c.mult > 4 || (!geom && info.is_none() && !soft_clip);
        let masked = !heavy && self.chance(2);
        if masked && !head.contains(" feather:") && self.chance(7) {
            let _ = write!(head, " feather:{}", self.lit(1, 8));
        }
        let name = match naming {
            Naming::Fixed(n) => Some(n),
            Naming::Auto if self.chance(5) || (info.is_some() && self.chance(4)) => {
                let n = self.fresh(PART_NAMES[self.counter as usize % PART_NAMES.len()]);
                let _ = write!(head, " name:{n}");
                Some(n)
            }
            Naming::Auto => None,
        };
        self.declared += u32::from(name.is_some());
        let off_canvas = head.contains(OFF_CANVAS);
        self.line(c.ind, head);
        let copies = match c.scope {
            Scope::Copies(n) => n,
            _ => 0,
        };
        let node = name.filter(|_| c.scope != Scope::Conditional).map(|n| Node {
            name: n,
            parts: info.map(|s| s.parts.clone()).unwrap_or_default(),
            is_shape: geom,
            copies,
        });
        let register = |g: &mut Self, node: Node| {
            if node.copies > 0 {
                g.parts.push(node);
                return;
            }
            if c.direct {
                g.parts.push(node.clone());
            }
            g.names.push(node);
        };
        let mut late = None;
        match node {
            Some(n) if plain => register(self, n),
            other => late = other,
        }
        let k = c.child();
        body(self, k);
        if masked {
            self.mask(k);
        }
        self.force_fx = off_canvas;
        self.fx_lines(k.ind, heavy);
        if info.is_some() {
            let filled = std::mem::take(&mut self.filled);
            if let Some(n) = late.as_mut() {
                n.parts.extend(filled);
            }
        }
        if let Some(n) = late {
            register(self, n);
        }
    }

    fn stmt(&mut self, c: Ctx) {
        #[derive(Clone, Copy)]
        enum K {
            Shape,
            Bool,
            Copy,
            Layer,
            Group,
            Inside,
            Mirror,
            Repeat,
            Grid,
            Call,
            Fork,
            Let,
            If,
            Each,
            Check,
            Canvas,
            Junk,
        }
        self.framed = c.framed;
        self.budget = self.budget.saturating_sub(1);
        let deep = c.ind >= 4 || self.budget == 0;
        let loops = c.mult <= 6 && !deep;
        let kids = !deep;
        let refs = !self.names.is_empty();
        let measurable = self.names.iter().any(|n| n.is_shape);
        let k = self.weighted(&[
            (26, K::Shape),
            (if kids { 5 } else { 0 }, K::Bool),
            (if refs { 3 } else { 0 }, K::Copy),
            (if kids { 5 } else { 0 }, K::Layer),
            (if kids { 3 } else { 0 }, K::Group),
            (if kids && refs { 2 } else { 0 }, K::Inside),
            (if kids { 2 } else { 0 }, K::Mirror),
            (if loops { 3 } else { 0 }, K::Repeat),
            (if loops { 2 } else { 0 }, K::Grid),
            (if self.scope.defs.is_empty() { 0 } else { 9 }, K::Call),
            (
                if kids && c.scope == Scope::Own && self.scope.defs.iter().any(|d| d.needs.is_none()) {
                    2
                } else {
                    0
                },
                K::Fork,
            ),
            (4, K::Let),
            (if kids { 4 } else { 0 }, K::If),
            (if loops { 4 } else { 0 }, K::Each),
            (if measurable { 5 } else { 0 }, K::Check),
            (if c.ind == 0 && c.scope == Scope::Own { 1 } else { 0 }, K::Canvas),
            (if self.wrong > 0 && self.junk_ok { 2 } else { 0 }, K::Junk),
        ]);
        match k {
            K::Shape => self.shape(c, kids),
            K::Bool => {
                let verb = *self.pick(&["subtract", "union", "intersect", "xor"]);
                let mut head = verb.to_string();
                self.keys(&[COMMON, STYLE, PAINT], &mut head);
                self.draw(c, head, None, Naming::Auto, |g, k| g.block(k.hidden(), 2, 3, true));
            }
            K::Copy => {
                let r = self.word(Ty::Ref);
                let mut head = format!("copy {r}");
                self.keys(&[COMMON, STYLE, PAINT], &mut head);
                self.draw(c, head, None, Naming::Auto, |_, _| {});
            }
            K::Layer | K::Group => {
                let mut head = if matches!(k, K::Layer) {
                    "layer".to_string()
                } else {
                    "group".to_string()
                };
                let mut naming = Naming::Auto;
                if matches!(k, K::Layer) || self.chance(6) {
                    let n = self.fresh("grp");
                    let _ = write!(head, " {n}");
                    naming = Naming::Fixed(n);
                }
                self.keys(&[COMMON, STYLE], &mut head);
                self.draw(c, head, None, naming, |g, k| g.block(k, 1, 3, false));
            }
            K::Inside => {
                let verb = *self.pick(&["inside", "outside"]);
                let r = self.word(Ty::Ref);
                let mut head = format!("{verb} {r}");
                self.keys(&[COMMON, SOFT_CLIP], &mut head);
                self.draw(Ctx { direct: false, ..c }, head, None, Naming::Auto, |g, k| {
                    g.block(Ctx { framed: true, ..k }, 1, 2, false)
                });
            }
            K::Mirror => {
                let axis = self.word(Ty::Axis);
                let mut head = format!("mirror {axis}");
                self.keys(&[COMMON], &mut head);
                self.draw(Ctx { direct: false, ..c }, head, None, Naming::Auto, |g, k| {
                    g.block(k.looped(2).hidden(), 1, 2, false)
                });
            }
            K::Repeat => {
                let count = self.n(2, 5);
                let shown = if self.bad() {
                    self.pick(&["", "nope", "0", "-2", "2.5", "3x"]).to_string()
                } else {
                    count.to_string()
                };
                let mut head = format!("repeat {shown}");
                let rot = self.value(Ty::Angle, 1);
                let step = self.value(Ty::Offset, 0);
                if self.chance(10) {
                    let _ = write!(head, " rot:{rot}");
                }
                if self.chance(8) {
                    let _ = write!(head, " step:{step}");
                }
                self.keys(&[COMMON], &mut head);
                self.draw(Ctx { direct: false, ..c }, head, None, Naming::Auto, |g, k| {
                    g.env.push(Bind {
                        name: "i".into(),
                        base: Base::Num,
                    });
                    g.block(k.looped(count), 1, 2, false);
                    if g.bad() {
                        g.mask(k);
                    }
                    g.env.pop();
                });
            }
            K::Grid => {
                let (cols, rows) = (self.n(1, 3), self.n(1, 2));
                let shown = if self.bad() {
                    self.pick(&["", "2", "0x0", "ax2", "2x", "-1x2"]).to_string()
                } else {
                    format!("{cols}x{rows}")
                };
                let mut head = format!("grid {shown}");
                if self.chance(8) {
                    let _ = write!(head, " gap:{}", self.lit(0, 10));
                }
                if self.chance(5) {
                    let _ = write!(head, " cell:{},{}", self.lit(4, 20), self.lit(4, 20));
                }
                self.keys(&[COMMON], &mut head);
                self.draw(Ctx { direct: false, ..c }, head, None, Naming::Auto, |g, k| {
                    let mark = g.env.len();
                    for n in ["i", "row", "col"] {
                        g.env.push(Bind {
                            name: n.into(),
                            base: Base::Num,
                        });
                    }
                    g.block(k.looped(cols * rows), 1, 2, false);
                    g.env.truncate(mark);
                });
            }
            K::Call => self.call_stmt(c),
            K::Fork => {
                let defs: Vec<Sig> = self.scope.defs.iter().filter(|d| d.needs.is_none()).cloned().collect();
                let sig = self.pick(&defs).clone();
                let name = self.fresh("fork");
                let cond = self.cond(1);
                let arm = Ctx {
                    direct: false,
                    ..c.child().hidden()
                };
                self.line(c.ind, format!("if {cond}"));
                self.call_of(arm, &sig, Some(name.clone()));
                self.line(c.ind, "else");
                self.call_of(arm, &sig, Some(name.clone()));
                let node = Node {
                    name,
                    parts: sig.parts.clone(),
                    is_shape: false,
                    copies: 0,
                };
                if c.direct {
                    self.parts.push(node.clone());
                }
                self.names.push(node);
            }
            K::Let => self.let_stmt(c),
            K::If => {
                let cond = self.cond(2);
                self.line(c.ind, format!("if {cond}"));
                self.block(c.child().hidden(), 1, 3, false);
                let mut chain = 0;
                while chain < 2 && self.chance(5) {
                    chain += 1;
                    if self.chance(7) {
                        let cond = self.cond(1);
                        self.line(c.ind, format!("else if {cond}"));
                        self.block(c.child().hidden(), 1, 2, false);
                    } else {
                        self.line(c.ind, "else");
                        self.block(c.child().hidden(), 1, 2, false);
                        break;
                    }
                }
            }
            K::Each => self.each_stmt(c),
            K::Check => self.check_stmt(c),
            K::Canvas => {
                let fill = self.paint(1);
                self.line(c.ind, format!("artboard fill:{fill}"));
            }
            K::Junk => {
                let j = self.pick(JUNK_STMTS).to_string();
                self.line(c.ind, j);
            }
        }
    }

    fn shape(&mut self, c: Ctx, kids: bool) {
        let sh = self.pick(SHAPES);
        let mut head = sh.verb.to_string();
        for &t in sh.pos {
            if t == Ty::Pts {
                if self.chance(3) {
                    let list = self.points_list();
                    let _ = write!(head, " {list}");
                } else {
                    for p in self.curve_points() {
                        let _ = write!(head, " {p}");
                    }
                }
            } else {
                let v = self.value(t, 2);
                let _ = write!(head, " {v}");
            }
        }
        self.keys(&[sh.keys, COMMON, STYLE, PAINT], &mut head);
        let nested = kids && self.chance(4);
        self.draw(c, head, None, Naming::Auto, |g, k| {
            if nested {
                g.block(Ctx { framed: true, ..k }, 1, 3, false);
            }
        });
    }

    fn let_stmt(&mut self, c: Ctx) {
        let (base, ty) = self.weighted(&[
            ((8), (Base::Num, Ty::Num)),
            (2, (Base::Color, Ty::Color)),
            (2, (Base::Vec, Ty::Vec)),
            (1, (Base::Bool, Ty::Bool)),
            (1, (Base::Field, Ty::Field)),
            (1, (Base::Pts, Ty::Pts)),
            (1, (Base::List, Ty::Num)),
        ]);
        let name = self.fresh("v");
        let v = if base == Base::List {
            let n = self.n(2, 4);
            let items: Vec<String> = (0..n).map(|_| self.lit(-20, 40)).collect();
            self.lists.push((name.clone(), n));
            format!("[{}]", items.join(" "))
        } else {
            self.value(ty, 2)
        };
        self.line(c.ind, format!("let {name} = {v}"));
        self.env.push(Bind {
            name: name.clone(),
            base,
        });
        if base == Base::Vec {
            self.env.push(Bind {
                name: format!("{name}.x"),
                base: Base::Num,
            });
        }
    }

    fn each_stmt(&mut self, c: Ctx) {
        let var = self.fresh("e");
        let ramps = self.scope.ramps.clone();
        let (iter, base, times) = match self.n(0, 4) {
            _ if !ramps.is_empty() && self.bad() => (self.pick(&ramps).clone(), Base::Color, 5),
            0 | 1 => {
                let n = self.n(1, 4);
                (format!("0..{n}"), Base::Num, n)
            }
            2 => {
                let n = self.n(1, 4);
                let items: Vec<String> = (0..n).map(|_| self.lit(0, 30)).collect();
                (format!("[{}]", items.join(" ")), Base::Num, n)
            }
            3 => {
                let n = self.n(1, 3);
                let items: Vec<String> = (0..n).map(|_| self.color(0)).collect();
                (format!("[{}]", items.join(" ")), Base::Color, n)
            }
            _ => {
                let comps = self.plain_comps();
                if comps.is_empty() {
                    ("0..2".to_string(), Base::Num, 2)
                } else {
                    let n = self.n(1, 3).min(comps.len().max(1));
                    let items: Vec<String> = (0..n).map(|_| self.pick(&comps).clone()).collect();
                    (format!("[{}]", items.join(" ")), Base::Comp, n)
                }
            }
        };
        self.line(c.ind, format!("each {var} in {iter}"));
        let mark = self.env.len();
        self.env.push(Bind {
            name: var.clone(),
            base,
        });
        let inner = Ctx {
            framed: c.framed,
            ..c.child().looped(times)
        };
        if base == Base::Comp {
            for _ in 0..self.n(1, 2) {
                let mut head = format!("{var} {}", self.lit(4, 30));
                self.keys(&[COMMON], &mut head);
                self.line(inner.ind, head);
            }
        } else {
            self.block(inner, 1, 3, false);
        }
        self.env.truncate(mark);
    }

    fn plain_comps(&self) -> Vec<String> {
        let plain = |d: &&Sig| {
            d.needs.is_none()
                && !d.needs_call_name()
                && d.params.first().is_some_and(|p| p.name == "size")
        };
        self.scope.defs.iter().filter(plain).map(|d| d.name.clone()).collect()
    }

    fn call_stmt(&mut self, c: Ctx) {
        let host = self.hosts.last().cloned().unwrap_or_default();
        let wrong = self.bad();
        let defs: Vec<Sig> = self
            .scope
            .defs
            .iter()
            .filter(|d| wrong || d.needs.as_ref().is_none_or(|n| host.contains(n)))
            .cloned()
            .collect();
        if defs.is_empty() {
            return self.shape(c, false);
        }
        let hosted: Vec<Sig> = defs.iter().filter(|d| d.needs.is_some()).cloned().collect();
        let sig = if !hosted.is_empty() && !wrong && self.chance(10) {
            self.pick(&hosted).clone()
        } else {
            self.pick(&defs).clone()
        };
        self.call_of(c, &sig, None);
    }

    fn call_of(&mut self, c: Ctx, sig: &Sig, name: Option<String>) {
        let mut head = sig.name.clone();
        for (i, p) in sig.params.iter().enumerate() {
            let positional = i == 0 && self.chance(10);
            if !(positional || self.chance(6)) {
                continue;
            }
            let v = match p.ty {
                _ if p.name == "size" && p.ty == Ty::Len && !self.bad() && self.chance(14) => self.lit(4, 30),
                Ty::Flag => {
                    let mut comps = self.plain_comps();
                    comps.push("none".into());
                    self.pick(&comps).clone()
                }
                t => self.value(t, 1),
            };
            if positional {
                let _ = write!(head, " {v}");
            } else {
                let _ = write!(head, " {}:{v}", p.name);
            }
        }
        self.keys(&[COMMON], &mut head);
        let name = match name {
            Some(n) => Some(n),
            None if sig.needs_call_name() && !self.bad() => {
                Some(self.fresh(PART_NAMES[self.counter as usize % PART_NAMES.len()]))
            }
            None => None,
        };
        let naming = match name {
            Some(n) => {
                let _ = write!(head, " name:{n}");
                Naming::Fixed(n)
            }
            None => Naming::Auto,
        };
        self.draw(c, head, Some(sig), naming, |g, k| {
            g.instance_body(k, sig);
        });
    }

    fn instance_body(&mut self, k: Ctx, sig: &Sig) {
        let mut roots = sig.parts.clone();
        for slot in &sig.slots {
            if self.chance(7) {
                self.line(k.ind, format!("{slot}:"));
                let host = sig.parts.iter().filter(|p| p.copies == 0).map(|p| p.name.clone());
                self.hosts.push(host.collect());
                let (parts, names) = (self.parts.len(), self.names.len());
                let fill = Ctx {
                    framed: true,
                    scope: Scope::Own,
                    direct: true,
                    ..k.child()
                };
                self.block(fill, 1, 2, false);
                self.hosts.pop();
                let content = self.parts.split_off(parts);
                self.names.truncate(names);
                roots.push(Node {
                    name: slot.clone(),
                    parts: content,
                    is_shape: false,
                    copies: 0,
                });
            }
        }
        for _ in 0..self.weighted(&[(8, 0), (5, 1), (2, 2)]) {
            if let Some(patch) = self.patch_line(&roots, sig) {
                self.line(k.ind, patch);
            }
        }
        let playable: Vec<String> = sig
            .anims
            .iter()
            .filter(|a| self.scope.anims.contains(a))
            .cloned()
            .collect();
        if !playable.is_empty() && self.chance(6) {
            let a = self.pick(&playable).clone();
            self.line(k.ind, format!("play {a}"));
        }
        if self.chance(1) && self.names.iter().any(|n| n.is_shape) {
            self.check_stmt(k);
        }
        self.filled = roots.split_off(sig.parts.len());
    }

    fn patch_line(&mut self, roots: &[Node], sig: &Sig) -> Option<String> {
        let mut addrs = Vec::new();
        Addr::collect(roots, "", 0, &mut addrs);
        let wrong = self.bad();
        if addrs.is_empty() {
            return wrong.then(|| ".* a:.5".to_string());
        }
        let a = self.pick(&addrs).clone();
        let star = a.has_children && !a.many && self.chance(3);
        let mut path = if star { format!("{}.*", a.text) } else { a.text.clone() };
        let root_star = sig.parts.iter().any(|r| r.copies == 0);
        if !star && root_star && self.chance(1) {
            path = "*".into();
        }
        if wrong {
            path = self.pick(BAD_PATCHES).replace("{}", &path);
        }
        let mut s = format!(".{path}");
        if self.chance(3) {
            s.push_str(" off");
        } else {
            self.keys(&[if a.geom && !star { PATCH } else { XFORM }], &mut s);
        }
        Some(s)
    }

    fn check_stmt(&mut self, c: Ctx) {
        let rules = self.scope.rules.clone();
        let kw = if self.chance(8) { "expect" } else { "assert" };
        let mut s = String::new();
        if self.bad() {
            let subject = self.word(Ty::Ref);
            let _ = write!(s, "{}({subject})", self.pick(&["norule", "contrast", "visible", "level_"]));
        } else if !rules.is_empty() && self.chance(4) {
            let r = self.pick(&rules).clone();
            let subject = self.word(Ty::Ref);
            let mut args = vec![subject];
            for p in r.params.iter().skip(1) {
                if self.chance(8) {
                    let v = self.value(p.ty, 1);
                    args.push(format!("{}:{v}", p.name));
                }
            }
            let _ = write!(s, "{}({})", r.name, args.join(" "));
        } else if self.chance(5) {
            let (a, b) = (self.word(Ty::Ref), self.word(Ty::Ref));
            let (ma, mb) = (*self.pick(MEASURES), *self.pick(MEASURES));
            let (lo, hi) = (self.unit(), self.lit(2, 5));
            let _ = write!(s, "{a}.{ma} / {b}.{mb} in {lo}..{hi}");
        } else {
            let chk = self.pick(CHECKS);
            let mut args: Vec<String> = (0..chk.refs).map(|_| self.word(Ty::Ref)).collect();
            if chk.name == "inside" && self.chance(8) {
                args[1] = "artboard".into();
            }
            if chk.name == "same_size" {
                args.truncate(self.n(1, 3));
            }
            if chk.name == "on_grid" {
                let v = self.value(Ty::Count, 0);
                args.push(v);
            }
            let loose = chk.name == "centered" && self.bad();
            if loose {
                args.push(self.word(Ty::Ref));
            } else if chk.name == "same_size" && self.chance(3) {
                let hubs: Vec<String> = (self.names.iter())
                    .filter(|n| n.parts.iter().any(|p| p.copies == 0))
                    .map(|n| n.name.clone())
                    .collect();
                if !hubs.is_empty() {
                    args[0] = format!("@{}.*", self.pick(&hubs));
                }
            }
            let mut call = format!("{}({}", chk.name, args.join(" "));
            let mut extra = String::new();
            if !loose {
                self.keys(&[chk.keys], &mut extra);
            }
            call.push_str(&extra);
            call.push(')');
            if chk.number {
                let op = *self.pick(&["<", ">", "~="]);
                let n = self.lit(1, 80);
                let _ = write!(s, "{call} {op} {n}");
            } else {
                s = call;
            }
        }
        if self.chance(3) && !s.contains("tol:") && !s.ends_with(')') {
            let _ = write!(s, " tol:{}", self.lit(0, 3));
        }
        if self.chance(3) {
            s.push(' ');
            s.push_str(self.pick(NOTES));
        }
        self.line(c.ind, format!("{kw} {s}"));
    }

    fn palette(&mut self, parent: Option<&Pal>) -> (Pal, Vec<String>) {
        let name = self.fresh("pal");
        let mut lines = vec![match parent {
            Some(p) => format!("palette {name} from {}", p.name),
            None if self.bad() => format!("palette {name} from nope"),
            None => format!("palette {name}"),
        }];
        let mut entries: Vec<(String, Option<usize>)> = Vec::new();
        for _ in 0..self.n(2, 5) {
            let mut entry = self.pick(PAL_NAMES).to_string();
            if entries.iter().any(|e| e.0 == entry) {
                entry = self.fresh(&entry);
            }
            let hex = |g: &mut Self| if g.chance(3) { g.color_literal() } else { g.hex_long() };
            if self.chance(10) {
                let mut row = format!("  ramp {entry} {}", hex(self));
                if self.chance(7) {
                    let _ = write!(row, " {}", hex(self));
                }
                let mut stops = 5;
                if self.chance(10) {
                    stops = self.n(2, 7);
                    let _ = write!(row, " {stops}");
                }
                if self.chance(5) {
                    let _ = write!(row, " hue:{}", self.lit(-20, 30));
                }
                lines.push(row);
                entries.push((entry, Some(stops)));
            } else {
                lines.push(format!("  color {entry} {}", hex(self)));
                entries.push((entry, None));
            }
        }
        let mut all = parent.map(|p| p.entries.clone()).unwrap_or_default();
        for e in entries {
            all.retain(|x| x.0 != e.0);
            all.push(e);
        }
        (Pal { name, entries: all }, lines)
    }

    fn hex_long(&mut self) -> String {
        let mut s = String::from("#");
        for _ in 0..6 {
            let d = self.n(0, 15);
            let _ = write!(s, "{d:x}");
        }
        s
    }

    fn color_literal(&mut self) -> String {
        format!(
            "oklch({} {} {})",
            self.unit(),
            self.pick(&[".03", ".1", ".15"]),
            self.lit(0, 360)
        )
    }

    fn style_decl(&mut self) {
        let name = self.fresh("sty");
        let mut head = format!("style {name}");
        let mut table: Vec<Key> = PAINT.to_vec();
        table.iter_mut().for_each(|k| k.2 = k.2.max(4));
        self.keys(&[&table], &mut head);
        self.line(0, head);
        self.scope.styles.push(name.clone());
        self.own.styles.push(name);
    }

    fn finish_decl(&mut self) {
        let name = self.fresh("fin");
        let mut params = Vec::new();
        let mut head = format!("finish {name}");
        let mark = self.env.len();
        for _ in 0..self.n(1, 3) {
            let pn = self.fresh("fp");
            let ty = *self.pick(&[Ty::Small, Ty::Unit, Ty::Strength]);
            let d = self.value(ty, 0);
            let _ = write!(head, " {pn}:{d}");
            self.env.push(Bind {
                name: pn.clone(),
                base: Base::Num,
            });
            params.push(Param {
                name: pn,
                ty,
                default: d,
            });
        }
        self.line(0, head);
        self.in_finish = true;
        let mut sink = Vec::new();
        for _ in 0..self.n(1, 3) {
            let l = self.fx_line(&mut sink);
            self.line(1, l);
        }
        self.in_finish = false;
        self.env.truncate(mark);
        let c = Callable { name, params };
        self.scope.finishes.push(c.clone());
        self.own.finishes.push(c);
    }

    fn rule_decl(&mut self) {
        let name = self.fresh("rule");
        let lim = self.fresh("lim");
        let limit = self.lit(20, 200);
        self.line(0, format!("rule {name} subject:@self {lim}:{limit}"));
        let m = *self.pick(&["w", "h", "cx", "area"]);
        let op = *self.pick(&["<", ">"]);
        self.line(1, format!("assert subject.{m} {op} {lim}"));
        let c = Callable {
            name,
            params: vec![
                Param {
                    name: "subject".into(),
                    ty: Ty::Ref,
                    default: "@self".into(),
                },
                Param {
                    name: lim,
                    ty: Ty::Len,
                    default: String::new(),
                },
            ],
        };
        self.scope.rules.push(c.clone());
        self.own.rules.push(c);
    }

    fn def_decl(&mut self) {
        let name = self.fresh("comp");
        let size_ty = if self.chance(4) { Ty::Size } else { Ty::Len };
        let size_default = if size_ty == Ty::Size {
            self.size_literal()
        } else {
            self.lit(8, 40)
        };
        let mut params = vec![Param {
            name: "size".into(),
            ty: size_ty,
            default: size_default,
        }];
        let has_comps = !self.plain_comps().is_empty();
        for _ in 0..self.n(0, 3) {
            let pn = self.fresh("pr");
            let (ty, default) = match self.n(0, 5) {
                0 | 1 => (Ty::Small, self.lit(0, 10)),
                2 => (Ty::Color, self.color(0)),
                3 => (Ty::Offset, format!("{},{}", self.lit(-3, 3), self.lit(-3, 3))),
                4 => (Ty::Bool, self.pick(&["true", "false"]).to_string()),
                _ if has_comps => (Ty::Flag, "none".to_string()),
                _ => (Ty::Unit, self.unit()),
            };
            params.push(Param { name: pn, ty, default });
        }
        let mut head = format!("def {name}");
        for p in &params {
            let _ = write!(head, " {}:{}", p.name, p.default);
        }
        let hosted: Vec<Node> = self
            .scope
            .defs
            .iter()
            .flat_map(|d| d.parts.iter().filter(|p| p.copies == 0).cloned())
            .collect();
        let needs = (!hosted.is_empty() && self.chance(3)).then(|| self.pick(&hosted).clone());
        if let Some(n) = &needs {
            let _ = write!(head, " needs:@{}", n.name);
        }
        self.line(0, head);
        let declared = self.declared;
        let (saved_names, saved_parts, saved_env) = (
            std::mem::take(&mut self.names),
            std::mem::take(&mut self.parts),
            self.env.len(),
        );
        self.names.extend(needs.clone().map(|n| Node {
            parts: vec![],
            ..n
        }));
        for p in &params {
            match p.ty {
                Ty::Size => {
                    for m in ["w", "h"] {
                        self.env.push(Bind {
                            name: format!("size.{m}"),
                            base: Base::Num,
                        });
                    }
                }
                Ty::Offset => {
                    for m in ["x", "y"] {
                        self.env.push(Bind {
                            name: format!("{}.{m}", p.name),
                            base: Base::Num,
                        });
                    }
                }
                Ty::Flag => self.env.push(Bind {
                    name: p.name.clone(),
                    base: Base::Comp,
                }),
                t => self.env.push(Bind {
                    name: p.name.clone(),
                    base: t.base(),
                }),
            }
        }
        self.budget = 14;
        let c = Ctx {
            ind: 1,
            mult: 5,
            framed: true,
            scope: Scope::Own,
            direct: true,
        };
        let mut slots = Vec::new();
        let comp_param = params.iter().find(|p| p.ty == Ty::Flag).map(|p| p.name.clone());
        for _ in 0..self.n(1, 5) {
            match self.n(0, 6) {
                0 => {
                    let s = self.fresh("slot");
                    self.declared += 1;
                    let mut l = format!("slot {s}");
                    if self.bad() {
                        l.push_str(" name:x");
                    }
                    if self.chance(8) {
                        let _ = write!(l, " at:{}", self.vector(Ty::Offset, 1));
                    }
                    self.line(1, l);
                    if self.chance(5) {
                        self.block(
                            Ctx {
                                direct: false,
                                ..c.child().hidden()
                            },
                            1,
                            2,
                            false,
                        );
                    }
                    slots.push(s);
                }
                1 => {
                    let a = self.fresh("anc");
                    self.declared += 1;
                    let v = self.vector(Ty::Offset, 1);
                    let l = match self.bad() {
                        true => self.pick(&["anchor", "anchor {a}", "anchor {v}"]).replace("{a}", &a).replace("{v}", &v),
                        false => format!("anchor {a} {v}"),
                    };
                    self.line(1, l);
                }
                2 if comp_param.is_some() => {
                    let p = comp_param.clone().unwrap_or_default();
                    let v = self.value(Ty::Len, 1);
                    self.line(1, format!("{p} {v}"));
                }
                _ => self.stmt(c),
            }
        }
        let outer = std::mem::take(&mut self.names);
        self.fx_lines(1, false);
        self.names = outer;
        let parts = self.parts.clone();
        self.names = saved_names;
        self.parts = saved_parts;
        self.env.truncate(saved_env);
        let mut sig = Sig {
            name,
            params,
            slots,
            parts,
            anims: vec![],
            declares_names: self.declared > declared,
            needs: needs.map(|n| n.name),
        };
        if !sig.parts.is_empty() && self.chance(8) {
            let a = self.anim_decl(&sig.parts.clone());
            sig.anims.push(a.clone());
            self.scope.anims.push(a.clone());
            self.own.anims.push(a);
        }
        self.scope.defs.push(sig.clone());
        self.own.defs.push(sig);
    }

    fn size_literal(&mut self) -> String {
        format!("{}x{}", self.lit(8, 40), self.lit(8, 40))
    }

    fn anim_decl(&mut self, targets: &[Node]) -> String {
        let name = self.fresh("anim");
        let saved_names = std::mem::take(&mut self.names);
        let dur = self.value(Ty::Time, 0);
        let dur = if self.bad() {
            dur
        } else {
            dur.trim_start_matches('-').to_string()
        };
        let looping = if self.chance(8) { " loop" } else { "" };
        self.line(
            0,
            format!(
                "anim {name} dur:{}{looping}",
                if dur == "0" { "1s".to_string() } else { dur }
            ),
        );
        self.in_anim = true;
        let mut addrs = Vec::new();
        Addr::collect(targets, "", 0, &mut addrs);
        for _ in 0..self.n(1, 3) {
            let target = self.pick(&addrs).clone();
            let star = target.has_children && !target.many && self.chance(3);
            let mut path = if star { format!("{}.*", target.text) } else { target.text.clone() };
            if self.bad() {
                path = self.pick(BAD_PATCHES).replace("{}", &path);
            }
            let target = Addr {
                geom: target.geom && !star,
                ..target
            };
            let mut s = format!(".{path}");
            if self.chance(1) {
                s.push_str(" off");
            } else {
                for (key, ty) in [
                    ("at", Ty::Offset),
                    ("rot", Ty::Angle),
                    ("scale", Ty::Unit),
                    ("a", Ty::Unit),
                    ("fill", Ty::Color),
                ] {
                    if self.chance(6) && (target.geom || key != "fill") {
                        let base = ty.base();
                        let v = match base {
                            Base::Num => format!(
                                "key({})",
                                (0..self.n(2, 3))
                                    .map(|_| self.value(ty, 0))
                                    .collect::<Vec<_>>()
                                    .join(" ")
                            ),
                            _ => self.key_call(base, 1),
                        };
                        let v = if self.chance(5) && key != "fill" {
                            format!("abs({v})")
                        } else {
                            v
                        };
                        let _ = write!(s, " {key}:{v}");
                    }
                }
                if s.matches(':').count() == 0 {
                    let v = self.key_call(Base::Num, 0);
                    let _ = write!(s, " rot:{v}");
                }
            }
            self.line(1, s);
        }
        self.in_anim = false;
        self.names = saved_names;
        name
    }

    fn decls(&mut self, lib: bool) {
        for _ in 0..self.n(0, 2) {
            self.let_stmt(Ctx {
                ind: 0,
                mult: 1,
                framed: false,
                scope: Scope::Conditional,
                direct: false,
            });
        }
        if self.chance(if lib { 10 } else { 5 }) {
            let parent = self.scope.pals.last().cloned().filter(|_| self.chance(6));
            let (pal, lines) = self.palette(parent.as_ref());
            self.lines.extend(lines);
            self.scope.add_pal(&pal.name, &pal);
            self.own.add_pal(&pal.name, &pal);
            self.scope.pals.push(pal.clone());
            self.own.pals.push(pal);
        }
        for _ in 0..self.weighted(&[(6, 0), (5, 1), (2, 2)]) {
            self.style_decl();
        }
        for _ in 0..self.weighted(&[(6, 0), (4, 1), (1, 2)]) {
            self.finish_decl();
        }
        if self.chance(4) {
            self.rule_decl();
        }
        for _ in 0..self.n(if lib { 1 } else { 0 }, if lib { 3 } else { 2 }) {
            self.def_decl();
        }
    }

    fn use_line(&mut self, path: &str, exports: &Module) -> Option<String> {
        let names = exports.export_names();
        let form = self.weighted(&[(5, 0), (3, 1), (1, 2)]);
        match form {
            0 => {
                self.scope.merge(exports);
                Some(format!("use {path}"))
            }
            1 if !names.is_empty() => {
                let chosen: Vec<String> = names.iter().filter(|_| self.chance(10)).cloned().collect();
                if chosen.is_empty() {
                    return None;
                }
                let only = exports.only(&chosen);
                self.scope.merge(&only);
                Some(format!("use {path} {{ {} }}", chosen.join(" ")))
            }
            _ => Some(format!("use {path} as {}", self.fresh("m"))),
        }
    }

    fn scene(&mut self, lo: usize, hi: usize) {
        self.budget = 40;
        let c = Ctx {
            ind: 0,
            mult: 1,
            framed: true,
            scope: Scope::Own,
            direct: true,
        };
        for _ in 0..self.n(lo, hi) {
            self.stmt(c);
        }
    }

    fn targets(&mut self, anims: &[String]) -> Vec<String> {
        let mut out = vec![format!("@artboard {}x{}", self.cw, self.ch)];
        let count = self.weighted(&[(10, 1), (5, 2)]);
        for i in 0..count {
            let kind = if i == 0 {
                self.weighted(&[(8, "png"), (3, "svg"), (2, "gif")])
            } else {
                self.weighted(&[(3, "png"), (2, "svg"), (5, "gif")])
            };
            let axis = self.chance(3);
            let stem = if axis { format!("a{i}_{{k}}") } else { format!("a{i}") };
            let mut s = format!("@target out/{stem}.{kind}");
            if self.chance(8) {
                let _ = write!(s, " {}x{}", self.cw, self.cw);
            }
            if kind == "png" && self.chance(4) {
                let _ = write!(s, " scale:{}", self.pick(&["1,2", "2", "1.5", "1"]));
            }
            if kind == "gif" {
                let bg = self.pick(&["#ffffff", "#102030", "#fff"]);
                let fps = self.pick(&[4, 8, 12]);
                let _ = write!(s, " bg:{bg} fps:{fps}");
                let no_dur = if anims.is_empty() { self.bad() } else { self.chance(3) };
                if !no_dur {
                    let _ = write!(s, " dur:{}", self.pick(&["1s", "2s", "500ms", "1.5s"]));
                }
            } else if self.chance(3) {
                let bg = self.color(0);
                let _ = write!(
                    s,
                    " bg:{}",
                    if bg.contains(' ') || bg.contains('(') {
                        "#fedcba".to_string()
                    } else {
                        bg
                    }
                );
            }
            if self.chance(2) {
                let _ = write!(s, " profile:{}", self.pick(&["vector", "svg", "raster"]));
            }
            if self.chance(3) {
                let _ = write!(s, " as:{}", self.pick(WORDS));
            }
            if self.chance(4) {
                let _ = write!(s, " t:{}", self.pick(&[".5", "0", "1", "250ms", "0s"]));
            }
            if !anims.is_empty() && self.chance(6) {
                let _ = write!(s, " clip:{}", self.pick(anims));
            } else if self.chance(2) {
                let _ = write!(s, " clip:{}", self.pick(WORDS));
            } else if self.chance(1) {
                s.push_str(" clip:none");
            }
            if self.bad() {
                s.push(' ');
                s.push_str(self.pick(TARGET_JUNK));
            }
            if axis {
                let _ = write!(s, " each k:{}", self.pick(&["[1 2]", "[-1 2]", "[a b]"]));
            }
            out.push(s);
        }
        out
    }

    fn manifest(&mut self, default_palette: &str) -> String {
        let lint = |g: &mut Self| *g.pick(&["warn", "off", "error", "info"]);
        let scale = *self.pick(&["[1]", "[1]", "[1, 2]", "[2]", "1.5"]);
        let bg = *self.pick(&["none", "none", "#ffffff", "#203040"]);
        let profile = *self.pick(&["auto", "auto", "raster", "svg", "vector"]);
        let (a, b, c) = (lint(self), lint(self), lint(self));
        let std_line = if self.chance(4) { "std = \"0.1\"\n" } else { "" };
        let build = if self.chance(4) {
            let backend = if self.bad() {
                *self.pick(&["\"bogus\"", "1", "\"\""])
            } else {
                *self.pick(&["\"cpu\"", "\"gpu\"", "\"auto\"", "\"cpu\""])
            };
            format!("\n[build]\nbackend = {backend}\n")
        } else {
            String::new()
        };
        format!(
            "[project]\nname = \"fuzz\"\nout = \"out\"\nlib = [\"lib\"]\n{std_line}\n[defaults]\npalette = \"{default_palette}\"\nscale = {scale}\nbg = \"{bg}\"\nprofile = \"{profile}\"\n\n[lint]\nraw-color = \"{a}\"\nunused = \"{b}\"\noff-artboard = \"{c}\"\n{build}"
        )
    }

    fn project(&mut self) -> Vec<u8> {
        let mut files: Vec<(String, String)> = Vec::new();
        let mine = self.chance(4);
        let mut base = Module::default();
        let default_palette = if mine {
            let (pal, lines) = self.palette(None);
            files.push(("lib/palette/mine.ramo".into(), lines.join("\n") + "\n"));
            base.add_pal("p", &pal);
            "palette/mine"
        } else {
            base.colors.extend(HOUSE.iter().map(|n| format!("p.{n}")));
            "palette/house"
        };
        let mut libs: Vec<(String, Module)> = Vec::new();
        let nlibs = self.weighted(&[(6, 0), (5, 1), (2, 2)]);
        for i in 0..nlibs {
            let path = format!("gen/l{i}");
            self.scope = base.clone();
            self.own = Module::default();
            self.env = base_env();
            self.lines.clear();
            let mut header = Vec::new();
            if let Some((prev, exp)) = libs.last().cloned()
                && self.chance(8)
                && let Some(l) = self.use_line(&prev, &exp)
            {
                header.push(l);
            }
            if nlibs > 1 && self.bad() {
                header.push(format!("use gen/l{}", self.n(0, nlibs - 1)));
            }
            if self.bad() {
                header.push("@artboard 8x8\n@target out/lib.png".into());
            }
            self.decls(true);
            let src = header
                .into_iter()
                .chain(std::mem::take(&mut self.lines))
                .collect::<Vec<_>>()
                .join("\n")
                + "\n";
            files.push((format!("lib/{path}.ramo"), src));
            libs.push((path, std::mem::take(&mut self.own)));
        }
        self.scope = base.clone();
        self.own = Module::default();
        self.env = base_env();
        self.names.clear();
        self.parts.clear();
        self.lines.clear();
        let mut uses = Vec::new();
        if !mine && self.scope.colors.len() == HOUSE.len() && self.chance(5) {
            let parts = ["eye", "blush", "open_mouth", "sparkle", "smile", "happy_eye"];
            let chosen: Vec<&str> = parts.iter().copied().filter(|_| self.chance(8)).collect();
            if !chosen.is_empty() {
                uses.push(format!("use face/parts {{ {} }}", chosen.join(" ")));
                for n in chosen {
                    self.scope.defs.push(chibi_def(n));
                }
            }
            if self.chance(4) {
                uses.push("use shade/cel".into());
            }
        }
        for (path, exp) in libs.clone() {
            if let Some(l) = self.use_line(&path, &exp) {
                uses.push(l);
            }
        }
        if mine && self.chance(4) {
            uses.push(format!("use palette/mine as {}", self.fresh("m")));
        }
        if self.bad() {
            uses.push(self.pick(&["use art/t", "use t", "use art/t as me"]).to_string());
        }
        self.junk_ok = true;
        self.decls(false);
        let decls = std::mem::take(&mut self.lines);
        let env_mark = self.env.len();
        self.scene(2, 7);
        let scene = std::mem::take(&mut self.lines);
        let top = self.parts.clone();
        let mut anim_lines = Vec::new();
        let mut plays = Vec::new();
        let mut anims: Vec<String> = Vec::new();
        if !top.is_empty() && self.chance(9) {
            self.env.truncate(env_mark);
            for _ in 0..self.n(1, 2) {
                let a = self.anim_decl(&top);
                anims.push(a);
            }
            anim_lines = std::mem::take(&mut self.lines);
        }
        self.lines.clear();
        for a in &anims {
            if !self.chance(10) {
                continue;
            }
            let start = if self.chance(5) {
                format!(" start:{}", self.pick(&["0s", "200ms", ".5"]))
            } else {
                String::new()
            };
            if self.chance(4) {
                plays.push(format!("if clip == {}", self.pick(WORDS)));
                plays.push(format!("  play {a}{start}"));
            } else {
                plays.push(format!("play {a}{start}"));
            }
        }
        if self.chance(1) {
            plays.push("play none".into());
        }
        let mut entry = self.targets(&anims).join("\n");
        let _ = write!(entry, "\n# gen wrong={}", self.wrong);
        for part in [uses, decls, anim_lines, scene, plays] {
            if !part.is_empty() {
                entry.push('\n');
                entry.push_str(&part.join("\n"));
            }
        }
        entry.push('\n');
        let mut out = entry.into_bytes();
        files.push(("ramose.toml".into(), self.manifest(default_palette)));
        for (path, src) in files {
            out.push(0);
            out.extend_from_slice(path.as_bytes());
            out.push(b'\n');
            out.extend_from_slice(src.as_bytes());
        }
        out
    }
}

fn base_env() -> Vec<Bind> {
    vec![]
}

fn chibi_def(name: &str) -> Sig {
    let param = |n: &str, ty: Ty, d: &str| Param {
        name: n.into(),
        ty,
        default: d.into(),
    };
    let params = match name {
        "eye" => vec![
            param("size", Ty::Size, "68x80"),
            param("iris", Ty::Color, "p.iris"),
            param("side", Ty::Small, "1"),
            param("look", Ty::Offset, "0,0"),
            param("lid", Ty::Unit, "0"),
        ],
        "blush" => vec![
            param("size", Ty::Size, "72x32"),
            param("color", Ty::Color, "p.blush"),
            param("lines", Ty::Count, "3"),
        ],
        "sparkle" => vec![
            param("size", Ty::Len, "40"),
            param("color", Ty::Color, "p.glint"),
            param("glow", Ty::Unit, ".35"),
        ],
        _ => vec![param("size", Ty::Len, "30"), param("color", Ty::Color, "p.ink")],
    };
    Sig {
        name: name.into(),
        params,
        slots: vec![],
        parts: vec![],
        anims: vec![],
        declares_names: matches!(name, "eye" | "open_mouth" | "o_mouth"),
        needs: None,
    }
}
