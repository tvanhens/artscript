#![no_main]

use ramose_fuzz::generator::{generate, show};
use ramose_fuzz::{exercise_eval, exercise_format, exercise_render, project};
use libfuzzer_sys::arbitrary::Unstructured;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let bytes = generate(&mut Unstructured::new(data));
    if std::env::var_os("RAMOSE_FUZZ_SHOW").is_some() {
        eprintln!("{}", show(&bytes));
    }
    if let Some(p) = project(&bytes) {
        exercise_eval(&p);
        exercise_render(&p);
        p.files
            .iter()
            .filter(|(path, _)| path.ends_with(".ramo"))
            .for_each(|(_, src)| exercise_format(src));
    }
});
