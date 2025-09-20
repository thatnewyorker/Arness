/*!
fallback.rs - Match-based fallback opcode dispatcher

Overview
========
Provides the match-based execution path used when:
- The table-driven dispatcher (if enabled) does not handle the current opcode.
- The orchestrator (`dispatch::step`) has already processed DMA stalls and interrupts.

Responsibilities
================
1. Fetch the opcode and advance PC.
2. Derive baseline cycle count via `base_cycles(opcode)`.
3. Invoke extracted opcode family handlers (load/store, logical, arithmetic, compare, branches, rmw, control_flow, misc):
   - Handlers may mutate `cycles` (page-cross penalties, BRK override, etc.).
   - Handlers must NOT tick the bus.
4. If no handler claims the opcode, delegate to `finalize::handle_trivial_or_unknown` (NOP, unknown -> halt).

Finalization
============
Finalization:
- Delegated to `finalize::finalize_and_tick` for opcodes handled by earlier family handlers.
- Trivial / unknown opcodes use `finalize::handle_trivial_or_unknown` (which also finalizes).

Behavior Notes
==============
- Unknown or unimplemented opcodes set `cpu.halted = true`.
- Page-cross (+1) penalties are applied only by page-cross aware addressing helpers inside family handlers.
- BRK semantics (status push, vector fetch, fixed 7 cycles, halt) reside in the control_flow handler.

This module is intentionally minimal; as table coverage grows, its usage will diminish.
*/

use crate::bus_impl::Bus;
use crate::cpu::regs::CpuRegs;
// Shared modular helpers
use crate::cpu::cycles::base_cycles;
use crate::cpu::dispatch::finalize::{finalize_and_tick, handle_trivial_or_unknown};
use crate::cpu::execute::{dex, dey, inx, iny};
/// Finalization now delegated to `fallback_final` (no local duplicate helper).

/// Execute one instruction using the match-based fallback dispatcher (post-interrupt / DMA already handled).
/// Returns the total cycles consumed (including penalties).
pub(crate) fn step<C: CpuRegs>(cpu: &mut C, bus: &mut Bus) -> u32 {
    // Fetch opcode (interrupts & DMA already handled by orchestrator)
    let opcode = bus.read(cpu.pc());
    cpu.advance_pc_one();

    let mut cycles = base_cycles(opcode);
    // Early dispatch: extracted families (load/store, logical, arithmetic, compare, branches, rmw, control_flow, misc)
    if super::load_store::handle(opcode, cpu, bus, &mut cycles)
        || super::logical::handle(opcode, cpu, bus, &mut cycles)
        || super::arithmetic::handle(opcode, cpu, bus, &mut cycles)
        || super::compare::handle(opcode, cpu, bus, &mut cycles)
        || super::branches::handle(opcode, cpu, bus, &mut cycles)
        || super::rmw::handle(opcode, cpu, bus, &mut cycles)
        || super::control_flow::handle(opcode, cpu, bus, &mut cycles)
        || super::misc::handle(opcode, cpu, bus, &mut cycles)
    {
        // Finalize via shared helper in finalize (removes local duplication)
        return finalize_and_tick(opcode, cycles, bus);
    }

    // Legacy giant match dispatcher
    match opcode {
        // (Transfers and Stack opcodes extracted to dispatch::misc::handle)

        // ------ Increment / Decrement ------
        0xE8 => inx(cpu),
        0xC8 => iny(cpu),
        0xCA => dex(cpu),
        0x88 => dey(cpu),

        // (INC/DEC memory opcodes extracted to dispatch::rmw::handle)

        // ------ Logical: AND ------
        // ------ Shifts / Rotates (Accumulator & Memory) ------
        // (Shift / Rotate accumulator + memory and INC/DEC opcodes extracted to dispatch::rmw::handle)

        // (Flag opcodes extracted to dispatch::misc::handle)

        // (Compare opcodes extracted to dispatch::compare::handle)

        // (Branch opcodes extracted to dispatch::branches::handle)

        // (Control-flow opcodes (JMP/JSR/RTS/RTI/BRK) extracted to dispatch::control_flow::handle)

        // ------ NOP ------
        0xEA => {
            // Delegate NOP to centralized trivial handler (ticks bus & returns cycles)
            return handle_trivial_or_unknown(opcode, cpu, bus, cycles);
        }

        // ------ Unknown / Unimplemented ------
        _ => {
            // Delegate unknown opcode handling (halts + ticks)
            return handle_trivial_or_unknown(opcode, cpu, bus, cycles);
        }
    }

    // Finalize and tick (RMW adjustment handled inside helper)

    finalize_and_tick(opcode, cycles, bus)
}

// All helper functionality is centralized in cpu::addressing and cpu::execute.

// (Helper implementations removed; dispatcher now uses shared modules: addressing.rs & execute.rs)
#[inline]
fn add_page_cross_penalty(cycles: &mut u32, crossed: bool) {
    if crossed {
        *cycles += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cartridge::Cartridge;
    use crate::cpu::core::Cpu;
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
    fn lda_abs_x_page_cross_cycles_match() {
        // Program: LDX #$01; LDA $12FF,X; BRK
        let (mut cpu, mut bus) = setup(&[0xA2, 0x01, 0xBD, 0xFF, 0x12, 0x00]);
        // LDX
        let c1 = cpu.step(&mut bus);
        assert_eq!(c1, 2);
        // LDA abs,X page cross
        let c2 = cpu.step(&mut bus);
        assert_eq!(c2, 5);
    }

    #[test]
    fn branch_taken_page_cross_cycles() {
        // Place branch near page boundary to force crossing
        let mut prg = vec![];
        prg.extend(std::iter::repeat(0xEA).take(0x00FF)); // fill to $80FF with NOP
        prg.push(0x18); // CLC
        prg.push(0x90); // BCC
        prg.push(0x01); // +1 -> crosses
        prg.push(0xEA);
        prg.push(0x00); // BRK
        let (mut cpu, mut bus) = setup(&prg);
        for _ in 0..0x00FF {
            assert_eq!(cpu.step(&mut bus), 2);
        }
        assert_eq!(cpu.step(&mut bus), 2); // CLC
        assert_eq!(cpu.step(&mut bus), 4); // BCC taken + page cross
    }
}
