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
    Command::new(env!("CARGO_BIN_EXE_artscript")).args(args).current_dir(dir).output().unwrap()
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn manifest(dir: &Path) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(dir.join("assets/art-manifest.json")).unwrap()).unwrap()
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
        .map(|o| (o["path"].as_str().unwrap(), o["hash"].as_str().unwrap(), o["status"].as_str().unwrap()))
        .collect();
    let (png_hash, svg_hash) = (fnv_hex(&png_bytes), fnv_hex(&svg_bytes));
    assert_eq!(
        outputs,
        [("produce/momo_peach.png", png_hash.as_str(), "pass"), ("produce/momo_peach.svg", svg_hash.as_str(), "pass")]
    );
    let manifest_bytes = fs::read(dir.join("assets/art-manifest.json")).unwrap();

    let started = std::time::Instant::now();
    let second = artscript(&dir, &only);
    assert_eq!(second.status.code(), Some(0), "{}", text(&second));
    assert!(started.elapsed().as_secs_f32() < 2.0, "cache miss: second build took {:?}", started.elapsed());
    assert_eq!(fs::read(&png).unwrap(), png_bytes);
    assert_eq!(fs::read(&svg).unwrap(), svg_bytes);
    assert_eq!(fs::read(dir.join("assets/art-manifest.json")).unwrap(), manifest_bytes);

    let art = dir.join("art/produce/momo_peach.art");
    let src = fs::read_to_string(&art).unwrap();
    fs::write(&art, src.replace("@target produce/momo_peach.svg 1024x1024\n", "")).unwrap();
    let third = artscript(&dir, &only);
    assert_eq!(third.status.code(), Some(0), "{}", text(&third));
    assert!(!svg.exists(), "stale svg survived");
    assert!(text(&third).contains("removed stale produce/momo_peach.svg"));
    let paths: Vec<String> = manifest(&dir)["outputs"].as_array().unwrap().iter().map(|o| o["path"].as_str().unwrap().to_string()).collect();
    assert_eq!(paths, ["produce/momo_peach.png"]);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn check_json_reports_failed_asserts_with_exit_2() {
    let dir = fixture("check");
    let art = dir.join("art/produce/momo_peach.art");
    let src = fs::read_to_string(&art).unwrap();
    fs::write(&art, src.replace("name:eye_r", "name:eye_r rot:0 at:550,640")).unwrap();
    let out = artscript(&dir, &["check", "--json"]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let diags = json["diagnostics"].as_array().unwrap();
    let level = diags.iter().find(|d| d["code"] == "C0101").unwrap_or_else(|| panic!("{json}"));
    assert_eq!(level["file"], "art/produce/momo_peach.art");
    assert_eq!(level["severity"], "error");
    assert_eq!(level["message"], "assert level(@eye_l @eye_r) tol:.5");
    assert!(level["fix"].as_str().unwrap().starts_with("set .eye_r at:550,"), "{level}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn check_passes_on_the_fixture_and_fmt_is_stable() {
    let dir = fixture("fmt");
    let out = artscript(&dir, &["check"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));
    let first = artscript(&dir, &["fmt"]);
    assert_eq!(first.status.code(), Some(0), "{}", text(&first));
    let again = artscript(&dir, &["fmt"]);
    assert_eq!(String::from_utf8_lossy(&again.stdout), "");
    let _ = fs::remove_dir_all(&dir);
}
