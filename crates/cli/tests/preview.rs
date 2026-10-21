use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fixture(name: &str, source: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ramose-preview-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("art")).unwrap();
    fs::write(dir.join("ramose.toml"), "[project]\nout = \"assets\"\n[lint]\nraw-color = \"off\"\nunused = \"off\"\noff-canvas = \"off\"\npositional-seed = \"off\"\nhidden-child = \"off\"\n").unwrap();
    fs::write(dir.join("art/test.ramo"), source).unwrap();
    dir
}

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ramose"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn success(dir: &Path, args: &[&str]) -> Output {
    let output = run(dir, args);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn image(path: &Path) -> (u32, u32, Vec<u8>) {
    let mut decoder = png::Decoder::new(std::io::BufReader::new(fs::File::open(path).unwrap()));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().unwrap();
    let mut bytes = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut bytes).unwrap();
    bytes.truncate(info.buffer_size());
    (info.width, info.height, bytes)
}

fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    if !dir.exists() {
        return Vec::new();
    }
    let mut paths = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect::<Vec<_>>();
    paths.sort();
    paths
        .into_iter()
        .flat_map(|path| {
            if path.is_dir() {
                snapshot(&path)
            } else {
                vec![(path.clone(), fs::read(path).unwrap())]
            }
        })
        .collect()
}

#[test]
fn production_pixels_context_occlusion_and_nearest_zoom_are_preserved() {
    let dir = fixture(
        "pixels",
        "@target test_{tone}.png 16x12 scale:1,2 as:small each tone:[red blue]\nparam guides:false\nrect canvas.w,canvas.h at:canvas.w/2,canvas.h/2 fill:if tone == red then #ff0000 else #0000ff name:back\nrect 6,4 at:5,5 fill:#00ff00 name:head\nrect 2,2 at:4,4 fill:#ffffff name:occluder\nif guides\n  rect 1,12 fill:#ff00ff name:guide\n",
    );
    success(&dir, &["build", "--backend", "cpu"]);
    let before_assets = snapshot(&dir.join("assets"));
    let before_cache = snapshot(&dir.join(".ramose"));
    let output = success(
        &dir,
        &[
            "preview",
            "art/test.ramo",
            "--target",
            "test_red@2x.png",
            "--crop",
            "head",
            "--zoom",
            "3",
            "--out",
            "review/head.png",
            "--backend",
            "cpu",
        ],
    );
    let receipt: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(receipt["crop_pixels"], serde_json::json!([4, 6, 12, 8]));
    assert_eq!(receipt["parameter_altered"], false);
    assert_eq!(receipt["parameters"][0]["guides"], "false");
    let (pw, _, production) = image(&dir.join("assets/test_red@2x.png"));
    let (w, h, preview) = image(&dir.join("review/head.png"));
    assert_eq!((w, h), (36, 24));
    for y in 0..h {
        for x in 0..w {
            let actual = ((y * w + x) * 4) as usize;
            let expected = (((6 + y / 3) * pw + 4 + x / 3) * 4) as usize;
            assert_eq!(
                &preview[actual..actual + 4],
                &production[expected..expected + 4]
            );
        }
    }
    assert_eq!(snapshot(&dir.join("assets")), before_assets);
    assert_eq!(snapshot(&dir.join(".ramose")), before_cache);
    let bytes = fs::read(dir.join("review/head.png")).unwrap();
    success(
        &dir,
        &[
            "preview",
            "art/test.ramo",
            "--target",
            "test_red@2x.png",
            "--crop",
            "head",
            "--zoom",
            "3",
            "--out",
            "review/head.png",
            "--backend",
            "cpu",
        ],
    );
    assert_eq!(fs::read(dir.join("review/head.png")).unwrap(), bytes);
}

#[test]
fn parameters_check_build_cache_and_import_scope_share_binding() {
    let dir = fixture(
        "params",
        "@target test.png 12x12\nparam view:one_of(normal geometry) size:4 guides:false\nparam width:size\nrect width,4 at:width/2,2 fill:if view == normal then #ff0000 else #00ff00 name:body\nif guides\n  rect 1,12 at:11,0 fill:#0000ff name:guide\n",
    );
    success(&dir, &["build", "--backend", "cpu"]);
    let normal = fs::read(dir.join("assets/test.png")).unwrap();
    success(
        &dir,
        &["check", "--set", "view=geometry", "--set", "size=8"],
    );
    success(
        &dir,
        &[
            "build",
            "--set",
            "view=geometry",
            "--set",
            "size=8",
            "--backend",
            "cpu",
        ],
    );
    let (_, _, pixels) = image(&dir.join("assets/test.png"));
    assert_eq!(&pixels[4 * 6..4 * 7], &[0, 255, 0, 255]);
    success(&dir, &["build", "--backend", "cpu"]);
    assert_eq!(fs::read(dir.join("assets/test.png")).unwrap(), normal);
    let output = success(
        &dir,
        &[
            "preview",
            "art/test.ramo",
            "--set",
            "view=geometry",
            "--set",
            "guides=true",
            "--out",
            "review/diagnostic.png",
            "--backend",
            "cpu",
        ],
    );
    let receipt: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(receipt["parameter_altered"], true);
    assert_eq!(fs::read(dir.join("assets/test.png")).unwrap(), normal);
    fs::create_dir_all(dir.join("lib")).unwrap();
    fs::write(dir.join("art/other.ramo"), "@target other.png 12x12\nparam view:one_of(normal geometry)\ndef part\n  rect 4,4 fill:if view == normal then #ff0000 else #00ff00\n").unwrap();
    fs::write(
        dir.join("art/test.ramo"),
        "@target test_{view}.png 12x12 each view:[geometry]\nuse art/other { part }\npart\n",
    )
    .unwrap();
    success(
        &dir,
        &["build", "--only", "art/test.ramo", "--backend", "cpu"],
    );
    let (_, _, pixels) = image(&dir.join("assets/test_geometry.png"));
    assert_eq!(&pixels[..4], &[255, 0, 0, 255]);
    fs::write(dir.join("art/test.ramo"), "@target test.png 12x12\nuse art/other { part }\nparam view:one_of(normal geometry)\npart\n").unwrap();
    success(
        &dir,
        &[
            "build",
            "--only",
            "art/test.ramo",
            "--set",
            "view=geometry",
            "--backend",
            "cpu",
        ],
    );
    let (_, _, pixels) = image(&dir.join("assets/test.png"));
    assert_eq!(&pixels[..4], &[255, 0, 0, 255]);
}

#[test]
fn invalid_inputs_fail_without_reporting_a_stale_image() {
    let dir = fixture(
        "invalid",
        "@target test.png 8x8 scale:1,2\nparam view:one_of(normal geometry) guides:false\nrect 4,4 fill:#f00\n",
    );
    for args in [
        vec!["--set", "unknown=1"],
        vec!["--set", "guides=word"],
        vec!["--set", "view=bogus"],
        vec!["--crop", "missing"],
        vec!["--crop", "0,0,-1,4"],
        vec!["--time", "NaN"],
        vec!["--clip", "missing"],
        vec!["--bg", "ééé"],
        vec!["--target", "missing.png"],
        vec!["--set", "guides=rand(0 1)"],
    ] {
        let mut command = vec![
            "preview",
            "art/test.ramo",
            "--target",
            "test.png",
            "--out",
            "review/stale.png",
            "--backend",
            "cpu",
        ];
        command.extend(args);
        fs::create_dir_all(dir.join("review")).unwrap();
        fs::write(dir.join("review/stale.png"), b"stale").unwrap();
        let output = run(&dir, &command);
        assert_eq!(output.status.code(), Some(1), "{command:?}");
        assert!(output.stdout.is_empty());
        assert_eq!(fs::read(dir.join("review/stale.png")).unwrap(), b"stale");
    }
    let ambiguous = run(&dir, &["preview", "art/test.ramo", "--out", "review/a.png"]);
    assert_eq!(ambiguous.status.code(), Some(1));
    for path in [
        "assets/test.png",
        "assets/../assets/new.png",
        ".ramose/cache/new.png",
    ] {
        let output = run(
            &dir,
            &[
                "preview",
                "art/test.ramo",
                "--target",
                "test.png",
                "--out",
                path,
            ],
        );
        assert_eq!(output.status.code(), Some(1));
    }
    #[cfg(unix)]
    {
        fs::create_dir_all(dir.join("assets")).unwrap();
        std::os::unix::fs::symlink(dir.join("assets"), dir.join("alias")).unwrap();
        let output = run(
            &dir,
            &[
                "preview",
                "art/test.ramo",
                "--target",
                "test.png",
                "--out",
                "alias/new.png",
            ],
        );
        assert_eq!(output.status.code(), Some(1));
    }
    fs::create_dir_all(dir.join("review/directory.png")).unwrap();
    let output = run(
        &dir,
        &[
            "preview",
            "art/test.ramo",
            "--target",
            "test.png",
            "--out",
            "review/directory.png",
        ],
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
}

#[test]
fn frame_sheets_union_motion_and_define_loop_end_sampling() {
    let dir = fixture(
        "frames",
        "@target test.png 12x8 dur:1 clip:idle\nrect 2,2 at:(1+t*4),1 fill:#ff0000 name:head\n",
    );
    let output = success(
        &dir,
        &[
            "preview",
            "art/test.ramo",
            "--clip",
            "idle",
            "--frames",
            "4",
            "--crop",
            "head",
            "--columns",
            "2",
            "--bg",
            "light",
            "--out",
            "review/sheet.png",
            "--backend",
            "cpu",
        ],
    );
    let receipt: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(receipt["times"], serde_json::json!([0.0, 0.25, 0.5, 0.75]));
    assert_eq!(receipt["crop_pixels"], serde_json::json!([0, 0, 5, 2]));
    assert_eq!(image(&dir.join("review/sheet.png")).0, 10);
    let output = success(
        &dir,
        &[
            "preview",
            "art/test.ramo",
            "--times",
            "0,1",
            "--out",
            "review/seam.png",
            "--backend",
            "cpu",
        ],
    );
    let receipt: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(receipt["times"], serde_json::json!([0.0, 1.0]));
}

#[test]
fn backgrounds_composite_pixels_and_padding_does_not_replace_opaque_canvas() {
    let dir = fixture("background", "@target test.png 4x4 bg:#ff0000\n");
    success(
        &dir,
        &[
            "preview",
            "art/test.ramo",
            "--pad",
            "1",
            "--bg",
            "dark",
            "--out",
            "review/bg.png",
            "--backend",
            "cpu",
        ],
    );
    let (w, h, bytes) = image(&dir.join("review/bg.png"));
    assert_eq!((w, h), (6, 6));
    assert_eq!(&bytes[..4], &[32, 32, 32, 255]);
    assert_eq!(&bytes[28..32], &[255, 0, 0, 255]);
}

#[test]
fn receipt_compares_defaults_at_requested_times() {
    let dir = fixture(
        "time-default",
        "@target test.png 8x8\nparam offset:t\nrect 2,2 at:4+offset,4 fill:#ff0000\n",
    );
    let output = success(
        &dir,
        &[
            "preview",
            "art/test.ramo",
            "--time",
            "1",
            "--set",
            "offset=0",
            "--out",
            "review/time.png",
            "--backend",
            "cpu",
        ],
    );
    let receipt: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(receipt["parameter_altered"], true);
    assert_eq!(receipt["parameters"][0]["offset"], "0");
}

#[test]
fn svg_preview_preserves_the_production_evaluation_scale() {
    let dir = fixture(
        "mixed",
        "@target test.svg 8x8 scale:1,2\nrect scale*2,2 at:4,4 fill:#ff0000 name:body\n",
    );
    let project = ramose_eval::load(&dir).unwrap();
    let plan = ramose_eval::plan(&project);
    assert!(plan.diagnostics.is_empty(), "{:?}", plan.diagnostics);
    let job = &plan.jobs[0];
    let output = job
        .outputs
        .iter()
        .find(|o| o.format == ramose_eval::Format::Svg)
        .unwrap();
    let evaluated = ramose_eval::evaluate_outputs(&project, job);
    let expected = ramose_render::render_rgba(
        &evaluated.at(job.sample_scale(output)).unwrap().scene,
        output.scale,
        ramose_render::Backend::Cpu,
    )
    .unwrap();
    success(
        &dir,
        &[
            "preview",
            "art/test.ramo",
            "--target",
            &output.path,
            "--out",
            "review/mixed.png",
            "--backend",
            "cpu",
        ],
    );
    assert_eq!(image(&dir.join("review/mixed.png")), expected);
}

#[test]
fn receipt_preserves_parameter_precision() {
    let dir = fixture(
        "precision",
        "@target test.png 8x8\nparam offset:.001\nrect 2,2 at:4+offset,4 fill:#ff0000\n",
    );
    let output = success(
        &dir,
        &[
            "preview",
            "art/test.ramo",
            "--set",
            "offset=.002",
            "--out",
            "review/precise.png",
            "--backend",
            "cpu",
        ],
    );
    let receipt: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(receipt["parameter_altered"], true);
    assert_eq!(receipt["parameters"][0]["offset"], "0.002");
}

#[test]
fn still_preview_probes_the_requested_time_when_zero_is_invalid() {
    let dir = fixture(
        "probe-time",
        "@target test.png 8x8 t:1\nlet width = if t == 0 then false else 4/t\nrect width,4 at:4,4 fill:#ff0000 name:body\n",
    );
    success(&dir, &["build", "--backend", "cpu"]);
    let production = image(&dir.join("assets/test.png"));
    for (out, time) in [
        ("review/target.png", None),
        ("review/explicit.png", Some("1")),
    ] {
        let mut args = vec!["preview", "art/test.ramo", "--out", out, "--backend", "cpu"];
        if let Some(time) = time {
            args.extend(["--time", time]);
        }
        let output = success(&dir, &args);
        let receipt: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(receipt["times"], serde_json::json!([1.0]));
        assert_eq!(image(&dir.join(out)), production);
    }
    let invalid = run(
        &dir,
        &[
            "preview",
            "art/test.ramo",
            "--time",
            "0",
            "--out",
            "review/zero.png",
            "--backend",
            "cpu",
        ],
    );
    assert_eq!(invalid.status.code(), Some(1));
    assert!(invalid.stdout.is_empty());
}

#[test]
fn check_failures_still_generate_preview_with_diagnostics_and_nonzero_status() {
    let dir = fixture(
        "check-failure",
        "@target test.png 8x8\nrect 4,4 at:4,4 fill:#ff0000\nassert false\n",
    );
    let built = run(&dir, &["build", "--backend", "cpu"]);
    assert_eq!(built.status.code(), Some(2));
    let output = run(
        &dir,
        &[
            "preview",
            "art/test.ramo",
            "--times",
            "0,1",
            "--out",
            "review/failed-check.png",
            "--backend",
            "cpu",
        ],
    );
    assert_eq!(
        output.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(receipt["exit_code"], 2);
    assert_eq!(receipt["diagnostics"].as_array().unwrap().len(), 1);
    assert_eq!(receipt["diagnostics"][0]["code"], "C0101");
    assert!(String::from_utf8_lossy(&output.stderr).contains("C0101"));
    let (width, height, pixels) = image(&dir.join("review/failed-check.png"));
    assert_eq!((width, height), (16, 8));
    let (_, _, production) = image(&dir.join("assets/test.png"));
    for y in 0..8usize {
        assert_eq!(
            &pixels[y * 64..y * 64 + 32],
            &production[y * 32..y * 32 + 32]
        );
    }
}

#[test]
fn preview_reports_entry_word_warnings_and_lint_errors_without_losing_images() {
    let dir = fixture(
        "word-warning",
        "@target test.png 8x8\nparam view:one_of(normal geometry)\nrect 4,4 at:4,4 fill:#ff0000\nif view == typo\n  circle 1\n",
    );
    for expected in [3, 2] {
        if expected == 2 {
            use std::io::Write;
            fs::OpenOptions::new()
                .append(true)
                .open(dir.join("ramose.toml"))
                .unwrap()
                .write_all(b"unknown-word = \"error\"\n")
                .unwrap();
        }
        let output = run(
            &dir,
            &[
                "preview",
                "art/test.ramo",
                "--out",
                "review/lint.png",
                "--backend",
                "cpu",
            ],
        );
        assert_eq!(output.status.code(), Some(expected));
        let receipt: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(receipt["exit_code"], expected);
        assert_eq!(receipt["diagnostics"].as_array().unwrap().len(), 1);
        assert_eq!(receipt["diagnostics"][0]["code"], "L0107");
        assert_eq!(image(&dir.join("review/lint.png")).0, 8);
    }
}

#[test]
fn named_crop_does_not_box_between_cancelling_rotations() {
    let dir = fixture(
        "cancel-rotation",
        "@target test.png 80x80\ngroup outer at:40,40 rot:-45\n  group inner\n    rect 20,10 rot:45 fill:#ff0000 name:head\n",
    );
    success(&dir, &["build", "--backend", "cpu"]);
    for name in ["head", "inner", "outer"] {
        let output = success(
            &dir,
            &[
                "preview",
                "art/test.ramo",
                "--crop",
                name,
                "--out",
                "review/head.png",
                "--backend",
                "cpu",
            ],
        );
        let receipt: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(receipt["crop_pixels"], serde_json::json!([30, 35, 20, 10]));
        let (width, height, pixels) = image(&dir.join("review/head.png"));
        assert_eq!((width, height), (20, 10));
        let (production_width, _, production) = image(&dir.join("assets/test.png"));
        for row in 0..10usize {
            let begin = ((35 + row) * production_width as usize + 30) * 4;
            assert_eq!(
                &pixels[row * 80..(row + 1) * 80],
                &production[begin..begin + 80]
            );
        }
    }
}
