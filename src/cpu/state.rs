/*!
state.rs - Core 6502 CPU state (registers, flags) plus basic helpers.

This module is the first extracted piece of the former monolithic `cpu6502.rs`.
It intentionally contains *only* the pure state representation and simple
flag / register utilities so that higher-level concerns (addressing, execution,
dispatch) can be migrated incrementally without churn.

During the transition:
- The legacy implementation (`crate::cpu6502`) still defines its own
  register fields and flag constants. Duplication is acceptable short‑term
  because this module names are all scoped under `crate::cpu::state`.
- Once the migration is complete, the legacy file will be removed and
  the rest of the CPU will depend on this canonical definition.

Design notes:
- Keeping flags as public `const` values (not an enum) matches the original
  style and avoids bit pattern conversions.
- A distinct `CpuState` struct isolates raw machine state from execution
  logic, enabling easier unit testing and potential future features
  like state snapshots, rewind, or serialization.
- Helper methods are small, inline-friendly and side-effect free
  (except for register mutation), making them suitable for reuse by both
  legacy and new dispatch paths.

Planned future additions here (post-migration):
- Pack / unpack helpers if a trace encoding is added.
- Optional trait implementations for state diffing / hashing.
*/

use crate::bus::Bus;

/// Processor status flag bit masks (mirrors legacy constants).
pub const CARRY: u8 = 0b0000_0001;
pub const ZERO: u8 = 0b0000_0010;
pub const IRQ_DISABLE: u8 = 0b0000_0100;
pub const DECIMAL: u8 = 0b0000_1000; // Unused by NES hardware but still a 6502 bit.
pub const BREAK: u8 = 0b0001_0000;
pub const UNUSED: u8 = 0b0010_0000; // Always reads as 1 on the 6502.
pub const OVERFLOW: u8 = 0b0100_0000;
pub const NEGATIVE: u8 = 0b1000_0000;

/// Core register/state snapshot of the 6502 CPU.
#[derive(Debug, Clone, Copy)]
pub struct CpuState {
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub sp: u8,
    pub pc: u16,
    pub status: u8,
    pub halted: bool, // Convenience flag used by the legacy core (e.g. after BRK or unknown opcode).
}

impl Default for CpuState {
    fn default() -> Self {
        // Power‑up / reset defaults: I (IRQ disable) and UNUSED bits set; SP to 0xFD.
        Self {
            a: 0,
            x: 0,
            y: 0,
            sp: 0xFD,
            pc: 0x0000,
            status: IRQ_DISABLE | UNUSED,
            halted: false,
        }
    }
}

impl CpuState {
    /// Construct a new CPU state with power‑up defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset CPU registers and load PC from the reset vector ($FFFC/$FFFD).
    /// Mirrors behavior in the legacy core so that downstream logic can
    /// transition seamlessly.
    pub fn reset(&mut self, bus: &mut Bus) {
        *self = Self::default();
        self.pc = bus.read_word(0xFFFC);
    }

    /// Set or clear a specific processor status flag.
    #[inline]
    pub fn set_flag(&mut self, mask: u8, on: bool) {
        if on {
            self.status |= mask;
        } else {
            self.status &= !mask;
        }
    }

    /// Query whether a flag is set.
    #[inline]
    pub fn get_flag(&self, mask: u8) -> bool {
        (self.status & mask) != 0
    }

    /// Update Zero and Negative flags based on a value.
    #[inline]
    pub fn update_zn(&mut self, v: u8) {
        self.set_flag(ZERO, v == 0);
        self.set_flag(NEGATIVE, (v & 0x80) != 0);
    }

    /// Push the current processor status onto the stack (with control over the Break flag).
    ///
    /// Semantics:
    /// - Bit 5 (UNUSED) is forced high.
    /// - The Break flag is included only if `set_break_on_push` is true (BRK / PHP),
    ///   and cleared for hardware IRQ / NMI pushes.
    ///
    /// This is provided here so that both legacy and refactored interrupt logic
    /// can share consistent status byte construction.
    pub fn compose_status_for_push(&self, set_break_on_push: bool) -> u8 {
        let mut v = self.status | UNUSED;
        if set_break_on_push {
            v |= BREAK;
        } else {
            v &= !BREAK;
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cartridge::Cartridge;
    use crate::test_utils::build_nrom_with_prg;

    #[test]
    fn default_matches_expected_power_up() {
        let s = CpuState::new();
        assert_eq!(s.a, 0);
        assert_eq!(s.x, 0);
        assert_eq!(s.y, 0);
        assert_eq!(s.sp, 0xFD);
        assert_eq!(s.status & (IRQ_DISABLE | UNUSED), IRQ_DISABLE | UNUSED);
        assert!(!s.halted);
    }

    #[test]
    fn reset_loads_vector() {
        // Build a minimal ROM with reset vector pointing to $C123.
        let reset_target: u16 = 0xC123;
        let mut prg = vec![0xEA]; // single NOP just to have a byte
        // Pad PRG to ensure we can place reset vector bytes in last 6 bytes of 16KiB bank
        // (test_utils helper handles vector patch normally; we replicate minimal path).
        // Instead rely on helper to do the correct vector installation:
        let rom = build_nrom_with_prg(&prg, 1, 1, Some(reset_target));
        let cart = Cartridge::from_ines_bytes(&rom).expect("parse cartridge");
        let mut bus = Bus::new();
        bus.attach_cartridge(cart);

        let mut s = CpuState::new();
        s.pc = 0x0000;
        s.reset(&mut bus);
        assert_eq!(s.pc, reset_target);
    }

    #[test]
    fn update_zn_sets_zero_and_negative() {
        let mut s = CpuState::new();
        s.update_zn(0);
        assert!(s.get_flag(ZERO));
        assert!(!s.get_flag(NEGATIVE));

        s.update_zn(0x80);
        assert!(!s.get_flag(ZERO));
        assert!(s.get_flag(NEGATIVE));
    }

    #[test]
    fn compose_status_break_behavior() {
        let s = CpuState::new();
        let with_break = s.compose_status_for_push(true);
        let without_break = s.compose_status_for_push(false);
        assert_ne!(with_break & BREAK, 0);
        assert_eq!(without_break & BREAK, 0);
        // Bit 5 always set
        assert_ne!(with_break & UNUSED, 0);
        assert_ne!(without_break & UNUSED, 0);
    }
}
