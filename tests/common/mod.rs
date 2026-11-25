//! Shared proptest strategies: generate arbitrary CRDT states by
//! sequences of `Update` operations.
//!
//! Thanks to the associated type `Update::Op`, generation is
//! universal: the operation strategy is given once per type, and an
//! arbitrary **reachable** state is a fold of a list of operations
//! ([`build`]). Tests check the laws on reachable states, as the
//! theory requires, and field privacy guarantees that no other states
//! exist.

// The module is compiled into each test binary separately, and each
// uses its own subset of the strategies.
#![allow(dead_code)]

use proptest::prelude::*;

use crdt_lab::{
    orset, pncounter, GCounter, GSet, LwwRegister, MvRegister, OrSet, Orswot, PnCounter, Update,
    VClock,
};

/// Small alphabet of actors: collisions between replicas are likely,
/// which makes merge checks stricter.
pub type ActorId = u8;

/// Fold a list of operations into a state: a universal generator of
/// reachable states for any `T: Update + Default`.
pub fn build<T: Update + Default>(ops: Vec<T::Op>) -> T {
    let mut state = T::default();
    for op in ops {
        state.apply(op);
    }
    state
}

fn states<T: Update + Default + std::fmt::Debug>(
    op: impl Strategy<Value = T::Op>,
) -> impl Strategy<Value = T>
where
    T::Op: std::fmt::Debug,
{
    proptest::collection::vec(op, 0..12).prop_map(build)
}

// --- Operations ---

pub fn vclock_op() -> impl Strategy<Value = ActorId> {
    0..4u8
}

pub fn gcounter_op() -> impl Strategy<Value = (ActorId, u64)> {
    (0..4u8, 1..20u64)
}

pub fn pncounter_op() -> impl Strategy<Value = pncounter::Op<ActorId>> {
    prop_oneof![
        (0..4u8).prop_map(pncounter::Op::Inc),
        (0..4u8).prop_map(pncounter::Op::Dec),
    ]
}

pub fn gset_op() -> impl Strategy<Value = u8> {
    0..32u8
}

pub fn lww_op() -> impl Strategy<Value = (u64, ActorId, i32)> {
    (0..16u64, 0..4u8, -100..100i32)
}

pub fn mvreg_op() -> impl Strategy<Value = (ActorId, i32)> {
    (0..4u8, -100..100i32)
}

pub fn orset_op() -> impl Strategy<Value = orset::Op<ActorId, u8>> {
    prop_oneof![
        (0..4u8, 0..8u8).prop_map(|(actor, item)| orset::Op::Add(actor, item)),
        (0..8u8).prop_map(orset::Op::Remove),
    ]
}

// --- States ---

pub fn vclock() -> impl Strategy<Value = VClock<ActorId>> {
    states(vclock_op())
}

pub fn gcounter() -> impl Strategy<Value = GCounter<ActorId>> {
    states(gcounter_op())
}

pub fn pncounter() -> impl Strategy<Value = PnCounter<ActorId>> {
    states(pncounter_op())
}

pub fn gset() -> impl Strategy<Value = GSet<u8>> {
    states(gset_op())
}

pub fn lww_register() -> impl Strategy<Value = LwwRegister<ActorId, i32>> {
    states(lww_op())
}

pub fn mvreg() -> impl Strategy<Value = MvRegister<ActorId, i32>> {
    states(mvreg_op())
}

pub fn orset() -> impl Strategy<Value = OrSet<ActorId, u8>> {
    states(orset_op())
}

/// ORSWOT uses the same alphabet of operations as OR-Set.
pub fn orswot() -> impl Strategy<Value = Orswot<ActorId, u8>> {
    states(orset_op())
}
