//! `text` sets glyph outlines centered on the origin.

use super::{shape, show};

#[test]
fn text_is_centered_on_its_origin_and_tracking_widens_it() {
    let width = |track: &str| {
        let ev = super::run_with(&format!("@target t.png 200x100\ntext \"HIT\" size:20 track:{track} at:100,50 name:t\n"), &[]);
        assert!(ev.diagnostics.is_empty(), "{}", show(&ev.diagnostics));
        let s = ev.scene.unwrap();
        shape(&s, "t").geometry.bounds().to_owned()
    };
    let (plain, tracked) = (width("0"), width("10"));
    if plain.is_empty() {
        return;
    }
    assert!(plain.center_x().abs() < 1.5 && plain.center_y().abs() < 0.5, "{plain:?}");
    assert!((tracked.width() - plain.width() - 20.0).abs() < 0.5, "{plain:?} {tracked:?}");
}
