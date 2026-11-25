//! Simulation check of strong eventual consistency (SEC).
//!
//! Scenario: N replicas independently apply random local operations,
//! then a full exchange of states follows. Each replica merges the
//! others' states **in its own order** (a cyclic shift) and with
//! **redelivery** of one of the states. If afterwards all replicas are
//! equal, that run has shown independence of the result from order and
//! multiplicity of delivery, i.e. a working consequence of the ACI
//! laws.
//!
//! Inflation of local operations (the second hypothesis of the
//! theorem) is checked generically in `tests/laws.rs` (macro
//! `inflation_law!`).

mod common;

use common::ActorId;
use crdt_lab::orset::Op as SetOp;
use crdt_lab::{CvRdt, GCounter, OrSet, Orswot, PnCounter, Update};
use proptest::prelude::*;

/// Full exchange: replica `i` merges the others starting from `i+1`
/// (each replica has its own order), and a neighbour's state is
/// delivered twice.
fn gossip_everyone<T: CvRdt>(replicas: &mut [T]) {
    let snapshot: Vec<T> = replicas.to_vec();
    let n = snapshot.len();
    for (i, replica) in replicas.iter_mut().enumerate() {
        for k in 1..n {
            replica.merge(&snapshot[(i + k) % n]);
        }
        replica.merge(&snapshot[(i + 1) % n]); // redelivery
    }
}

fn assert_all_equal<T: CvRdt + std::fmt::Debug>(replicas: &[T]) {
    for pair in replicas.windows(2) {
        assert_eq!(pair[0], pair[1], "replicas did not converge");
    }
}

proptest! {
    /// SEC for G-Counter: the value after convergence is the sum of
    /// all operations.
    #[test]
    fn gcounter_replicas_converge(
        ops in proptest::collection::vec((0..3usize, 1..10u64), 0..20)
    ) {
        let mut replicas = vec![GCounter::new(); 3];
        let mut total = 0u64;
        for (replica, amount) in ops {
            // actor = replica index: each writes only its own component
            replicas[replica].increment_by(replica as u8, amount);
            total += amount;
        }

        gossip_everyone(&mut replicas);

        assert_all_equal(&replicas);
        prop_assert_eq!(replicas[0].value(), total);
    }

    /// SEC for PN-Counter: the value is the algebraic sum of all
    /// operations.
    #[test]
    fn pncounter_replicas_converge(
        ops in proptest::collection::vec((0..3usize, proptest::bool::ANY), 0..20)
    ) {
        let mut replicas = vec![PnCounter::new(); 3];
        let mut total = 0i64;
        for (replica, positive) in ops {
            if positive {
                replicas[replica].increment(replica as u8);
                total += 1;
            } else {
                replicas[replica].decrement(replica as u8);
                total -= 1;
            }
        }

        gossip_everyone(&mut replicas);

        assert_all_equal(&replicas);
        prop_assert_eq!(replicas[0].value(), total);
    }

    /// SEC for OR-Set: replicas converge to one observed set.
    #[test]
    fn orset_replicas_converge(
        ops in proptest::collection::vec((0..3usize, common::orset_op()), 0..24)
    ) {
        let mut replicas: Vec<OrSet<ActorId, u8>> = vec![OrSet::new(); 3];
        for (replica, op) in ops {
            // tags are made unique by construction: actor = replica index
            let op = match op {
                SetOp::Add(_, item) => SetOp::Add(replica as u8, item),
                remove => remove,
            };
            replicas[replica].apply(op);
        }

        gossip_everyone(&mut replicas);
        assert_all_equal(&replicas);
    }

    /// SEC for ORSWOT: convergence despite a non-trivial `join`
    /// (tags may die on merge) and redeliveries.
    #[test]
    fn orswot_replicas_converge(
        ops in proptest::collection::vec((0..3usize, common::orset_op()), 0..24)
    ) {
        let mut replicas: Vec<Orswot<ActorId, u8>> = vec![Orswot::new(); 3];
        for (replica, op) in ops {
            // tags are made unique by construction: actor = replica index
            let op = match op {
                SetOp::Add(_, item) => SetOp::Add(replica as u8, item),
                remove => remove,
            };
            replicas[replica].apply(op);
        }

        gossip_everyone(&mut replicas);
        assert_all_equal(&replicas);
    }

    /// Merging after a merge changes nothing: a fixed point has been
    /// reached.
    #[test]
    fn merge_is_stable_after_convergence(
        ops in proptest::collection::vec((0..3usize, common::orset_op()), 0..24)
    ) {
        let mut replicas: Vec<OrSet<ActorId, u8>> = vec![OrSet::new(); 3];
        for (replica, op) in ops {
            replicas[replica].apply(op);
        }
        gossip_everyone(&mut replicas);
        let converged = replicas.clone();
        gossip_everyone(&mut replicas);
        prop_assert_eq!(replicas, converged);
    }
}
