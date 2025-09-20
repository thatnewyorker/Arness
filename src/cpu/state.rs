/*!
state.rs - Canonical 6502 CPU architectural state (registers + flags) and
inline-friendly helpers.

Overview
========
`CpuState` is the single authoritative owner for all architecturally visible
registers and execution control booleans. It intentionally excludes:
  - Bus / memory logic
  - Instruction decode / dispatch logic
  - Timing / cycle accounting
Those live in higher layers (dispatch, execute, bus modules).

Rationale
=========
Centralizing state eliminates duplication with legacy monolithic CPU representations and
enables:
  * Trait-based register / flag access (planned `CpuRegs`)
  * Cheap state snapshotting (e.g. for rewind or debugger)
  * Cleaner unit tests for pure flag and register semantics
  * Future serialization / tracing hooks

Migration Plan Notes
====================
Phase B steps will:
  1. Keep this file the single source of flag mask constants.
  2. Introduce a `CpuRegs` trait mapping almost 1:1 to the accessor/mutator
     methods defined here.
  3. Convert execution + dispatch code to use `impl CpuRegs`.
  4. Remove direct field access to legacy concrete CPU representations.
  5. Optionally make legacy CPU types a façade re-export of `CpuState`.

Constants Re-Export Plan
========================
`cpu::mod` should `pub use` the flag constants from here. After migration,
remove any duplicated constants elsewhere and update imports.

Design Choices
==============
- Methods are small, inlinable, and side-effect isolated.
- Public setters do not mask bits; higher layers enforce invariants.
- Flag helpers segregated into atomic operations (`set_flag`, `clear_flag`,
  `assign_flag`) plus convenience composites (`update_zn`).
- Stack helpers accept a mutable Bus reference (write/read) and implement
  authentic 6502 push/pop sequence semantics.

6502 Status Register Bit Layout (for reference)
===============================================
Bit: 7 6 5 4 3 2 1 0
     N V 1 B D I Z C
Where:
  N = NEGATIVE
  V = OVERFLOW
  1 = UNUSED (always reads as 1)
  B = BREAK (PHP/BRK only; hardware IRQ/NMI push with B clear)
  D = DECIMAL (unused on NES but still toggled by instructions / flags)
  I = IRQ_DISABLE
  Z = ZERO
  C = CARRY
*/

use crate::bus_impl::Bus;

/// Processor status flag bit masks (canonical definitions).
pub const CARRY: u8 = 0b0000_0001;
pub const ZERO: u8 = 0b0000_0010;
pub const IRQ_DISABLE: u8 = 0b0000_0100;
pub const DECIMAL: u8 = 0b0000_1000; // Not used by NES hardware, still part of 6502.
pub const BREAK: u8 = 0b0001_0000;
pub const UNUSED: u8 = 0b0010_0000; // Always set when read.
pub const OVERFLOW: u8 = 0b0100_0000;
pub const NEGATIVE: u8 = 0b1000_0000;

/// Pure architectural register / flag container for the 6502 CPU.
///
/// Fields are kept `pub(crate)` (or private by accessor methods) in future
/// if stricter encapsulation becomes desirable; currently exposed for
/// migration ease. Prefer method access over direct field mutation.
#[derive(Debug, Clone, Copy)]
pub struct CpuState {
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub sp: u8,
    pub pc: u16,
    pub status: u8,
    pub halted: bool, // Execution halted (e.g. after BRK or intentional stop)
}

impl Default for CpuState {
    fn default() -> Self {
        // Common 6502 reset defaults: SP=0xFD, IRQ disabled, UNUSED bit set.
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
    // ---------------------------------------------------------------------
    // Construction / Reset
    // ---------------------------------------------------------------------

    /// Create a new CPU state using power-up defaults.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset registers and load PC from the reset vector at $FFFC/$FFFD.
    ///
    /// NOTE: This calls into the bus to fetch the vector; bus must supply
    /// correct mapping and mirroring semantics.
    pub fn reset(&mut self, bus: &mut Bus) {
        *self = Self::default();
        self.pc = bus.read_word(0xFFFC);
    }

    // ---------------------------------------------------------------------
    // Basic Accessors (Read)
    // ---------------------------------------------------------------------
    #[inline]
    pub fn a(&self) -> u8 {
        self.a
    }
    #[inline]
    pub fn x(&self) -> u8 {
        self.x
    }
    #[inline]
    pub fn y(&self) -> u8 {
        self.y
    }
    #[inline]
    pub fn sp(&self) -> u8 {
        self.sp
    }
    #[inline]
    pub fn pc(&self) -> u16 {
        self.pc
    }
    #[inline]
    pub fn status(&self) -> u8 {
        self.status
    }
    #[inline]
    pub fn halted(&self) -> bool {
        self.halted
    }

    // ---------------------------------------------------------------------
    // Mutators (Write)
    // ---------------------------------------------------------------------
    #[inline]
    pub fn set_a(&mut self, v: u8) {
        self.a = v;
    }
    #[inline]
    pub fn set_x(&mut self, v: u8) {
        self.x = v;
    }
    #[inline]
    pub fn set_y(&mut self, v: u8) {
        self.y = v;
    }
    #[inline]
    pub fn set_sp(&mut self, v: u8) {
        self.sp = v;
    }
    #[inline]
    pub fn set_pc(&mut self, v: u16) {
        self.pc = v;
    }
    #[inline]
    pub fn set_status(&mut self, v: u8) {
        self.status = v;
    }
    #[inline]
    pub fn set_halted(&mut self, h: bool) {
        self.halted = h;
    }

    // ---------------------------------------------------------------------
    // Program Counter Helpers
    // ---------------------------------------------------------------------

    /// Advance PC by `delta` (wrapping at 16 bits).
    #[inline]
    pub fn advance_pc(&mut self, delta: u16) {
        self.pc = self.pc.wrapping_add(delta);
    }

    /// Advance PC by 1 (common path).
    #[inline]
    pub fn advance_pc_one(&mut self) {
        self.advance_pc(1);
    }

    /// Fetch a byte from memory at current PC and then advance PC by 1.
    ///
    /// This localizes a very common pattern: read then increment PC.
    #[inline]
    pub fn fetch_u8(&mut self, bus: &mut Bus) -> u8 {
        let b = bus.read(self.pc);
        self.advance_pc_one();
        b
    }

    /// Fetch a little-endian 16-bit word (low then high) from current PC and
    /// advance PC by 2.
    #[inline]
    pub fn fetch_u16(&mut self, bus: &mut Bus) -> u16 {
        let lo = self.fetch_u8(bus) as u16;
        let hi = self.fetch_u8(bus) as u16;
        (hi << 8) | lo
    }

    // ---------------------------------------------------------------------
    // Flag Operations
    // ---------------------------------------------------------------------

    /// Return true if a status flag (bit mask) is set.
    #[inline]
    pub fn is_flag_set(&self, mask: u8) -> bool {
        (self.status & mask) != 0
    }

    /// Set a flag bit (OR).
    #[inline]
    pub fn set_flag_bit(&mut self, mask: u8) {
        self.status |= mask;
    }

    /// Clear a flag bit (AND NOT).
    #[inline]
    pub fn clear_flag_bit(&mut self, mask: u8) {
        self.status &= !mask;
    }

    /// Assign a flag bit based on boolean `value`.
    #[inline]
    pub fn assign_flag(&mut self, mask: u8, value: bool) {
        if value {
            self.set_flag_bit(mask);
        } else {
            self.clear_flag_bit(mask);
        }
    }

    /// Composite helper to update ZERO + NEGATIVE according to 6502 rules.
    #[inline]
    pub fn update_zn(&mut self, result: u8) {
        self.assign_flag(ZERO, result == 0);
        self.assign_flag(NEGATIVE, (result & 0x80) != 0);
    }

    /// Helper for setting carry from a boolean (e.g. shift/rotate).
    #[inline]
    pub fn update_carry(&mut self, carry: bool) {
        self.assign_flag(CARRY, carry);
    }

    /// Helper for overflow (commonly used in ADC/SBC).
    #[inline]
    pub fn update_overflow(&mut self, overflow: bool) {
        self.assign_flag(OVERFLOW, overflow);
    }

    /// Compose the status byte for pushing to stack (BRK/PHP vs. IRQ/NMI).
    ///
    /// - Bit 5 (UNUSED) always forced to 1.
    /// - BREAK bit included only if `set_break_on_push` = true.
    pub fn compose_status_for_push(&self, set_break_on_push: bool) -> u8 {
        let mut v = self.status | UNUSED;
        if set_break_on_push {
            v |= BREAK;
        } else {
            v &= !BREAK;
        }
        v
    }

    // ---------------------------------------------------------------------
    // Stack Helpers
    // ---------------------------------------------------------------------
    //
    // 6502 stack is located on page 0x0100, with SP post-decrement on push
    // and pre-increment on pull:
    //   Push: write at 0x0100 | SP, then SP = SP - 1
    //   Pull: SP = SP + 1, then read at 0x0100 | SP

    /// Push a byte onto the stack.
    #[inline]
    pub fn push_u8(&mut self, bus: &mut Bus, value: u8) {
        let addr = 0x0100u16 | (self.sp as u16);
        bus.write(addr, value);
        self.sp = self.sp.wrapping_sub(1);
    }

    /// Pull (pop) a byte from the stack.
    #[inline]
    pub fn pop_u8(&mut self, bus: &mut Bus) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        let addr = 0x0100u16 | (self.sp as u16);
        bus.read(addr)
    }

    /// Push 16-bit word high first? (NOT how 6502 does it) — we provide only
    /// the authentic low/high layering: push high, then low? Actually 6502
    /// for JSR pushes high then low. Provide a helper mirroring that usage.
    ///
    /// JSR push order (PC-1 high, then PC-1 low):
    ///   push( (pc - 1 >> 8) )
    ///   push( (pc - 1 & 0xFF) )
    /// Instead of encoding that policy here, keep generic word helpers:
    #[inline]
    pub fn push_u16_le(&mut self, bus: &mut Bus, value: u16) {
        // Little-endian order: high byte second on stack retrieval = push high first? 6502 pushes high then low for return addresses.
        let hi = (value >> 8) as u8;
        let lo = value as u8;
        self.push_u8(bus, hi);
        self.push_u8(bus, lo);
    }

    #[inline]
    pub fn pop_u16_le(&mut self, bus: &mut Bus) -> u16 {
        let lo = self.pop_u8(bus) as u16;
        let hi = self.pop_u8(bus) as u16;
        (hi << 8) | lo
    }

    // ---------------------------------------------------------------------
    // Misc / Convenience
    // ---------------------------------------------------------------------

    /// Mark CPU halted.
    #[inline]
    pub fn halt(&mut self) {
        self.halted = true;
    }

    /// Clear halted state.
    #[inline]
    pub fn resume(&mut self) {
        self.halted = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cartridge::Cartridge;
    use crate::test_utils::{build_nrom_with_prg, build_nrom_with_prg_reset_only};

    fn build_bus_with_reset_vector(target: u16) -> Bus {
        let prg = vec![0xEA]; // NOP filler
        let rom = build_nrom_with_prg_reset_only(&prg, 1, 1, Some(target));
        let cart = Cartridge::from_ines_bytes(&rom).expect("parse cart");
        let mut bus = Bus::new();
        bus.attach_cartridge(cart);
        bus
    }

    #[test]
    fn default_power_up() {
        let s = CpuState::new();
        assert_eq!(s.a(), 0);
        assert_eq!(s.x(), 0);
        assert_eq!(s.y(), 0);
        assert_eq!(s.sp(), 0xFD);
        assert!(s.is_flag_set(IRQ_DISABLE));
        assert!(s.is_flag_set(UNUSED));
        assert!(!s.halted());
    }

    #[test]
    fn reset_sets_pc_from_vector() {
        let target = 0xC123;
        let mut bus = build_bus_with_reset_vector(target);
        let mut s = CpuState::new();
        s.reset(&mut bus);
        assert_eq!(s.pc(), target);
    }

    #[test]
    fn flag_assignment() {
        let mut s = CpuState::new();
        s.clear_flag_bit(IRQ_DISABLE);
        assert!(!s.is_flag_set(IRQ_DISABLE));
        s.set_flag_bit(IRQ_DISABLE);
        assert!(s.is_flag_set(IRQ_DISABLE));
        s.assign_flag(DECIMAL, true);
        assert!(s.is_flag_set(DECIMAL));
        s.assign_flag(DECIMAL, false);
        assert!(!s.is_flag_set(DECIMAL));
    }

    #[test]
    fn update_zn_behavior() {
        let mut s = CpuState::new();
        s.update_zn(0x00);
        assert!(s.is_flag_set(ZERO));
        assert!(!s.is_flag_set(NEGATIVE));
        s.update_zn(0x80);
        assert!(!s.is_flag_set(ZERO));
        assert!(s.is_flag_set(NEGATIVE));
        s.update_zn(0x7F);
        assert!(!s.is_flag_set(ZERO));
        assert!(!s.is_flag_set(NEGATIVE));
    }

    #[test]
    fn pc_advance_wraps() {
        let mut s = CpuState::new();
        s.set_pc(0xFFFF);
        s.advance_pc_one();
        assert_eq!(s.pc(), 0x0000);
        s.advance_pc(2);
        assert_eq!(s.pc(), 0x0002);
    }

    #[test]
    fn stack_push_pop_round_trip() {
        let rom = build_nrom_with_prg(&[0xEA], 1, 1, None);
        let cart = Cartridge::from_ines_bytes(&rom).unwrap();
        let mut bus = Bus::new();
        bus.attach_cartridge(cart);
        let mut s = CpuState::new();
        let original_sp = s.sp();
        s.push_u8(&mut bus, 0xAB);
        s.push_u8(&mut bus, 0xCD);
        assert_ne!(s.sp(), original_sp);
        let v1 = s.pop_u8(&mut bus);
        let v0 = s.pop_u8(&mut bus);
        assert_eq!(v1, 0xCD);
        assert_eq!(v0, 0xAB);
        assert_eq!(s.sp(), original_sp);
    }

    #[test]
    fn compose_status_break_flag_behavior() {
        let s = CpuState::new();
        let with_break = s.compose_status_for_push(true);
        let without_break = s.compose_status_for_push(false);
        assert_ne!(with_break & BREAK, 0);
        assert_eq!(without_break & BREAK, 0);
        assert_ne!(with_break & UNUSED, 0);
        assert_ne!(without_break & UNUSED, 0);
    }

    #[test]
    fn fetch_helpers_advance_pc() {
        let mut bus = build_bus_with_reset_vector(0x8000);
        // Build a small ROM with predictable bytes at $8000+
        // test_utils helper loaded NOP; we manually place more bytes if needed
        let mut s = CpuState::new();
        s.reset(&mut bus);
        let pc_start = s.pc();
        let _b = s.fetch_u8(&mut bus);
        assert_eq!(s.pc(), pc_start.wrapping_add(1));
    }
}
