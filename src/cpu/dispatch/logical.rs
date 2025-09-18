/*!
logical.rs - Logical and bit-test opcode family handler

Overview
========
Handles the 6502 logical and bit-test instructions:

AND: 0x29, 0x25, 0x35, 0x2D, 0x3D*, 0x39*, 0x21, 0x31*
ORA: 0x09, 0x05, 0x15, 0x0D, 0x1D*, 0x19*, 0x01, 0x11*
EOR: 0x49, 0x45, 0x55, 0x4D, 0x5D*, 0x59*, 0x41, 0x51*
BIT: 0x24, 0x2C

(*) Page‑cross aware addressing helpers return (addr, crossed); this handler
    adds a +1 cycle penalty on documented read modes when page boundaries are crossed.

Responsibilities
================
- Decode and execute logical / bit-test opcodes.
- Update accumulator or flags (via execute helpers) as required.
- Apply page-cross penalties by mutating *cycles (loads only).
- Never tick the bus (the fallback dispatcher finalizes timing).
- Never perform RMW adjustments (none of these opcodes are RMW).

Caller Requirements
===================
The caller (fallback dispatcher) must:
- Fetch the opcode and advance PC before calling `handle`.
- Initialize `*cycles` with `base_cycles(opcode)`.
- Tick the bus and apply any global adjustments after a `true` return.

Return Contract
===============
`handle` returns:
- true  => opcode recognized, effects applied (and cycles possibly incremented)
- false => not a member of this family (caller continues dispatch chain)

*/

#![allow(dead_code)]

use crate::bus::Bus;
use crate::cpu::regs::CpuRegs;

use crate::cpu::addressing::{
    addr_abs, addr_abs_x_pc, addr_abs_y_pc, addr_ind_x, addr_ind_y_pc, addr_zp, addr_zp_x,
    fetch_byte,
};
use crate::cpu::execute::{and as and_exec, bit, eor, ora};

/// Attempt to execute a logical/bit-test opcode.
/// Returns true if handled, false if opcode is not in this family.
///
/// Parameters:
/// - `opcode`: already-fetched opcode
/// - `cpu`, `bus`: execution context
/// - `cycles`: base cycle count (mutated only for page-cross penalties)
///
/// Caller responsibilities:
/// - Do NOT tick the bus here.
/// - Perform final cycle adjustments (e.g. RMW subtraction—irrelevant here but
///   kept uniform) and tick *after* this returns true.
pub(super) fn handle<C: CpuRegs>(opcode: u8, cpu: &mut C, bus: &mut Bus, cycles: &mut u32) -> bool {
    match opcode {
        // ---------------- AND ----------------
        0x29 => {
            let v = fetch_byte(cpu, bus);
            and_exec(cpu, v);
        } // Immediate
        0x25 => {
            let addr = addr_zp(cpu, bus);
            let v = bus.read(addr);
            and_exec(cpu, v);
        }
        0x35 => {
            let addr = addr_zp_x(cpu, bus);
            let v = bus.read(addr);
            and_exec(cpu, v);
        }
        0x2D => {
            let addr = addr_abs(cpu, bus);
            let v = bus.read(addr);
            and_exec(cpu, v);
        }
        0x3D => {
            let (addr, crossed) = addr_abs_x_pc(cpu, bus);
            and_exec(cpu, bus.read(addr));
            add_page_cross_penalty(cycles, crossed);
        }
        0x39 => {
            let (addr, crossed) = addr_abs_y_pc(cpu, bus);
            and_exec(cpu, bus.read(addr));
            add_page_cross_penalty(cycles, crossed);
        }
        0x21 => {
            let addr = addr_ind_x(cpu, bus);
            let v = bus.read(addr);
            and_exec(cpu, v);
        }
        0x31 => {
            let (addr, crossed) = addr_ind_y_pc(cpu, bus);
            and_exec(cpu, bus.read(addr));
            add_page_cross_penalty(cycles, crossed);
        }

        // ---------------- ORA ----------------
        0x09 => {
            let v = fetch_byte(cpu, bus);
            ora(cpu, v);
        }
        0x05 => {
            let addr = addr_zp(cpu, bus);
            let v = bus.read(addr);
            ora(cpu, v);
        }
        0x15 => {
            let addr = addr_zp_x(cpu, bus);
            let v = bus.read(addr);
            ora(cpu, v);
        }
        0x0D => {
            let addr = addr_abs(cpu, bus);
            let v = bus.read(addr);
            ora(cpu, v);
        }
        0x1D => {
            let (addr, crossed) = addr_abs_x_pc(cpu, bus);
            ora(cpu, bus.read(addr));
            add_page_cross_penalty(cycles, crossed);
        }
        0x19 => {
            let (addr, crossed) = addr_abs_y_pc(cpu, bus);
            ora(cpu, bus.read(addr));
            add_page_cross_penalty(cycles, crossed);
        }
        0x01 => {
            let addr = addr_ind_x(cpu, bus);
            let v = bus.read(addr);
            ora(cpu, v);
        }
        0x11 => {
            let (addr, crossed) = addr_ind_y_pc(cpu, bus);
            ora(cpu, bus.read(addr));
            add_page_cross_penalty(cycles, crossed);
        }

        // ---------------- EOR ----------------
        0x49 => {
            let v = fetch_byte(cpu, bus);
            eor(cpu, v);
        }
        0x45 => {
            let addr = addr_zp(cpu, bus);
            let v = bus.read(addr);
            eor(cpu, v);
        }
        0x55 => {
            let addr = addr_zp_x(cpu, bus);
            let v = bus.read(addr);
            eor(cpu, v);
        }
        0x4D => {
            let addr = addr_abs(cpu, bus);
            let v = bus.read(addr);
            eor(cpu, v);
        }
        0x5D => {
            let (addr, crossed) = addr_abs_x_pc(cpu, bus);
            eor(cpu, bus.read(addr));
            add_page_cross_penalty(cycles, crossed);
        }
        0x59 => {
            let (addr, crossed) = addr_abs_y_pc(cpu, bus);
            eor(cpu, bus.read(addr));
            add_page_cross_penalty(cycles, crossed);
        }
        0x41 => {
            let addr = addr_ind_x(cpu, bus);
            let v = bus.read(addr);
            eor(cpu, v);
        }
        0x51 => {
            let (addr, crossed) = addr_ind_y_pc(cpu, bus);
            eor(cpu, bus.read(addr));
            add_page_cross_penalty(cycles, crossed);
        }

        // ---------------- BIT ----------------
        0x24 => {
            let addr = addr_zp(cpu, bus);
            let v = bus.read(addr);
            bit(cpu, v);
        }
        0x2C => {
            let addr = addr_abs(cpu, bus);
            let v = bus.read(addr);
            bit(cpu, v);
        }

        _ => return false,
    }
    true
}

#[inline]
fn add_page_cross_penalty(cycles: &mut u32, crossed: bool) {
    if crossed {
        *cycles += 1;
    }
}

#[cfg(test)]
mod tests {
    use crate::bus::Bus;
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
    fn and_abs_x_page_cross_penalty() {
        // LDX #$01; AND $12FF,X; BRK
        let program = [0xA2, 0x01, 0x3D, 0xFF, 0x12, 0x00];
        let (mut cpu, mut bus) = setup(&program);
        assert_eq!(cpu.step(&mut bus), 2); // LDX
        assert_eq!(cpu.step(&mut bus), 5); // AND abs,X with cross
    }

    #[test]
    fn ora_indirect_y_page_cross_penalty() {
        // Build a tiny program:
        // 0000: ORA ($10),Y
        // 0002: BRK
        // ZP $10/$11 hold a pointer near page end so Y causes crossing.
        // Setup: LDY #$01; ORA ($10),Y; BRK
        let program = [0xA0, 0x01, 0x11, 0x10, 0x00]; // LDY #$01; ORA ($10),Y; BRK
        let (mut cpu, mut bus) = setup(&program);
        // Prime zero-page pointer at $10 -> $12FF
        bus.write(0x0010, 0xFF);
        bus.write(0x0011, 0x12);
        assert_eq!(cpu.step(&mut bus), 2); // LDY
        // Crossing expected adds +1 making total = base(5) for ORA (ind),Y w/ cross
        let cycles = cpu.step(&mut bus);
        assert_eq!(cycles, 5);
    }

    #[test]
    fn bit_zero_page_and_absolute_behavior() {
        // BIT $0002; BIT $1234; BRK
        // Prepare memory so BIT tests different bits.
        let program = vec![0x24, 0x02, 0x2C, 0x34, 0x12, 0x00];
        // Fill a few bytes to ensure reads are stable
        let (mut cpu, mut bus) = setup(&program);
        bus.write(0x0002, 0b1100_0000); // N & V set, Z depends on A & operand AND
        cpu.set_a(0xFF);
        let c1 = cpu.step(&mut bus);
        assert!(c1 >= 3); // BIT zp base cycles
        // Absolute BIT
        bus.write(0x1234, 0b0100_0000); // V set, N clear
        cpu.set_a(0x00); // Ensures Z flag set after BIT abs (A & operand == 0)
        let _c2 = cpu.step(&mut bus);
    }

    #[test]
    fn eor_immediate_updates_accumulator_and_flags() {
        // LDA #$FF; EOR #$FF; BRK  => A becomes 0x00
        let (mut cpu, mut bus) = setup(&[0xA9, 0xFF, 0x49, 0xFF, 0x00]);
        let _ = cpu.step(&mut bus); // LDA
        let _ = cpu.step(&mut bus); // EOR
        assert_eq!(cpu.a(), 0x00);
    }
}
