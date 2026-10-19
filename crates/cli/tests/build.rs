//! End-to-end CLI runs on a temporary copy of `examples/chibi`, trimmed to
//! the art files each test needs, since `check` evaluates every file.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap().flatten() {
        let (path, dest) = (entry.path(), to.join(entry.file_name()));
        if path.is_dir() {
            copy_dir(&path, &dest);
        } else {
            fs::copy(&path, &dest).unwrap();
        }
    }
}

fn fixture(tag: &str, art: &[&str]) -> PathBuf {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/chibi");
    let dir = std::env::temp_dir().join(format!("artscript-cli-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    copy_dir(&src.join("lib"), &dir.join("lib"));
    fs::copy(src.join("artscript.toml"), dir.join("artscript.toml")).unwrap();
    fs::create_dir_all(dir.join("art")).unwrap();
    for file in art {
        let dest = dir.join("art").join(file);
        fs::create_dir_all(dest.parent().unwrap()).unwrap();
        fs::copy(src.join("art").join(file), dest).unwrap();
    }
    dir
}

fn artscript(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_artscript"))
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap()
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn manifest(dir: &Path) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(dir.join("assets/art-manifest.json")).unwrap())
        .unwrap()
}

fn fnv_hex(bytes: &[u8]) -> String {
    format!("{:016x}", artscript_eval::fnv1a(bytes))
}

#[test]
fn build_writes_outputs_manifest_cache_and_deletes_stale_outputs() {
    let dir = fixture("build", &["produce/momo_peach.art"]);
    let png = dir.join("assets/produce/momo_peach.png");
    let svg = dir.join("assets/produce/momo_peach.svg");

    let only = ["build", "--only", "produce/momo_peach.*"];
    let first = artscript(&dir, &only);
    assert_eq!(first.status.code(), Some(0), "{}", text(&first));
    assert!(!text(&first).contains("L0101"), "{}", text(&first));
    let (png_bytes, svg_bytes) = (fs::read(&png).unwrap(), fs::read(&svg).unwrap());
    assert_eq!(&png_bytes[1..4], b"PNG");
    assert!(String::from_utf8_lossy(&svg_bytes).contains("<svg"));

    let m = manifest(&dir);
    assert_eq!(m["version"], 1);
    let outputs: Vec<(&str, &str, &str)> = m["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| {
            (
                o["path"].as_str().unwrap(),
                o["hash"].as_str().unwrap(),
                o["status"].as_str().unwrap(),
            )
        })
        .collect();
    let (png_hash, svg_hash) = (fnv_hex(&png_bytes), fnv_hex(&svg_bytes));
    assert_eq!(
        outputs,
        [
            ("produce/momo_peach.png", png_hash.as_str(), "pass"),
            ("produce/momo_peach.svg", svg_hash.as_str(), "pass")
        ]
    );
    let manifest_bytes = fs::read(dir.join("assets/art-manifest.json")).unwrap();

    let started = std::time::Instant::now();
    let second = artscript(&dir, &only);
    assert_eq!(second.status.code(), Some(0), "{}", text(&second));
    assert!(
        started.elapsed().as_secs_f32() < 2.0,
        "cache miss: second build took {:?}",
        started.elapsed()
    );
    assert_eq!(fs::read(&png).unwrap(), png_bytes);
    assert_eq!(fs::read(&svg).unwrap(), svg_bytes);
    assert_eq!(
        fs::read(dir.join("assets/art-manifest.json")).unwrap(),
        manifest_bytes
    );

    let art = dir.join("art/produce/momo_peach.art");
    let src = fs::read_to_string(&art).unwrap();
    fs::write(
        &art,
        src.replace("@target produce/momo_peach.svg 1024x1024\n", ""),
    )
    .unwrap();
    let third = artscript(&dir, &only);
    assert_eq!(third.status.code(), Some(0), "{}", text(&third));
    assert!(!svg.exists(), "stale svg survived");
    assert!(text(&third).contains("removed stale produce/momo_peach.svg"));
    let paths: Vec<String> = manifest(&dir)["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["path"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(paths, ["produce/momo_peach.png"]);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn check_json_reports_failed_asserts_with_exit_2() {
    let dir = fixture("check", &["produce/momo_peach.art"]);
    let art = dir.join("art/produce/momo_peach.art");
    let src = fs::read_to_string(&art).unwrap();
    fs::write(
        &art,
        src.replace("name:eye_r", "name:eye_r rot:0 at:550,640"),
    )
    .unwrap();
    let out = artscript(&dir, &["check", "--json"]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let diags = json["diagnostics"].as_array().unwrap();
    let level = diags
        .iter()
        .find(|d| d["code"] == "C0101")
        .unwrap_or_else(|| panic!("{json}"));
    assert_eq!(level["file"], "art/produce/momo_peach.art");
    assert_eq!(level["severity"], "error");
    assert_eq!(level["message"], "assert level(@eye_l @eye_r) tol:.5");
    assert!(
        level["fix"]
            .as_str()
            .unwrap()
            .starts_with("set .eye_r at:550,"),
        "{level}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn gif_frames_match_the_sampled_palette() {
    let dir = std::env::temp_dir().join(format!("artscript-cli-gif-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("art")).unwrap();
    fs::write(
        dir.join("artscript.toml"),
        "[project]\nname = \"gif\"\nout = \"assets\"\nlib = []\nstd = \"0.1\"\n",
    )
    .unwrap();
    fs::write(
        dir.join("art/orb.art"),
        "@target orb_{c}.gif 96x96 bg:#f1ede4 fps:24 each c:[spin pulse] clip:c

palette p
  ramp moss #6f8f4e 5
  color ink #2b2440

group at:48,48 name:orb
  circle 30 fill:grad(down p.moss.3 p.moss.1)
  ellipse 12,6 at:-10,-14 fill:p.moss.4 a:.8 blur:3
  circle 5 at:12,0 fill:p.ink

anim spin dur:500ms loop
  .orb rot:key(0 180 360)
anim pulse dur:300ms
  .orb scale:key(1 1.2 1)

if clip == spin
  play spin
if clip == pulse
  play pulse
",
    )
    .unwrap();
    let built = artscript(&dir, &["build", "--backend", "cpu"]);
    assert_eq!(built.status.code(), Some(0), "{}", text(&built));
    let project = artscript_eval::load(&dir).unwrap();
    let plan = artscript_eval::plan(&project);
    let jobs: Vec<_> = plan
        .jobs
        .iter()
        .filter(|j| {
            j.outputs
                .iter()
                .any(|o| o.format == artscript_eval::Format::Gif)
        })
        .collect();
    assert_eq!(jobs.len(), 2, "spin and pulse");
    for job in jobs {
        let sampled = artscript_eval::sample(&project, job);
        let bad: Vec<_> = sampled
            .diagnostics
            .iter()
            .filter(|d| d.severity == artscript_syntax::Severity::Error)
            .map(|d| d.code.clone())
            .collect();
        assert!(bad.is_empty(), "{bad:?} {}", job.outputs[0].path);
        let bytes = fs::read(dir.join("assets").join(&job.outputs[0].path)).unwrap();
        let mut reader = gif::DecodeOptions::new()
            .read_info(std::io::Cursor::new(&bytes))
            .unwrap();
        assert_eq!(reader.width(), 96);
        assert_eq!(reader.height(), 96);
        let expect_repeat = if sampled.loops {
            gif::Repeat::Infinite
        } else {
            gif::Repeat::Finite(0)
        };
        assert_eq!(reader.repeat(), expect_repeat, "{}", job.outputs[0].path);
        let mut rasters = Vec::with_capacity(sampled.frames.len());
        for frame in &sampled.frames {
            let (w, h, px) =
                artscript_render::render_rgba(
                &frame.scene,
                job.outputs[0].scale,
                artscript_render::Backend::Cpu,
            )
            .unwrap();
            assert_eq!((w, h), (96, 96));
            rasters.push(px);
        }
        let flat: Vec<u8> = rasters.iter().flatten().copied().collect();
        let nq = color_quant::NeuQuant::new(10, 256, &flat);
        let delays = artscript_eval::frame_delays(sampled.frames.len(), sampled.dur).unwrap();
        let mut seen = 0usize;
        while let Some(frame) = reader.read_next_frame().unwrap() {
            assert_eq!(
                frame.delay, delays[seen],
                "frame {seen} {}",
                job.outputs[0].path
            );
            assert_eq!((frame.width, frame.height), (96, 96));
            assert!(frame.palette.is_none(), "one global palette");
            let src = &rasters[seen];
            assert_eq!(frame.buffer.len(), src.len() / 4);
            for (px, idx) in src.as_chunks::<4>().0.iter().zip(frame.buffer.iter()) {
                assert_eq!(
                    *idx as usize,
                    nq.index_of(px),
                    "frame {seen} {}",
                    job.outputs[0].path
                );
            }
            seen += 1;
        }
        assert_eq!(seen, sampled.frames.len());
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_gif_with_more_frames_than_centiseconds_is_e0206() {
    let dir = std::env::temp_dir().join(format!("artscript-cli-centi-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("art")).unwrap();
    fs::write(
        dir.join("artscript.toml"),
        "[project]\nname = \"centi\"\nout = \"assets\"\nlib = [\"lib\"]\nstd = \"0.1\"\n",
    )
    .unwrap();
    fs::write(
        dir.join("art/main.art"),
        "@target tiny.gif 8x8 bg:#ffffff fps:120 dur:1s\n",
    )
    .unwrap();
    let out = artscript(&dir, &["build"]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    let err = text(&out);
    assert!(
        err.contains("E0206")
            && err.contains("a gif has more frames than centiseconds")
            && err.contains("lower `fps` or lengthen `dur`"),
        "{err}"
    );
    assert!(!dir.join("assets/tiny.gif").exists(), "{err}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_gif_frame_longer_than_the_field_is_e0206() {
    let dir = std::env::temp_dir().join(format!("artscript-cli-long-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("art")).unwrap();
    fs::write(
        dir.join("artscript.toml"),
        "[project]\nname = \"long\"\nout = \"assets\"\nlib = [\"lib\"]\nstd = \"0.1\"\n",
    )
    .unwrap();
    fs::write(
        dir.join("art/main.art"),
        "@target tiny.gif 8x8 bg:#ffffff fps:.001 dur:700s\n",
    )
    .unwrap();
    let out = artscript(&dir, &["build"]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    let err = text(&out);
    assert!(
        err.contains("E0206")
            && err.contains("a gif frame is longer than 65535cs")
            && err.contains("raise `fps` or shorten `dur`"),
        "{err}"
    );
    assert!(!dir.join("assets/tiny.gif").exists(), "{err}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn check_samples_a_gif_so_a_later_assert_fails() {
    let dir = std::env::temp_dir().join(format!("artscript-cli-gifcheck-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("art")).unwrap();
    fs::write(
        dir.join("artscript.toml"),
        "[project]\nname = \"gifcheck\"\nout = \"assets\"\nlib = [\"lib\"]\nstd = \"0.1\"\n",
    )
    .unwrap();
    fs::write(
        dir.join("art/main.art"),
        "@target still.png 16x16\n@target clip.gif 16x16 bg:#ffffff fps:2 dur:1s\ncircle 4\nassert t < .5\n",
    )
    .unwrap();
    let out = artscript(&dir, &["check"]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    let err = text(&out);
    assert!(err.contains("C0101"), "{err}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn check_reports_a_gif_build_would_refuse() {
    let dir = std::env::temp_dir().join(format!("artscript-cli-giftime-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("art")).unwrap();
    fs::write(
        dir.join("artscript.toml"),
        "[project]\nname = \"giftime\"\nout = \"assets\"\nlib = [\"lib\"]\nstd = \"0.1\"\n",
    )
    .unwrap();
    fs::write(
        dir.join("art/main.art"),
        "@target tiny.gif 8x8 bg:#ffffff fps:120 dur:1s\n",
    )
    .unwrap();
    let out = artscript(&dir, &["check"]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    let err = text(&out);
    assert!(
        err.contains("E0206") && err.contains("a gif has more frames than centiseconds"),
        "{err}"
    );
    fs::write(
        dir.join("art/main.art"),
        "@target tiny.gif 8x8 bg:#ffffff\n",
    )
    .unwrap();
    let bare = artscript(&dir, &["check"]);
    assert_eq!(bare.status.code(), Some(1), "{}", text(&bare));
    assert!(
        text(&bare).contains("clip has no duration"),
        "{}",
        text(&bare)
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_sample_raster_cap_matches_the_encoder() {
    assert_eq!(
        artscript_eval::GIF_CLIP_BUDGET,
        artscript_render::RASTER_BUDGET
    );
}

#[test]
fn check_reports_a_gif_wider_than_65535px() {
    let dir = std::env::temp_dir().join(format!("artscript-cli-gifwide-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("art")).unwrap();
    fs::write(
        dir.join("artscript.toml"),
        "[project]\nname = \"gifwide\"\nout = \"assets\"\nlib = [\"lib\"]\nstd = \"0.1\"\n",
    )
    .unwrap();
    fs::write(
        dir.join("art/main.art"),
        "@target wide.gif 70000x1 bg:#ffffff dur:1s fps:1\ncircle 1\n",
    )
    .unwrap();
    let out = artscript(&dir, &["check"]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    assert!(
        text(&out).contains("a gif is larger than 65535px"),
        "{}",
        text(&out)
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn check_reports_a_gif_that_would_retain_too_many_scenes() {
    let dir = std::env::temp_dir().join(format!("artscript-cli-gifscene-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("art")).unwrap();
    fs::write(
        dir.join("artscript.toml"),
        "[project]\nname = \"gifscene\"\nout = \"assets\"\nlib = [\"lib\"]\nstd = \"0.1\"\n",
    )
    .unwrap();
    fs::write(
        dir.join("art/main.art"),
        "@target heavy.gif 8x8 bg:#ffffff dur:10s fps:100\neach i in 0..1500\n  circle 1 at:i,0\n",
    )
    .unwrap();
    let out = artscript(&dir, &["check"]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    assert!(
        text(&out).contains("a gif retains more than 256MiB"),
        "{}",
        text(&out)
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn check_reports_a_gif_raster_over_the_budget() {
    let dir = std::env::temp_dir().join(format!("artscript-cli-gifraster-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("art")).unwrap();
    fs::write(
        dir.join("artscript.toml"),
        "[project]\nname = \"gifraster\"\nout = \"assets\"\nlib = [\"lib\"]\nstd = \"0.1\"\n",
    )
    .unwrap();
    fs::write(
        dir.join("art/main.art"),
        "@target huge.gif 4096x4096 bg:#ffffff dur:1s fps:5\ncircle 1\n",
    )
    .unwrap();
    let out = artscript(&dir, &["check"]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    assert!(
        text(&out).contains("a gif raster is larger than 256MiB"),
        "{}",
        text(&out)
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn check_reports_a_gif_without_an_opaque_background() {
    let dir = std::env::temp_dir().join(format!("artscript-cli-gifbg-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("art")).unwrap();
    fs::write(
        dir.join("artscript.toml"),
        "[project]\nname = \"gifbg\"\nout = \"assets\"\nlib = [\"lib\"]\nstd = \"0.1\"\n",
    )
    .unwrap();
    fs::write(
        dir.join("art/main.art"),
        "@target tiny.gif 8x8 dur:1s fps:2\ncircle 1\n",
    )
    .unwrap();
    let out = artscript(&dir, &["check"]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    assert!(
        text(&out).contains("a gif needs an opaque `bg:`"),
        "{}",
        text(&out)
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn backend_flag_is_validated_and_switching_backends_rebuilds() {
    let dir = fixture("backend", &["produce/momo_peach.art"]);
    let bad = artscript(&dir, &["build", "--backend", "metal"]);
    assert_eq!(bad.status.code(), Some(1), "{}", text(&bad));
    assert!(text(&bad).contains("backend `metal`"), "{}", text(&bad));
    let missing = artscript(&dir, &["build", "--backend"]);
    assert_eq!(missing.status.code(), Some(1), "{}", text(&missing));

    let only = ["build", "--only", "produce/momo_peach.*"];
    let cpu = artscript(&dir, &[&only[..], &["--backend", "cpu"]].concat());
    assert!(cpu.status.success(), "{}", text(&cpu));
    let png = dir.join("assets/produce/momo_peach.png");
    let cpu_bytes = fs::read(&png).unwrap();
    let again = artscript(&dir, &[&only[..], &["--backend=cpu"]].concat());
    assert!(again.status.success(), "{}", text(&again));
    assert_eq!(fs::read(&png).unwrap(), cpu_bytes);

    if artscript_render::gpu_available() {
        let gpu = artscript(&dir, &[&only[..], &["--backend", "gpu"]].concat());
        assert!(gpu.status.success(), "{}", text(&gpu));
        let project = artscript_eval::load(&dir).unwrap();
        let plan = artscript_eval::plan(&project);
        let job = plan.jobs.iter().find(|j| j.source.ends_with("momo_peach.art")).unwrap();
        let scene = artscript_eval::evaluate(&project, job).scene.unwrap();
        let want = artscript_render::render_png(&scene, 1.0, artscript_render::Backend::Gpu).unwrap();
        assert_eq!(fs::read(&png).unwrap(), want, "a backend switch must re-render on the gpu");
    }
}

#[test]
fn forced_gpu_without_a_gpu_still_reports_plan_errors() {
    if artscript_render::gpu_available() {
        return;
    }
    let dir = fixture("nogpu", &[]);
    fs::write(dir.join("art/broken.art"), "@target broken.png 64x64 scale:-1\n").unwrap();
    let out = artscript(&dir, &["build", "--backend", "gpu"]);
    let report = text(&out);
    assert_ne!(out.status.code(), Some(0), "{report}");
    assert!(report.contains("E0400"), "{report}");
    assert!(report.contains("E0206"), "plan errors survive a missing gpu: {report}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_file_with_only_unknown_directives_is_e0208() {
    let dir = fixture("unknown-directive", &[]);
    fs::write(
        dir.join("art/broken.art"),
        "@taget broken.png 64x64\nrect 0 0 10 10\n",
    )
    .unwrap();
    fs::write(dir.join("lib/bogus.art"), "@bogus\n").unwrap();
    for cmd in ["check", "build"] {
        let out = artscript(&dir, &[cmd]);
        let err = text(&out);
        assert_eq!(out.status.code(), Some(1), "{cmd}: {err}");
        for (file, name) in [("art/broken.art", "@taget"), ("lib/bogus.art", "@bogus")] {
            let line = format!("error[E0208] {file}:1:1");
            assert_eq!(err.matches(&line).count(), 1, "{cmd}: {err}");
            assert!(
                err.contains(&format!("unknown directive `{name}`")),
                "{cmd}: {err}"
            );
        }
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn adding_2x_leaves_the_1x_taper_png_unchanged() {
    let dir = fixture("taper", &[]);
    let curve = "curve 40,200 128,40 216,200 stroke:p.ink,[2 14 2] taper:.3\n";
    let mut builds = Vec::new();
    for target in [
        "@target taper.png 256x256",
        "@target taper.png 256x256 scale:1,2",
    ] {
        fs::write(dir.join("art/taper.art"), format!("{target}\n{curve}")).unwrap();
        let out = artscript(&dir, &["build", "--backend", "cpu"]);
        assert!(out.status.success(), "{target}: {}", text(&out));
        builds.push(fs::read(dir.join("assets/taper.png")).unwrap());
    }
    assert!(dir.join("assets/taper@2x.png").exists());
    assert!(
        builds[0] == builds[1],
        "taper.png changed when scale:1,2 was added"
    );
}

#[test]
fn a_tapered_stroke_draws_over_the_fill() {
    let dir = fixture("taper-fill", &[]);
    fs::write(
        dir.join("art/t.art"),
        "@target t.png 96x96\nblob 30 at:48,48 fill:#d8a840 stroke:#202030,[2 8 2] name:a\n",
    )
    .unwrap();
    let out = artscript(&dir, &["build", "--backend", "cpu", "--allow-warnings"]);
    assert!(out.status.success(), "{}", text(&out));
    let mut decoder = png::Decoder::new(std::io::Cursor::new(fs::read(dir.join("assets/t.png")).unwrap()));
    decoder.set_transformations(png::Transformations::normalize_to_color8() | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    let at = |x: usize, y: usize| {
        let i = (y * info.width as usize + x) * 4;
        [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
    };
    assert_eq!(at(48, 48), [0xd8, 0xa8, 0x40, 0xff], "the fill shows inside a width-list stroke");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_bad_target_profile_is_e0206_on_every_format() {
    let dir = std::env::temp_dir().join(format!("artscript-cli-profile-{}", std::process::id()));
    let cases = [
        ("broken.png", "bogus", "unknown profile `bogus`"),
        ("broken.gif", "bogus", "unknown profile `bogus`"),
        ("broken.svg", "bogus", "unknown profile `bogus`"),
        ("broken.svg", "#fff", "unknown profile"),
        ("broken.png", "vector", "`profile:vector` needs an .svg target"),
        ("broken.gif", "svg", "`profile:svg` needs an .svg target"),
    ];
    for (path, profile, message) in cases {
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("art")).unwrap();
        fs::write(
            dir.join("artscript.toml"),
            "[project]\nname = \"profile\"\nout = \"assets\"\nlib = [\"lib\"]\nstd = \"0.1\"\n",
        )
        .unwrap();
        fs::write(
            dir.join("art/main.art"),
            format!("@target {path} 8x8 bg:#ffffff dur:1s profile:{profile}\nrect 4x4\n"),
        )
        .unwrap();
        let out = artscript(&dir, &["build"]);
        let err = text(&out);
        assert_eq!(out.status.code(), Some(1), "{path} profile:{profile}: {err}");
        assert!(err.contains("E0206") && err.contains(message), "{path} profile:{profile}: {err}");
        assert!(!dir.join("assets").join(path).exists(), "{err}");
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_manifest_profile_still_applies_to_raster_targets() {
    let dir = std::env::temp_dir().join(format!("artscript-cli-mprofile-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("art")).unwrap();
    fs::write(
        dir.join("artscript.toml"),
        "[project]\nname = \"mprofile\"\nout = \"assets\"\nlib = [\"lib\"]\nstd = \"0.1\"\n\n[defaults]\nprofile = \"vector\"\n",
    )
    .unwrap();
    fs::write(dir.join("art/a.art"), "@target ok.png 8x8\nrect 4x4\n").unwrap();
    fs::write(dir.join("art/b.art"), "@target ok.svg 8x8\nrect 4x4\n").unwrap();
    let out = artscript(&dir, &["build"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert!(dir.join("assets/ok.png").exists() && dir.join("assets/ok.svg").exists());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_bad_manifest_profile_is_e0200_once_at_the_manifest() {
    let dir = std::env::temp_dir().join(format!("artscript-cli-badmprofile-{}", std::process::id()));
    for (value, shown) in [("\"bogus\"", "\"bogus\""), ("3", "3")] {
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("art")).unwrap();
        fs::write(
            dir.join("artscript.toml"),
            format!("[project]\nname = \"m\"\nout = \"assets\"\nlib = [\"lib\"]\nstd = \"0.1\"\n\n[defaults]\nprofile = {value}\n"),
        )
        .unwrap();
        fs::write(dir.join("art/a.art"), "@target a.png 8x8\nrect 4x4\n").unwrap();
        fs::write(dir.join("art/b.art"), "@target b.png 8x8\nrect 4x4\n").unwrap();
        let out = artscript(&dir, &["build"]);
        let err = text(&out);
        assert_eq!(out.status.code(), Some(1), "{err}");
        assert_eq!(err.matches("error[").count(), 1, "{err}");
        assert!(
            err.contains("E0200")
                && err.contains("artscript.toml:8:11")
                && err.contains(&format!("[defaults] profile {shown} is not one of")),
            "{err}"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn an_error_on_every_gif_frame_prints_once_with_the_png_targets() {
    let dir = std::env::temp_dir().join(format!("artscript-cli-gifonce-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("art")).unwrap();
    fs::write(
        dir.join("artscript.toml"),
        "[project]\nname = \"g\"\nout = \"assets\"\nlib = [\"lib\"]\nstd = \"0.1\"\n",
    )
    .unwrap();
    fs::write(
        dir.join("art/x.art"),
        "@target a.png 32x32\n@target b.png 32x32\n@target c.gif 32x32 bg:#000000 dur:1s fps:2\n\npalette k\n  color red #ff0000\n\ngroup name:g\n  anchor j 0,0\n  group name:h\n    anchor j 0,0\n",
    )
    .unwrap();
    let out = artscript(&dir, &["check"]);
    let err = text(&out);
    assert_eq!(out.status.code(), Some(1), "{err}");
    assert_eq!(err.matches("error[E0333] art/x.art:11:5").count(), 1, "{err}");
    assert!(!err.contains("first at") && !err.contains("worst at"), "{err}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn only_globs_build_their_union_and_a_glob_that_matches_nothing_is_e0200() {
    let dir = std::env::temp_dir().join(format!("artscript-cli-only-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("artscript.toml"),
        "[project]\nname = \"o\"\nout = \"assets\"\nlib = [\"lib\"]\nstd = \"0.1\"\n",
    )
    .unwrap();
    for (file, out) in [("tiles/x/a", "a"), ("tiles/x/b", "b"), ("props/x/c", "c")] {
        fs::create_dir_all(dir.join("art").join(file).parent().unwrap()).unwrap();
        fs::write(dir.join(format!("art/{file}.art")), format!("@target {out}.png 8x8\nrect 4x4\n")).unwrap();
    }
    let built = |args: &[&str]| -> (Option<i32>, String, Vec<String>) {
        let _ = fs::remove_dir_all(dir.join("assets"));
        let out = artscript(&dir, &[&["build"], args].concat());
        let mut pngs: Vec<String> = fs::read_dir(dir.join("assets"))
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".png"))
            .collect();
        pngs.sort();
        (out.status.code(), text(&out), pngs)
    };
    let all = vec!["a.png".to_string(), "b.png".into(), "c.png".into()];

    let (code, err, pngs) = built(&["--only", "art/tiles/x/*.art", "--only=art/props/x/*.art"]);
    assert_eq!((code, &pngs), (Some(0), &all), "{err}");
    let (code, err, pngs) = built(&["--only", "art/{tiles,props}/x/*.art"]);
    assert_eq!((code, &pngs), (Some(0), &all), "{err}");

    for glob in ["art/tiles/*", "art/nope.art"] {
        let (code, err, pngs) = built(&["--only", "art/props/x/c.art", "--only", glob]);
        assert_eq!(code, Some(1), "{err}");
        assert!(err.contains(&format!("`--only '{glob}'` matches no target")), "{err}");
        assert_eq!(err.matches("error[").count(), 1, "{err}");
        assert!(pngs.is_empty(), "{pngs:?}");
    }
    let (code, err, _) = built(&["--only", "art/{tiles"]);
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("`{` without `}` in glob `art/{tiles`"), "{err}");

    fs::write(dir.join("art/props/x/c.art"), "@target c.png 8x8\nrect (\n").unwrap();
    let (code, err, _) = built(&["--only", "art/props/x/c.art"]);
    assert_eq!(code, Some(1), "{err}");
    assert!(!err.contains("matches no target"), "{err}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn check_limits_itself_to_the_selected_entries_and_their_libraries() {
    let dir = std::env::temp_dir().join(format!("artscript-cli-checksel-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("art/sub")).unwrap();
    fs::create_dir_all(dir.join("lib/kit")).unwrap();
    fs::write(
        dir.join("artscript.toml"),
        "[project]\nname = \"sel\"\nout = \"assets\"\nlib = [\"lib\"]\nstd = \"0.1\"\n",
    )
    .unwrap();
    fs::write(dir.join("lib/kit/cell.art"), "def cell\n  rect 8x8\n").unwrap();
    fs::write(
        dir.join("art/a.art"),
        "@target a.png 8x8\nuse kit/cell { cell }\n\ncell at:4,4\n",
    )
    .unwrap();
    fs::write(dir.join("art/b.art"), "@target b.png 8x8\nrect (\n").unwrap();
    fs::write(
        dir.join("art/c.art"),
        "@target c.png 8x8\nrect 4x4\nassert false\n",
    )
    .unwrap();
    let check = |args: &[&str]| {
        let out = artscript(&dir, &[&["check"], args].concat());
        (out.status.code(), text(&out))
    };

    let (code, err) = check(&[]);
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("art/b.art"), "{err}");

    for args in [
        &["art/a.art"][..],
        &["--only", "art/a*"],
        &["--only=art/a.art"],
        &["--only", "a.png"],
    ] {
        let (code, err) = check(args);
        assert_eq!(code, Some(0), "{args:?}: {err}");
        assert!(
            !err.contains("art/b.art") && !err.contains("art/c.art"),
            "{args:?}: {err}"
        );
    }
    let out = artscript(&dir.join("art/sub"), &["check", "../a.art"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));

    let (code, err) = check(&["art/b.art"]);
    assert_eq!(code, Some(1), "{err}");
    assert!(
        err.contains("art/b.art") && !err.contains("matches no target"),
        "{err}"
    );
    let (code, err) = check(&["art/c.art"]);
    assert_eq!(code, Some(2), "{err}");
    let (code, err) = check(&["art/a.art", "art/c.art"]);
    assert_eq!(code, Some(2), "{err}");

    let (code, err) = check(&["--only", "art/zzz/*"]);
    assert_eq!(code, Some(1), "{err}");
    assert!(
        err.contains("`--only 'art/zzz/*'` matches no target") && !err.contains("art/b.art"),
        "{err}"
    );
    let (code, err) = check(&["art/a.art", "--only", "art/zzz/*"]);
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("matches no target"), "{err}");

    let (code, err) = check(&["nope.art"]);
    assert_eq!(code, Some(1), "{err}");
    assert!(
        err.contains("cannot read `nope.art`") && err.contains("usage:"),
        "{err}"
    );
    let (code, err) = check(&["--bogus"]);
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("unknown flag `--bogus`"), "{err}");

    fs::write(dir.join("lib/kit/cell.art"), "def cell\n  rect (\n").unwrap();
    let (code, err) = check(&["art/a.art"]);
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("lib/kit/cell.art"), "{err}");
    let (code, err) = check(&["art/c.art"]);
    assert_eq!(code, Some(2), "{err}");
    assert!(!err.contains("lib/kit/cell.art"), "{err}");

    fs::write(dir.join("lib/kit/cell.art"), "def cell\n  rect 8x8\n").unwrap();
    fs::write(dir.join("art/n.art"), "rect 4x4\n").unwrap();
    let (code, err) = check(&["art/n.art"]);
    assert_eq!(code, Some(1), "{err}");
    assert!(err.contains("`art/n.art` has no @target"), "{err}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_library_diagnostic_names_each_target_that_reached_it() {
    let dir = std::env::temp_dir().join(format!("artscript-cli-libsite-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("art")).unwrap();
    fs::create_dir_all(dir.join("lib/kit")).unwrap();
    fs::write(
        dir.join("artscript.toml"),
        "[project]\nname = \"libsite\"\nout = \"assets\"\nlib = [\"lib\"]\nstd = \"0.1\"\n",
    )
    .unwrap();
    fs::write(dir.join("lib/kit/cell.art"), "def cell\n  rect 80x20 fill:#88ccff name:wide\n").unwrap();
    let body = "use kit/cell { cell }\n\ncell at:32,32 name:tile\n";
    fs::write(dir.join("art/a.art"), format!("@target a.png 64x64\n{body}")).unwrap();
    fs::write(dir.join("art/b.art"), format!("@target b.png 64x64\n{body}")).unwrap();
    fs::write(dir.join("art/c.art"), format!("@target c_{{k}}.png 64x64 each k:[x y]\n{body}")).unwrap();

    let out = artscript(&dir, &["check", "--json"]);
    assert_eq!(out.status.code(), Some(3), "{}", text(&out));
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let diags = json["diagnostics"].as_array().unwrap();
    let mut sites: Vec<(String, String)> = diags
        .iter()
        .filter(|d| d["code"] == "L0102")
        .map(|d| {
            assert_eq!(d["file"], "lib/kit/cell.art", "{d}");
            let notes = d["notes"].as_array().unwrap();
            let site = notes
                .iter()
                .filter_map(|n| n.as_str())
                .find(|n| n.starts_with("in "))
                .unwrap_or_else(|| panic!("no call-site note: {d}"));
            (d["target"].as_str().unwrap_or_default().to_string(), site.to_string())
        })
        .collect();
    sites.sort();
    let want = [
        ("a.png", "in art/a.art:4"),
        ("b.png", "in art/b.art:4"),
        ("c_x.png", "in art/c.art:4"),
        ("c_y.png", "in art/c.art:4"),
    ]
    .map(|(t, s)| (t.to_string(), s.to_string()));
    assert_eq!(sites, want, "{json}");
    let raw: Vec<_> = diags.iter().filter(|d| d["code"] == "L0101").collect();
    assert_eq!(raw.len(), 1, "a library's own text is reported once: {json}");

    let plain = text(&artscript(&dir, &["check"]));
    assert_eq!(plain.matches("warning[L0102]").count(), 4, "{plain}");
    assert!(plain.contains("in art/a.art:4"), "{plain}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn check_warns_when_an_effect_is_cut_by_the_canvas_edge() {
    let dir = fixture("fx-cut", &[]);
    let art = "@target t.png 64x64\nrect 10x20 at:32,55 fill:p.rim name:r\n  fx outline 3\nrect 40x10 at:32,8 fill:p.rim name:bar\n  fx glow 12\nassert inside(@bar canvas margin:3)\n";
    fs::write(dir.join("art/t.art"), art).unwrap();

    let out = artscript(&dir, &["check", "--json"]);
    assert_eq!(out.status.code(), Some(3), "{}", text(&out));
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let cut: Vec<(String, u64)> = json["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["code"] == "L0102")
        .map(|d| {
            (
                d["node"].as_str().unwrap().to_string(),
                d["span"]["line"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        cut,
        [("r".to_string(), 2), ("bar".to_string(), 4)],
        "{json}"
    );

    fs::write(
        dir.join("art/t.art"),
        art.replace("inside(@bar canvas margin:3)", "inside(@bar.outer canvas)"),
    )
    .unwrap();
    let out = artscript(&dir, &["check"]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_viewport_target_skips_the_off_canvas_lint() {
    let dir = fixture("viewport", &[]);
    let body = "rect 40x20 at:60,32 fill:p.rim\n";
    fs::write(
        dir.join("art/t.art"),
        format!("@target a.png 64x64\n@target b.png 64x64 viewport\n{body}"),
    )
    .unwrap();
    let out = artscript(&dir, &["check", "--json"]);
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let cut: Vec<&str> = json["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["code"] == "L0102")
        .map(|d| d["target"].as_str().unwrap())
        .collect();
    assert_eq!(cut, ["a.png"], "{json}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_diagnostic_several_targets_raise_alike_prints_once_with_its_targets() {
    let dir = fixture("across-targets", &[]);
    fs::write(
        dir.join("art/t.art"),
        "@target a.png 64x64\n@target b.png 64x64\n@target c_{k}.png 64x64 each k:[x y]\nrect 40x20 at:60,32 fill:p.rim name:wide\n",
    )
    .unwrap();
    let out = artscript(&dir, &["check", "--json"]);
    assert_eq!(out.status.code(), Some(3), "{}", text(&out));
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let cut: Vec<&serde_json::Value> = json["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["code"] == "L0102")
        .collect();
    assert_eq!(cut.len(), 1, "{json}");
    assert_eq!(cut[0]["targets"], serde_json::json!(["a.png", "b.png", "c_x.png", "c_y.png"]), "{json}");

    let plain = text(&artscript(&dir, &["check"]));
    assert_eq!(plain.matches("warning[L0102]").count(), 1, "{plain}");
    assert!(plain.contains("in 4 targets: a.png, b.png, c_x.png, … (+1)"), "{plain}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn an_unknown_target_word_or_key_is_an_error() {
    let dir = fixture("target-unknown", &[]);
    for (line, what) in [
        ("@target t.png 64x64 foo", "unknown target word `foo`"),
        (
            "@target t.png 64x64 viewport:yes",
            "unknown target key `viewport`",
        ),
    ] {
        fs::write(dir.join("art/t.art"), format!("{line}\nrect 4x4 at:8,8\n")).unwrap();
        let out = artscript(&dir, &["check"]);
        assert_eq!(out.status.code(), Some(1), "{}", text(&out));
        let err = text(&out);
        assert!(err.contains("E0206") && err.contains(what), "{err}");
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_closed_output_pipe_keeps_the_exit_code() {
    let dir = fixture("closed-pipe", &[]);
    let art: String = (1..=3000).map(|i| format!("circl{i} 4\n")).collect();
    fs::write(dir.join("art/t.art"), format!("@target t.png 64x64\n{art}")).unwrap();
    for args in [&["build"][..], &["check"], &["check", "--json"]] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_artscript"))
            .args(args)
            .current_dir(&dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        drop(child.stdout.take());
        drop(child.stderr.take());
        assert_eq!(child.wait().unwrap().code(), Some(1), "{args:?}");
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn init_names_the_project_after_its_folder() {
    let folder = format!("playtest \"metroidvania\" {}", std::process::id());
    let dir = std::env::temp_dir().join(&folder);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let out = artscript(&dir, &["init"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let manifest = artscript_eval::Manifest::parse(&fs::read_to_string(dir.join("artscript.toml")).unwrap()).unwrap();
    assert_eq!(manifest.name, folder);
    let out = artscript(&dir, &["check"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
}

fn rig_project(tag: &str, art: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("artscript-cli-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("art")).unwrap();
    fs::write(
        dir.join("artscript.toml"),
        "[project]\nname = \"rig\"\nout = \"assets\"\nlib = [\"lib\"]\nstd = \"0.1\"\n",
    )
    .unwrap();
    fs::write(dir.join("art/main.art"), art).unwrap();
    dir
}

fn entry(dir: &Path, path: &str) -> serde_json::Value {
    manifest(dir)["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["path"] == path)
        .unwrap_or_else(|| panic!("no manifest entry for {path}"))
        .clone()
}

#[test]
fn the_manifest_carries_each_outputs_pivot_and_anchors_in_its_pixels() {
    let dir = rig_project(
        "rig",
        "@target x.png 64x64 scale:1,2 pivot:@body.neck\n\
         @target y.svg 64x64 pivot:50%,100%\n\n\
         def doll\n  circle 8\n  anchor neck 0,-10\n  anchor _hidden 3,3\n\n\
         doll at:30,40 name:body\n",
    );
    let first = artscript(&dir, &["build"]);
    assert_eq!(first.status.code(), Some(0), "{}", text(&first));
    let one = entry(&dir, "x.png");
    assert_eq!(one["pivot"], serde_json::json!([30, 30]));
    assert_eq!(one["anchors"], serde_json::json!({ "body.neck": [30, 30] }));
    let two = entry(&dir, "x@2x.png");
    assert_eq!(two["pivot"], serde_json::json!([60, 60]));
    assert_eq!(two["anchors"], serde_json::json!({ "body.neck": [60, 60] }));
    let svg = entry(&dir, "y.svg");
    assert_eq!(svg["pivot"], serde_json::json!([32, 64]));
    assert_eq!(svg["anchors"], one["anchors"]);

    let bytes = fs::read(dir.join("assets/art-manifest.json")).unwrap();
    let second = artscript(&dir, &["build"]);
    assert_eq!(second.status.code(), Some(0), "{}", text(&second));
    assert_eq!(fs::read(dir.join("assets/art-manifest.json")).unwrap(), bytes);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_target_without_pivot_writes_null_and_a_gif_writes_neither_key() {
    let dir = rig_project(
        "rig-none",
        "@target a.png 8x8\n@target b.gif 8x8 bg:#ffffff dur:1s\nrect 4x4\n",
    );
    let out = artscript(&dir, &["build"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let png = entry(&dir, "a.png");
    assert_eq!(png["pivot"], serde_json::Value::Null);
    assert_eq!(png["anchors"], serde_json::json!({}));
    let gif = entry(&dir, "b.gif");
    assert!(gif.get("pivot").is_none() && gif.get("anchors").is_none(), "{gif}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_pivot_that_is_not_a_point_is_e0206() {
    let cases = [
        (
            "@target a.png 8x8 pivot:@body\ncircle 4 name:body\n",
            "is a node, not a point",
        ),
        (
            "@target a.png 8x8 pivot:red\ncircle 4\n",
            "`pivot:` needs a point",
        ),
        (
            "@target a.gif 8x8 bg:#ffffff dur:1s pivot:4,4\ncircle 4\n",
            "`pivot:` needs a .png or .svg target",
        ),
    ];
    for (i, (art, message)) in cases.into_iter().enumerate() {
        let dir = rig_project(&format!("rig-bad{i}"), art);
        for cmd in ["check", "build"] {
            let out = artscript(&dir, &[cmd]);
            let err = text(&out);
            assert_eq!(out.status.code(), Some(1), "{cmd} {art}: {err}");
            assert!(err.contains("E0206") && err.contains(message), "{cmd} {art}: {err}");
        }
        let _ = fs::remove_dir_all(&dir);
    }
}

fn png_size(path: &Path) -> (u32, u32) {
    let bytes = fs::read(path).unwrap();
    let be = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap());
    (be(16), be(20))
}

#[test]
fn size_placeholders_name_the_files_a_build_writes() {
    let dir = rig_project(
        "sized",
        "@target icon_{w}.png 32x20 scale:.5,1,2\n@target vec_{w}x{h}_{scale}.svg 32x20 scale:2\nrect 4x4\n",
    );
    let out = artscript(&dir, &["build"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    for (name, size) in [("icon_16.png", (16, 10)), ("icon_32.png", (32, 20)), ("icon_64.png", (64, 40))] {
        assert_eq!(png_size(&dir.join("assets").join(name)), size, "{name}");
    }
    assert!(dir.join("assets/vec_32x20_1.svg").exists());
    let paths: Vec<String> = manifest(&dir)["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["path"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(paths, ["icon_16.png", "icon_32.png", "icon_64.png", "vec_32x20_1.svg"]);
    assert_eq!(entry(&dir, "icon_64.png")["scale"], 2.0);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_diagnostic_that_only_one_scale_raises_is_reported() {
    let dir = rig_project(
        "scale-diag",
        "@target x.png 8x8 scale:1,2\nrect 4x4\nexpect scale in 0..1.5 \"small scale only\"\nexpect 5 in 0..1 \"every scale\"\n",
    );
    for cmd in ["check", "build"] {
        let out = artscript(&dir, &[cmd]);
        let err = text(&out);
        assert_eq!(err.matches("2 is outside 0..1.5").count(), 1, "{cmd}: {err}");
        assert_eq!(err.matches("5 is outside 0..1\n").count(), 1, "{cmd}: {err}");
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_compile_error_only_one_scale_raises_stops_the_build() {
    let dir = rig_project(
        "scale-error",
        "@target x.png 8x8 scale:1,2\nrect 4x4\nif scale > 1\n  circle bogus\n",
    );
    let out = artscript(&dir, &["build"]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    assert!(!dir.join("assets/x.png").exists() && !dir.join("assets/x@2x.png").exists());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn each_outputs_anchors_come_from_its_own_scene() {
    let dir = rig_project(
        "scale-rig",
        "@target x.png 64x64 scale:1,2 pivot:@body.hip\n\
         def doll\n  circle 8\n  anchor hip 0,10*scale\n\n\
         doll at:30,40 name:body\n",
    );
    let out = artscript(&dir, &["build"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    assert_eq!(entry(&dir, "x.png")["anchors"], serde_json::json!({ "body.hip": [30, 50] }));
    assert_eq!(entry(&dir, "x@2x.png")["anchors"], serde_json::json!({ "body.hip": [60, 120] }));
    assert_eq!(entry(&dir, "x@2x.png")["pivot"], serde_json::json!([60, 120]));
    let again = artscript(&dir, &["build"]);
    assert_eq!(again.status.code(), Some(0), "{}", text(&again));
    assert_eq!(entry(&dir, "x@2x.png")["anchors"], serde_json::json!({ "body.hip": [60, 120] }));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn build_only_keeps_outputs_of_unselected_targets_in_a_rebuilt_file() {
    let dir = fixture("only-sibling", &[]);
    let art = dir.join("art/x.art");
    let src = "@target a.png 32x32 as:a\n@target b.png 32x32 as:b\n\npalette k\n  color red #ff0000\n\ncircle 10 at:16,16 fill:k.red\n";
    fs::write(&art, src).unwrap();
    let full = artscript(&dir, &["build"]);
    assert_eq!(full.status.code(), Some(0), "{}", text(&full));
    let b = dir.join("assets/b.png");
    let b_bytes = fs::read(&b).unwrap();

    fs::write(&art, src.replace("circle 10", "circle 12")).unwrap();
    let only = artscript(&dir, &["build", "--only", "a.png"]);
    assert_eq!(only.status.code(), Some(0), "{}", text(&only));
    assert!(!text(&only).contains("removed stale"), "{}", text(&only));
    assert_eq!(fs::read(&b).unwrap(), b_bytes, "b.png changed");
    let paths: Vec<String> = manifest(&dir)["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["path"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(paths, ["a.png", "b.png"]);
}
