/*!
core::Cpu - Canonical 6502 CPU façade wrapping `CpuState`.

Status
======
This is the first Phase B migration step: introducing a canonical `Cpu`
type that owns a `CpuState` while the rest of the emulator (dispatch,
execute helpers, tests) previously relied on a legacy, monolithic CPU
representation.

Goal
====
Gradually move all internal code from referencing the legacy CPU type
directly to using `Cpu` (and, internally, `CpuState`). Once every
execution / dispatch path operates on `CpuState`, the legacy monolithic
CPU file can be reduced to a thin compatibility alias or removed.

Design
======
- `Cpu` stores a single field: `state: CpuState`.
- Public API exposes constructor, reset, register accessors, flag helpers,
  and halt state control.
- During the incremental migration a temporary bridging `step` approach
  was used to interoperate with existing legacy dispatch code. This
  bridge was intended as a stop‑gap to allow incremental migration
  without touching every handler simultaneously. The copying cost is
  negligible for functional correctness testing and has been removed
  once dispatch paths were updated to accept `CpuState` directly.

Conversion
==========
Conversion helpers were provided to allow seamless round‑tripping from
legacy representations while tests or modules were converted.

Planned Follow-Ups
==================
1. Introduce a `CpuRegs` (or similarly named) trait implemented by both
   `CpuState` and the legacy CPU representation, letting execute /
   dispatch be generic.
2. Port handlers and helpers to use the trait instead of the legacy type.
3. Delete any transitional bridge inside `Cpu::step`.
4. Deprecate / remove the legacy monolithic CPU type.

Caveats
=======
Do NOT add new emulator features to this bridging file. Focus on
migrating existing logic out of the legacy path first.

*/

use crate::bus_impl::Bus;
use crate::cpu::state::{CpuState, NEGATIVE, ZERO};

#[derive(Debug, Clone)]
pub struct Cpu {
    state: CpuState,
}

impl Cpu {
    /// Construct a new CPU with power‑up defaults.
    pub fn new() -> Self {
        Self {
            state: CpuState::new(),
        }
    }

    /// Return immutable reference to internal state (for inspection / testing).
    pub fn state(&self) -> &CpuState {
        &self.state
    }

    /// Return mutable reference to internal state (temporary escape hatch).
    pub fn state_mut(&mut self) -> &mut CpuState {
        &mut self.state
    }

    /// Reset internal state and load PC from the reset vector.
    pub fn reset(&mut self, bus: &mut Bus) {
        self.state.reset(bus);
    }

    /// True if execution has been halted (BRK or unknown opcode, current semantics).
    pub fn is_halted(&self) -> bool {
        self.state.halted
    }

    /// Set or clear the halted flag.
    pub fn set_halted(&mut self, h: bool) {
        self.state.halted = h;
    }

    // ---------------------------------------------------------------------
    // Register accessors (read)
    // ---------------------------------------------------------------------
    pub fn a(&self) -> u8 {
        self.state.a
    }
    pub fn x(&self) -> u8 {
        self.state.x
    }
    pub fn y(&self) -> u8 {
        self.state.y
    }
    pub fn sp(&self) -> u8 {
        self.state.sp
    }
    pub fn pc(&self) -> u16 {
        self.state.pc
    }
    pub fn status(&self) -> u8 {
        self.state.status
    }

    // ---------------------------------------------------------------------
    // Register mutators (write)
    // ---------------------------------------------------------------------
    pub fn set_a(&mut self, v: u8) {
        self.state.a = v;
    }
    pub fn set_x(&mut self, v: u8) {
        self.state.x = v;
    }
    pub fn set_y(&mut self, v: u8) {
        self.state.y = v;
    }
    pub fn set_sp(&mut self, v: u8) {
        self.state.sp = v;
    }
    pub fn set_pc(&mut self, v: u16) {
        self.state.pc = v;
    }
    pub fn set_status(&mut self, v: u8) {
        self.state.status = v;
    }

    // ---------------------------------------------------------------------
    // Flag helpers
    // ---------------------------------------------------------------------
    pub fn set_flag(&mut self, mask: u8, on: bool) {
        if on {
            self.state.status |= mask;
        } else {
            self.state.status &= !mask;
        }
    }

    pub fn get_flag(&self, mask: u8) -> bool {
        (self.state.status & mask) != 0
    }

    pub fn update_zn(&mut self, v: u8) {
        self.set_flag(ZERO, v == 0);
        self.set_flag(NEGATIVE, (v & 0x80) != 0);
    }

    // ---------------------------------------------------------------------
    // Bridged step (temporary)
    // ---------------------------------------------------------------------
    /// Execute one instruction using the generic dispatcher (bridge removed).
    ///
    /// Now that all dispatch and execute paths operate on `CpuState` via the
    /// `CpuRegs` trait, this method delegates directly to the generic
    /// dispatcher operating on `CpuState`.
    pub fn step(&mut self, bus: &mut Bus) -> u32 {
        crate::cpu::dispatch::step(&mut self.state, bus)
    }

    /// Convenience: run up to `max_instructions` or until halted.
    pub fn run(&mut self, bus: &mut Bus, max_instructions: usize) {
        for _ in 0..max_instructions {
            if self.is_halted() {
                break;
            }
            self.step(bus);
        }
    }
}

// -------------------------------------------------------------------------
// Conversions
// -------------------------------------------------------------------------

// -------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cartridge::Cartridge;
    use crate::cpu::state::{IRQ_DISABLE, UNUSED};
    use crate::test_utils::build_nrom_with_prg;

    fn setup() -> (Cpu, Bus) {
        let rom = build_nrom_with_prg(&[0xEA], 1, 1, None);
        let cart = Cartridge::from_ines_bytes(&rom).expect("parse");
        let mut bus = Bus::new();
        bus.attach_cartridge(cart);
        let mut cpu = Cpu::new();
        cpu.reset(&mut bus);
        (cpu, bus)
    }

    #[test]
    fn construction_and_reset() {
        let (cpu, _bus) = setup();
        assert_eq!(cpu.sp(), 0xFD);
        assert!(cpu.get_flag(IRQ_DISABLE));
        assert!(cpu.get_flag(UNUSED));
    }

    #[test]
    fn bridge_step_executes_nop() {
        let (mut cpu, mut bus) = setup();
        let pc_before = cpu.pc();
        let cycles = cpu.step(&mut bus);
        assert!(cycles >= 2);
        assert!(cpu.pc() > pc_before);
    }
}
