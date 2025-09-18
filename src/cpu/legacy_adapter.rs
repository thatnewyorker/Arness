/*!
legacy_adapter.rs

Temporary bridge module during CPU core refactor.

Context:
- The original 6502 implementation still lives in `src/cpu6502.rs`
  at the crate root (module path: `crate::cpu6502`).
- The new modular layout (`src/cpu/`) introduces a public façade
  (`cpu::mod`) that will eventually own all CPU logic.
- While code is being migrated into submodules (`state`, `addressing`,
  `execute`, `table`, `decode_legacy`, `dispatch`), we re-export the
  existing `Cpu6502` so downstream code continues to compile without
  modification.

Planned Decommission:
- Once the instruction dispatch (table-driven) and supporting helpers
  are fully relocated, this adapter will be removed.
- At that point, `Cpu6502` will be defined directly inside the new
  modular hierarchy and this file deleted.

Usage:
    use crate::cpu::Cpu6502; // still works transparently

Do not add new logic here—only re-export during transition.
*/

pub use crate::cpu6502::Cpu6502;
