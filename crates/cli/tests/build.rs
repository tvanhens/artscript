//! End-to-end CLI runs on a temporary copy of `examples/chibi`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap().flatten() {
        let name = entry.file_name();
        if [".artscript", "assets"].contains(&name.to_str().unwrap_or_default()) {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            copy_dir(&path, &to.join(&name));
        } else {
            fs::copy(&path, to.join(&name)).unwrap();
        }
    }
}

fn fixture(tag: &str) -> PathBuf {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/chibi");
    let dir = std::env::temp_dir().join(format!("artscript-cli-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    copy_dir(&src, &dir);
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
    let dir = fixture("build");
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
    let dir = fixture("check");
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
fn monk_gif_frames_match_the_sampled_palette() {
    let dir = fixture("gif");
    let built = artscript(&dir, &["build", "--only", "character/moss_monk_*.gif", "--backend", "cpu"]);
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
    assert_eq!(jobs.len(), 3, "idle, blink and yawn");
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
        assert_eq!(reader.width(), 512);
        assert_eq!(reader.height(), 512);
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
            assert_eq!((w, h), (512, 512));
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
            assert_eq!((frame.width, frame.height), (512, 512));
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
    let dir = fixture("backend");
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
        assert_ne!(fs::read(&png).unwrap(), cpu_bytes, "a backend switch must re-render");
    }
}
