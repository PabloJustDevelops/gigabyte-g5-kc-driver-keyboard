// Fan status, as `g5fan status --json` reports it.
//
// The shape and the maths behind it now live in `g5core::fan`, which is the same
// code the `g5fan` binary links: the panel used to carry its own copy of
// `duty_for`, `curve_problem` and the status structs, held in step with the CLI
// by a comment and a hand-copied pair of test vectors. Those copies were already
// wrong — the panel rounded curve ties away from zero where the daemon rounds
// them to even, so on the curve `(40,10) (42,11)` at 41 °C it drew 11 % while
// asking the fans for 10 % — and nothing failed when they drifted.
//
// So this module is deliberately small: it re-exports what the rest of the panel
// imports, and adds only what is panel-specific (nothing, currently). The field
// names it re-exports are a contract with the CLI, and `g5core`'s own tests pin
// both halves of it.
//
//   panel  --`g5fan status --cached --json`-->  g5fan (root by pkexec only for
//          writes)

pub use g5core::fan::{
    FanMode, RawFanReading, RawFanStatus, curve_problem, duty_for, format_curve,
};
