//! State size of OR-Set vs ORSWOT after add/remove cycles.
//!
//! Companion to the `merge` time benches (`cargo bench`): prints a
//! table of state growth on the same operation history. Tombstone
//! OR-Set trace grows linearly with the number of deletes; ORSWOT
//! state is bounded by live tags plus one counter per actor.
//!
//! Run: `cargo run --example state_size`

use crdt_lab::orset::Op;
use crdt_lab::{OrSet, Orswot, Update};

/// Same history as in `benches/crdts.rs`: items cycle through an
/// alphabet of 64; every fifth insertion skips the matching remove
/// (5 is coprime with 64, so a surviving element is deleted on its
/// next appearance and live state stays bounded).
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

fn main() {
    println!(
        "{:>7} | {:>13} | {:>15} | {:>13} | {:>13}",
        "cycles", "OR-Set: elems", "OR-Set: tombs", "ORSWOT: elems", "ORSWOT: tags"
    );
    for cycles in [10usize, 100, 1_000, 10_000, 100_000] {
        let ops = churn_ops(0, cycles);
        let mut orset: OrSet<u8, u16> = OrSet::new();
        let mut orswot: Orswot<u8, u16> = Orswot::new();
        for op in &ops {
            orset.apply(op.clone());
            orswot.apply(op.clone());
        }
        println!(
            "{:>7} | {:>13} | {:>15} | {:>13} | {:>13}",
            cycles,
            orset.len(),
            orset.tombstone_count(),
            orswot.len(),
            orswot.dot_count(),
        );
    }
    println!("\nORSWOT context is one counter per actor (here: 1) at any number of cycles.");
}
