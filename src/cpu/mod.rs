/*!
cpu::mod - Public façade for the 6502 CPU core.

This module reorganizes the previous monolithic `cpu6502.rs` into a
multi-file structure:

    state.rs        - Core CPU state (registers, flags) + constructors.
    addressing.rs   - Addressing mode enum & operand resolution helpers.
    execute.rs      - Instruction semantic helpers (ALU, stack, RMW, branch).
    decode_legacy.rs- Legacy giant match-based dispatcher (temporary).
    table.rs        - Feature-gated table-driven metadata & dispatch.
    dispatch.rs     - Orchestrates a single CPU step (DMA/IRQ/NMI + dispatch).

The public surface is exposed via the `Cpu` facade (wrapping `CpuState`).
Downstream code should not rely on internal module layout; internal organization
may evolve as optimizations and table-driven dispatch mature.

Feature flags:
    table_dispatch  - Enables the new table-driven opcode dispatcher.
                      When disabled, the legacy decoder remains in use.
Future planned flags (not yet implemented):
    cycle_exact     - Potential microcycle timing refinement.
    trace           - Optional instruction tracing instrumentation.

Migration status:
- Legacy monolithic `cpu6502.rs` has been removed.
- All execution now targets `CpuState` through the `Cpu` facade and generic
  helpers in the dispatch / execute modules.

Usage:
```rust
use arness::cpu::core::Cpu;

let mut cpu = Cpu::new();
cpu.reset(&mut bus);
cpu.step(&mut bus);
```

NOTE: The facade offers stable stepping (`step`, `run`) while internal modules
(e.g. dispatch) remain free to change implementation details.

*/

// Legacy adapter removed; all functionality resides in modular submodules.

// Planned future modules (introduced incrementally; initially empty stubs):
// mod state;
// mod addressing;
// mod execute;
// mod decode_legacy;
// #[cfg(feature = "table_dispatch")]
// mod table;
// mod dispatch;

// Public crate-internal CPU modules (ordered for clarity)
pub mod addressing;
pub mod core;
pub mod cycles;
pub mod dispatch;
pub mod execute;
pub mod regs;
pub mod state;
#[cfg(feature = "table_dispatch")]
pub mod table;

// Re-exports:
// - Cpu (facade over CpuState)
// - CpuState (raw state; exposed for tests, snapshots, trait impls)
// - Flag constants (canonical bit masks)
pub use crate::cpu::core::Cpu;
pub use crate::cpu::regs::CpuRegs;
pub use crate::cpu::state::{
    BREAK, CARRY, CpuState, DECIMAL, IRQ_DISABLE, NEGATIVE, OVERFLOW, UNUSED, ZERO,
};
