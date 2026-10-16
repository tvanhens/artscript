use std::fs;
use std::path::PathBuf;

use artscript_syntax::ast::{
    BinOp, CheckKind, DeclKind, Draw, DrawBody, DrawItem, Expr, File, Item, Mask, SceneKind, Span,
    UnOp,
};
use artscript_syntax::{format, format_source, parse};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn read(name: &str) -> String {
    fs::read_to_string(fixtures().join(name)).unwrap_or_else(|e| panic!("read {name}: {e}"))
}

fn must_parse(src: &str) -> File {
    parse(src).unwrap_or_else(|errs| {
        let text = errs
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        panic!("parse failed\n{src}\n{text}")
    })
}

fn round_trip_ast(file: &File) {
    let once = format(file);
    let again = format(&must_parse(&once));
    assert_eq!(once, again, "format is not idempotent:\n{once}");
}

fn round_trip_source(src: &str) {
    let once = format_source(src).unwrap_or_else(|errs| panic!("{errs:?}"));
    let twice = format_source(&once).unwrap_or_else(|errs| panic!("second pass\n{once}\n{errs:?}"));
    assert_eq!(once, twice, "format_source is not idempotent:\n{once}");
}

fn ident(s: &str) -> Expr {
    Expr::Ident(s.into())
}

fn num(n: f64) -> Expr {
    Expr::Num(n)
}

fn neg(e: Expr) -> Expr {
    Expr::Unary(UnOp::Neg, Box::new(e))
}

fn bin(op: BinOp, l: Expr, r: Expr) -> Expr {
    Expr::Binary(op, Box::new(l), Box::new(r))
}

fn mem(base: Expr, seg: &str) -> Expr {
    Expr::Member(Box::new(base), seg.into())
}

fn call(func: &str, args: Vec<Expr>) -> Expr {
    Expr::Call {
        func: func.into(),
        args,
        named: vec![],
    }
}

fn node(file: &File) -> &SceneKind {
    match &file.items[0] {
        Item::Scene(s) => &s.kind,
        other => panic!("not a scene: {other:?}"),
    }
}

fn decl(file: &File) -> &DeclKind {
    match &file.items[0] {
        Item::Decl(d) => &d.kind,
        other => panic!("not a declaration: {other:?}"),
    }
}

fn draw(file: &File) -> &Draw {
    match node(file) {
        SceneKind::Draw(d) => d,
        other => panic!("not a draw: {other:?}"),
    }
}

fn leaf(verb: &str, args: Vec<Expr>) -> SceneKind {
    SceneKind::Draw(Draw {
        verb: verb.into(),
        args,
        named: vec![],
        body: DrawBody::default(),
    })
}

fn errors(src: &str) -> Vec<artscript_syntax::Diagnostic> {
    parse(src).expect_err(src)
}

fn one_error(src: &str) -> artscript_syntax::Diagnostic {
    let errs = errors(src);
    assert_eq!(errs.len(), 1, "{src}: {errs:?}");
    errs.into_iter().next().unwrap()
}

#[test]
fn doc_fixtures_parse_and_round_trip() {
    let cases = [
        ("language_palette.art", "\"garden\"", "\"f3a2a6\""),
        ("language_curve.art", "\"curve\"", "\"peach\""),
        ("language_eye.art", "\"eye\"", "\"iris\""),
        ("language_checks.art", "\"level\"", "\"eye_l\""),
        ("mvp_palette.art", "\"dusk\"", "\"oklch\""),
        ("mvp_eye.art", "\"palette/dusk\"", "\"fff\""),
        ("mvp_hero.art", "\"chars/hero.svg\"", "\"arm_r\""),
        ("mvp_water.art", "\"water\"", "\"ripple\""),
        ("mvp_shader.art", "\"scales\"", "half4 main"),
        ("mvp_light.art", "\"sun\"", "\"ambient\""),
        ("mvp_body.art", "\"body\"", "\"torso\""),
        ("mvp_tunic.art", "\"tunic\"", "\"torso\""),
        ("mvp_villager.art", "\"villager\"", "\"hat\""),
        ("mvp_face.art", "\"face\"", "\"eye_l\""),
        ("mvp_icons.art", "\"icon_grid\"", "\"subject\""),
    ];
    for (name, left, right) in cases {
        let src = read(name);
        let file = must_parse(&src);
        let debug = format!("{file:?}");
        assert!(debug.contains(left), "{name} missing {left} in {debug}");
        assert!(debug.contains(right), "{name} missing {right} in {debug}");
        assert!(
            !file.directives.is_empty() || !file.items.is_empty(),
            "{name} produced an empty file"
        );
        round_trip_ast(&file);
        round_trip_source(&src);
    }
}

#[test]
fn mvp_targets_reject_reserved_alias() {
    let src = read("mvp_targets.art");
    let errs = parse(&src).expect_err("alias `t` is reserved");
    assert!(
        errs.iter().all(|e| e.code == "E0105" && e.fix.is_some()),
        "{errs:?}"
    );
    assert!(errs.iter().all(|e| e.message.contains("`t`")), "{errs:?}");
    let fixed = src
        .replace(" as t", " as teams")
        .replace("t.{team}", "teams.{team}");
    let file = must_parse(&fixed);
    assert_eq!(file.directives.len(), 2);
    assert_eq!(file.directives[0].path.as_deref(), Some("icons/{name}.svg"));
    assert_eq!(
        file.directives[1].path.as_deref(),
        Some("chars/knight-{armor}-{team}.png")
    );
    assert_eq!(file.directives[1].each.len(), 2);
    assert_eq!(file.directives[1].each[0].key, "armor");
    assert_eq!(file.directives[1].each[1].key, "team");
    round_trip_ast(&file);
    round_trip_source(&fixed);
}

#[test]
fn directive_target_matrix() {
    let src = "@target chars/hero.png 256x256 scale:1,2 as:knights each armor:[plate leather] team:[red blue]\n";
    let file = must_parse(src);
    let d = &file.directives[0];
    assert_eq!(d.name, "target");
    assert_eq!(d.path.as_deref(), Some("chars/hero.png"));
    assert_eq!(d.span, Span { line: 1, col: 1 });
    assert_eq!(
        d.args,
        vec![Expr::Size(Box::new(num(256.0)), Box::new(num(256.0)))]
    );
    assert_eq!(d.named[0].key, "scale");
    assert_eq!(d.named[0].value, Expr::Tuple(vec![num(1.0), num(2.0)]));
    assert_eq!(d.named[1].key, "as");
    assert_eq!(d.named[1].value, ident("knights"));
    assert_eq!(d.each[0].key, "armor");
    assert_eq!(
        d.each[0].value,
        Expr::List(vec![ident("plate"), ident("leather")])
    );
    assert_eq!(d.each[1].key, "team");
    assert_eq!(
        d.each[1].value,
        Expr::List(vec![ident("red"), ident("blue")])
    );
    assert_eq!(
        format(&file),
        "@target chars/hero.png 256x256 scale:1,2 as:knights each armor:[plate leather] team:[red blue]\n"
    );
    let clip = must_parse("@target monk_{c}.gif 512x512 bg:#f1ede4 each c:[idle blink] clip:c\n");
    assert_eq!(clip.directives[0].each[0].key, "c");
    assert_eq!(
        clip.directives[0]
            .named
            .iter()
            .map(|n| n.key.as_str())
            .collect::<Vec<_>>(),
        ["bg", "clip"]
    );
    assert_eq!(
        format(&clip),
        "@target monk_{c}.gif 512x512 bg:#f1ede4 each c:[idle blink] clip:c\n"
    );
}

#[test]
fn use_forms() {
    let aliased = must_parse("use a/b as x\n");
    assert_eq!(
        decl(&aliased),
        &DeclKind::Use {
            path: "a/b".into(),
            alias: Some("x".into()),
            items: vec![]
        }
    );
    let items = must_parse("use a/b { x y }\n");
    assert_eq!(
        decl(&items),
        &DeclKind::Use {
            path: "a/b".into(),
            alias: None,
            items: vec!["x".into(), "y".into()]
        }
    );
}

#[test]
fn curve_points() {
    let src = "curve -w/2,0 w/2,0 w*.4,70 -w*.4,70 closed\n";
    let file = must_parse(src);
    let w = ident("w");
    let point = |x, y| Expr::Tuple(vec![x, y]);
    assert_eq!(
        node(&file),
        &leaf(
            "curve",
            vec![
                point(bin(BinOp::Div, neg(w.clone()), num(2.0)), num(0.0)),
                point(bin(BinOp::Div, w.clone(), num(2.0)), num(0.0)),
                point(bin(BinOp::Mul, w.clone(), num(0.4)), num(70.0)),
                point(bin(BinOp::Mul, neg(w), num(0.4)), num(70.0)),
                ident("closed"),
            ],
        )
    );
    assert_eq!(format(&file), src);
}

#[test]
fn spaced_minus_starts_an_item() {
    let file = must_parse("draw a -b\n");
    assert_eq!(
        node(&file),
        &leaf("draw", vec![ident("a"), neg(ident("b"))])
    );
    let tight = must_parse("let v = a-b\n");
    let spaced = must_parse("let v = a - b\n");
    assert_eq!(node(&tight), node(&spaced));
    match node(&tight) {
        SceneKind::Let { value, .. } => {
            assert_eq!(value, &bin(BinOp::Sub, ident("a"), ident("b")));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn field_fill_and_stroke() {
    let fill = must_parse("rect 40x40 fill:fbm(worley(24)) * .6 + simplex(80) * .4\n");
    let d = draw(&fill);
    assert_eq!(d.named[0].key, "fill");
    assert_eq!(
        d.named[0].value,
        bin(
            BinOp::Add,
            bin(
                BinOp::Mul,
                call("fbm", vec![call("worley", vec![num(24.0)])]),
                num(0.6)
            ),
            bin(BinOp::Mul, call("simplex", vec![num(80.0)]), num(0.4)),
        )
    );
    let mapped = must_parse("rect 40x40 fill:worley(24) -> p.stone\n");
    assert_eq!(
        draw(&mapped).named[0].value,
        bin(
            BinOp::Map,
            call("worley", vec![num(24.0)]),
            mem(ident("p"), "stone")
        )
    );
    let stroke = must_parse("curve 0,0 9,9 stroke:p.ink,[2 8 3] taper:0,.6\n");
    let d = draw(&stroke);
    assert_eq!(d.verb, "curve");
    assert!(d.body.items().is_empty());
    assert_eq!(d.named[0].key, "stroke");
    assert_eq!(
        d.named[0].value,
        Expr::Tuple(vec![
            mem(ident("p"), "ink"),
            Expr::List(vec![num(2.0), num(8.0), num(3.0)]),
        ])
    );
    assert_eq!(d.named[1].key, "taper");
    assert_eq!(d.named[1].span, Span { line: 1, col: 36 });
    assert_eq!(d.named[1].value, Expr::Tuple(vec![num(0.0), num(0.6)]));
}

#[test]
fn let_if_and_assert_proportion() {
    let file = must_parse("let w = if build == slim then 34 else 42\n");
    assert_eq!(
        node(&file),
        &SceneKind::Let {
            name: "w".into(),
            value: Expr::If {
                cond: Box::new(bin(BinOp::Eq, ident("build"), ident("slim"))),
                then: Box::new(num(34.0)),
                otherwise: Box::new(num(42.0)),
            },
        }
    );
    let check = must_parse("assert @hero.head.h / @hero.h in 1/8..1/6 \"msg\"\n");
    assert_eq!(
        node(&check),
        &SceneKind::Check {
            kind: CheckKind::Assert,
            expr: bin(
                BinOp::In,
                bin(
                    BinOp::Div,
                    Expr::Ref(vec!["hero".into(), "head".into(), "h".into()]),
                    Expr::Ref(vec!["hero".into(), "h".into()])
                ),
                Expr::Range(
                    Box::new(bin(BinOp::Div, num(1.0), num(8.0))),
                    Box::new(bin(BinOp::Div, num(1.0), num(6.0))),
                ),
            ),
            named: vec![],
            message: Some("msg".into()),
        }
    );
}

#[test]
fn calls_members_and_colors() {
    let circle = must_parse("circle size*.55 at:look*size*.3 fill:iris\n");
    match node(&circle) {
        SceneKind::Draw(Draw {
            verb,
            args,
            named,
            body,
        }) => {
            assert_eq!(verb, "circle");
            assert!(body.items().is_empty());
            assert_eq!(args, &vec![bin(BinOp::Mul, ident("size"), num(0.55))]);
            assert_eq!(named[0].key, "at");
            assert_eq!(
                named[0].value,
                bin(
                    BinOp::Mul,
                    bin(BinOp::Mul, ident("look"), ident("size")),
                    num(0.3)
                )
            );
            assert_eq!(named[1].key, "fill");
            assert_eq!(named[1].value, ident("iris"));
        }
        other => panic!("{other:?}"),
    }
    let grad = must_parse("let v = grad(down p.sky.2 p.sky.5,.4)\n");
    match node(&grad) {
        SceneKind::Let { value, .. } => {
            assert_eq!(
                value,
                &call(
                    "grad",
                    vec![
                        ident("down"),
                        mem(mem(ident("p"), "sky"), "2"),
                        Expr::Tuple(vec![mem(mem(ident("p"), "sky"), "5"), num(0.4)]),
                    ]
                )
            );
        }
        other => panic!("{other:?}"),
    }
    let shade = must_parse("let v = shade(c -2)\n");
    match node(&shade) {
        SceneKind::Let { value, .. } => {
            assert_eq!(value, &call("shade", vec![ident("c"), neg(num(2.0))]));
        }
        other => panic!("{other:?}"),
    }
    let rect = must_parse("rect 100%x60% at:0,30%\n");
    match node(&rect) {
        SceneKind::Draw(Draw {
            verb, args, named, ..
        }) => {
            assert_eq!(verb, "rect");
            assert_eq!(
                args,
                &vec![Expr::Size(
                    Box::new(Expr::Percent(1.0)),
                    Box::new(Expr::Percent(60.0 / 100.0))
                )]
            );
            assert_eq!(named[0].key, "at");
            assert_eq!(named[0].span, Span { line: 1, col: 15 });
            assert_eq!(
                named[0].value,
                Expr::Tuple(vec![num(0.0), Expr::Percent(30.0 / 100.0)])
            );
        }
        other => panic!("{other:?}"),
    }
    let colors = must_parse("# comment\nlet a = #fff\nlet b = #1b1f3a\n");
    assert_eq!(colors.items.len(), 2);
    let lets: Vec<&Expr> = colors
        .scenes()
        .map(|s| match &s.kind {
            SceneKind::Let { value, .. } => value,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        lets,
        [&Expr::Hex("fff".into()), &Expr::Hex("1b1f3a".into())]
    );
    let formatted = format_source("# comment\nlet a = #fff\nlet b = #1b1f3a\n").unwrap();
    assert_eq!(formatted, "# comment\nlet a = #fff\nlet b = #1b1f3a\n");
    assert_eq!(format_source(&formatted).unwrap(), formatted);
}

#[test]
fn small_nodes() {
    let mirror = must_parse("mirror x\n");
    assert_eq!(node(&mirror), &leaf("mirror", vec![ident("x")]));
    let patch = must_parse("hero\n  .head.eye_l iris:p.accent\n  .cape off\n");
    match draw(&patch).body.items() {
        [DrawItem::Patch(p), DrawItem::Patch(cape)] => {
            assert_eq!(p.path, vec!["head".to_string(), "eye_l".to_string()]);
            assert!(!p.off);
            assert_eq!(p.named[0].key, "iris");
            assert_eq!(p.named[0].span, Span { line: 2, col: 15 });
            assert_eq!(p.named[0].value, mem(ident("p"), "accent"));
            assert_eq!(cape.path, vec!["cape".to_string()]);
            assert!(cape.named.is_empty() && cape.off);
        }
        other => panic!("{other:?}"),
    }
    let copy = must_parse("copy @body at:-14,-10\n");
    match node(&copy) {
        SceneKind::Draw(Draw {
            verb, args, named, ..
        }) => {
            assert_eq!(verb, "copy");
            assert_eq!(args, &vec![Expr::Ref(vec!["body".into()])]);
            assert_eq!(named[0].key, "at");
            assert_eq!(named[0].span, Span { line: 1, col: 12 });
            assert_eq!(
                named[0].value,
                Expr::Tuple(vec![neg(num(14.0)), neg(num(10.0))])
            );
        }
        other => panic!("{other:?}"),
    }
    let noise = must_parse("noise worley scale:24\n");
    match node(&noise) {
        SceneKind::Draw(Draw {
            verb, args, named, ..
        }) => {
            assert_eq!(verb, "noise");
            assert_eq!(args, &vec![ident("worley")]);
            assert_eq!(named[0].key, "scale");
            assert_eq!(named[0].span, Span { line: 1, col: 14 });
            assert_eq!(named[0].value, num(24.0));
        }
        other => panic!("{other:?}"),
    }
    let mask = must_parse("rect 40x40\n  mask simplex(30) > .5\n");
    assert_eq!(
        draw(&mask).body.mask(),
        Some(&Mask::Value(bin(
            BinOp::Gt,
            call("simplex", vec![num(30.0)]),
            num(0.5)
        )))
    );
}

#[test]
fn suffixes_slots_and_else_if() {
    let ang = must_parse("let ang = .25turn\n");
    match node(&ang) {
        SceneKind::Let { value, .. } => assert_eq!(value, &Expr::Angle(90.0)),
        other => panic!("{other:?}"),
    }
    let deg = must_parse("let ang = 30deg\n");
    match node(&deg) {
        SceneKind::Let { value, .. } => assert_eq!(value, &Expr::Angle(30.0)),
        other => panic!("{other:?}"),
    }
    let pct = must_parse("let pct = 50%\n");
    match node(&pct) {
        SceneKind::Let { value, .. } => assert_eq!(value, &Expr::Percent(0.5)),
        other => panic!("{other:?}"),
    }
    assert_eq!(format(&must_parse("let n = 0.50\n")), "let n = .5\n");
    assert_eq!(format(&must_parse("let n = 1.0\n")), "let n = 1\n");
    let slot = must_parse("hero\n  head:\n    eye 9\n");
    match draw(&slot).body.items() {
        [DrawItem::SlotFill(fill)] => {
            assert_eq!(fill.name, "head");
            assert_eq!(fill.body.len(), 1);
            assert_eq!(fill.body[0].kind, leaf("eye", vec![num(9.0)]));
        }
        other => panic!("{other:?}"),
    }
    let chain = must_parse("if a\n  x\nelse if b\n  y\nelse\n  z\n");
    match node(&chain) {
        SceneKind::If {
            cond,
            then,
            otherwise,
        } => {
            assert_eq!(cond, &ident("a"));
            assert_eq!(then[0].kind, leaf("x", vec![]));
            match &otherwise[0].kind {
                SceneKind::If {
                    cond,
                    then,
                    otherwise,
                } => {
                    assert_eq!(cond, &ident("b"));
                    assert_eq!(then[0].kind, leaf("y", vec![]));
                    assert_eq!(otherwise[0].kind, leaf("z", vec![]));
                }
                other => panic!("{other:?}"),
            }
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(format(&chain), "if a\n  x\nelse if b\n  y\nelse\n  z\n");
}

#[test]
fn statement_if_then_says_to_remove_then() {
    let errs = parse("if ready then\n  circle 1\n").unwrap_err();
    let e = errs.iter().find(|e| e.code == "E0101").expect("E0101");
    assert_eq!(
        e.fix.as_deref(),
        Some("statement `if` takes a block; remove `then`")
    );
}

#[test]
fn each_expr_is_one_list_and_formats_tight() {
    let src = "curve [each k in 0..70 -> cx+r*cos(k*.3),cy+r*sin(k*.3)]\n";
    let file = must_parse(src);
    match node(&file) {
        SceneKind::Draw(Draw { args, .. }) => match &args[0] {
            Expr::Each { var, iter, body } => {
                assert_eq!(var, "k");
                assert!(matches!(iter.as_ref(), Expr::Range(_, _)));
                assert!(matches!(body.as_ref(), Expr::Tuple(_)));
            }
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
    let once = format_source(src).unwrap();
    assert_eq!(once, src);
    assert_eq!(format_source(&once).unwrap(), once);
}

#[test]
fn syntax_errors_carry_a_fix() {
    let tab = parse("def arm\n\tcircle 1\n").unwrap_err();
    let tab_err = tab.iter().find(|e| e.code == "E0102").expect("tab");
    assert_eq!(tab_err.fix.as_deref(), Some("replace the tab with spaces"));
    assert_eq!(tab_err.span, Span { line: 2, col: 1 });

    let dedent = parse("def arm\n  circle 1\n circle 2\n").unwrap_err();
    let dedent_err = dedent.iter().find(|e| e.code == "E0103").expect("dedent");
    assert_eq!(
        dedent_err.notes,
        vec!["nearest valid parent is line 1".to_string()]
    );
    assert_eq!(
        dedent_err.fix.as_deref(),
        Some("indent to 0 spaces to align with line 1")
    );

    let open = parse("let a = \"hi\n").unwrap_err();
    let open_err = open
        .iter()
        .find(|e| e.message == "unterminated string")
        .expect("string");
    assert_eq!(open_err.code, "E0101");
    assert_eq!(open_err.fix.as_deref(), Some("close the string with `\"`"));

    let raw = parse("shader s\n  sksl \"\"\"\nno end\n").unwrap_err();
    let raw_err = raw
        .iter()
        .find(|e| e.message == "unterminated raw string")
        .expect("raw");
    assert_eq!(raw_err.code, "E0101");
    assert_eq!(
        raw_err.fix.as_deref(),
        Some("close the raw string with `\"\"\"`")
    );

    let reserved = parse("let t = 1\n").unwrap_err();
    assert_eq!(reserved[0].code, "E0105");
    assert_eq!(reserved[0].message, "`t` is reserved");
    assert!(
        reserved[0]
            .fix
            .as_ref()
            .is_some_and(|f| f.contains("rename `t`"))
    );

    let many = parse("let t = 1\n\tcircle 1\nlet a = \"nope\n").unwrap_err();
    assert!(many.iter().any(|e| e.code == "E0105"));
    assert!(many.iter().any(|e| e.code == "E0102"));
    assert!(
        many.iter()
            .any(|e| e.code == "E0101" && e.message == "unterminated string")
    );
    assert!(many.iter().all(|e| e.fix.is_some()));
    assert!(many.len() >= 3);
}

#[test]
fn separator_commas_are_rejected() {
    for (src, fix) in [
        (
            "let v = shade(c, -2)\n",
            "remove the comma: write `f(a b)`, not `f(a, b)`",
        ),
        (
            "let v = [a, b]\n",
            "remove the comma: write `[a b]`, not `[a, b]`",
        ),
        (
            "use a/b { x, y }\n",
            "remove the comma: write `use lib { a b }`",
        ),
    ] {
        let errs = parse(src).unwrap_err();
        assert_eq!(errs.len(), 1, "{src}: {errs:?}");
        assert_eq!(errs[0].code, "E0101");
        assert_eq!(errs[0].fix.as_deref(), Some(fix));
    }
    let points = must_parse("let v = [0,0 10,5]\n");
    match node(&points) {
        SceneKind::Let { value, .. } => assert_eq!(
            value,
            &Expr::List(vec![
                Expr::Tuple(vec![num(0.0), num(0.0)]),
                Expr::Tuple(vec![num(10.0), num(5.0)]),
            ])
        ),
        other => panic!("{other:?}"),
    }
}

#[test]
fn numbers_drop_leading_and_trailing_zeros() {
    assert_eq!(format(&must_parse("let n = 0.50\n")), "let n = .5\n");
    assert_eq!(format(&must_parse("let n = 1.0\n")), "let n = 1\n");
    assert_eq!(format(&must_parse("let n = .25turn\n")), "let n = 90deg\n");
    assert_eq!(format(&must_parse("rect 100%x60%\n")), "rect 100%x60%\n");
}

#[test]
fn statement_heads_round_trip_in_the_new_spelling() {
    let src = "\
palette garden from base
  ramp peach #f3a2a6 5
  ramp leaf #7db956 5 hue:12
  ramp sky #1b1f3a #f2a65a 7
  color ink #3d2632
def cup color:p.ink
  circle 10 fill:color
  slot mask
  fx shadow 0,6 blur:8
layer body
  fx rim 4 dir:1,1 color:p.rim a:.5 as:soft
  circle 12 fill:ramp(#fff 3).1
  mask
    ellipse 10,8 at:0,0
  fx glow 6 of:soft
group
  mask @body
  rect 4x4
if wet
  circle 3
    fx blur 2
cup
  mask:
    circle 4
  mask simplex(30) > .5
";
    let file = must_parse(src);
    assert_eq!(format(&file), src);
    round_trip_source(src);
}

#[test]
fn palette_row_in_the_old_order_says_how_to_reorder_it() {
    let e = one_error("palette p\n  cream ramp #f6e2c6 5\n");
    assert_eq!(e.code, "E0101");
    assert_eq!(e.message, "a palette row starts with `ramp` or `color`");
    assert_eq!(e.fix.as_deref(), Some("write `ramp cream #f6e2c6 5`"));
    assert_eq!(e.span, Span { line: 2, col: 3 });

    let e = one_error("palette p\n  ink #3d2632\n");
    assert_eq!(e.fix.as_deref(), Some("write `color ink #3d2632`"));
}

#[test]
fn palette_rows_take_ramp_or_color_payloads_only() {
    for (src, message) in [
        ("palette p\n  ramp a\n", "`ramp a` needs a color"),
        (
            "palette p\n  ramp a #fff #000 5 9\n",
            "a ramp takes one or two colors and an optional stop count",
        ),
        (
            "palette p\n  ramp a #fff 5 step:3\n",
            "a ramp takes no `step:`",
        ),
        (
            "palette p\n  color a #fff #000\n",
            "`color a` takes one color",
        ),
        (
            "palette p\n  fx blur 2\n",
            "a palette row starts with `ramp` or `color`",
        ),
    ] {
        assert_eq!(one_error(src).message, message, "{src}");
    }
}

#[test]
fn fx_is_rejected_where_no_draw_owns_it() {
    let top = one_error("fx grade warm:.2\n");
    assert_eq!(top.message, "`fx` cannot go at the top level");
    assert!(
        top.fix
            .as_ref()
            .is_some_and(|f| f.contains("directly under"))
    );

    let each = one_error("layer l\n  each i in 0..3\n    fx blur 2\n");
    assert_eq!(each.message, "`fx` cannot go directly under `each`");
    assert_eq!(each.span, Span { line: 3, col: 5 });

    let under_if = one_error("if a\n  fx blur 2\n");
    assert_eq!(under_if.message, "`fx` cannot go directly under `if`");

    must_parse("each i in 0..3\n  circle 2\n    fx blur 2\n");
}

#[test]
fn glued_mask_value_is_an_error_that_names_both_fixes() {
    let e = one_error("rect 4x4\n  mask:simplex(30) > .5\n");
    assert_eq!(e.message, "`mask:` must end the line");
    assert_eq!(
        e.fix.as_deref(),
        Some("write `mask simplex(30) > .5`, or end the line with `mask:` to fill a slot")
    );
    let e = one_error("fill:fbm(worley(24))\n");
    assert_eq!(e.message, "`fill:` must end the line");
}

#[test]
fn a_mask_has_a_value_or_a_block() {
    let both = one_error("group\n  mask @body\n    circle 3\n");
    assert_eq!(both.message, "a mask takes a value or a block, not both");
    let neither = one_error("group\n  mask\n");
    assert_eq!(neither.message, "`mask` needs a value or an indented block");
    let twice = one_error("group\n  mask @a\n  circle 1\n  mask @b\n");
    assert_eq!(twice.message, "a draw takes one mask");
    assert_eq!(twice.span, Span { line: 4, col: 3 });
}

#[test]
fn a_trailing_colon_on_a_keyword_is_still_a_slot() {
    let file = must_parse("cup\n  fx:\n    circle 4\n");
    let cup = draw(&file);
    match cup.body.items() {
        [DrawItem::SlotFill(fill)] => assert_eq!(fill.name, "fx"),
        other => panic!("{other:?}"),
    }
    assert!(cup.body.fx().next().is_none());
    must_parse("slot fx\n  circle 1\nslot mask\n");
    let file = must_parse("cup\n  .mask off\n  .head.fx rot:1\n");
    match draw(&file).body.items() {
        [DrawItem::Patch(mask), DrawItem::Patch(fx)] => {
            assert_eq!(mask.path, vec!["mask".to_string()]);
            assert!(mask.off);
            assert_eq!(fx.path, vec!["head".to_string(), "fx".to_string()]);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn mask_colon_is_a_slot_fill_and_mask_is_a_mask_block() {
    let file = must_parse("cup\n  mask:\n    circle 4\ngroup\n  mask\n    circle 4\n");
    let [Item::Scene(cup), Item::Scene(group)] = file.items.as_slice() else {
        panic!("{file:?}")
    };
    let (SceneKind::Draw(cup), SceneKind::Draw(group)) = (&cup.kind, &group.kind) else {
        panic!("{file:?}")
    };
    match cup.body.items() {
        [DrawItem::SlotFill(fill)] => assert_eq!(fill.name, "mask"),
        other => panic!("{other:?}"),
    }
    assert!(cup.body.mask().is_none());
    assert!(matches!(group.body.mask(), Some(Mask::Block(b)) if b.len() == 1));
    assert_eq!(
        format(&file),
        "cup\n  mask:\n    circle 4\ngroup\n  mask\n    circle 4\n"
    );
}

#[test]
fn def_body_holds_fx_but_no_mask_slot_fill_or_patch() {
    must_parse("def cup\n  circle 4\n    mask @x\n  fx glow 4\n");
    for (src, message) in [
        ("def cup\n  mask @x\n", "a `def` body cannot hold a mask"),
        (
            "def cup\n  mask\n    circle 1\n",
            "a `def` body cannot hold a mask",
        ),
        (
            "def cup\n  head:\n    circle 1\n",
            "a `def` body cannot hold slot fill `head:`",
        ),
        ("def cup\n  .arm off\n", "a `def` body cannot hold a patch"),
    ] {
        assert_eq!(one_error(src).message, message, "{src}");
    }
}

#[test]
fn fx_mask_and_slot_are_reserved_but_ramp_and_color_are_names() {
    for word in ["fx", "mask", "slot", "finish"] {
        let e = one_error(&format!("let {word} = 1\n"));
        assert_eq!(e.code, "E0105");
        assert_eq!(e.message, format!("`{word}` is reserved"));
    }
    must_parse("let ramp = 1\nlet color = ramp(#fff 3)\ncircle 3 fill:color\n");
}

#[test]
fn a_def_parameter_cannot_be_a_reserved_word() {
    for word in ["fx", "mask", "slot", "finish"] {
        let e = one_error(&format!("def cup {word}:1\n  circle 4\n"));
        assert_eq!(e.code, "E0105");
        assert_eq!(e.message, format!("`{word}` is reserved"));
    }
    must_parse("def cup color:#fff\n  circle 4 fill:color\n");
    must_parse("circle 4\n  fx blur 2 as:soft\n");
}

#[test]
fn fill_starts_an_ordinary_draw() {
    assert_eq!(
        node(&must_parse("fill #fff\n")),
        &leaf("fill", vec![Expr::Hex("fff".into())])
    );
}

#[test]
fn style_with_a_block_is_an_error_pointing_at_finish() {
    let errs = parse("style lacquer stroke:#000,2\n  fx rim 5\n").unwrap_err();
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert_eq!(errs[0].code, "E0101");
    assert_eq!(errs[0].span.line, 2);
    assert_eq!(
        errs[0].message,
        "a style takes named keys only, not a block"
    );
    assert_eq!(
        errs[0].fix.as_deref(),
        Some("declare `finish <name>` with this block and write `fx <name>` under the node")
    );
}

#[test]
fn finish_holds_fx_lines_and_round_trips() {
    let src = "finish surface width:4 a:.45 # shared\n  fx rim width dir:1,1 color:p.rim a:a\n\n  fx grain .04 scale:2.5\n\ncurve 0,0 9,9 name:cap\n  fx surface width:5\n";
    let file = must_parse(src);
    match decl(&file) {
        DeclKind::Finish { name, params, body } => {
            assert_eq!(name, "surface");
            let keys: Vec<&str> = params.iter().map(|p| p.key.as_str()).collect();
            assert_eq!(keys, ["width", "a"]);
            let ops: Vec<&str> = body.iter().map(|l| l.op.as_str()).collect();
            assert_eq!(ops, ["rim", "grain"]);
            assert_eq!(body[0].args, vec![ident("width")]);
            assert_eq!(body[1].span, Span { line: 4, col: 3 });
        }
        other => panic!("{other:?}"),
    }
    match &file.items[1] {
        Item::Scene(s) => match &s.kind {
            SceneKind::Draw(d) => match &d.body.items()[0] {
                DrawItem::Fx(line) => {
                    assert_eq!(line.op, "surface");
                    assert!(line.args.is_empty());
                    assert_eq!(line.named[0].key, "width");
                }
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
    let once = format_source(src).unwrap();
    assert_eq!(once, src);
    round_trip_ast(&file);
    round_trip_source(src);
}

#[test]
fn finish_body_takes_only_fx_lines() {
    let errs =
        parse("finish f\n  fx rim 3\n  circle 4\n  finish g\n    fx grain .1\n").unwrap_err();
    let lines: Vec<(u32, &str)> = errs
        .iter()
        .map(|e| (e.span.line, e.message.as_str()))
        .collect();
    assert_eq!(
        lines,
        [
            (3, "a finish holds only `fx` lines"),
            (4, "a finish holds only `fx` lines")
        ]
    );
    assert!(errs.iter().all(|e| e.code == "E0101" && e.fix.is_some()));

    let named = parse("finish f 3\n  fx rim 3\n").unwrap_err();
    assert_eq!(named[0].message, "finish parameters are named arguments");
    let empty = parse("finish f\n").unwrap_err();
    assert_eq!(
        empty[0].message,
        "finish `f` needs an indented block of `fx` lines"
    );
}

#[test]
fn time_anim_and_play_round_trip() {
    let src = "\
anim idle dur:2s loop
  .face at:0,key(0 6 0)
  .eyes.* lid:key(.6 1 .6 ease:in_out)
anim yawn dur:1.6s
  .mouth open:key(0 1 1 0 at:[0 .3 .7 1])
anim blink dur:200ms
  .eyes.* lid:key(.6 1 .6)
play idle start:0s
play none
let n = t
";
    let file = must_parse(src);
    match decl(&file) {
        DeclKind::Anim {
            name,
            dur,
            loop_,
            body,
        } => {
            assert_eq!(name, "idle");
            assert_eq!(dur, &Expr::Time(2.0));
            assert!(*loop_);
            assert_eq!(body.len(), 2);
            assert_eq!(body[1].path, ["eyes".to_string(), "*".into()]);
        }
        other => panic!("{other:?}"),
    }
    match &file.items[1] {
        Item::Decl(d) => match &d.kind {
            DeclKind::Anim { dur, loop_, .. } => {
                assert_eq!(dur, &Expr::Time(1.6));
                assert!(!*loop_);
            }
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
    match &file.items[2] {
        Item::Decl(d) => match &d.kind {
            DeclKind::Anim { dur, .. } => assert_eq!(dur, &Expr::Time(0.2)),
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
    match &file.items[3] {
        Item::Scene(s) => match &s.kind {
            SceneKind::Play { name, start } => {
                assert_eq!(name.as_deref(), Some("idle"));
                assert_eq!(start, &Some(Expr::Time(0.0)));
            }
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
    match &file.items[4] {
        Item::Scene(s) => match &s.kind {
            SceneKind::Play {
                name: None,
                start: None,
            } => {}
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
    let once = format(&file);
    assert!(once.contains("dur:200ms"), "{once}");
    assert!(once.contains("dur:2s"), "{once}");
    assert!(once.contains("dur:1.6s"), "{once}");
    assert!(once.contains(".eyes.*"), "{once}");
    round_trip_ast(&file);
    round_trip_source(src);
    assert_eq!(format(&must_parse("let n = 0.5s\n")), "let n = 500ms\n");
    assert_eq!(format(&must_parse("let n = 1500ms\n")), "let n = 1.5s\n");
    assert_eq!(format(&must_parse("let n = 0s\n")), "let n = 0s\n");
}
