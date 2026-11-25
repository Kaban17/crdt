//! Checks of **semantics** against a sequential reference model.
//!
//! Lattice laws and the SEC simulation guarantee that replicas
//! converge, but not that they converge to the *intended* value: they
//! could equally converge to the empty set. This file closes the gap
//! with model-based testing (cf. "Testing CRDTs in Rust, From Theory
//! to Practice", where the same technique found a bug in a published
//! paper): the observed CRDT state is compared with a simple,
//! obviously correct model.
//!
//! Models:
//! * sequential: one replica == an ordinary data structure;
//! * concurrent (exchange at the end): the specification from the
//!   literature — add-wins for OR-Set, maximum in a total order for
//!   LWW, the set of last replica writes for MV-Register.
//!
//! The concurrent-run model is simple because replicas do not talk
//! until the final exchange: operations are causally ordered inside a
//! replica and pairwise concurrent across replicas.

mod common;

use std::collections::BTreeSet;

use common::ActorId;
use crdt_lab::orset::Op as SetOp;
use crdt_lab::{JoinSemilattice, LwwRegister, MvRegister, OrSet, Orswot, Update};
use proptest::prelude::*;

/// Merge the states of all replicas into one.
fn converge<T: JoinSemilattice>(replicas: &[T]) -> T {
    let mut result = replicas[0].clone();
    for r in &replicas[1..] {
        result.join(r);
    }
    result
}

/// Add-wins reference for a "exchange only at the end" run: an
/// element is present if some replica has an insertion of it that is
/// not cancelled by a **later** deletion on the same replica
/// (deletions on other replicas are concurrent with the insertion and
/// lose by the specification).
fn addwins_model(per_replica: &[Vec<SetOp<ActorId, u8>>]) -> BTreeSet<u8> {
    let mut expected = BTreeSet::new();
    for ops in per_replica {
        for (i, op) in ops.iter().enumerate() {
            if let SetOp::Add(_, item) = op {
                let removed_later = ops[i + 1..]
                    .iter()
                    .any(|later| matches!(later, SetOp::Remove(x) if x == item));
                if !removed_later {
                    expected.insert(*item);
                }
            }
        }
    }
    expected
}

proptest! {
    /// A single OR-Set replica is indistinguishable from an ordinary set.
    #[test]
    fn orset_sequential_matches_plain_set(
        ops in proptest::collection::vec(common::orset_op(), 0..24)
    ) {
        let mut set: OrSet<ActorId, u8> = OrSet::new();
        let mut model: BTreeSet<u8> = BTreeSet::new();
        for op in ops {
            match &op {
                SetOp::Add(_, item) => { model.insert(*item); }
                SetOp::Remove(item) => { model.remove(item); }
            }
            set.apply(op);
            let observed: BTreeSet<u8> = set.iter().copied().collect();
            prop_assert_eq!(&observed, &model);
        }
    }

    /// A converged OR-Set equals the add-wins reference of the
    /// specification.
    #[test]
    fn orset_converged_matches_addwins_model(
        per_replica in proptest::collection::vec(
            proptest::collection::vec(common::orset_op(), 0..12), 3..=3
        )
    ) {
        // uniqueness of tags: actor = replica index
        let per_replica: Vec<Vec<SetOp<ActorId, u8>>> = per_replica
            .into_iter()
            .enumerate()
            .map(|(id, ops)| ops
                .into_iter()
                .map(|op| match op {
                    SetOp::Add(_, item) => SetOp::Add(id as u8, item),
                    remove => remove,
                })
                .collect())
            .collect();

        let mut replicas: Vec<OrSet<ActorId, u8>> = vec![OrSet::new(); 3];
        for (replica, ops) in replicas.iter_mut().zip(&per_replica) {
            for op in ops {
                replica.apply(op.clone());
            }
        }

        let converged = converge(&replicas);
        let observed: BTreeSet<u8> = converged.iter().copied().collect();
        prop_assert_eq!(observed, addwins_model(&per_replica));
    }

    /// A single ORSWOT replica is indistinguishable from an ordinary set.
    #[test]
    fn orswot_sequential_matches_plain_set(
        ops in proptest::collection::vec(common::orset_op(), 0..24)
    ) {
        let mut set: Orswot<ActorId, u8> = Orswot::new();
        let mut model: BTreeSet<u8> = BTreeSet::new();
        for op in ops {
            match &op {
                SetOp::Add(_, item) => { model.insert(*item); }
                SetOp::Remove(item) => { model.remove(item); }
            }
            set.apply(op);
            let observed: BTreeSet<u8> = set.iter().copied().collect();
            prop_assert_eq!(&observed, &model);
        }
    }

    /// Differential test: on the same history a tombstone OR-Set and
    /// ORSWOT agree observably, both with the add-wins reference and
    /// with each other. The implementations differ only in state size,
    /// not in semantics.
    #[test]
    fn orswot_converged_matches_orset_and_addwins_model(
        per_replica in proptest::collection::vec(
            proptest::collection::vec(common::orset_op(), 0..12), 3..=3
        )
    ) {
        // uniqueness of tags: actor = replica index
        let per_replica: Vec<Vec<SetOp<ActorId, u8>>> = per_replica
            .into_iter()
            .enumerate()
            .map(|(id, ops)| ops
                .into_iter()
                .map(|op| match op {
                    SetOp::Add(_, item) => SetOp::Add(id as u8, item),
                    remove => remove,
                })
                .collect())
            .collect();

        let mut swots: Vec<Orswot<ActorId, u8>> = vec![Orswot::new(); 3];
        let mut orsets: Vec<OrSet<ActorId, u8>> = vec![OrSet::new(); 3];
        for (id, ops) in per_replica.iter().enumerate() {
            for op in ops {
                swots[id].apply(op.clone());
                orsets[id].apply(op.clone());
            }
        }

        let swot: BTreeSet<u8> = converge(&swots).iter().copied().collect();
        let orset: BTreeSet<u8> = converge(&orsets).iter().copied().collect();
        let model = addwins_model(&per_replica);
        prop_assert_eq!(&swot, &model);
        prop_assert_eq!(&orset, &model);
    }

    /// A converged LWW register holds the maximum of all writes in the
    /// total order `(timestamp, actor, value)`, including the initial
    /// state.
    #[test]
    fn lww_converged_is_max_write(
        per_replica in proptest::collection::vec(
            proptest::collection::vec(common::lww_op(), 0..8), 3..=3
        )
    ) {
        let mut replicas = vec![LwwRegister::new(0u8, 0i32); 3];
        for (replica, writes) in replicas.iter_mut().zip(&per_replica) {
            for &(ts, actor, value) in writes {
                replica.write(ts, actor, value);
            }
        }

        let expected = per_replica
            .iter()
            .flatten()
            .copied()
            .chain([(0, 0, 0)]) // the initial state also participates
            .max()
            .unwrap();

        let converged = converge(&replicas);
        prop_assert_eq!(*converged.value(), expected.2);
    }

    /// A converged MV-register holds exactly the last writes of all
    /// writing replicas: inside a replica a later write displaces an
    /// earlier one; across replicas the writes are concurrent and all
    /// survive.
    #[test]
    fn mvreg_converged_holds_last_write_of_each_replica(
        per_replica in proptest::collection::vec(
            proptest::collection::vec(-100..100i32, 0..6), 3..=3
        )
    ) {
        let mut replicas: Vec<MvRegister<ActorId, i32>> = vec![MvRegister::new(); 3];
        for (id, (replica, writes)) in replicas.iter_mut().zip(&per_replica).enumerate() {
            for &value in writes {
                replica.write(id as u8, value);
            }
        }

        let mut expected: Vec<i32> = per_replica
            .iter()
            .filter_map(|writes| writes.last().copied())
            .collect();
        expected.sort();

        let converged = converge(&replicas);
        let mut observed: Vec<i32> = converged.read().copied().collect();
        observed.sort();
        prop_assert_eq!(observed, expected);
    }
}
