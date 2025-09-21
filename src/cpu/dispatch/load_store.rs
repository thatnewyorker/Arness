/*!
load_store.rs - Load / Store opcode family handler (part of dispatch fallback chain)

Overview
=====
Handles all 6502 load and store instructions:

Loads (set Z/N flags; may incur page-cross penalty):
    LDA: A9, A5, B5, AD, BD*, B9*, A1, B1*
    LDX: A2, A6, B6, AE, BE*
    LDY: A0, A4, B4, AC, BC*

Stores (no flags changed, no page-cross cycle penalties):
    STA: 85, 95, 8D, 9D, 99, 81, 91
    STX: 86, 96, 8E
    STY: 84, 94, 8C

(*) Page‑cross aware addressing helpers return (addr, crossed) and this handler
    adds a +1 cycle penalty on documented read cases (never for stores).

Integration
====================
This module provides a single `handle` function invoked early by the fallback dispatcher (`dispatch::fallback::step`) after base cycle lookup.

The fallback dispatcher should not also carry duplicate match arms for these opcodes.

Cycle Accounting
================
- Base cycle count is computed by the caller before invoking this handler.
- This handler only mutates `*cycles` to add a +1 page-cross penalty for
  the documented read cases (never for stores).
- No bus ticking occurs here; the fallback dispatcher finalizes (including any RMW adjustment).

Future Work
===========
Potential improvements:
- Unify small load/store-specific micro-optimizations (if added) with table dispatch metadata.
- Introduce an operand abstraction to reduce addressing boilerplate.

*/

#![allow(dead_code)]

use crate::bus::Bus;
use crate::cpu::regs::CpuRegs;

use crate::cpu::addressing::{
    addr_abs, addr_abs_x, addr_abs_x_pc, addr_abs_y, addr_abs_y_pc, addr_ind_x, addr_ind_y,
    addr_ind_y_pc, addr_zp, addr_zp_x, addr_zp_y, fetch_byte,
};
use crate::cpu::execute::{lda, ldx, ldy};

/// Attempt to execute a load/store opcode.
///
/// Parameters:
/// - `opcode`: already-fetched opcode byte
/// - `cpu`, `bus`: execution context
/// - `cycles`: mutable base cycle counter (page-cross penalty may be added)
///
/// Returns:
/// - true if this opcode was recognized and executed
/// - false if the opcode does not belong to the load/store family
///
/// Assumptions:
/// - PC has already been advanced past the opcode byte by the caller.
/// - Base cycles have been populated in `*cycles` prior to this call.
pub(super) fn handle<C: CpuRegs>(opcode: u8, cpu: &mut C, bus: &mut Bus, cycles: &mut u32) -> bool {
    match opcode {
        // ---------------- LDA ----------------
        0xA9 => {
            let v = fetch_byte(cpu, bus);
            lda(cpu, v);
        } // Immediate
        0xA5 => {
            let a = addr_zp(cpu, bus);
            lda(cpu, bus.read(a));
        }
        0xB5 => {
            let a = addr_zp_x(cpu, bus);
            lda(cpu, bus.read(a));
        }
        0xAD => {
            let a = addr_abs(cpu, bus);
            lda(cpu, bus.read(a));
        }
        0xBD => {
            let (a, crossed) = addr_abs_x_pc(cpu, bus);
            lda(cpu, bus.read(a));
            add_page_cross_penalty(cycles, crossed);
        }
        0xB9 => {
            let (a, crossed) = addr_abs_y_pc(cpu, bus);
            lda(cpu, bus.read(a));
            add_page_cross_penalty(cycles, crossed);
        }
        0xA1 => {
            let a = addr_ind_x(cpu, bus);
            lda(cpu, bus.read(a));
        }
        0xB1 => {
            let (a, crossed) = addr_ind_y_pc(cpu, bus);
            lda(cpu, bus.read(a));
            add_page_cross_penalty(cycles, crossed);
        }

        // ---------------- LDX ----------------
        0xA2 => {
            let v = fetch_byte(cpu, bus);
            ldx(cpu, v);
        }
        0xA6 => {
            let a = addr_zp(cpu, bus);
            ldx(cpu, bus.read(a));
        }
        0xB6 => {
            let a = addr_zp_y(cpu, bus);
            ldx(cpu, bus.read(a));
        }
        0xAE => {
            let a = addr_abs(cpu, bus);
            ldx(cpu, bus.read(a));
        }
        0xBE => {
            let (a, crossed) = addr_abs_y_pc(cpu, bus);
            ldx(cpu, bus.read(a));
            add_page_cross_penalty(cycles, crossed);
        }

        // ---------------- LDY ----------------
        0xA0 => {
            let v = fetch_byte(cpu, bus);
            ldy(cpu, v);
        }
        0xA4 => {
            let a = addr_zp(cpu, bus);
            ldy(cpu, bus.read(a));
        }
        0xB4 => {
            let a = addr_zp_x(cpu, bus);
            ldy(cpu, bus.read(a));
        }
        0xAC => {
            let a = addr_abs(cpu, bus);
            ldy(cpu, bus.read(a));
        }
        0xBC => {
            let (a, crossed) = addr_abs_x_pc(cpu, bus);
            ldy(cpu, bus.read(a));
            add_page_cross_penalty(cycles, crossed);
        }

        // ---------------- STA ----------------
        0x85 => {
            let a = addr_zp(cpu, bus);
            let a_val = cpu.a();
            bus.write(a, a_val);
        }
        0x95 => {
            let a = addr_zp_x(cpu, bus);
            let a_val = cpu.a();
            bus.write(a, a_val);
        }
        0x8D => {
            let a = addr_abs(cpu, bus);
            let a_val = cpu.a();
            bus.write(a, a_val);
        }
        0x9D => {
            let a = addr_abs_x(cpu, bus);
            let a_val = cpu.a();
            bus.write(a, a_val);
        }
        0x99 => {
            let a = addr_abs_y(cpu, bus);
            let a_val = cpu.a();
            bus.write(a, a_val);
        }
        0x81 => {
            let a = addr_ind_x(cpu, bus);
            let a_val = cpu.a();
            bus.write(a, a_val);
        }
        0x91 => {
            let a = addr_ind_y(cpu, bus);
            let a_val = cpu.a();
            bus.write(a, a_val);
        }

        // ---------------- STX ----------------
        0x86 => {
            let a = addr_zp(cpu, bus);
            let x_val = cpu.x();
            bus.write(a, x_val);
        }
        0x96 => {
            let a = addr_zp_y(cpu, bus);
            let x_val = cpu.x();
            bus.write(a, x_val);
        }
        0x8E => {
            let a = addr_abs(cpu, bus);
            let x_val = cpu.x();
            bus.write(a, x_val);
        }

        // ---------------- STY ----------------
        0x84 => {
            let a = addr_zp(cpu, bus);
            let y_val = cpu.y();
            bus.write(a, y_val);
        }
        0x94 => {
            let a = addr_zp_x(cpu, bus);
            let y_val = cpu.y();
            bus.write(a, y_val);
        }
        0x8C => {
            let a = addr_abs(cpu, bus);
            let y_val = cpu.y();
            bus.write(a, y_val);
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

    // NOTE: These tests exercise the public Cpu facade.

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
    fn lda_abs_x_page_cross_penalty_applied() {
        // LDX #$01; LDA $12FF,X; BRK
        let (mut cpu, mut bus) = setup(&[0xA2, 0x01, 0xBD, 0xFF, 0x12, 0x00]);
        let c1 = cpu.step(&mut bus);
        assert_eq!(c1, 2); // LDX imm
        let c2 = cpu.step(&mut bus);
        assert_eq!(c2, 5); // LDA abs,X with page cross
    }

    #[test]
    fn sta_abs_x_no_page_cross_penalty() {
        // LDX #$01; STA $12FF,X; BRK
        let (mut cpu, mut bus) = setup(&[0xA2, 0x01, 0x9D, 0xFF, 0x12, 0x00]);
        let c1 = cpu.step(&mut bus);
        assert_eq!(c1, 2); // LDX
        let c2 = cpu.step(&mut bus);
        // STA abs,X base is 5 cycles (no extra penalty even if address crosses)
        assert_eq!(c2, 5);
    }
}
