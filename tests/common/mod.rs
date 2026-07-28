//! Общие proptest-стратегии: генерация произвольных состояний CRDT
//! через последовательности операций трейта `Update`.
//!
//! Благодаря ассоциированному типу `Update::Op` генерация универсальна:
//! стратегия операций задаётся один раз на тип, а произвольное
//! **достижимое** состояние — это свёртка списка операций ([`build`]).
//! Тесты проверяют законы именно на достижимых состояниях, как и требует
//! теория, а приватность полей гарантирует, что других не бывает.

// Модуль компилируется в каждый тестовый бинарник отдельно,
// и каждый использует своё подмножество стратегий.
#![allow(dead_code)]

use proptest::prelude::*;

use crdt_lab::{
    orset, pncounter, GCounter, GSet, LwwRegister, MvRegister, OrSet, Orswot, PnCounter, Update,
    VClock,
};

/// Небольшой алфавит акторов: коллизии между репликами вероятны,
/// что даёт более жёсткую проверку слияния.
pub type ActorId = u8;

/// Свёртка списка операций в состояние — универсальный генератор
/// достижимых состояний для любого `T: Update + Default`.
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

// --- Операции ---

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

// --- Состояния ---

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

/// ORSWOT использует тот же алфавит операций, что и OR-Set.
pub fn orswot() -> impl Strategy<Value = Orswot<ActorId, u8>> {
    states(orset_op())
}
