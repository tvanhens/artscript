#![no_main]

use artscript_fuzz::{exercise_render, project};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Some(p) = project(data) {
        exercise_render(&p);
    }
});
