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

Only the stable public surface (Cpu6502 methods) is re-exported here.
Downstream code should not rely on internal module layout; all internals
are subject to change while the table-driven refactor proceeds.

Feature flags:
    table_dispatch  - Enables the new table-driven opcode dispatcher.
                      When disabled, the legacy decoder remains in use.
Future planned flags (not yet implemented):
    cycle_exact     - Potential microcycle timing refinement.
    trace           - Optional instruction tracing instrumentation.

Migration status:
- Initial lift: this façade file created.
- The legacy `cpu6502.rs` content is *temporarily* still in its original
  location. Subsequent steps will move code into the submodules declared
  below. During the transition, Cpu6502 here delegates to the old file's
  implementation via a thin wrapper (see `legacy_adapter` module).
  Once migration finishes, the adapter and the old file will be removed.

Usage:
```rust
use arness::cpu::Cpu6502;

let mut cpu = Cpu6502::new();
cpu.reset(&mut bus);
cpu.step(&mut bus);
```

NOTE: Until the full refactor is complete, some methods (run, irq, nmi,
etc.) forward to the legacy implementation.

*/

mod legacy_adapter;

// Planned future modules (introduced incrementally; initially empty stubs):
// mod state;
// mod addressing;
// mod execute;
// mod decode_legacy;
// #[cfg(feature = "table_dispatch")]
// mod table;
// mod dispatch;

pub use legacy_adapter::Cpu6502;
