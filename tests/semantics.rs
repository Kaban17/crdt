//! Проверка **семантики** против эталонной последовательной модели.
//!
//! Законы решётки и SEC-имитация гарантируют, что реплики сойдутся, но не
//! говорят, что они сойдутся к *правильному* значению: сходиться можно и к
//! пустому множеству. Этот файл закрывает пробел приёмом model-based
//! testing (ср. «Testing CRDTs in Rust, From Theory to Practice», где так
//! была найдена ошибка в опубликованной статье): наблюдаемое состояние
//! CRDT сверяется с простой заведомо корректной моделью.
//!
//! Модели:
//! * последовательная: одна реплика == обычная структура данных;
//! * конкурентная (обмен в конце): спецификация из литературы —
//!   add-wins для OR-Set, максимум по тотальному порядку для LWW,
//!   множество последних записей реплик для MV-Register.
//!
//! Модель конкурентного прогона проста, потому что реплики не общаются
//! до финального обмена: внутри реплики операции причинно упорядочены,
//! между репликами — попарно конкурентны.

mod common;

use std::collections::BTreeSet;

use common::ActorId;
use crdt_lab::orset::Op as SetOp;
use crdt_lab::{JoinSemilattice, LwwRegister, MvRegister, OrSet, Orswot, Update};
use proptest::prelude::*;

/// Слить состояния всех реплик в одно.
fn converge<T: JoinSemilattice>(replicas: &[T]) -> T {
    let mut result = replicas[0].clone();
    for r in &replicas[1..] {
        result.join(r);
    }
    result
}

/// Эталон add-wins для прогона «обмен только в конце»: элемент присутствует,
/// если на какой-то реплике есть его вставка, не отменённая **последующим**
/// удалением на той же реплике (удаления чужих реплик конкурентны вставке
/// и по спецификации проигрывают).
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
    /// Одна реплика OR-Set неотличима от обычного множества.
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

    /// Сошедшийся OR-Set равен эталону add-wins спецификации.
    #[test]
    fn orset_converged_matches_addwins_model(
        per_replica in proptest::collection::vec(
            proptest::collection::vec(common::orset_op(), 0..12), 3..=3
        )
    ) {
        // уникальность меток: актор = номер реплики
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

    /// Одна реплика ORSWOT неотличима от обычного множества.
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

    /// Дифференциальный тест: на одной истории надгробный OR-Set и ORSWOT
    /// наблюдаемо совпадают — и с add-wins эталоном, и друг с другом.
    /// Различие реализаций только в размере состояния, не в семантике.
    #[test]
    fn orswot_converged_matches_orset_and_addwins_model(
        per_replica in proptest::collection::vec(
            proptest::collection::vec(common::orset_op(), 0..12), 3..=3
        )
    ) {
        // уникальность меток: актор = номер реплики
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

    /// Сошедшийся LWW-регистр хранит максимум всех записей по тотальному
    /// порядку `(timestamp, actor, value)` — включая начальное состояние.
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
            .chain([(0, 0, 0)]) // начальное состояние тоже участвует
            .max()
            .unwrap();

        let converged = converge(&replicas);
        prop_assert_eq!(*converged.value(), expected.2);
    }

    /// Сошедшийся MV-регистр хранит в точности последние записи всех
    /// писавших реплик: внутри реплики поздняя запись вытесняет раннюю,
    /// между репликами записи конкурентны и выживают все.
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
