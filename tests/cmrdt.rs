//! Tests of the op-based model (CmRDT) with a small causal-broadcast
//! model.
//!
//! Transport is modelled explicitly: each operation is tagged with the
//! source's vector clock, and delivery to a replica is allowed only
//! when all causal predecessors have already been delivered (the
//! causal-broadcast condition on `VClock`). Checked:
//!
//! * [`OpCounter`] converges under *any* delivery order: its `effect`
//!   commutes unconditionally;
//! * [`OpOrSet`] converges under any **causally admissible** delivery
//!   order (a property test over random histories and orders);
//! * counter-test: breaking causal order (Remove before its Add)
//!   diverges op-based OR-Set replicas forever. The delivery
//!   hypothesis is not expressible in types and lives in the
//!   transport.

use crdt_lab::cmrdt::{CmRdt, OpCounter, OpOrSet};
use crdt_lab::{JoinSemilattice, VClock};
use proptest::collection::vec;
use proptest::prelude::*;

// --- Small causal-broadcast model ---

/// Message: an operation with the source's causal stamp.
struct Message<O> {
    origin: usize,
    clock: VClock<usize>,
    op: O,
}

/// Network of `n` replicas with a log of every sent operation.
struct CausalNet<C: CmRdt> {
    replicas: Vec<C>,
    /// Causal clocks of what has been delivered on each replica.
    clocks: Vec<VClock<usize>>,
    log: Vec<Message<C::Op>>,
    /// For each replica, the set of indices of delivered messages.
    delivered: Vec<std::collections::BTreeSet<usize>>,
}

impl<C: CmRdt + Default> CausalNet<C> {
    fn new(n: usize) -> Self {
        Self {
            replicas: (0..n).map(|_| C::default()).collect(),
            clocks: vec![VClock::new(); n],
            log: Vec::new(),
            delivered: vec![Default::default(); n],
        }
    }

    /// Local operation: the source applies `effect` immediately and
    /// broadcasts the operation with the current clock.
    fn local(&mut self, r: usize, op: C::Op) {
        self.clocks[r].increment(r);
        self.replicas[r].effect(&op);
        self.delivered[r].insert(self.log.len());
        self.log.push(Message {
            origin: r,
            clock: self.clocks[r].clone(),
            op,
        });
    }

    /// Causal-broadcast condition: the next-in-order message of the
    /// source, with all foreign predecessors already delivered.
    fn admissible(&self, r: usize, i: usize) -> bool {
        if self.delivered[r].contains(&i) {
            return false;
        }
        let m = &self.log[i];
        m.clock.get(&m.origin) == self.clocks[r].get(&m.origin) + 1
            && (0..self.replicas.len())
                .filter(|a| *a != m.origin)
                .all(|a| m.clock.get(&a) <= self.clocks[r].get(&a))
    }

    fn admissible_at(&self, r: usize) -> Vec<usize> {
        (0..self.log.len())
            .filter(|&i| self.admissible(r, i))
            .collect()
    }

    fn deliver(&mut self, r: usize, i: usize) {
        assert!(self.admissible(r, i), "delivery outside causal order");
        let op = self.log[i].op.clone();
        let clock = self.log[i].clock.clone();
        self.replicas[r].effect(&op);
        self.clocks[r].join(&clock);
        self.delivered[r].insert(i);
    }

    /// Deliver everything remaining in a pseudo-random causally
    /// admissible order determined by `seed`.
    fn flush(&mut self, mut seed: u64) {
        loop {
            let mut progress = false;
            for r in 0..self.replicas.len() {
                let adm = self.admissible_at(r);
                if !adm.is_empty() {
                    seed = seed
                        .wrapping_mul(6364136223846793005)
                        .wrapping_add(1442695040888963407);
                    self.deliver(r, adm[(seed >> 33) as usize % adm.len()]);
                    progress = true;
                }
            }
            if !progress {
                break;
            }
        }
    }

    fn all_converged(&self) -> bool {
        self.replicas.windows(2).all(|w| w[0] == w[1])
    }
}

// --- OpCounter: delivery order does not matter ---

proptest! {
    /// The counter's `effect` commutes unconditionally: any
    /// permutation of the same set of operations yields the same
    /// state (causality is not needed).
    #[test]
    fn opcounter_converges_under_any_delivery_order(
        (deltas, shuffled) in vec(-5i64..=5, 1..16)
            .prop_flat_map(|v| (Just(v.clone()), Just(v).prop_shuffle()))
    ) {
        let mut a = OpCounter::new();
        let mut b = OpCounter::new();
        for d in &deltas {
            a.effect(d);
        }
        for d in &shuffled {
            b.effect(d);
        }
        prop_assert_eq!(a, b);
    }
}

// --- OpOrSet: convergence under causal delivery ---

/// Step of a random history: a local operation or a delivery.
#[derive(Debug, Clone)]
enum Cmd {
    Add(u8, u8),
    Remove(u8, u8),
    /// Deliver to a replica one of the admissible packets (choice by
    /// modulus).
    Deliver(u8, u8),
}

fn cmds() -> impl Strategy<Value = Vec<Cmd>> {
    vec(
        prop_oneof![
            ((0u8..3), (0u8..4)).prop_map(|(r, x)| Cmd::Add(r, x)),
            ((0u8..3), (0u8..4)).prop_map(|(r, x)| Cmd::Remove(r, x)),
            ((0u8..3), any::<u8>()).prop_map(|(r, k)| Cmd::Deliver(r, k)),
        ],
        0..40,
    )
}

proptest! {
    /// Any history of operations and any causally admissible delivery
    /// order bring all replicas into one state.
    #[test]
    fn op_orset_converges_under_causal_delivery(cmds in cmds(), seed in any::<u64>()) {
        let mut net: CausalNet<OpOrSet<usize, u8>> = CausalNet::new(3);
        for cmd in cmds {
            match cmd {
                Cmd::Add(r, x) => {
                    let r = r as usize;
                    let op = net.replicas[r].prepare_add(r, x);
                    net.local(r, op);
                }
                Cmd::Remove(r, x) => {
                    let r = r as usize;
                    let op = net.replicas[r].prepare_remove(&x);
                    net.local(r, op);
                }
                Cmd::Deliver(r, k) => {
                    let r = r as usize;
                    let adm = net.admissible_at(r);
                    if !adm.is_empty() {
                        net.deliver(r, adm[k as usize % adm.len()]);
                    }
                }
            }
        }
        net.flush(seed);
        prop_assert!(net.all_converged());
    }
}

/// Add-wins is preserved in the op-based variant: a concurrent
/// insertion carries a tag that the removal did not observe.
#[test]
fn op_orset_add_wins_under_causal_delivery() {
    let mut net: CausalNet<OpOrSet<usize, &str>> = CausalNet::new(2);
    let add = net.replicas[0].prepare_add(0, "x");
    net.local(0, add);
    net.deliver(1, 0); // replica 1 observes the insertion…
    let rem = net.replicas[1].prepare_remove(&"x");
    net.local(1, rem); // …and removes it
    let readd = net.replicas[0].prepare_add(0, "x"); // concurrent insertion
    net.local(0, readd);
    net.flush(42);
    assert!(net.all_converged());
    assert!(net.replicas[0].contains(&"x")); // add-wins
}

// --- Counter-test: without causal delivery an op-based OR-Set diverges ---

/// The same pair of operations, delivered in a different order,
/// diverges the replicas forever: `Remove` before "its" `Add` deletes
/// nothing, and then `Add` resurrects the element. For a state-based
/// OR-Set that scenario is impossible: the tombstone is part of
/// growing state, and `⊔` is idempotent and commutative for any merge
/// orders.
#[test]
fn op_orset_without_causal_delivery_diverges() {
    let mut source: OpOrSet<u8, &str> = OpOrSet::new();
    let add = source.prepare_add(0, "x");
    source.effect(&add);
    let rem = source.prepare_remove(&"x"); // causally follows add
    source.effect(&rem);

    // Causal order: add, then rem — matches the source.
    let mut causal: OpOrSet<u8, &str> = OpOrSet::new();
    causal.effect(&add);
    causal.effect(&rem);
    assert_eq!(causal, source);
    assert!(!causal.contains(&"x"));

    // Broken order: rem delivered before add.
    let mut reordered: OpOrSet<u8, &str> = OpOrSet::new();
    reordered.effect(&rem); // nothing to delete
    reordered.effect(&add); // the element is "resurrected"
    assert!(reordered.contains(&"x"));
    assert_ne!(causal, reordered); // one set of operations, different states
}
