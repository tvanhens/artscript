//! Visual goldens for `catalog/`. A cold CPU build turns every output into a PNG view: a PNG as
//! itself, a GIF as a strip of its frames, an SVG rasterized. Each view must match
//! `catalog/golden/<output path>[.png]` within a small tolerance. See `catalog/README.md`.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::Command;

const BLESS: &str = "ARTSCRIPT_BLESS";
const ONLY: &str = "ARTSCRIPT_VISUAL_ONLY";
const FRAMES_PER_ROW: usize = 8;
const GAP: usize = 2;
const GAP_RGBA: [u8; 4] = [128, 128, 128, 255];
const TOLERANCE: u8 = 8;
const OUTLIER_RATIO: usize = 1000;

struct View {
    width: usize,
    height: usize,
    rgba: Vec<u8>,
}

impl View {
    fn blank(width: usize, height: usize, fill: [u8; 4]) -> View {
        View { width, height, rgba: fill.repeat(width * height) }
    }

    fn px(&self, x: usize, y: usize) -> [u8; 4] {
        let i = (y * self.width + x) * 4;
        self.rgba[i..i + 4].try_into().unwrap()
    }

    fn put(&mut self, x: usize, y: usize, c: [u8; 4]) {
        let i = (y * self.width + x) * 4;
        self.rgba[i..i + 4].copy_from_slice(&c);
    }

    fn paste(&mut self, other: &View, ox: usize, oy: usize) {
        for y in 0..other.height {
            for x in 0..other.width {
                self.put(ox + x, oy + y, other.px(x, y));
            }
        }
    }

    /// Lays views left to right, `per_row` to a row, with a gray gap between them.
    fn tile(views: &[View], per_row: usize) -> View {
        let (w, h) = (views[0].width, views[0].height);
        let cols = views.len().min(per_row);
        let rows = views.len().div_ceil(per_row);
        let mut out = View::blank(cols * (w + GAP) - GAP, rows * (h + GAP) - GAP, GAP_RGBA);
        for (i, v) in views.iter().enumerate() {
            out.paste(v, (i % per_row) * (w + GAP), (i / per_row) * (h + GAP));
        }
        out
    }

    fn decode_png(bytes: &[u8]) -> View {
        let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        decoder.set_transformations(png::Transformations::normalize_to_color8() | png::Transformations::ALPHA);
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!(info.color_type, png::ColorType::Rgba, "unexpected png color type");
        buf.truncate(info.buffer_size());
        View { width: info.width as usize, height: info.height as usize, rgba: buf }
    }

    fn decode_gif(bytes: &[u8]) -> View {
        let mut options = gif::DecodeOptions::new();
        options.set_color_output(gif::ColorOutput::RGBA);
        let mut decoder = options.read_info(bytes).unwrap();
        let (w, h) = (decoder.width() as usize, decoder.height() as usize);
        let mut frames = Vec::new();
        while let Some(frame) = decoder.read_next_frame().unwrap() {
            assert_eq!((frame.left, frame.top), (0, 0), "gif frame is not full-canvas");
            assert_eq!((frame.width as usize, frame.height as usize), (w, h), "gif frame is not full-canvas");
            frames.push(View { width: w, height: h, rgba: frame.buffer.to_vec() });
        }
        assert!(!frames.is_empty(), "gif has no frames");
        View::tile(&frames, FRAMES_PER_ROW)
    }

    fn rasterize_svg(bytes: &[u8]) -> View {
        let tree = resvg::usvg::Tree::from_data(bytes, &resvg::usvg::Options::default()).unwrap();
        let size = tree.size().to_int_size();
        let mut pixmap = resvg::tiny_skia::Pixmap::new(size.width(), size.height()).unwrap();
        resvg::render(&tree, resvg::tiny_skia::Transform::identity(), &mut pixmap.as_mut());
        let rgba = pixmap.pixels().iter().flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        });
        View { width: size.width() as usize, height: size.height() as usize, rgba: rgba.collect() }
    }

    fn encode_png(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, self.width as u32, self.height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::High);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&self.rgba).unwrap();
        writer.finish().unwrap();
        out
    }
}

/// Skia's CPU raster rounds differently across platforms, so a pixel counts as changed only when
/// a channel moves by more than `TOLERANCE`, and a view still matches with up to one changed pixel
/// in `OUTLIER_RATIO`, which absorbs field-threshold pixels that flip. Bless rewrites a golden on any
/// changed pixel, so small intended edits never leave a stale golden behind.
fn changed(g: [u8; 4], a: [u8; 4]) -> bool {
    g.iter().zip(a).any(|(g, a)| g.abs_diff(a) > TOLERANCE)
}

fn changed_pixels(golden: &View, actual: &View) -> usize {
    if (golden.width, golden.height) != (actual.width, actual.height) {
        return actual.width * actual.height;
    }
    golden.rgba.chunks(4).zip(actual.rgba.chunks(4)).filter(|(g, a)| changed((*g).try_into().unwrap(), (*a).try_into().unwrap())).count()
}

fn matches(golden: &View, actual: &View) -> bool {
    (golden.width, golden.height) == (actual.width, actual.height)
        && changed_pixels(golden, actual) * OUTLIER_RATIO <= actual.width * actual.height
}

/// `golden | actual | diff`, where the diff dims unchanged pixels and paints changed pixels red.
fn diff_sheet(golden: &View, actual: &View) -> View {
    let mut diff = View::blank(actual.width, actual.height, [255, 0, 0, 255]);
    if (golden.width, golden.height) == (actual.width, actual.height) {
        for y in 0..actual.height {
            for x in 0..actual.width {
                let a = actual.px(x, y);
                if !changed(golden.px(x, y), a) {
                    let l = ((a[0] as u32 + a[1] as u32 + a[2] as u32) / 3 * a[3] as u32 / 255) as u8;
                    diff.put(x, y, [l / 3 + 170, l / 3 + 170, l / 3 + 170, 255]);
                }
            }
        }
    }
    let h = golden.height.max(actual.height);
    let mut sheet = View::blank(golden.width + 2 * actual.width + 2 * GAP, h, GAP_RGBA);
    sheet.paste(golden, 0, 0);
    sheet.paste(actual, golden.width + GAP, 0);
    sheet.paste(&diff, golden.width + actual.width + 2 * GAP, 0);
    sheet
}

fn view_of(path: &Path) -> View {
    let bytes = fs::read(path).unwrap();
    match path.extension().and_then(|e| e.to_str()) {
        Some("png") => View::decode_png(&bytes),
        Some("gif") => View::decode_gif(&bytes),
        Some("svg") => View::rasterize_svg(&bytes),
        _ => panic!("{}: no view for this output format", path.display()),
    }
}

fn golden_name(output: &str) -> String {
    if output.ends_with(".png") { output.to_string() } else { format!("{output}.png") }
}

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

fn files_under(dir: &Path, base: &Path, out: &mut BTreeSet<String>) {
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            files_under(&path, base, out);
        } else {
            out.insert(path.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/"));
        }
    }
}

#[test]
fn catalog_matches_its_goldens() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog");
    let golden_dir = root.join("golden");
    let diff_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/visual-diff");
    let bless = std::env::var_os(BLESS).is_some();
    let only = std::env::var(ONLY).ok();

    let work = std::env::temp_dir().join(format!("artscript-catalog-{}", std::process::id()));
    let _ = fs::remove_dir_all(&work);
    for part in ["art", "lib", "pictures"] {
        copy_dir(&root.join(part), &work.join(part));
    }
    fs::copy(root.join("artscript.toml"), work.join("artscript.toml")).unwrap();

    let fmt = Command::new(env!("CARGO_BIN_EXE_artscript")).args(["fmt", "--check"]).current_dir(&work).output().unwrap();
    assert_eq!(fmt.status.code(), Some(0), "run `artscript fmt` in catalog/\n{}", String::from_utf8_lossy(&fmt.stdout));

    let mut args = vec!["build", "--backend", "cpu"];
    if let Some(glob) = &only {
        args.extend(["--only", glob]);
    }
    let built = Command::new(env!("CARGO_BIN_EXE_artscript")).args(&args).current_dir(&work).output().unwrap();
    assert_eq!(
        built.status.code(),
        Some(0),
        "catalog build failed\n{}{}",
        String::from_utf8_lossy(&built.stdout),
        String::from_utf8_lossy(&built.stderr)
    );

    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(work.join("build/art-manifest.json")).unwrap()).unwrap();
    let mut outputs = BTreeSet::new();
    for entry in manifest["outputs"].as_array().unwrap() {
        let (path, source) = (entry["path"].as_str().unwrap(), entry["source"].as_str().unwrap());
        let stem = source.strip_prefix("art/").and_then(|s| s.strip_suffix(".art")).unwrap();
        assert!(path.starts_with(stem), "{source}: output {path} must start with `{stem}` so a golden names its fixture");
        outputs.insert(path.to_string());
    }
    let expected: BTreeSet<String> = outputs.iter().map(|o| golden_name(o)).collect();

    let _ = fs::remove_dir_all(&diff_dir);
    let mut failures = Vec::new();
    for output in &outputs {
        let name = golden_name(output);
        let actual = view_of(&work.join("build").join(output));
        let golden_path = golden_dir.join(&name);
        let golden = fs::read(&golden_path).ok().map(|b| View::decode_png(&b));
        let settled = match &golden {
            Some(g) if bless => changed_pixels(g, &actual) == 0,
            Some(g) => matches(g, &actual),
            None => false,
        };
        if settled {
            continue;
        }
        if bless {
            fs::create_dir_all(golden_path.parent().unwrap()).unwrap();
            fs::write(&golden_path, actual.encode_png()).unwrap();
            println!("blessed {name}");
            continue;
        }
        let (sheet, what) = match &golden {
            Some(golden) => {
                (diff_sheet(golden, &actual), format!("{} pixels changed", changed_pixels(golden, &actual)))
            }
            None => (actual, "no golden".to_string()),
        };
        let sheet_path = diff_dir.join(&name);
        fs::create_dir_all(sheet_path.parent().unwrap()).unwrap();
        fs::write(&sheet_path, sheet.encode_png()).unwrap();
        failures.push(format!("{name}: {what}, see {}", sheet_path.display()));
    }

    if only.is_none() {
        let mut goldens = BTreeSet::new();
        files_under(&golden_dir, &golden_dir, &mut goldens);
        for stale in goldens.difference(&expected) {
            if bless {
                fs::remove_file(golden_dir.join(stale)).unwrap();
                println!("removed {stale}");
            } else {
                failures.push(format!("{stale}: golden has no catalog output"));
            }
        }
    }
    let _ = fs::remove_dir_all(&work);

    assert!(
        failures.is_empty(),
        "{} golden(s) out of date. Inspect each sheet (golden | actual | diff), then run \
         `{BLESS}=1 cargo test -p artscript --test visual` and commit catalog/golden.\n{}",
        failures.len(),
        failures.join("\n")
    );
}
