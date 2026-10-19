#![no_main]

use ramose_fuzz::{exercise_eval, project};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Some(p) = project(data) {
        exercise_eval(&p);
    }
});
