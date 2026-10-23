//! The checker's error families, each at the `.ramo` line it names, and the bodies it accepts.

use ramose_brush::{Diag, Shader};

fn errors(src: &str) -> Vec<Diag> {
    Shader::from_text(src)
        .err()
        .unwrap_or_else(|| panic!("checked clean:\n{src}"))
}

fn ok(src: &str) -> Shader {
    Shader::from_text(src).unwrap_or_else(|d| panic!("{d:#?}"))
}

#[test]
fn each_fault_family_is_one_e0360_at_its_line() {
    let cases: &[(&str, u32, &str)] = &[
        (
            "brush b\n  let n = nope\n  out m.paint.rgb cover:n\n",
            2,
            "unknown name `nope`",
        ),
        (
            "brush b\n  let n = m.uv + (1,2,3)\n  out m.paint.rgb cover:n.x\n",
            2,
            "cannot combine a vec2 and a vec3",
        ),
        (
            "brush b\n  let n = m.uv + 1,2,3\n  out m.paint.rgb cover:n.x\n",
            2,
            "a tuple has two or three components, this one has 4",
        ),
        (
            "brush b\n  out m.paint.rgb cover:rand(0 1)\n",
            2,
            "`rand` is not available in a body",
        ),
        (
            "brush b k:3\n  let n = max([each i in 0..k -> i])\n  out m.paint.rgb cover:n\n",
            2,
            "the bounds of a range in a body are constants",
        ),
        (
            "brush b\n  let n = max([each i in 0..300 -> i])\n  out m.paint.rgb cover:n\n",
            2,
            "runs at most 256 items, this one runs 300",
        ),
        (
            "brush b\n  let n = max([each i in 0..20 -> max([each j in 0..20 -> i*j])])\n  out m.paint.rgb cover:n\n",
            2,
            "runs at most 256 items, this one runs 400",
        ),
        ("brush b\n  let n = 1\n", 1, "brush `b` has no `out` line"),
        (
            "brush b\n  sksl \"\"\"\n    float4 shade(Mark m) { return m.paint; }\n  \"\"\"\n",
            2,
            "not SkSL",
        ),
        (
            "brush b\n  let edge = 1\n  out m.paint.rgb\n",
            2,
            "a `let` cannot be named `edge`: it is a field of `m`",
        ),
        (
            "brush b\n  fn f(edge) = edge\n  out m.paint.rgb cover:f(m.edge)\n",
            2,
            "a helper parameter cannot be named `edge`",
        ),
        (
            "brush b tint:#ff0000\n  out lerp(m.paint tint .5).rgb\n",
            2,
            "`lerp` on two colors is not available in a body",
        ),
        (
            "brush b\n  out m.paint\n",
            2,
            "`out` takes a vec3 of straight rgb, got a color",
        ),
        (
            "brush b\n  out m.paint.rgb cover:value(m.uv salt:1.5)\n",
            2,
            "must be a whole number",
        ),
        (
            "brush b\n  out m.paint.rgb cover:value(m.uv seed:-1)\n",
            2,
            "must be a whole number",
        ),
        (
            "brush b\n  out m.paint.rgb cover:fbm(m.uv oct:9)\n",
            2,
            "must be a whole number from 1 to 8",
        ),
        (
            "brush b\n  out m.paint.rgb cover:m.light.x\n",
            2,
            "`m.light` lands with #256",
        ),
        (
            "brush b\n  let n = #ff0000\n  out n.rgb\n",
            2,
            "a color literal `#ff0000` cannot be written in a body",
        ),
    ];
    for (src, line, needle) in cases {
        let ds = errors(src);
        assert_eq!(ds.len(), 1, "{src}\n{ds:#?}");
        assert_eq!(
            (ds[0].code, ds[0].at.file.as_str(), ds[0].at.span.line),
            ("E0360", "text", *line),
            "{src}\n{:#?}",
            ds[0]
        );
        assert!(ds[0].message.contains(needle), "{src}\n{:#?}", ds[0]);
    }
}

#[test]
fn a_parameter_named_for_the_sample_a_field_or_a_builtin_is_e0363_at_its_key() {
    for (src, needle) in [
        (
            "brush b edge:1\n  out m.paint.rgb\n",
            "`edge`: it is a field of `m`",
        ),
        (
            "brush b sin:1\n  out m.paint.rgb\n",
            "`sin`: it is a builtin",
        ),
        ("brush b m:1\n  out m.paint.rgb\n", "`m`: it is the sample"),
    ] {
        let ds = errors(src);
        assert_eq!(ds.len(), 1, "{src}\n{ds:#?}");
        assert_eq!(
            (ds[0].code, ds[0].at.span.line, ds[0].at.span.col),
            ("E0363", 1, 9),
            "{src}\n{:#?}",
            ds[0]
        );
        assert!(ds[0].message.contains(needle), "{src}\n{:#?}", ds[0]);
        assert_eq!(ds[0].fix.as_deref(), Some("rename the parameter"));
    }
}

#[test]
fn shader_words_are_free_names_and_every_type_of_parameter_binds() {
    let s = ok(
        "brush b half:2 float:4 shade:1 u_seed:1 v:1,2 w:1,2,3 c:#ff0000\n  let n = half*float*shade*u_seed\n  out c.rgb*n cover:cover(m.edge)*step(0 v.x+w.z)\n",
    );
    let names: Vec<&str> = s.body().params.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["half", "float", "shade", "u_seed", "v", "w", "c"]);
    assert!(s.sksl().contains("uniform float p_half;"), "{}", s.sksl());
    assert!(s.sksl().contains("uniform float4 p_c;"), "{}", s.sksl());
}

#[test]
fn every_builtin_reduction_and_noise_form_type_checks() {
    ok(
        "brush b\n  let p = m.uv/9\n  let a = abs(p)+floor(p)+round(p)+fract(p)+sqrt(p)+sin(p)+cos(p)+exp(p)\n  let b = min(a 1)+max(1 a)+clamp(a 0 1)+lerp(a p .5)+step(.5 a)+smoothstep(0 1 a)+pow(a 2)+mod(a 3)\n  let c = atan2(p.y p.x)+rad(90)+dot(p a)+length(b)+rot(p 45).x\n  let n = hash(p)+hash2(p).y+value(p salt:1)+simplex(p seed:7)+worley(p)+fbm(p oct:2 salt:3)\n  let r = max([each y in -1..2 -> sum([each x in -1..2 -> hash(floor(p)+(x,y))])])+min([1 2 c])\n  let v3 = (p,1)\n  let k = if n > .5 and not c < 0 then v3.xy.x else v3.z\n  out m.paint.rgb*k*r*normalize(v3).z cover:cover(m.edge+n)\n",
    );
}

#[test]
fn the_default_cover_is_the_edge_and_a_helper_sees_m_but_not_a_let() {
    let s = ok("brush b\n  fn f(x) = x*m.px\n  let k = f(2)\n  out m.paint.rgb*k\n");
    assert!(s.sksl().contains("cover(m.edge, m)"), "{}", s.sksl());
    let ds = errors("brush b\n  let k = 2\n  fn f(x) = x*k\n  out m.paint.rgb*f(1)\n");
    assert_eq!(ds.len(), 1, "{ds:#?}");
    assert_eq!(
        ds[0].at.span.line, 3,
        "at the helper, not the call: {:#?}",
        ds[0]
    );
    assert!(
        ds[0]
            .message
            .contains("a helper does not see the body's `let k`"),
        "{:#?}",
        ds[0]
    );
}

#[test]
fn a_body_that_reads_below_says_so_and_one_that_does_not_does_not() {
    assert!(ok("brush b\n  out m.below.rgb\n").reads_below());
    assert!(!ok("brush b\n  out m.paint.rgb\n").reads_below());
}

struct Fns(Vec<std::rc::Rc<ramose_brush::TopFn>>);

impl ramose_brush::Library for Fns {
    fn lookup(&self, _file: &str, name: &str) -> Option<ramose_brush::Global> {
        self.0.iter().find(|f| f.name == name).cloned().map(ramose_brush::Global::Fn)
    }
}

/// Checks every brush of `src` in order with one build-wide checker, as eval does.
fn check_all(src: &str) -> Vec<(String, Result<Shader, Vec<Diag>>)> {
    use ramose_syntax::ast::DeclKind;
    let file = ramose_syntax::parse(src).unwrap_or_else(|e| panic!("{e:?}"));
    let fns = Fns(
        file.decls()
            .filter_map(|d| match &d.kind {
                DeclKind::Fn { name, params, body } => Some(std::rc::Rc::new(ramose_brush::TopFn {
                    name: name.clone(),
                    params: params.clone(),
                    body: body.clone(),
                    module: "lib/fns.ramo".into(),
                    span: d.span,
                })),
                _ => None,
            })
            .collect(),
    );
    let mut ck = ramose_brush::Checker::default();
    file.decls()
        .filter_map(|d| match &d.kind {
            DeclKind::Brush { name, body, .. } => Some((name, body, d.span)),
            _ => None,
        })
        .map(|(name, lines, span)| {
            let src = ramose_brush::BrushSource { file: "art/t.ramo", name, span, params: &[], lines };
            (name.clone(), ck.check(&src, &fns))
        })
        .collect()
}

#[test]
fn a_fn_over_the_loop_budget_in_one_brush_is_sound_in_another_outside_a_loop() {
    let got = check_all(
        "fn g(x) = sum([each i in 0..20 -> x])\nbrush a\n  out m.paint.rgb cover:max([each j in 0..20 -> g(j)])\nbrush b\n  out m.paint.rgb cover:g(1)/20\n",
    );
    let ds = got[0].1.as_ref().err().unwrap_or_else(|| panic!("`a` checked clean"));
    assert!(ds.iter().any(|d| d.message.contains("at most 256 items")), "{ds:#?}");
    assert!(got[1].1.is_ok(), "`b` runs 20 items: {:#?}", got[1].1.as_ref().err());
}
