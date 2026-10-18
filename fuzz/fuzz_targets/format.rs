#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| artscript_fuzz::exercise_format(&String::from_utf8_lossy(data)));
