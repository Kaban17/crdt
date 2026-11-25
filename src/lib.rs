//! # crdt-lab
//!
//! A small Rust library of state-based CRDTs (CvRDTs).
//!
//! ## Theory
//!
//! A state-based CRDT (CvRDT, *convergent replicated data type*) after
//! Shapiro, Preguiça, Baquero, Zawirski (2011) is a data type whose
//! states form a **join-semilattice**: a partially ordered set `(S, ≤)`
//! with a join operation `⊔` (least upper bound) that satisfies:
//!
//! * **associativity**: `(a ⊔ b) ⊔ c = a ⊔ (b ⊔ c)`;
//! * **commutativity**: `a ⊔ b = b ⊔ a`;
//! * **idempotence**: `a ⊔ a = a`.
//!
//! Local updates must be **inflationary** (`a ≤ update(a)`), and replica
//! merge is exactly `⊔`. These conditions imply **strong eventual
//! consistency** (SEC): two replicas that have seen the same set of
//! updates, in *any* order and with *any* duplicates, end in the same
//! state.
//!
//! ## Role of the Rust type system
//!
//! The semilattice contract is the [`lattice::JoinSemilattice`] trait.
//! From that the crate gets:
//!
//! 1. **Generic law checks**: property-based tests (see `tests/`) are
//!    written once for any `T: JoinSemilattice` and instantiated by a
//!    macro for each implementation;
//! 2. **Derived order**: `≤` is not written by hand; it is *derived*
//!    from `join` (`a ≤ b ⟺ a ⊔ b = b`), so the order and the merge
//!    cannot disagree;
//! 3. **Static guarantees**: parameterization by replica id (`A: Actor`)
//!    and ownership (`&mut self` / `&Self`) keep distinct types from
//!    mixing and keep merge from mutating the other replica;
//! 4. **Typed invariants**: protocol invariants that types cannot
//!    express (tag uniqueness) are lifted into the API as `Result` via
//!    [`lattice::ValidateMerge`] and an associated error type.
//!
//! ## Implemented types
//!
//! | Module | Type | Kind |
//! |---|---|---|
//! | [`vclock`] | [`VClock`] | vector clock (building block) |
//! | [`gcounter`] | [`GCounter`] | grow-only counter |
//! | [`pncounter`] | [`PnCounter`] | counter with increment/decrement |
//! | [`gset`] | [`GSet`] | grow-only set |
//! | [`lww`] | [`LwwRegister`] | last-writer-wins register |
//! | [`mvreg`] | [`MvRegister`] | multi-value register (concurrent writes are kept) |
//! | [`orset`] | [`OrSet`] | observed-remove set (add-wins) |
//! | [`orswot`] | [`Orswot`] | OR-Set without tombstones (causal context) |
//! | [`cmrdt`] | [`OpCounter`], [`OpOrSet`] | op-based model (CmRDT) for comparison |

pub mod cmrdt;
pub mod gcounter;
pub mod gset;
pub mod lattice;
pub mod lww;
pub mod mvreg;
pub mod orset;
pub mod orswot;
pub mod pncounter;
pub mod vclock;

pub use cmrdt::{CmRdt, OpCounter, OpOrSet};
pub use gcounter::GCounter;
pub use gset::GSet;
pub use lattice::{Actor, CvRdt, JoinSemilattice, Update, ValidateMerge};
pub use lww::LwwRegister;
pub use mvreg::MvRegister;
pub use orset::OrSet;
pub use orswot::Orswot;
pub use pncounter::PnCounter;
pub use vclock::VClock;
