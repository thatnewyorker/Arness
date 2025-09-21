/*!
table.rs - Feature-gated table-driven dispatcher using a function-pointer table.

Purpose
=======
This module provides a function-pointer dispatch table (256 entries)
mapping each opcode to a small handler function. The table path is a
fast alternative to the match-based fallback dispatcher and can be
expanded incrementally. Handlers execute instruction semantics, advance
PC appropriately, compute the correct cycle count (including penalties),
and let the entrypoint tick the bus exactly once per instruction.

Design
------
- Table: [Option<OpHandler>; 256], where `OpHandler = fn(&mut dyn CpuRegs, &mut Bus) -> u32`.
- Entrypoint: `try_table_step(&mut dyn CpuRegs, &mut Bus, opcode) -> Option<u32>`.
- If an opcode is handled, a handler returns the total cycles consumed.
  The entrypoint then calls `bus.tick(cycles)` and returns `Some(cycles)`.
- If an opcode is not handled in the table, the entrypoint returns `None`
  so the caller falls back to the match-based dispatcher.

Scope (prototype)
-----------------
Implements a minimal subset:
- NOP (0xEA)
- LDA immediate/zero-page/zero-page,X/absolute/absolute,X/absolute,Y/(indirect,X)/(indirect),Y
- CLC (0x18), SEC (0x38)

The subset is sufficient to validate the table path and related tests,
and can be expanded over time.

Notes
-----
- Handlers begin by advancing PC by 1 to skip the opcode byte because
  the orchestrator only peeks the opcode (PC still points at it).
- Addressing helpers from `addressing.rs` fetch operands and advance PC.
- Page-cross cycle penalties are handled explicitly in the LDA handlers
  that can cross pages.

*/

#![allow(dead_code)]

use crate::bus::Bus;
use crate::cpu::regs::CpuRegs;

#[cfg(feature = "table_dispatch")]
use crate::cpu::{
    addressing::{
        addr_abs, addr_abs_x_pc, addr_abs_y_pc, addr_ind_x, addr_ind_y_pc, addr_zp, addr_zp_x,
        fetch_byte,
    },
    execute::{lda, set_flag},
    state::CARRY,
};

/// Attempt to execute the given opcode using the table path.
/// Returns Some(cycles) if handled; otherwise None (caller should fall back).
#[cfg(feature = "table_dispatch")]
pub(crate) fn try_table_step(cpu: &mut dyn CpuRegs, bus: &mut Bus, opcode: u8) -> Option<u32> {
    let handler = EXEC_TABLE[opcode as usize]?;
    // Execute handler (handlers DO NOT tick the bus; they return total cycles).
    let cycles = handler(cpu, bus);
    // Tick exactly once for the total cycles consumed.
    bus.tick(cycles);
    Some(cycles)
}

#[cfg(not(feature = "table_dispatch"))]
pub(crate) fn try_table_step(_cpu: &mut dyn CpuRegs, _bus: &mut Bus, _opcode: u8) -> Option<u32> {
    None
}

#[cfg(feature = "table_dispatch")]
type OpHandler = fn(&mut dyn CpuRegs, &mut Bus) -> u32;

// ------------------------------------------
// Opcode Handlers (prototype subset)
// ------------------------------------------

#[cfg(feature = "table_dispatch")]
fn op_nop(cpu: &mut dyn CpuRegs, _bus: &mut Bus) -> u32 {
    cpu.advance_pc_one(); // skip opcode
    2
}

#[cfg(feature = "table_dispatch")]
fn op_clc(cpu: &mut dyn CpuRegs, _bus: &mut Bus) -> u32 {
    cpu.advance_pc_one(); // skip opcode
    set_flag(cpu, CARRY, false);
    2
}

#[cfg(feature = "table_dispatch")]
fn op_sec(cpu: &mut dyn CpuRegs, _bus: &mut Bus) -> u32 {
    cpu.advance_pc_one(); // skip opcode
    set_flag(cpu, CARRY, true);
    2
}

// LDA #imm
#[cfg(feature = "table_dispatch")]
fn op_lda_imm(cpu: &mut dyn CpuRegs, bus: &mut Bus) -> u32 {
    cpu.advance_pc_one(); // skip opcode
    let v = fetch_byte(cpu, bus);
    lda(cpu, v);
    2
}

// LDA zp
#[cfg(feature = "table_dispatch")]
fn op_lda_zp(cpu: &mut dyn CpuRegs, bus: &mut Bus) -> u32 {
    cpu.advance_pc_one(); // skip opcode
    let addr = addr_zp(cpu, bus);
    let v = bus.read(addr);
    lda(cpu, v);
    3
}

// LDA zp,X
#[cfg(feature = "table_dispatch")]
fn op_lda_zpx(cpu: &mut dyn CpuRegs, bus: &mut Bus) -> u32 {
    cpu.advance_pc_one(); // skip opcode
    let addr = addr_zp_x(cpu, bus);
    let v = bus.read(addr);
    lda(cpu, v);
    4
}

// LDA abs
#[cfg(feature = "table_dispatch")]
fn op_lda_abs(cpu: &mut dyn CpuRegs, bus: &mut Bus) -> u32 {
    cpu.advance_pc_one(); // skip opcode
    let addr = addr_abs(cpu, bus);
    let v = bus.read(addr);
    lda(cpu, v);
    4
}

// LDA abs,X (+1 if page cross)
#[cfg(feature = "table_dispatch")]
fn op_lda_absx(cpu: &mut dyn CpuRegs, bus: &mut Bus) -> u32 {
    cpu.advance_pc_one(); // skip opcode
    let (addr, crossed) = addr_abs_x_pc(cpu, bus);
    let v = bus.read(addr);
    lda(cpu, v);
    4 + if crossed { 1 } else { 0 }
}

// LDA abs,Y (+1 if page cross)
#[cfg(feature = "table_dispatch")]
fn op_lda_absy(cpu: &mut dyn CpuRegs, bus: &mut Bus) -> u32 {
    cpu.advance_pc_one(); // skip opcode
    let (addr, crossed) = addr_abs_y_pc(cpu, bus);
    let v = bus.read(addr);
    lda(cpu, v);
    4 + if crossed { 1 } else { 0 }
}

// LDA (indirect,X)
#[cfg(feature = "table_dispatch")]
fn op_lda_indx(cpu: &mut dyn CpuRegs, bus: &mut Bus) -> u32 {
    cpu.advance_pc_one(); // skip opcode
    let addr = addr_ind_x(cpu, bus);
    let v = bus.read(addr);
    lda(cpu, v);
    6
}

// LDA (indirect),Y (+1 if page cross)
#[cfg(feature = "table_dispatch")]
fn op_lda_indy(cpu: &mut dyn CpuRegs, bus: &mut Bus) -> u32 {
    cpu.advance_pc_one(); // skip opcode
    let (addr, crossed) = addr_ind_y_pc(cpu, bus);
    let v = bus.read(addr);
    lda(cpu, v);
    5 + if crossed { 1 } else { 0 }
}

// ------------------------------------------
// Dispatch Table (256 entries)
// ------------------------------------------

#[cfg(feature = "table_dispatch")]
static EXEC_TABLE: [Option<OpHandler>; 256] = {
    let mut t: [Option<OpHandler>; 256] = [None; 256];

    // NOP
    t[0xEA] = Some(op_nop);

    // Flags
    t[0x18] = Some(op_clc);
    t[0x38] = Some(op_sec);

    // LDA family
    t[0xA9] = Some(op_lda_imm);
    t[0xA5] = Some(op_lda_zp);
    t[0xB5] = Some(op_lda_zpx);
    t[0xAD] = Some(op_lda_abs);
    t[0xBD] = Some(op_lda_absx);
    t[0xB9] = Some(op_lda_absy);
    t[0xA1] = Some(op_lda_indx);
    t[0xB1] = Some(op_lda_indy);

    t
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cartridge::Cartridge;
    use crate::cpu::state::CpuState;
    use crate::test_utils::build_nrom_with_prg;

    // Only run when feature enabled.
    #[cfg(feature = "table_dispatch")]
    fn setup(prg: &[u8]) -> (CpuState, Bus) {
        let rom = build_nrom_with_prg(prg, 1, 1, None);
        let cart = Cartridge::from_ines_bytes(&rom).unwrap();
        let mut bus = Bus::new();
        bus.attach_cartridge(cart);
        let mut cpu = CpuState::new();
        cpu.reset(&mut bus);
        (cpu, bus)
    }

    #[test]
    #[cfg(feature = "table_dispatch")]
    fn lda_imm_table_cycles() {
        // Program: LDA #$10; NOP
        let (mut cpu, mut bus) = setup(&[0xA9, 0x10, 0xEA, 0x00]);
        let op1 = bus.read(cpu.pc()); // peek
        assert_eq!(op1, 0xA9);
        let c = try_table_step(&mut cpu, &mut bus, op1).unwrap();
        assert_eq!(c, 2);
        assert_eq!(cpu.a(), 0x10);
    }

    #[test]
    #[cfg(feature = "table_dispatch")]
    fn lda_abs_x_page_cross_penalty() {
        // NOP; LDA $80FF,X; BRK
        let (mut cpu, mut bus) = setup(&[0xEA, 0xBD, 0xFF, 0x80, 0x00]);
        // Execute NOP via table
        let next_opcode = bus.read(cpu.pc());
        let _ = try_table_step(&mut cpu, &mut bus, next_opcode).unwrap();
        cpu.set_x(0x01);
        let lda_opcode = bus.read(cpu.pc());
        assert_eq!(lda_opcode, 0xBD);
        let cycles = try_table_step(&mut cpu, &mut bus, lda_opcode).unwrap();
        // Base 4 + 1 page cross
        assert_eq!(cycles, 5);
    }

    #[test]
    #[cfg(not(feature = "table_dispatch"))]
    fn table_disabled_returns_none() {
        let mut cpu = CpuState::new();
        let mut bus = Bus::new();
        assert!(try_table_step(&mut cpu, &mut bus, 0xA9).is_none());
    }
}
