use std::fs;
use std::path::PathBuf;

use artscript_syntax::ast::{BinOp, CheckKind, Expr, File, Span, StmtKind, UnOp};
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

fn node(file: &File) -> &StmtKind {
    &file.stmts[0].kind
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
            !file.directives.is_empty() || !file.stmts.is_empty(),
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
}

#[test]
fn use_forms() {
    let aliased = must_parse("use a/b as x\n");
    assert_eq!(
        node(&aliased),
        &StmtKind::Use {
            path: "a/b".into(),
            alias: Some("x".into()),
            items: vec![]
        }
    );
    let items = must_parse("use a/b { x y }\n");
    assert_eq!(
        node(&items),
        &StmtKind::Use {
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
        &StmtKind::Node {
            verb: "curve".into(),
            args: vec![
                point(bin(BinOp::Div, neg(w.clone()), num(2.0)), num(0.0)),
                point(bin(BinOp::Div, w.clone(), num(2.0)), num(0.0)),
                point(bin(BinOp::Mul, w.clone(), num(0.4)), num(70.0)),
                point(bin(BinOp::Mul, neg(w), num(0.4)), num(70.0)),
                ident("closed"),
            ],
            named: vec![],
            body: vec![],
        }
    );
    assert_eq!(format(&file), src);
}

#[test]
fn spaced_minus_starts_an_item() {
    let file = must_parse("draw a -b\n");
    assert_eq!(
        node(&file),
        &StmtKind::Node {
            verb: "draw".into(),
            args: vec![ident("a"), neg(ident("b"))],
            named: vec![],
            body: vec![],
        }
    );
    let tight = must_parse("let v = a-b\n");
    let spaced = must_parse("let v = a - b\n");
    assert_eq!(tight.stmts[0].kind, spaced.stmts[0].kind);
    match &tight.stmts[0].kind {
        StmtKind::Let { value, .. } => {
            assert_eq!(value, &bin(BinOp::Sub, ident("a"), ident("b")));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn field_fill_and_stroke() {
    let fill = must_parse("fill:fbm(worley(24)) * .6 + simplex(80) * .4\n");
    assert_eq!(
        node(&fill),
        &StmtKind::Node {
            verb: "fill".into(),
            args: vec![bin(
                BinOp::Add,
                bin(
                    BinOp::Mul,
                    call("fbm", vec![call("worley", vec![num(24.0)])]),
                    num(0.6)
                ),
                bin(BinOp::Mul, call("simplex", vec![num(80.0)]), num(0.4)),
            )],
            named: vec![],
            body: vec![],
        }
    );
    let mapped = must_parse("fill:worley(24) -> p.stone\n");
    assert_eq!(
        node(&mapped),
        &StmtKind::Node {
            verb: "fill".into(),
            args: vec![bin(
                BinOp::Map,
                call("worley", vec![num(24.0)]),
                mem(ident("p"), "stone")
            )],
            named: vec![],
            body: vec![],
        }
    );
    let stroke = must_parse("stroke:p.ink,[2 8 3] taper:0,.6\n");
    match node(&stroke) {
        StmtKind::Node {
            verb,
            args,
            named,
            body,
        } => {
            assert_eq!(verb, "stroke");
            assert!(body.is_empty());
            assert_eq!(
                args,
                &vec![Expr::Tuple(vec![
                    mem(ident("p"), "ink"),
                    Expr::List(vec![num(2.0), num(8.0), num(3.0)]),
                ])]
            );
            assert_eq!(named.len(), 1);
            assert_eq!(named[0].key, "taper");
            assert_eq!(named[0].span, Span { line: 1, col: 22 });
            assert_eq!(named[0].value, Expr::Tuple(vec![num(0.0), num(0.6)]));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn let_if_and_assert_proportion() {
    let file = must_parse("let w = if build == slim then 34 else 42\n");
    assert_eq!(
        node(&file),
        &StmtKind::Let {
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
        &StmtKind::Check {
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
        StmtKind::Node {
            verb,
            args,
            named,
            body,
        } => {
            assert_eq!(verb, "circle");
            assert!(body.is_empty());
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
    match &grad.stmts[0].kind {
        StmtKind::Let { value, .. } => {
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
    match &shade.stmts[0].kind {
        StmtKind::Let { value, .. } => {
            assert_eq!(value, &call("shade", vec![ident("c"), neg(num(2.0))]));
        }
        other => panic!("{other:?}"),
    }
    let rect = must_parse("rect 100%x60% at:0,30%\n");
    match node(&rect) {
        StmtKind::Node {
            verb, args, named, ..
        } => {
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
    assert_eq!(colors.stmts.len(), 2);
    match &colors.stmts[0].kind {
        StmtKind::Let { value, .. } => assert_eq!(value, &Expr::Hex("fff".into())),
        other => panic!("{other:?}"),
    }
    match &colors.stmts[1].kind {
        StmtKind::Let { value, .. } => assert_eq!(value, &Expr::Hex("1b1f3a".into())),
        other => panic!("{other:?}"),
    }
    let formatted = format_source("# comment\nlet a = #fff\nlet b = #1b1f3a\n").unwrap();
    assert_eq!(formatted, "# comment\nlet a = #fff\nlet b = #1b1f3a\n");
    assert_eq!(format_source(&formatted).unwrap(), formatted);
}

#[test]
fn small_nodes() {
    let mirror = must_parse("mirror x\n");
    assert_eq!(
        node(&mirror),
        &StmtKind::Node {
            verb: "mirror".into(),
            args: vec![ident("x")],
            named: vec![],
            body: vec![]
        }
    );
    let patch = must_parse(".head.eye_l iris:p.accent\n");
    match node(&patch) {
        StmtKind::Patch { path, named, off } => {
            assert_eq!(path, &vec!["head".to_string(), "eye_l".to_string()]);
            assert!(!off);
            assert_eq!(named[0].key, "iris");
            assert_eq!(named[0].span, Span { line: 1, col: 13 });
            assert_eq!(named[0].value, mem(ident("p"), "accent"));
        }
        other => panic!("{other:?}"),
    }
    let copy = must_parse("copy @body at:-14,-10\n");
    match node(&copy) {
        StmtKind::Node {
            verb, args, named, ..
        } => {
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
        StmtKind::Node {
            verb, args, named, ..
        } => {
            assert_eq!(verb, "noise");
            assert_eq!(args, &vec![ident("worley")]);
            assert_eq!(named[0].key, "scale");
            assert_eq!(named[0].span, Span { line: 1, col: 14 });
            assert_eq!(named[0].value, num(24.0));
        }
        other => panic!("{other:?}"),
    }
    let mask = must_parse("mask:simplex(30) > .5\n");
    assert_eq!(
        node(&mask),
        &StmtKind::Node {
            verb: "mask".into(),
            args: vec![bin(BinOp::Gt, call("simplex", vec![num(30.0)]), num(0.5))],
            named: vec![],
            body: vec![],
        }
    );
    let cape = must_parse(".cape off\n");
    assert_eq!(
        node(&cape),
        &StmtKind::Patch {
            path: vec!["cape".into()],
            named: vec![],
            off: true
        }
    );
}

#[test]
fn suffixes_slots_and_else_if() {
    let ang = must_parse("let ang = .25turn\n");
    match &ang.stmts[0].kind {
        StmtKind::Let { value, .. } => assert_eq!(value, &Expr::Angle(90.0)),
        other => panic!("{other:?}"),
    }
    let deg = must_parse("let ang = 30deg\n");
    match &deg.stmts[0].kind {
        StmtKind::Let { value, .. } => assert_eq!(value, &Expr::Angle(30.0)),
        other => panic!("{other:?}"),
    }
    let pct = must_parse("let pct = 50%\n");
    match &pct.stmts[0].kind {
        StmtKind::Let { value, .. } => assert_eq!(value, &Expr::Percent(0.5)),
        other => panic!("{other:?}"),
    }
    assert_eq!(format(&must_parse("let n = 0.50\n")), "let n = .5\n");
    assert_eq!(format(&must_parse("let n = 1.0\n")), "let n = 1\n");
    let slot = must_parse("head:\n  eye 9\n");
    match node(&slot) {
        StmtKind::SlotFill { name, body } => {
            assert_eq!(name, "head");
            assert_eq!(body.len(), 1);
            match &body[0].kind {
                StmtKind::Node { verb, args, .. } => {
                    assert_eq!(verb, "eye");
                    assert_eq!(args, &vec![num(9.0)]);
                }
                other => panic!("{other:?}"),
            }
        }
        other => panic!("{other:?}"),
    }
    let chain = must_parse("if a\n  x\nelse if b\n  y\nelse\n  z\n");
    match node(&chain) {
        StmtKind::If {
            cond,
            then,
            otherwise,
        } => {
            assert_eq!(cond, &ident("a"));
            assert!(matches!(&then[0].kind, StmtKind::Node { verb, .. } if verb == "x"));
            match &otherwise[0].kind {
                StmtKind::If {
                    cond,
                    then,
                    otherwise,
                } => {
                    assert_eq!(cond, &ident("b"));
                    assert!(matches!(&then[0].kind, StmtKind::Node { verb, .. } if verb == "y"));
                    assert!(
                        matches!(&otherwise[0].kind, StmtKind::Node { verb, .. } if verb == "z")
                    );
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
    assert_eq!(e.fix.as_deref(), Some("statement `if` takes a block; remove `then`"));
}

#[test]
fn each_expr_is_one_list_and_formats_tight() {
    let src = "curve [each k in 0..70 -> cx+r*cos(k*.3),cy+r*sin(k*.3)]\n";
    let file = must_parse(src);
    match node(&file) {
        StmtKind::Node { args, .. } => match &args[0] {
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
        ("let v = shade(c, -2)\n", "remove the comma: write `f(a b)`, not `f(a, b)`"),
        ("let v = [a, b]\n", "remove the comma: write `[a b]`, not `[a, b]`"),
        ("use a/b { x, y }\n", "remove the comma: write `use lib { a b }`"),
    ] {
        let errs = parse(src).unwrap_err();
        assert_eq!(errs.len(), 1, "{src}: {errs:?}");
        assert_eq!(errs[0].code, "E0101");
        assert_eq!(errs[0].fix.as_deref(), Some(fix));
    }
    let points = must_parse("let v = [0,0 10,5]\n");
    match node(&points) {
        StmtKind::Let { value, .. } => assert_eq!(
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
