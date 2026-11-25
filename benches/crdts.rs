//! Criterion benchmarks of `merge` time.
//!
//! Two stories:
//!
//! * **OR-Set vs ORSWOT** — the cost of tombstones: both replicas
//!   replay the same add/remove history (shared operation alphabet),
//!   then `join` of the diverged states is measured. Tombstone OR-Set
//!   state grows with the number of cycles; ORSWOT stays bounded by
//!   live tags and the context;
//! * **LWW vs MV-register** — the cost of keeping concurrent values:
//!   LWW `join` compares a pair of triples in O(1); MV-register `join`
//!   canonicalizes an antichain of width W in O(W²) clock comparisons.
//!
//! State size (the second metric) is printed by
//! `cargo run --example state_size`.

// criterion_group!/criterion_main! generate undocumented pub functions;
// the crate-level missing_docs lint does not apply to them.
#![allow(missing_docs)]

use std::hint::black_box;

use crdt_lab::orset::Op;
use crdt_lab::{JoinSemilattice, LwwRegister, MvRegister, OrSet, Orswot, Update};
use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion};

/// Add/remove churn: items cycle through an alphabet of 64; every
/// fifth insertion skips the matching remove (5 is coprime with 64, so
/// a surviving element is deleted on its next appearance — live state
/// stays bounded, only the history trace grows). The same history is
/// used for both set types.
fn churn_ops(actor: u8, cycles: usize) -> Vec<Op<u8, u16>> {
    let mut ops = Vec::with_capacity(cycles * 2);
    for i in 0..cycles {
        let item = (i % 64) as u16;
        ops.push(Op::Add(actor, item));
        if i % 5 != 0 {
            ops.push(Op::Remove(item));
        }
    }
    ops
}

/// Fold operations into a state (same trick as in tests/common).
fn build<T: Update + Default>(ops: &[T::Op]) -> T
where
    T::Op: Clone,
{
    let mut state = T::default();
    for op in ops {
        state.apply(op.clone());
    }
    state
}

/// `join` of diverged set replicas after the same history.
fn bench_set_merge(c: &mut Criterion) {
    let mut group = c.benchmark_group("set_merge");
    for cycles in [64usize, 256, 1024, 4096] {
        let left_ops = churn_ops(0, cycles);
        let right_ops = churn_ops(1, cycles);

        let (l, r): (OrSet<u8, u16>, OrSet<u8, u16>) = (build(&left_ops), build(&right_ops));
        group.bench_with_input(BenchmarkId::new("orset", cycles), &cycles, |b, _| {
            b.iter_batched(
                || l.clone(),
                |mut acc| {
                    acc.join(black_box(&r));
                    acc
                },
                BatchSize::SmallInput,
            )
        });

        let (l, r): (Orswot<u8, u16>, Orswot<u8, u16>) = (build(&left_ops), build(&right_ops));
        group.bench_with_input(BenchmarkId::new("orswot", cycles), &cycles, |b, _| {
            b.iter_batched(
                || l.clone(),
                |mut acc| {
                    acc.join(black_box(&r));
                    acc
                },
                BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

/// Antichain of `width` concurrent writes: each from its own replica
/// (its own actor), none having observed the others.
fn mv_antichain(actors: std::ops::Range<u16>) -> MvRegister<u16, u64> {
    let mut acc = MvRegister::new();
    for a in actors {
        let mut replica = MvRegister::new();
        replica.write(a, u64::from(a));
        acc.join(&replica);
    }
    acc
}

/// Register `join`: O(1) for LWW versus O(W²) canonicalization for the
/// MV-register.
fn bench_register_merge(c: &mut Criterion) {
    let mut group = c.benchmark_group("register_merge");

    let mut l: LwwRegister<u16, u64> = LwwRegister::new(0, 0);
    let mut r: LwwRegister<u16, u64> = LwwRegister::new(0, 0);
    l.write(100, 1, 42);
    r.write(101, 2, 43);
    group.bench_function("lww", |b| {
        b.iter_batched(
            || l.clone(),
            |mut acc| {
                acc.join(black_box(&r));
                acc
            },
            BatchSize::SmallInput,
        )
    });

    for width in [1u16, 4, 16, 64] {
        let l = mv_antichain(0..width);
        let r = mv_antichain(width..2 * width);
        group.bench_with_input(BenchmarkId::new("mvreg", width), &width, |b, _| {
            b.iter_batched(
                || l.clone(),
                |mut acc| {
                    acc.join(black_box(&r));
                    acc
                },
                BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

fn quick(mut c: Criterion) -> Criterion {
    c = c
        .warm_up_time(std::time::Duration::from_millis(500))
        .measurement_time(std::time::Duration::from_secs(2))
        .sample_size(50);
    c
}

criterion_group! {
    name = benches;
    config = quick(Criterion::default());
    targets = bench_set_merge, bench_register_merge
}
criterion_main!(benches);
