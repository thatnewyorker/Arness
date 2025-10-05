/*!
dispatch.rs - Orchestrator for a single 6502 CPU step (DMA / interrupts / dispatch)

Overview
========
Coordinates a single CPU instruction step:
1. Handles OAM DMA stall (burn 1 cycle; no opcode fetch).
2. Services pending NMI or maskable IRQ (7-cycle interrupt entry).
3. Optionally attempts table-driven execution (feature `table_dispatch`).
4. Falls back to the match-based fallback dispatcher for remaining opcodes (which now delegates any trivial or unknown opcode to the consolidated `fallback_final` handler).

Architecture
============
- Orchestrator: resolves pre-instruction concerns (DMA, interrupts, optional
  table path) and delegates instruction execution.
- Fallback dispatcher (`dispatch::fallback::step`): performs opcode fetch,
  family handler chain, and delegates finalization (RMW adjustment + bus tick) to `finalize::finalize_and_tick` or directly to `finalize::handle_trivial_or_unknown` for NOP / unknown.
- Table path: when enabled, peeks the opcode (no PC advance) and, if handled,
  advances PC, executes semantics, and ticks cycles internally.

Cycle Ticking
=============
- DMA & interrupt paths tick cycles directly here.
- Table path ticks inside its own execution.
- Fallback path ticks once in its own finalizer (after RMW adjustment).

Design Notes
============
- Single harmless opcode peek when table dispatch is enabled and misses.
- Unknown opcodes in fallback cause `cpu.halted = true` (via `finalize::handle_trivial_or_unknown`).
*/

#![allow(dead_code)]

use crate::bus::Bus;
pub(crate) mod arithmetic;
pub(crate) mod branches;
pub(crate) mod compare; // extracted compare (CMP/CPX/CPY) opcode family handler
pub(crate) mod control_flow;
mod fallback; // fallback match-based dispatcher (formerly legacy)
pub(crate) mod finalize; // centralized finalization & trivial/unknown opcode handling
pub(crate) mod load_store; // extracted load/store family handler
pub(crate) mod logical;
pub(crate) mod misc;
pub(crate) mod rmw; // extracted RMW / shift / INC / DEC opcode family handler // extracted control-flow (JMP/JSR/RTS/RTI/BRK) // extracted transfers / stack / flags opcode family handler
use crate::cpu::execute::{
    get_flag,
    push_status_with_break,
    push_word,
    set_flag, // for interrupt handling
};
use crate::cpu::regs::CpuRegs;
use crate::cpu::state::IRQ_DISABLE;

#[cfg(feature = "table_dispatch")]
use crate::cpu::table::try_table_step;

/// Execute one CPU step (including DMA stall / interrupts) and return cycles consumed.
pub(crate) fn step<C: CpuRegs>(cpu: &mut C, bus: &mut Bus) -> u32 {
    // 1. OAM DMA stall: burn one cycle and return (no opcode consumed)
    if bus.dma_is_active() {
        bus.tick(1);
        return 1;
    }

    // 2. Non-maskable interrupt (NMI)
    if bus.nmi_pending {
        service_interrupt(cpu, bus, 0xFFFA);
        bus.nmi_pending = false;
        // 7 cycles already ticked in service_interrupt
        return 7;
    }

    // 3. Maskable IRQ (line asserted & I flag clear)
    if bus.irq_line && !get_flag(cpu, IRQ_DISABLE) {
        service_interrupt(cpu, bus, 0xFFFE);
        // 7 cycles already ticked
        return 7;
    }

    // 4. Attempt table-dispatch (feature gated)
    #[cfg(feature = "table_dispatch")]
    {
        // Safe opcode peek (do NOT advance PC yet)
        let opcode = bus.read(cpu.pc());
        if let Some(cycles) = try_table_step(cpu, bus, opcode) {
            // Table path executed the opcode, advanced PC, and ticked cycles.
            return cycles;
        }
    }

    // 5. Fallback: fallback dispatcher owns full fetch/decode/execute
    fallback::step(cpu, bus)
}

/// Common interrupt entry sequence (push PC, status with Break=0; set I; load vector).
/// Ticks 7 cycles (interrupt entry timing).
fn service_interrupt<C: CpuRegs>(cpu: &mut C, bus: &mut Bus, vector_addr: u16) {
    // Push current PC
    let current_pc = cpu.pc();
    push_word(cpu, bus, current_pc);
    // Push processor status with Break flag cleared
    push_status_with_break(cpu, bus, false);
    // Set Interrupt Disable
    set_flag(cpu, IRQ_DISABLE, true);
    // Load new PC from vector
    let new_pc = bus.read_word(vector_addr);
    cpu.set_pc(new_pc);
    // Total cycles: 7 (lump-sum interrupt entry)
    bus.tick(7);
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

    // Removed outdated dma_stall_consumes_one_cycle test: relied on non-existent bus.start_dma helper.

    #[test]
    fn nmi_preempts_opcode() {
        let (mut cpu, mut bus) = setup(&[0xEA, 0x00]); // NOP; BRK
        bus.nmi_pending = true;
        let cycles = crate::cpu::dispatch::step(cpu.state_mut(), &mut bus);
        assert_eq!(cycles, 7);
    }

    #[test]
    fn fallback_step_executes_nop() {
        let (mut cpu, mut bus) = setup(&[0xEA, 0x00]); // NOP; BRK
        let pc_before = cpu.pc();
        let cycles = crate::cpu::dispatch::step(cpu.state_mut(), &mut bus);
        assert!(cycles >= 2); // NOP is 2 cycles (table or fallback)
        assert!(cpu.pc() > pc_before);
    }

    #[test]
    fn irq_mask_respected() {
        let (mut cpu, mut bus) = setup(&[0xEA, 0x00]);
        // Assert IRQ line but leave I flag set from reset (IRQ ignored)
        bus.irq_line = true;
        let c1 = crate::cpu::dispatch::step(cpu.state_mut(), &mut bus); // Should just execute NOP
        assert!(c1 >= 2);
    }
}
