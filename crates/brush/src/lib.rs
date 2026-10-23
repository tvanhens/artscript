//! The brush shading language, in one crate: its vocabulary ([`lang`]), the checker that turns a
//! parsed body into the typed IR ([`ir`]), the SkSL backend that prints the IR under the shader
//! and blender frames, and the seed reduction. Eval checks a `brush` declaration through
//! [`Checker::check`] and, with the `skia` feature, compiles it once through [`verify`]. Render
//! compiles the same text through [`effect`] and binds every body uniform through
//! [`Shader::bind`].

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::sync::Arc;

use ramose_syntax::ast::{BodyLine, Expr, Span};

mod check;
pub mod ir;
pub mod lang;
mod sksl;

#[cfg(feature = "skia")]
mod compile;
#[cfg(feature = "skia")]
pub use compile::{effect, verify};
pub use sksl::{DECODE, ENCODE, HASH};

use lang::{Field, Surface, Ty};

/// Device pixels per whole-chunk step of the ribbon field's `s` channel. The rest channel then
/// stays below this, where a half float resolves 1/64 of a pixel.
pub const S_CHUNK: f32 = 32.0;

/// Placement keys a brush declaration or call may set. Every other head key is a uniform.
pub const PLACEMENT_KEYS: &[&str] = &["dir", "reach", "on"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MainKind {
    Area,
    Ribbon,
}

/// How a stage runs: as a runtime shader over the slot's field, or as a runtime blender whose
/// `dst` is the backdrop. Render picks the blender for every stage of a stack in which some
/// stage reads `m.below`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Frame {
    Shader,
    Blender,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UniformValue {
    Float(f32),
    Float2([f32; 2]),
    Float3([f32; 3]),
    /// Straight rgba.
    Float4([f32; 4]),
}

impl UniformValue {
    pub fn ty(&self) -> Ty {
        match self {
            UniformValue::Float(_) => Ty::Num,
            UniformValue::Float2(_) => Ty::Vec2,
            UniformValue::Float3(_) => Ty::Vec3,
            UniformValue::Float4(_) => Ty::Color,
        }
    }

    pub fn floats(&self) -> &[f32] {
        match self {
            UniformValue::Float(v) => std::slice::from_ref(v),
            UniformValue::Float2(v) => v,
            UniformValue::Float3(v) => v,
            UniformValue::Float4(v) => v,
        }
    }
}

/// Where something was written: a project-relative file and a span in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Site {
    pub file: String,
    pub span: Span,
}

/// A fault the checker found. `code` is `E0360` inside a body or a fn it calls and `E0363` for
/// a parameter.
#[derive(Clone, Debug, PartialEq)]
pub struct Diag {
    pub code: &'static str,
    pub at: Site,
    pub message: String,
    pub notes: Vec<String>,
    pub fix: Option<String>,
}

/// The first read of each field of `m`, for the notes of the errors at a use the body cannot
/// run at. The default coverage is not a read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reads {
    first: BTreeMap<Field, Site>,
}

impl Reads {
    fn record(&mut self, field: Field, site: &Site) {
        self.first.entry(field).or_insert_with(|| site.clone());
    }

    /// The widest surface requirement over every read; `Any` when the body reads nothing.
    pub fn surface(&self) -> Surface {
        self.first.keys().map(|f| f.surface()).max().unwrap_or(Surface::Any)
    }

    pub fn below(&self) -> Option<&Site> {
        self.first.get(&Field::Below)
    }

    /// The first read that makes the body 2D-only: a 2D extra, or `below`.
    pub fn two_d(&self) -> Option<(Field, &Site)> {
        self.first.iter().find(|(f, _)| f.surface() >= Surface::TwoD).map(|(f, s)| (*f, s))
    }
}

/// A brush parameter after eval evaluated its default. The type comes from the default.
#[derive(Clone, Debug)]
pub struct Param {
    pub name: String,
    pub ty: Ty,
    pub span: Span,
}

/// One `brush` declaration as the checker reads it.
pub struct BrushSource<'a> {
    pub file: &'a str,
    pub name: &'a str,
    /// The `brush` line: where a missing `out` and a backend's internal error are reported.
    pub span: Span,
    pub params: &'a [Param],
    pub lines: &'a [BodyLine],
}

/// A top-level `fn`. Eval's fn definition is this type, so a body and `values` read one
/// definition.
#[derive(Clone, Debug)]
pub struct TopFn {
    pub name: String,
    pub params: Vec<String>,
    pub body: Expr,
    /// The module whose names the body resolves in.
    pub module: String,
    pub span: Span,
}

/// What a name means at a module's top level, as far as a body cares.
pub enum Global {
    Fn(Rc<TopFn>),
    /// A palette, a `let`, a def, and so on. A body cannot read it; the text names it.
    Other(&'static str),
}

/// Eval's module scope. `file` is the module the name is looked up in, so a top-level fn
/// resolves its own names in its own file, imports included.
pub trait Library {
    fn lookup(&self, file: &str, name: &str) -> Option<Global>;
}

/// Build-wide checker state, so a fault in a fn is reported once however many brushes call it.
/// One per build, owned by the evaluator. Lookups only, so its maps never decide output order.
#[derive(Default)]
pub struct Checker {
    /// Keyed by the trip count of the loops around the call too, since a fn's loops fit the
    /// budget only under so many.
    types: BTreeMap<(check::FnKey, Vec<Ty>, u32), Option<Ty>>,
    reported: BTreeSet<(check::FnKey, String, Span, String)>,
}

impl Checker {
    /// Checks one body and builds its [`Shader`]. Diags are in line order. An `Err` can be empty
    /// when every fault is in a fn that an earlier brush already reported.
    pub fn check(&mut self, src: &BrushSource<'_>, lib: &dyn Library) -> Result<Shader, Vec<Diag>> {
        check::Lower::new(self, lib, src).run()
    }
}

/// A checked brush body plus its printed SkSL `shade` function. Immutable, and shared by every
/// stage that uses the brush.
#[derive(Debug)]
pub struct Shader {
    body: ir::Body,
    reads: Reads,
    sksl: Arc<str>,
    names: Vec<String>,
}

impl Shader {
    fn new(body: ir::Body, reads: Reads) -> Shader {
        let sksl = sksl::print(&body);
        let mut names = vec!["u_seed".to_string(), "u_seedo".to_string()];
        names.extend((0..body.salts.len()).map(|i| format!("u_salt{i}")));
        names.extend(body.params.iter().map(|(p, _)| format!("p_{p}")));
        Shader { body, reads, sksl: sksl.into(), names }
    }

    pub fn body(&self) -> &ir::Body {
        &self.body
    }

    pub fn reads(&self) -> &Reads {
        &self.reads
    }

    /// Whether the body reads `m.below`. Every stage of a stack with such a stage runs under
    /// the blender frame.
    pub fn reads_below(&self) -> bool {
        self.reads.below().is_some()
    }

    /// The printed `shade` function with its uniform lines.
    pub fn sksl(&self) -> &Arc<str> {
        &self.sksl
    }

    /// Every uniform the body's own text declares, with its value for a stage with `seed` and
    /// the parameter `values` (in `body().params` order): `u_seed`, `u_seedo`, each `u_salt{i}`,
    /// and `p_{name}`. Render binds what this yields and never spells a body name itself.
    pub fn bind<'a>(&'a self, seed: u32, values: &'a [UniformValue]) -> impl Iterator<Item = (&'a str, UniformValue)> + 'a {
        assert_eq!(values.len(), self.body.params.len(), "one value per parameter");
        let seeds = Seeds::from(seed);
        let fixed = [UniformValue::Float(seeds.seed), UniformValue::Float2(seeds.offset)];
        let salts = self.body.salts.iter().map(move |k| UniformValue::Float2(Seeds::salted(seed, *k)));
        self.names.iter().map(String::as_str).zip(fixed.into_iter().chain(salts).chain(values.iter().copied()))
    }

    /// Parses and checks one `brush` declaration whose head defaults are literals, with no
    /// library. For tests and fuzzing.
    pub fn from_text(src: &str) -> Result<Shader, Vec<Diag>> {
        check::from_text(src)
    }
}

/// The stage seeds. `seed` has 256 values and `offset` 61 x 59, so hash inputs stay small
/// enough for the exact integer hash. Two brushes in one scene can share a pattern.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seeds {
    pub seed: f32,
    pub offset: [f32; 2],
}

impl From<u32> for Seeds {
    fn from(seed: u32) -> Seeds {
        Seeds {
            seed: (seed % 256) as f32,
            offset: [(seed % 61) as f32, ((seed / 61) % 59) as f32],
        }
    }
}

impl Seeds {
    /// The offset for `salt:k`: the stage seed mixed with `k` by an integer hash (murmur3's
    /// finalizer), then reduced as `From<u32>` does. Pure integer code, so every build agrees.
    pub fn salted(seed: u32, salt: u32) -> [f32; 2] {
        let mut h = seed ^ salt.wrapping_mul(0x9e37_79b9);
        h ^= h >> 16;
        h = h.wrapping_mul(0x85eb_ca6b);
        h ^= h >> 13;
        h = h.wrapping_mul(0xc2b2_ae35);
        h ^= h >> 16;
        Seeds::from(h).offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_reduce_as_the_contract_says() {
        assert_eq!(Seeds::from(0), Seeds { seed: 0.0, offset: [0.0, 0.0] });
        assert_eq!(Seeds::from(257), Seeds { seed: 1.0, offset: [13.0, 4.0] });
    }

    #[test]
    fn a_salt_gives_another_offset_in_range() {
        let plain = Seeds::from(257).offset;
        let salted = Seeds::salted(257, 1);
        assert_ne!(plain, salted);
        assert_ne!(salted, Seeds::salted(257, 2));
        assert!(salted[0] < 61.0 && salted[1] < 59.0);
        assert_eq!(salted, Seeds::salted(257, 1), "pure");
    }

    #[test]
    fn bind_yields_every_body_uniform_in_declaration_order() {
        let s = Shader::from_text("brush b strength:1 tint:#ff0000\n  let n = value(m.uv salt:7) + simplex(m.uv salt:2)\n  out m.paint.rgb cover:cover(m.edge) * strength * n\n").unwrap_or_else(|d| panic!("{d:?}"));
        let bound: Vec<(&str, UniformValue)> = s.bind(257, &[UniformValue::Float(0.5), UniformValue::Float4([1.0, 0.0, 0.0, 1.0])]).collect();
        let names: Vec<&str> = bound.iter().map(|(n, _)| *n).collect();
        assert_eq!(names, ["u_seed", "u_seedo", "u_salt0", "u_salt1", "p_strength", "p_tint"]);
        assert_eq!(bound[0].1, UniformValue::Float(1.0));
        assert_eq!(bound[1].1, UniformValue::Float2([13.0, 4.0]));
        assert_eq!(bound[2].1, UniformValue::Float2(Seeds::salted(257, 2)), "salts are sorted");
        assert_eq!(bound[4].1, UniformValue::Float(0.5));
    }
}
