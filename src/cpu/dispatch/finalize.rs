/*!
finalize.rs - Centralized instruction finalization & trivial/unknown opcode handling.

Overview
========
This module consolidates:
  1. Timing finalization (`finalize_and_tick`) that applies the RMW tick adjustment
     and drives the bus cycle ticking uniformly for all dispatch paths.
  2. A trivial / unknown opcode handler (`handle_trivial_or_unknown`) used by the
     fallback dispatcher for:
        - NOP (0xEA): No state changes besides cycle consumption.
        - Unknown / unimplemented opcodes: Current project semantics set `cpu.halted = true`
          after consuming the baseline cycles.

Rationale
=========
Previously this logic lived in `fallback_final.rs`. As part of Phase A refactor,
it is extracted into a clearly named, dispatcher-adjacent module so both the
match-based fallback path and the (optional) table-driven path can share a single
finalization implementation without duplicating RMW / bus tick semantics.

Responsibilities
================
- Provide `finalize_and_tick`:
    * Given an opcode and its externally reported cycle count, tick the bus for
      either `cycles` or `cycles - 2` (when the opcode is RMW per `is_rmw`).
      The returned value remains the unadjusted `cycles` reported to callers.
- Provide `handle_trivial_or_unknown`:
    * For NOP (0xEA): Do nothing but finalize timing.
    * For any other opcode reaching it: mark CPU halted (unknown) then finalize timing.

Non-Responsibilities
====================
- Does NOT compute base cycle counts (caller must have them already).
- Does NOT apply page-cross or branch penalties (caller must incorporate those
  before calling `finalize_and_tick`).
- Does NOT mutate PC (fetch/advance happens before dispatch reaches here).
- Does NOT attempt to emulate unofficial/undocumented opcode semantics.

Integration Points
==================
Fallback dispatcher (`fallback.rs`):
  - After a family handler returns true: call `finalize_and_tick`.
  - If no family handler claims an opcode:
       return finalize::handle_trivial_or_unknown(opcode, cpu, bus, cycles);

Table-driven dispatcher (future):
  - After determining total cycles for an opcode, call `finalize_and_tick`
    instead of open-coding RMW adjustments.

RMW Adjustment Semantics
========================
RMW opcodes internally perform extra micro-operations (read -> dummy write -> final write).
Externally we continue to report the documented cycle count but only tick `cycles - 2`
so higher-level scheduling stays aligned with expected 6502 timing while still allowing
fine-grained microcycle modeling elsewhere if added later.

Testing
=======
Unit tests verify:
  - NOP returns its cycles and does not halt the CPU.
  - Unknown opcode halts and returns its passed cycle count.
  - RMW adjustment path: a synthetic call emulating an RMW opcode ticks `cycles - 2`.

Future
======
- If undocumented opcode support is added later, this module can expose an
  extension hook (e.g., `handle_extended_opcode`).
- A microcycle trace feature could be layered on top by capturing the
  tick count before/after finalization.

*/

use crate::bus_impl::Bus;
use crate::cpu::cycles::is_rmw;
use crate::cpu::regs::CpuRegs;

/// Apply the unified finalization policy:
/// - Adjust ticked cycles for RMW opcodes (subtract 2).
/// - Tick the bus exactly once for the instruction.
/// - Return the externally visible *original* cycle count.
///
/// Parameters:
/// - `opcode`: The opcode just executed.
/// - `cycles`: Total externally reported cycles (base + any dynamic penalties).
/// - `bus`:    System bus (ticked here).
///
/// Returns:
/// - The same `cycles` value passed in (unmodified).
pub(crate) fn finalize_and_tick(opcode: u8, cycles: u32, bus: &mut Bus) -> u32 {
    let tick_cycles = if is_rmw(opcode) {
        cycles.saturating_sub(2)
    } else {
        cycles
    };
    bus.tick(tick_cycles);
    cycles
}

/// Handle trivial (NOP) or unknown opcodes and finalize timing.
///
/// Assumptions:
/// - PC has already advanced past opcode.
/// - `cycles` is the fully computed external cycle count (base + penalties).
/// - Caller wants a single place to apply RMW adjustment + bus ticking.
///
/// Behavior:
/// - 0xEA (NOP): no state mutation.
/// - Any other opcode: mark CPU halted (unknown/unimplemented).
///
/// Returns:
/// - The externally reported cycle count (unchanged).
pub(crate) fn handle_trivial_or_unknown<C: CpuRegs>(
    opcode: u8,
    cpu: &mut C,
    bus: &mut Bus,
    cycles: u32,
) -> u32 {
    if opcode != 0xEA {
        cpu.set_halted(true);
    }
    finalize_and_tick(opcode, cycles, bus)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cartridge::Cartridge;
    use crate::cpu::core::Cpu;
    use crate::cpu::cycles::base_cycles;
    use crate::test_utils::build_nrom_with_prg;

    fn setup(prg: &[u8]) -> (Cpu, Bus) {
        let rom = build_nrom_with_prg(prg, 1, 1, None);
        let cart = Cartridge::from_ines_bytes(&rom).expect("parse");
        let mut bus = Bus::new();
        bus.attach_cartridge(cart);
        let mut cpu = Cpu::new();
        cpu.reset(&mut bus);
        (cpu, bus)
    }

    #[test]
    fn nop_trivial_not_halted() {
        let (mut cpu, mut bus) = setup(&[0xEA, 0x00]);
        let opcode = bus.read(cpu.pc());
        assert_eq!(opcode, 0xEA);
        cpu.set_pc(cpu.pc().wrapping_add(1));
        let cycles = base_cycles(opcode);
        let reported = handle_trivial_or_unknown(opcode, cpu.state_mut(), &mut bus, cycles);
        assert_eq!(reported, cycles);
        assert!(!cpu.is_halted());
    }

    #[test]
    fn unknown_opcode_halts() {
        let (mut cpu, mut bus) = setup(&[0x02, 0x00]);
        let opcode = bus.read(cpu.pc());
        assert_eq!(opcode, 0x02);
        cpu.set_pc(cpu.pc().wrapping_add(1));
        let cycles = base_cycles(opcode); // default 2
        let reported = handle_trivial_or_unknown(opcode, cpu.state_mut(), &mut bus, cycles);
        assert_eq!(reported, cycles);
        assert!(cpu.is_halted());
    }

    #[test]
    fn rmw_adjustment_ticks_cycles_minus_two() {
        // Use an RMW opcode (e.g., 0xE6 INC zp).
        let (mut cpu, mut bus) = setup(&[0xE6, 0x10, 0x00]);
        let opcode = bus.read(cpu.pc());
        assert_eq!(opcode, 0xE6);
        cpu.set_pc(cpu.pc().wrapping_add(1));
        let cycles = base_cycles(opcode); // 5
        // We simulate "post-execution" finalization only; no actual INC performed here.
        let before_ticks = bus.total_ticks();
        let reported = finalize_and_tick(opcode, cycles, &mut bus);
        let after_ticks = bus.total_ticks();
        assert_eq!(reported, cycles);
        assert_eq!(after_ticks - before_ticks, (cycles - 2) as u64);
    }
}
