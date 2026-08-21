use criterion::{black_box, criterion_group, criterion_main, Criterion};
use lumen_fixtures::{real_antigravity_session_dump, real_claude_session_dump, real_opencode_session_dump};
use lumen_session::detect_orchestrator;

fn fingerprint_benchmark(c: &mut Criterion) {
    let claude_sample = real_claude_session_dump().as_bytes();
    let agy_sample = real_antigravity_session_dump().as_bytes();
    let opencode_sample = real_opencode_session_dump().as_bytes();

    let mut group = c.benchmark_group("detect_orchestrator");
    group.bench_function("claude_code", |b| b.iter(|| detect_orchestrator(black_box(claude_sample))));
    group.bench_function("antigravity", |b| b.iter(|| detect_orchestrator(black_box(agy_sample))));
    group.bench_function("opencode", |b| b.iter(|| detect_orchestrator(black_box(opencode_sample))));
    group.finish();
}

criterion_group!(benches, fingerprint_benchmark);
criterion_main!(benches);
