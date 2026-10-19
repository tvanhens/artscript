#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let src = String::from_utf8_lossy(data);
    if let Err(errs) = ramose_syntax::parse(&src) {
        assert!(!errs.is_empty(), "parse failed without a diagnostic");
        let lines = src.split('\n').count() as u32;
        for e in &errs {
            assert!(e.span.line <= lines, "error past the last line ({lines}): {e:?}");
        }
    }
});
