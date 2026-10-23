//! `g5core` — the one implementation of everything both halves of this product
//! have to agree on.
//!
//! The fan curve, the unit conversions, the EC register map and the on-disk
//! state used to exist twice: once in `src/g5fan.py` (what the daemon actually
//! applies) and once in Rust inside the panel (what the graph draws). The two
//! were held together by hand-written comments ("Mirrors `duty_for` in
//! src/g5fan.py") and by a pair of test vectors copied by hand into each suite.
//! That is a promise nothing enforces, and the two copies had already drifted:
//! the Python implementation rounds like Python's `round()` (half to even) with
//! `f64`, the Rust one used `f32` and `f32::round` (half away from zero). On the
//! curve `(40,10) (42,11)` at 41 °C the daemon asks for 10 % and the panel drew
//! 11 %.
//!
//! So this crate is deliberately the only place those rules live.
//!
//! * [`fan`] — the curve maths, the presets, the mode vocabulary and the
//!   `status --json` contract the panel reads.
//! * [`units`] — duty percent ↔ EC byte, tacho period ↔ rpm.
//! * [`ec`] — the register map, the mailbox primitives, the dry-run EC and the
//!   DMI model guard.
//! * [`state`] — the fan state, the keyboard state, profiles, the published
//!   snapshot and the EC lock.
//! * [`cli`] — a small argument splitter, so the two CLIs parse flags the same
//!   way (and can be tested without spawning a process).
//!
//! The shared test vectors live in `testdata/curve_vectors.json` and are read by
//! *both* sides: `tests/vectors.rs` here and `tests/test_g5fan.py` in Python. A
//! divergence now fails a test on whichever side moved.

pub mod cli;
pub mod ec;
pub mod fan;
pub mod state;
pub mod units;

pub use fan::{CurveError, FanMode, Point};
