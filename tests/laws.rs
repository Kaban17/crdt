//! Обобщённая проверка законов полурешётки соединения.
//!
//! Центральный методический приём работы: законы записаны **один раз**
//! для произвольного `T: JoinSemilattice` (функции `assert_*`), а макрос
//! `lattice_laws!` инстанцирует их для каждой реализации. Добавление
//! нового CRDT в библиотеку требует одной строки в этом файле — всё
//! остальное делает система типов.
//!
//! Проверяются:
//! * ассоциативность, коммутативность, идемпотентность `⊔`;
//! * инфляционность слияния (`a ≤ a ⊔ b`) и свойство точной верхней
//!   грани (`a ≤ c ∧ b ≤ c ⇒ a ⊔ b ≤ c`);
//! * согласованность выводимого порядка (рефлексивность, транзитивность,
//!   антисимметричность);
//! * инфляционность локальных операций (`a ≤ apply(a, op)`) — обобщённо
//!   через `Update::Op` (макрос `inflation_law!`).

mod common;

use crdt_lab::{JoinSemilattice, Update};
use proptest::prelude::*;

fn assert_associative<T: JoinSemilattice + std::fmt::Debug>(a: &T, b: &T, c: &T) {
    let left = a.clone().joined(b).joined(c);
    let right = a.clone().joined(&b.clone().joined(c));
    assert_eq!(left, right, "(a ⊔ b) ⊔ c ≠ a ⊔ (b ⊔ c)");
}

fn assert_commutative<T: JoinSemilattice + std::fmt::Debug>(a: &T, b: &T) {
    assert_eq!(
        a.clone().joined(b),
        b.clone().joined(a),
        "a ⊔ b ≠ b ⊔ a"
    );
}

fn assert_idempotent<T: JoinSemilattice + std::fmt::Debug>(a: &T) {
    assert_eq!(a.clone().joined(a), *a, "a ⊔ a ≠ a");
}

fn assert_join_is_lub<T: JoinSemilattice + std::fmt::Debug>(a: &T, b: &T, c: &T) {
    let ab = a.clone().joined(b);
    // верхняя грань
    assert!(a.leq(&ab), "a ≰ a ⊔ b");
    assert!(b.leq(&ab), "b ≰ a ⊔ b");
    // точная: любая верхняя грань c мажорирует a ⊔ b
    let upper = c.clone().joined(a).joined(b);
    assert!(ab.leq(&upper), "a ⊔ b ≰ верхней грани");
}

fn assert_partial_order<T: JoinSemilattice + std::fmt::Debug>(a: &T, b: &T, c: &T) {
    assert!(a.leq(a), "порядок не рефлексивен");
    if a.leq(b) && b.leq(c) {
        assert!(a.leq(c), "порядок не транзитивен");
    }
    if a.leq(b) && b.leq(a) {
        assert_eq!(a, b, "порядок не антисимметричен");
    }
}

/// Инфляционность операции: `a ≤ apply(a, op)` — второе условие
/// теоремы о сходимости CvRDT, обобщённое благодаря `Update::Op`.
fn assert_inflationary<T: Update + std::fmt::Debug>(state: &T, op: T::Op)
where
    T::Op: std::fmt::Debug + Clone,
{
    let mut after = state.clone();
    after.apply(op.clone());
    assert!(state.leq(&after), "операция {op:?} не инфляционна");
}

macro_rules! inflation_law {
    ($name:ident, $state:expr, $op:expr) => {
        mod $name {
            use super::*;

            proptest! {
                #[test]
                fn updates_are_inflationary(state in $state, op in $op) {
                    assert_inflationary(&state, op);
                }
            }
        }
    };
}

macro_rules! lattice_laws {
    ($name:ident, $strategy:expr) => {
        mod $name {
            use super::*;

            proptest! {
                #[test]
                fn associative(a in $strategy, b in $strategy, c in $strategy) {
                    assert_associative(&a, &b, &c);
                }

                #[test]
                fn commutative(a in $strategy, b in $strategy) {
                    assert_commutative(&a, &b);
                }

                #[test]
                fn idempotent(a in $strategy) {
                    assert_idempotent(&a);
                }

                #[test]
                fn join_is_least_upper_bound(a in $strategy, b in $strategy, c in $strategy) {
                    assert_join_is_lub(&a, &b, &c);
                }

                #[test]
                fn derived_relation_is_partial_order(
                    a in $strategy, b in $strategy, c in $strategy
                ) {
                    assert_partial_order(&a, &b, &c);
                }
            }
        }
    };
}

lattice_laws!(vclock_laws, common::vclock());
lattice_laws!(gcounter_laws, common::gcounter());
lattice_laws!(pncounter_laws, common::pncounter());
lattice_laws!(gset_laws, common::gset());
lattice_laws!(lww_register_laws, common::lww_register());
lattice_laws!(orset_laws, common::orset());

inflation_law!(vclock_inflation, common::vclock(), common::vclock_op());
inflation_law!(gcounter_inflation, common::gcounter(), common::gcounter_op());
inflation_law!(pncounter_inflation, common::pncounter(), common::pncounter_op());
inflation_law!(gset_inflation, common::gset(), common::gset_op());
inflation_law!(lww_inflation, common::lww_register(), common::lww_op());
inflation_law!(orset_inflation, common::orset(), common::orset_op());
