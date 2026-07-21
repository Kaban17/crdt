//! Общие proptest-стратегии: генерация произвольных состояний CRDT
//! через последовательности допустимых операций.
//!
//! Состояния порождаются только операциями публичного API — тесты
//! проверяют законы на достижимых состояниях, как и требует теория
//! (полурешётка нужна на множестве достижимых состояний, а приватность
//! полей гарантирует, что других не бывает).

// Модуль компилируется в каждый тестовый бинарник отдельно,
// и каждый использует своё подмножество стратегий.
#![allow(dead_code)]

use proptest::prelude::*;

use crdt_lab::{GCounter, GSet, LwwRegister, OrSet, PnCounter, VClock};

/// Небольшой алфавит акторов: коллизии между репликами вероятны,
/// что даёт более жёсткую проверку слияния.
pub type ActorId = u8;

pub fn vclock() -> impl Strategy<Value = VClock<ActorId>> {
    proptest::collection::vec((0..4u8, 1..8u64), 0..8).prop_map(|ops| {
        let mut clock = VClock::new();
        for (actor, times) in ops {
            for _ in 0..times {
                clock.increment(actor);
            }
        }
        clock
    })
}

pub fn gcounter() -> impl Strategy<Value = GCounter<ActorId>> {
    proptest::collection::vec((0..4u8, 1..20u64), 0..8).prop_map(|ops| {
        let mut counter = GCounter::new();
        for (actor, amount) in ops {
            counter.increment_by(actor, amount);
        }
        counter
    })
}

pub fn pncounter() -> impl Strategy<Value = PnCounter<ActorId>> {
    proptest::collection::vec((0..4u8, proptest::bool::ANY), 0..12).prop_map(|ops| {
        let mut counter = PnCounter::new();
        for (actor, positive) in ops {
            if positive {
                counter.increment(actor);
            } else {
                counter.decrement(actor);
            }
        }
        counter
    })
}

pub fn gset() -> impl Strategy<Value = GSet<u8>> {
    proptest::collection::btree_set(0..32u8, 0..8).prop_map(|items| {
        let mut set = GSet::new();
        for item in items {
            set.insert(item);
        }
        set
    })
}

pub fn lww_register() -> impl Strategy<Value = LwwRegister<ActorId, i32>> {
    proptest::collection::vec((0..16u64, 0..4u8, -100..100i32), 0..8).prop_map(|writes| {
        let mut register = LwwRegister::new(0, 0);
        for (timestamp, actor, value) in writes {
            register.write(timestamp, actor, value);
        }
        register
    })
}

#[derive(Clone, Debug)]
pub enum SetOp {
    Add(ActorId, u8),
    Remove(u8),
}

pub fn set_op() -> impl Strategy<Value = SetOp> {
    prop_oneof![
        (0..4u8, 0..8u8).prop_map(|(actor, item)| SetOp::Add(actor, item)),
        (0..8u8).prop_map(SetOp::Remove),
    ]
}

pub fn apply_set_ops(set: &mut OrSet<ActorId, u8>, ops: &[SetOp]) {
    for op in ops {
        match *op {
            SetOp::Add(actor, item) => set.add(actor, item),
            SetOp::Remove(ref item) => set.remove(item),
        }
    }
}

pub fn orset() -> impl Strategy<Value = OrSet<ActorId, u8>> {
    proptest::collection::vec(set_op(), 0..12).prop_map(|ops| {
        let mut set = OrSet::new();
        apply_set_ops(&mut set, &ops);
        set
    })
}
