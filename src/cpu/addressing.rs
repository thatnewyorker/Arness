/*!
addressing.rs - 6502 addressing and operand fetch helpers (shared by dispatch)

Overview
========
Provides canonical helpers for:
- Instruction stream byte/word fetch
- Effective address calculation for all standard 6502 addressing modes
- Variants that report page-cross events (for cycle penalty logic)
- Emulation of the 6502 JMP (indirect) page-wrap quirk

Scope & Responsibilities
=======================
- Pure address / operand resolution only.
- Does NOT tick the bus or apply cycle penalties (caller / fallback dispatcher manages that).
- Page-cross detection helpers return `(addr, crossed)` so dispatch layers can add +1 cycle where required.
- Functions are `pub(crate)`; they are an internal implementation detail of the CPU core.

Caller Assumptions
==================
- PC points at the next unread instruction byte when a fetch helper is invoked.
- Callers advance PC exclusively via these helpers (no manual PC arithmetic inside handlers).

Migration Status
================
Generic over `CpuRegs`. Backward compatibility is provided via trait implementations for legacy representations.

Function Inventory (generic forms)
----------------------------------
Fetch:
    fetch_byte(&mut impl CpuRegs, &mut Bus) -> u8
    fetch_word(&mut impl CpuRegs, &mut Bus) -> u16

Basic addressing (effective address):
    addr_zp
    addr_zp_x
    addr_zp_y
    addr_abs
    addr_abs_x
    addr_abs_y
    addr_ind_x
    addr_ind_y

Addressing with page-cross detection (return (addr, crossed)):
    addr_abs_x_pc
    addr_abs_y_pc
    addr_ind_y_pc

Low-level word reads:
    read_word_zp(base: u8)
    read_word_indirect_bug(addr)

Testing Notes
=============
Existing higher-level CPU tests exercise these paths implicitly. Targeted unit tests
can be added later for edge cases (page boundary crossings, indirect wrap quirk).
*/

#![allow(dead_code)]

use crate::bus::Bus;
use crate::cpu::regs::CpuRegs;

/// Fetch next byte from the instruction stream, incrementing PC.
pub(crate) fn fetch_byte<C: CpuRegs>(cpu: &mut C, bus: &mut Bus) -> u8 {
    let pc = cpu.pc();
    let v = bus.read(pc);
    cpu.advance_pc_one();
    v
}

/// Fetch next little-endian word (low, then high), incrementing PC twice.
pub(crate) fn fetch_word<C: CpuRegs>(cpu: &mut C, bus: &mut Bus) -> u16 {
    let lo = fetch_byte(cpu, bus) as u16;
    let hi = fetch_byte(cpu, bus) as u16;
    (hi << 8) | lo
}

// -------------------------
// Basic addressing helpers
// -------------------------

#[inline]
pub(crate) fn addr_zp<C: CpuRegs>(cpu: &mut C, bus: &mut Bus) -> u16 {
    fetch_byte(cpu, bus) as u16
}

#[inline]
pub(crate) fn addr_zp_x<C: CpuRegs>(cpu: &mut C, bus: &mut Bus) -> u16 {
    fetch_byte(cpu, bus).wrapping_add(cpu.x()) as u16
}

#[inline]
pub(crate) fn addr_zp_y<C: CpuRegs>(cpu: &mut C, bus: &mut Bus) -> u16 {
    fetch_byte(cpu, bus).wrapping_add(cpu.y()) as u16
}

#[inline]
pub(crate) fn addr_abs<C: CpuRegs>(cpu: &mut C, bus: &mut Bus) -> u16 {
    fetch_word(cpu, bus)
}

#[inline]
pub(crate) fn addr_abs_x<C: CpuRegs>(cpu: &mut C, bus: &mut Bus) -> u16 {
    fetch_word(cpu, bus).wrapping_add(cpu.x() as u16)
}

#[inline]
pub(crate) fn addr_abs_y<C: CpuRegs>(cpu: &mut C, bus: &mut Bus) -> u16 {
    fetch_word(cpu, bus).wrapping_add(cpu.y() as u16)
}

#[inline]
pub(crate) fn addr_ind_x<C: CpuRegs>(cpu: &mut C, bus: &mut Bus) -> u16 {
    let zp = fetch_byte(cpu, bus).wrapping_add(cpu.x());
    read_word_zp(bus, zp)
}

#[inline]
pub(crate) fn addr_ind_y<C: CpuRegs>(cpu: &mut C, bus: &mut Bus) -> u16 {
    let zp = fetch_byte(cpu, bus);
    read_word_zp(bus, zp).wrapping_add(cpu.y() as u16)
}

// ------------------------------------------------------
// Addressing with page-cross detection (for penalties)
// ------------------------------------------------------

#[inline]
pub(crate) fn addr_abs_x_pc<C: CpuRegs>(cpu: &mut C, bus: &mut Bus) -> (u16, bool) {
    let base = fetch_word(cpu, bus);
    let addr = base.wrapping_add(cpu.x() as u16);
    let crossed = (base & 0xFF00) != (addr & 0xFF00);
    (addr, crossed)
}

#[inline]
pub(crate) fn addr_abs_y_pc<C: CpuRegs>(cpu: &mut C, bus: &mut Bus) -> (u16, bool) {
    let base = fetch_word(cpu, bus);
    let addr = base.wrapping_add(cpu.y() as u16);
    let crossed = (base & 0xFF00) != (addr & 0xFF00);
    (addr, crossed)
}

#[inline]
pub(crate) fn addr_ind_y_pc<C: CpuRegs>(cpu: &mut C, bus: &mut Bus) -> (u16, bool) {
    let zp = fetch_byte(cpu, bus);
    let base = read_word_zp(bus, zp);
    let addr = base.wrapping_add(cpu.y() as u16);
    let crossed = (base & 0xFF00) != (addr & 0xFF00);
    (addr, crossed)
}

// -------------------------
// Low-level word helpers
// -------------------------

/// Read a 16-bit little endian pointer from zero page with wraparound
/// on the high byte (standard 6502 zero-page indirect behavior).
#[inline]
pub(crate) fn read_word_zp(bus: &mut Bus, base: u8) -> u16 {
    let lo = bus.read(base as u16) as u16;
    let hi = bus.read(((base as u16 + 1) & 0x00FF) as u16) as u16;
    (hi << 8) | lo
}

/// Emulate the original 6502 JMP (indirect) hardware bug: when the
/// low byte of the indirect vector is 0xFF, the high byte does not
/// cross to the next page; it wraps within the same page.
#[inline]
pub(crate) fn read_word_indirect_bug(bus: &mut Bus, addr: u16) -> u16 {
    let lo = bus.read(addr) as u16;
    let hi_addr = (addr & 0xFF00) | ((addr + 1) & 0x00FF);
    let hi = bus.read(hi_addr) as u16;
    (hi << 8) | lo
}

// ---------------
// (Optional) Tests
// ---------------
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
    fn abs_x_page_cross_detection() {
        // LDX #$10 ; LDA $80F5,X (base high changes when X=0x10 -> $8105)
        // Program bytes: LDX #$10, LDA $F5 $80
        let (mut cpu, mut bus) = setup(&[0xA2, 0x10, 0xBD, 0xF5, 0x80, 0x00]);
        // Execute LDX (manual fetch sequence) using CpuState via facade
        assert_eq!(fetch_byte(cpu.state_mut(), &mut bus), 0xA2);
        let x_val = fetch_byte(cpu.state_mut(), &mut bus); // #$10
        cpu.set_x(x_val);
        let (addr, crossed) = addr_abs_x_pc(cpu.state_mut(), &mut bus);
        assert!(crossed);
        assert_eq!(addr, 0x80F5 + 0x10);
    }

    #[test]
    fn indirect_jmp_bug() {
        // Place vector at $10FF (lo) and $1000 (hi should wrap to $1000 not $1100)
        let rom = build_nrom_with_prg(&[0xEA], 1, 1, None);
        let cart = Cartridge::from_ines_bytes(&rom).unwrap();
        let mut bus = Bus::new();
        bus.attach_cartridge(cart);
        bus.write(0x10FF, 0x34);
        bus.write(0x1000, 0x12);
        let target = read_word_indirect_bug(&mut bus, 0x10FF);
        assert_eq!(target, 0x1234);
    }
}
