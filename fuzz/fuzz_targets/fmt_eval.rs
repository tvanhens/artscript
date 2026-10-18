#![no_main]

use artscript_eval::{evaluate, plan};
use artscript_fuzz::{fingerprint, project};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some(p) = project(data) else { return };
    let Ok(formatted) = artscript_syntax::format_source(&p.files["art/t.art"]) else { return };
    let mut q = p.clone();
    q.files.insert("art/t.art".into(), formatted.clone());
    let (before, after) = (plan(&p), plan(&q));
    assert_eq!(before.jobs.len(), after.jobs.len(), "fmt changed the targets:\n{formatted}");
    for (a, b) in before.jobs.iter().zip(&after.jobs).take(2) {
        let (ea, eb) = (evaluate(&p, a), evaluate(&q, b));
        let codes = |ds: &[artscript_syntax::Diagnostic]| ds.iter().map(|d| d.code.clone()).collect::<Vec<_>>();
        assert_eq!(codes(&ea.diagnostics), codes(&eb.diagnostics), "fmt changed the diagnostics:\n{formatted}");
        assert_eq!(without_origins(&ea.scene), without_origins(&eb.scene), "fmt changed the scene:\n{formatted}");
    }
});

fn without_origins(scene: &Option<artscript_ir::Scene>) -> String {
    let text = fingerprint(scene);
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some(i) = rest.find("origin: \"") {
        out.push_str(&rest[..i]);
        let tail = &rest[i + "origin: \"".len()..];
        rest = &tail[tail.find('"').map_or(tail.len(), |j| j + 1)..];
    }
    out.push_str(rest);
    out
}
