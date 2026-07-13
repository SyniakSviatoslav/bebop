//! Baseline telemetry for the bebop host-agent hot paths.
//! Run: `cargo bench -p bebop` (from crates/bebop).

use bebop::{loop_runtime::LoopRuntime, wiring::L5Proposal};
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn bench_loop_cycle_benign(c: &mut Criterion) {
    let l5 = L5Proposal {
        v_prev: 1.0,
        v_cur: 0.9,
        dt: 1.0,
        proposed_delta: 0.3,
        limit: 0.5,
        ..Default::default()
    };
    c.bench_function("loop_cycle/benign", |b| {
        b.iter(|| {
            let mut rt = LoopRuntime::new(1.0);
            black_box(rt.cycle("write the docs", &l5, None, &[], &[], None, None));
        })
    });
}

fn bench_wire_benign(c: &mut Criterion) {
    let l5 = L5Proposal {
        v_prev: 1.0,
        v_cur: 0.9,
        dt: 1.0,
        proposed_delta: 0.3,
        limit: 0.5,
        ..Default::default()
    };
    c.bench_function("wire/benign", |b| {
        b.iter(|| {
            let mut mm = bebop::memory::LivingMemory::default();
            let mut audit = bebop::audit::AuditLog::default();
            black_box(bebop::wiring::wire(
                "write the docs",
                &l5,
                None,
                &[],
                &[],
                None,
                None,
                &mut mm,
                &mut audit,
            ))
        })
    });
}

criterion_group!(benches, bench_loop_cycle_benign, bench_wire_benign);
criterion_main!(benches);
