/*!
regs.rs - CpuRegs trait (Phase B migration) providing a minimal, generic
register + flag manipulation interface for 6502 execution / dispatch.

Option A (Chosen):
==================
The trait does NOT include:
  - Stack push/pop
  - Instruction fetch helpers (fetch_u8/fetch_u16)
  - Bus access of any kind

Rationale: Keeps the trait focused purely on architectural register /
status semantics. Memory, stack, and fetch operations remain explicit
at call sites via `&mut Bus` to avoid over-borrowing and to keep trait
implementations simple. If later ergonomics justify it, stack or fetch
helpers can be added (non-breaking if provided with default methods).

Implementations Provided:
=========================
- CpuRegs for `CpuState` (the canonical state owner)
- CpuRegs for legacy compatibility types (enabling incremental migration)
  This allows execution / dispatch code to become generic without
  immediately deleting legacy representations.

Design Goals:
=============
1. Small surface area: only what existing instruction helpers require.
2. Static dispatch via generics (no trait objects) for zero overhead in
   hot paths.
3. Default methods for composites (advance_pc_one, update_zn, etc.)
   reduce duplication across implementors.
4. Method names mirror existing `CpuState` methods to keep refactors
   mechanical (search/replace-friendly).

Migration Usage Pattern:
========================
Before:
    fn lda(cpu: &mut legacy concrete CPU type, v: u8) { cpu.a = v; update_zn(cpu, cpu.a); }

After (example pseudo):
    fn lda<C: CpuRegs>(cpu: &mut C, v: u8) {
        cpu.set_a(v);
        cpu.update_zn(cpu.a());
    }

Post-Migration Cleanup:
=======================
When all code depends only on the trait:
  - Remove direct field accesses to legacy concrete CPU representations.
  - Make legacy CPU types a façade (type alias / re-export) or remove them.
  - Optionally add stack / fetch helpers to the trait (evaluate need).

*/

use crate::cpu::state::{BREAK, CARRY, CpuState, NEGATIVE, OVERFLOW, UNUSED, ZERO};

/// Trait exposing the minimal 6502 CPU architectural register + flag API
/// needed by instruction semantic and dispatch code.
///
/// Intentionally excludes:
///   * Bus access
///   * Stack push/pop
///   * Instruction fetch
///
/// Additions should be justified by repeated cross-cutting usage patterns
/// to avoid trait bloat.
///
/// ALL mutating methods take &mut self, enabling generic call sites:
///   fn op<T: CpuRegs>(cpu: &mut T) { ... }
pub trait CpuRegs {
    // ---------------------------------------------------------------------
    // Read accessors
    // ---------------------------------------------------------------------
    fn a(&self) -> u8;
    fn x(&self) -> u8;
    fn y(&self) -> u8;
    fn sp(&self) -> u8;
    fn pc(&self) -> u16;
    fn status(&self) -> u8;
    fn halted(&self) -> bool;

    // ---------------------------------------------------------------------
    // Mutators
    // ---------------------------------------------------------------------
    fn set_a(&mut self, v: u8);
    fn set_x(&mut self, v: u8);
    fn set_y(&mut self, v: u8);
    fn set_sp(&mut self, v: u8);
    fn set_pc(&mut self, v: u16);
    fn set_status(&mut self, v: u8);
    fn set_halted(&mut self, h: bool);

    // ---------------------------------------------------------------------
    // Program Counter helpers
    // ---------------------------------------------------------------------

    /// Advance PC by `delta` (wrapping at 16 bits).
    fn advance_pc(&mut self, delta: u16);

    /// Advance PC by 1. Default implemented via `advance_pc(1)`.
    #[inline]
    fn advance_pc_one(&mut self) {
        self.advance_pc(1);
    }

    // ---------------------------------------------------------------------
    // Flag operations
    // ---------------------------------------------------------------------

    /// Return true if mask bits are set.
    fn is_flag_set(&self, mask: u8) -> bool;

    /// Assign specific flag bits based on boolean `value` (set or clear).
    fn assign_flag(&mut self, mask: u8, value: bool);

    /// Composite: update ZERO and NEGATIVE based on result.
    #[inline]
    fn update_zn(&mut self, result: u8) {
        self.assign_flag(ZERO, result == 0);
        self.assign_flag(NEGATIVE, (result & 0x80) != 0);
    }

    /// Composite: update CARRY from bool.
    #[inline]
    fn update_carry(&mut self, carry: bool) {
        self.assign_flag(CARRY, carry);
    }

    /// Composite: update OVERFLOW from bool.
    #[inline]
    fn update_overflow(&mut self, overflow: bool) {
        self.assign_flag(OVERFLOW, overflow);
    }

    /// Compose processor status byte for a stack push (PHP / BRK vs IRQ/NMI).
    /// - UNUSED bit forced set
    /// - BREAK bit included only when `set_break` is true
    #[inline]
    fn compose_status_for_push(&self, set_break: bool) -> u8 {
        let mut v = self.status() | UNUSED;
        if set_break {
            v |= BREAK;
        } else {
            v &= !BREAK;
        }
        v
    }
}

// -------------------------------------------------------------------------
// Implementation: CpuState (canonical)
// -------------------------------------------------------------------------

impl CpuRegs for CpuState {
    #[inline]
    fn a(&self) -> u8 {
        self.a()
    }
    #[inline]
    fn x(&self) -> u8 {
        self.x()
    }
    #[inline]
    fn y(&self) -> u8 {
        self.y()
    }
    #[inline]
    fn sp(&self) -> u8 {
        self.sp()
    }
    #[inline]
    fn pc(&self) -> u16 {
        self.pc()
    }
    #[inline]
    fn status(&self) -> u8 {
        self.status()
    }
    #[inline]
    fn halted(&self) -> bool {
        self.halted()
    }

    #[inline]
    fn set_a(&mut self, v: u8) {
        self.set_a(v);
    }
    #[inline]
    fn set_x(&mut self, v: u8) {
        self.set_x(v);
    }
    #[inline]
    fn set_y(&mut self, v: u8) {
        self.set_y(v);
    }
    #[inline]
    fn set_sp(&mut self, v: u8) {
        self.set_sp(v);
    }
    #[inline]
    fn set_pc(&mut self, v: u16) {
        self.set_pc(v);
    }
    #[inline]
    fn set_status(&mut self, v: u8) {
        self.set_status(v);
    }
    #[inline]
    fn set_halted(&mut self, h: bool) {
        self.set_halted(h);
    }

    #[inline]
    fn advance_pc(&mut self, delta: u16) {
        self.advance_pc(delta);
    }

    #[inline]
    fn is_flag_set(&self, mask: u8) -> bool {
        self.is_flag_set(mask)
    }

    #[inline]
    fn assign_flag(&mut self, mask: u8, value: bool) {
        self.assign_flag(mask, value);
    }

    // update_zn / update_carry / update_overflow use default implementations
}
// Migration note: legacy compatibility implementations and parity tests were removed during the CpuState / CpuRegs migration.
