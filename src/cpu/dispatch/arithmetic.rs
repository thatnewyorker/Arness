/*!
arithmetic.rs - ADC / SBC opcode family handler

Overview
========
Implements 6502 add / subtract with carry instructions:

ADC: 0x69, 0x65, 0x75, 0x6D, 0x7D*, 0x79*, 0x61, 0x71
SBC: 0xE9, 0xE5, 0xF5, 0xED, 0xFD*, 0xF9*, 0xE1, 0xF1

(*) Page‑cross capable addressing modes add +1 cycle when a page boundary is crossed
    (handled here by incrementing *cycles for the *_pc helpers).

Responsibilities
================
- Fetch effective operand (immediate or memory) using addressing helpers.
- Invoke `adc` / `sbc` from `cpu::execute`.
- Apply page-cross penalty (+1) for *_pc addressing helpers when crossing occurred.
- Never tick the bus; never recompute base cycles; never perform RMW adjustments.

Caller Requirements
===================
The fallback dispatcher must:
- Fetch opcode, advance PC, and initialize *cycles via `base_cycles(opcode)`.
- After a true return from `handle`, perform finalization (RMW adjustment if needed, bus tick).

Return Contract
===============
`handle` returns true if the opcode was recognized and executed (cycles may have been increased),
false otherwise so the dispatcher can continue down the chain.
*/

#![allow(dead_code)]

use crate::bus::Bus;
use crate::cpu::regs::CpuRegs; // generic trait for arithmetic handler

use crate::cpu::addressing::{
    addr_abs, addr_abs_x_pc, addr_abs_y_pc, addr_ind_x, addr_ind_y_pc, addr_zp, addr_zp_x,
    fetch_byte,
};
use crate::cpu::execute::{adc, sbc};

/// Attempt to execute an ADC or SBC opcode.
///
/// Returns:
///   true  - opcode handled (operand fetched, operation executed, cycles possibly bumped)
///   false - not an arithmetic opcode; caller should continue dispatch
///
/// Assumptions:
/// - PC has already been advanced past the opcode.
/// - *cycles contains base cycle count (from `base_cycles(opcode)`).
/// - Bus ticking is performed by the caller after early return.
pub(super) fn handle<C: CpuRegs>(opcode: u8, cpu: &mut C, bus: &mut Bus, cycles: &mut u32) -> bool {
    match opcode {
        // ---------------- ADC ----------------
        0x69 => {
            let v = fetch_byte(cpu, bus);
            adc(cpu, v);
        } // Immediate
        0x65 => {
            let addr = addr_zp(cpu, bus);
            let v = bus.read(addr);
            adc(cpu, v);
        }
        0x75 => {
            let addr = addr_zp_x(cpu, bus);
            let v = bus.read(addr);
            adc(cpu, v);
        }
        0x6D => {
            let addr = addr_abs(cpu, bus);
            let v = bus.read(addr);
            adc(cpu, v);
        }
        0x7D => {
            let (addr, crossed) = addr_abs_x_pc(cpu, bus);
            adc(cpu, bus.read(addr));
            add_page_cross_penalty(cycles, crossed);
        }
        0x79 => {
            let (addr, crossed) = addr_abs_y_pc(cpu, bus);
            adc(cpu, bus.read(addr));
            add_page_cross_penalty(cycles, crossed);
        }
        0x61 => {
            let addr = addr_ind_x(cpu, bus);
            let v = bus.read(addr);
            adc(cpu, v);
        }
        0x71 => {
            let (addr, crossed) = addr_ind_y_pc(cpu, bus);
            adc(cpu, bus.read(addr));
            add_page_cross_penalty(cycles, crossed);
        }

        // ---------------- SBC ----------------
        0xE9 => {
            let v = fetch_byte(cpu, bus);
            sbc(cpu, v);
        } // Immediate
        0xE5 => {
            let addr = addr_zp(cpu, bus);
            let v = bus.read(addr);
            sbc(cpu, v);
        }
        0xF5 => {
            let addr = addr_zp_x(cpu, bus);
            let v = bus.read(addr);
            sbc(cpu, v);
        }
        0xED => {
            let addr = addr_abs(cpu, bus);
            let v = bus.read(addr);
            sbc(cpu, v);
        }
        0xFD => {
            let (addr, crossed) = addr_abs_x_pc(cpu, bus);
            sbc(cpu, bus.read(addr));
            add_page_cross_penalty(cycles, crossed);
        }
        0xF9 => {
            let (addr, crossed) = addr_abs_y_pc(cpu, bus);
            sbc(cpu, bus.read(addr));
            add_page_cross_penalty(cycles, crossed);
        }
        0xE1 => {
            let addr = addr_ind_x(cpu, bus);
            let v = bus.read(addr);
            sbc(cpu, v);
        }
        0xF1 => {
            let (addr, crossed) = addr_ind_y_pc(cpu, bus);
            sbc(cpu, bus.read(addr));
            add_page_cross_penalty(cycles, crossed);
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
    fn adc_abs_x_page_cross_penalty() {
        // LDX #$01; ADC $12FF,X; BRK
        let (mut cpu, mut bus) = setup(&[0xA2, 0x01, 0x7D, 0xFF, 0x12, 0x00]);
        assert_eq!(crate::cpu::dispatch::step(cpu.state_mut(), &mut bus), 2); // LDX
        assert_eq!(crate::cpu::dispatch::step(cpu.state_mut(), &mut bus), 5); // ADC abs,X with page cross
    }

    #[test]
    fn sbc_indirect_y_page_cross_penalty() {
        // LDY #$01; SBC ($10),Y; BRK   (ZP $10/$11 -> $12FF so adding Y crosses)
        let (mut cpu, mut bus) = setup(&[0xA0, 0x01, 0xF1, 0x10, 0x00]);
        // Prime zero-page pointer at $10 -> $12FF
        bus.write(0x0010, 0xFF);
        bus.write(0x0011, 0x12);
        assert_eq!(crate::cpu::dispatch::step(cpu.state_mut(), &mut bus), 2); // LDY
        let cycles = crate::cpu::dispatch::step(cpu.state_mut(), &mut bus);
        assert_eq!(cycles, 6); // SBC (ind),Y with page cross
    }

    #[test]
    fn adc_immediate_basic() {
        // LDA #$01; ADC #$02; BRK
        let (mut cpu, mut bus) = setup(&[0xA9, 0x01, 0x69, 0x02, 0x00]);
        let _ = crate::cpu::dispatch::step(cpu.state_mut(), &mut bus); // LDA
        let _ = crate::cpu::dispatch::step(cpu.state_mut(), &mut bus); // ADC
        assert_eq!(cpu.a(), 0x03);
    }

    #[test]
    fn sbc_immediate_basic() {
        // LDA #$05; SEC; SBC #$02; BRK  => A = 0x03
        let (mut cpu, mut bus) = setup(&[0xA9, 0x05, 0x38, 0xE9, 0x02, 0x00]);
        let _ = crate::cpu::dispatch::step(cpu.state_mut(), &mut bus); // LDA
        let _ = crate::cpu::dispatch::step(cpu.state_mut(), &mut bus); // SEC
        let _ = crate::cpu::dispatch::step(cpu.state_mut(), &mut bus); // SBC
        assert_eq!(cpu.a(), 0x03);
    }
}
