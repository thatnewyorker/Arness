/*!
execute.rs - 6502 instruction semantic helpers (ALU, flags, stack, RMW)

Status (Phase B Migration)
==========================
Priority A migration applied: pure register / flag helpers and
accumulator-only operations now use the generic `CpuRegs` trait so they
operate directly on the canonical `CpuState` (legacy type removed)
without code duplication.

Migrated to generic (CpuRegs):
  - set_flag, get_flag, update_zn
  - lda/ldx/ldy
  - tax/tay/txa/tya/tsx/txs
  - inx/iny/dex/dey
  - and/ora/eor/bit
  - asl_acc/lsr_acc/rol_acc/ror_acc

All helpers (register, stack, arithmetic/logical, memory RMW, shifts/rotates, INC/DEC, branch) are now generic over CpuRegs.
(Phase B/E generic migration complete — next step: remove legacy bridging in core::Cpu and deprecate direct usage of legacy CPU representations.)

Purpose
=======
Centralize side-effect logic for instructions so multiple dispatch
strategies (legacy match, future table-driven) share a single
implementation. Migration proceeds incrementally to keep each commit
small and reviewable.

Scope (crate-visible)
---------------------
Flag & status helpers (generic):
    set_flag, get_flag, update_zn

Stack helpers (generic):
    push, pop, push_word, pop_word, push_status_with_break
    php, plp, pha, pla

Core ALU / register transfer (generic subset):
    lda/ldx/ldy, tax/tay/txa/tya, tsx/txs
    and/ora/eor/bit
    inx/iny/dex/dey
    (adc/sbc now generic)

Shifts / rotates:
    Accumulator + memory versions generic (rmw_memory choreography generic)

RMW choreography (generic):
    rmw_memory

Design Notes
============
- Generic helpers rely only on the `CpuRegs` API (no bus coupling).
- All helpers now accept a generic type implementing `CpuRegs`; the legacy
  monolithic type and its transitional signatures have been removed.

*/

#![allow(dead_code)]

use crate::bus_impl::Bus;
use crate::cpu::regs::CpuRegs;
use crate::cpu::state::{BREAK, CARRY, NEGATIVE, OVERFLOW, UNUSED, ZERO};

// ---------------------------------------------------------------------------
// Flag helpers (Priority A migrated to CpuRegs)
// ---------------------------------------------------------------------------

#[inline]
pub(crate) fn set_flag<C: CpuRegs>(cpu: &mut C, mask: u8, on: bool) {
    cpu.assign_flag(mask, on);
}

#[inline]
pub(crate) fn get_flag<C: CpuRegs>(cpu: &C, mask: u8) -> bool {
    cpu.is_flag_set(mask)
}

#[inline]
pub(crate) fn update_zn<C: CpuRegs>(cpu: &mut C, v: u8) {
    cpu.update_zn(v);
}

// ---------------------------------------------------------------------------
// Stack helpers (NOT YET MIGRATED)
// ---------------------------------------------------------------------------

#[inline]
pub(crate) fn push<C: CpuRegs>(cpu: &mut C, bus: &mut Bus, v: u8) {
    let sp = cpu.sp();
    let addr = 0x0100u16 | sp as u16;
    bus.write(addr, v);
    cpu.set_sp(sp.wrapping_sub(1));
}

#[inline]
pub(crate) fn pop<C: CpuRegs>(cpu: &mut C, bus: &mut Bus) -> u8 {
    let sp = cpu.sp().wrapping_add(1);
    cpu.set_sp(sp);
    let addr = 0x0100u16 | sp as u16;
    bus.read(addr)
}

#[inline]
pub(crate) fn push_word<C: CpuRegs>(cpu: &mut C, bus: &mut Bus, v: u16) {
    push(cpu, bus, (v >> 8) as u8);
    push(cpu, bus, (v & 0xFF) as u8);
}

#[inline]
pub(crate) fn pop_word<C: CpuRegs>(cpu: &mut C, bus: &mut Bus) -> u16 {
    let lo = pop(cpu, bus) as u16;
    let hi = pop(cpu, bus) as u16;
    (hi << 8) | lo
}

/// Push P with control over Break flag semantics (BRK/PHP vs IRQ/NMI).
pub(crate) fn push_status_with_break<C: CpuRegs>(cpu: &mut C, bus: &mut Bus, set_break: bool) {
    let v = cpu.compose_status_for_push(set_break);
    push(cpu, bus, v);
}

#[inline]
pub(crate) fn php<C: CpuRegs>(cpu: &mut C, bus: &mut Bus) {
    push_status_with_break(cpu, bus, true);
}

#[inline]
pub(crate) fn plp<C: CpuRegs>(cpu: &mut C, bus: &mut Bus) {
    let v = pop(cpu, bus);
    cpu.set_status((v | UNUSED) & !BREAK);
}

#[inline]
pub(crate) fn pha<C: CpuRegs>(cpu: &mut C, bus: &mut Bus) {
    // Avoid simultaneous immutable + mutable borrow of cpu in one expression.
    let a = cpu.a();
    push(cpu, bus, a);
}

#[inline]
pub(crate) fn pla<C: CpuRegs>(cpu: &mut C, bus: &mut Bus) {
    // Evaluate pop first to avoid overlapping mutable borrows in a single expression.
    let val = pop(cpu, bus);
    cpu.set_a(val);
    update_zn(cpu, val);
}

// ---------------------------------------------------------------------------
// Loads / Stores / Transfers (generic)
// ---------------------------------------------------------------------------

#[inline]
pub(crate) fn lda<C: CpuRegs>(cpu: &mut C, v: u8) {
    cpu.set_a(v);
    update_zn(cpu, cpu.a());
}

#[inline]
pub(crate) fn ldx<C: CpuRegs>(cpu: &mut C, v: u8) {
    cpu.set_x(v);
    update_zn(cpu, cpu.x());
}

#[inline]
pub(crate) fn ldy<C: CpuRegs>(cpu: &mut C, v: u8) {
    cpu.set_y(v);
    update_zn(cpu, cpu.y());
}

#[inline]
pub(crate) fn tax<C: CpuRegs>(cpu: &mut C) {
    cpu.set_x(cpu.a());
    update_zn(cpu, cpu.x());
}

#[inline]
pub(crate) fn tay<C: CpuRegs>(cpu: &mut C) {
    cpu.set_y(cpu.a());
    update_zn(cpu, cpu.y());
}

#[inline]
pub(crate) fn txa<C: CpuRegs>(cpu: &mut C) {
    cpu.set_a(cpu.x());
    update_zn(cpu, cpu.a());
}

#[inline]
pub(crate) fn tya<C: CpuRegs>(cpu: &mut C) {
    cpu.set_a(cpu.y());
    update_zn(cpu, cpu.a());
}

#[inline]
pub(crate) fn tsx<C: CpuRegs>(cpu: &mut C) {
    cpu.set_x(cpu.sp());
    update_zn(cpu, cpu.x());
}

#[inline]
pub(crate) fn txs<C: CpuRegs>(cpu: &mut C) {
    cpu.set_sp(cpu.x());
}

// ---------------------------------------------------------------------------
// Logical / Bit (generic)
// ---------------------------------------------------------------------------

#[inline]
pub(crate) fn and<C: CpuRegs>(cpu: &mut C, v: u8) {
    cpu.set_a(cpu.a() & v);
    update_zn(cpu, cpu.a());
}

#[inline]
pub(crate) fn ora<C: CpuRegs>(cpu: &mut C, v: u8) {
    cpu.set_a(cpu.a() | v);
    update_zn(cpu, cpu.a());
}

#[inline]
pub(crate) fn eor<C: CpuRegs>(cpu: &mut C, v: u8) {
    cpu.set_a(cpu.a() ^ v);
    update_zn(cpu, cpu.a());
}

#[inline]
pub(crate) fn bit<C: CpuRegs>(cpu: &mut C, v: u8) {
    set_flag(cpu, ZERO, (cpu.a() & v) == 0);
    set_flag(cpu, NEGATIVE, (v & 0x80) != 0);
    set_flag(cpu, OVERFLOW, (v & 0x40) != 0);
}

// ---------------------------------------------------------------------------
// Increment / Decrement (register) (generic)
// ---------------------------------------------------------------------------

#[inline]
pub(crate) fn inx<C: CpuRegs>(cpu: &mut C) {
    cpu.set_x(cpu.x().wrapping_add(1));
    update_zn(cpu, cpu.x());
}

#[inline]
pub(crate) fn iny<C: CpuRegs>(cpu: &mut C) {
    cpu.set_y(cpu.y().wrapping_add(1));
    update_zn(cpu, cpu.y());
}

#[inline]
pub(crate) fn dex<C: CpuRegs>(cpu: &mut C) {
    cpu.set_x(cpu.x().wrapping_sub(1));
    update_zn(cpu, cpu.x());
}

#[inline]
pub(crate) fn dey<C: CpuRegs>(cpu: &mut C) {
    cpu.set_y(cpu.y().wrapping_sub(1));
    update_zn(cpu, cpu.y());
}

// ---------------------------------------------------------------------------
// Shifts / Rotates - Accumulator (generic)
// ---------------------------------------------------------------------------

#[inline]
pub(crate) fn asl_acc<C: CpuRegs>(cpu: &mut C) {
    let v = cpu.a();
    set_flag(cpu, CARRY, (v & 0x80) != 0);
    cpu.set_a(v << 1);
    update_zn(cpu, cpu.a());
}

#[inline]
pub(crate) fn lsr_acc<C: CpuRegs>(cpu: &mut C) {
    let v = cpu.a();
    set_flag(cpu, CARRY, (v & 0x01) != 0);
    cpu.set_a(v >> 1);
    update_zn(cpu, cpu.a());
}

#[inline]
pub(crate) fn rol_acc<C: CpuRegs>(cpu: &mut C) {
    let v = cpu.a();
    let carry_in = if get_flag(cpu, CARRY) { 1 } else { 0 };
    set_flag(cpu, CARRY, (v & 0x80) != 0);
    cpu.set_a((v << 1) | carry_in);
    update_zn(cpu, cpu.a());
}

#[inline]
pub(crate) fn ror_acc<C: CpuRegs>(cpu: &mut C) {
    let v = cpu.a();
    let carry_in = if get_flag(cpu, CARRY) { 0x80 } else { 0 };
    set_flag(cpu, CARRY, (v & 0x01) != 0);
    cpu.set_a((v >> 1) | carry_in);
    update_zn(cpu, cpu.a());
}

// ---------------------------------------------------------------------------
// ADC / SBC (generic)
// ---------------------------------------------------------------------------

#[inline]
pub(crate) fn adc<C: CpuRegs>(cpu: &mut C, v: u8) {
    let a = cpu.a();
    let carry_in = if get_flag(cpu, CARRY) { 1 } else { 0 };
    let sum16 = a as u16 + v as u16 + carry_in as u16;
    let result = sum16 as u8;

    set_flag(cpu, CARRY, sum16 > 0xFF);
    // Overflow: ( !(A ^ M) & (A ^ R) & 0x80 ) != 0
    set_flag(cpu, OVERFLOW, ((!(a ^ v)) & (a ^ result) & 0x80) != 0);

    cpu.set_a(result);
    update_zn(cpu, cpu.a());
}

#[inline]
pub(crate) fn sbc<C: CpuRegs>(cpu: &mut C, v: u8) {
    adc(cpu, v ^ 0xFF);
}

// ---------------------------------------------------------------------------
// Compare (generic)
// ---------------------------------------------------------------------------

#[inline]
pub(crate) fn cmp_generic<C: CpuRegs>(cpu: &mut C, reg: u8, v: u8) {
    set_flag(cpu, CARRY, reg >= v);
    let r = reg.wrapping_sub(v);
    update_zn(cpu, r);
}

// ---------------------------------------------------------------------------
// Read-Modify-Write (memory) choreography (legacy)
// ---------------------------------------------------------------------------

/// Perform canonical 6502 RMW sequence: read -> dummy write old -> write new.
/// The two dummy microcycle ticks are issued here. Returns the final value.
pub(crate) fn rmw_memory<C: CpuRegs, F>(cpu: &mut C, bus: &mut Bus, addr: u16, transform: F) -> u8
where
    F: FnOnce(&mut C, u8) -> u8,
{
    let old = bus.read(addr);
    bus.tick(1);
    bus.write(addr, old);
    bus.tick(1);
    let newv = transform(cpu, old);
    bus.write(addr, newv);
    newv
}

// ---------------------------------------------------------------------------
// Shifts / Rotates - Memory (legacy)
// ---------------------------------------------------------------------------

#[inline]
pub(crate) fn asl_mem<C: CpuRegs>(cpu: &mut C, bus: &mut Bus, addr: u16) {
    let r = rmw_memory(cpu, bus, addr, |c, old| {
        set_flag(c, CARRY, (old & 0x80) != 0);
        old << 1
    });
    update_zn(cpu, r);
}

#[inline]
pub(crate) fn lsr_mem<C: CpuRegs>(cpu: &mut C, bus: &mut Bus, addr: u16) {
    let r = rmw_memory(cpu, bus, addr, |c, old| {
        set_flag(c, CARRY, (old & 0x01) != 0);
        old >> 1
    });
    update_zn(cpu, r);
}

#[inline]
pub(crate) fn rol_mem<C: CpuRegs>(cpu: &mut C, bus: &mut Bus, addr: u16) {
    let r = rmw_memory(cpu, bus, addr, |c, old| {
        let carry_in = if get_flag(c, CARRY) { 1 } else { 0 };
        set_flag(c, CARRY, (old & 0x80) != 0);
        (old << 1) | carry_in
    });
    update_zn(cpu, r);
}

#[inline]
pub(crate) fn ror_mem<C: CpuRegs>(cpu: &mut C, bus: &mut Bus, addr: u16) {
    let r = rmw_memory(cpu, bus, addr, |c, old| {
        let carry_in = if get_flag(c, CARRY) { 0x80 } else { 0 };
        set_flag(c, CARRY, (old & 0x01) != 0);
        (old >> 1) | carry_in
    });
    update_zn(cpu, r);
}

// ---------------------------------------------------------------------------
// INC / DEC memory (legacy)
// ---------------------------------------------------------------------------

#[inline]
pub(crate) fn inc_mem<C: CpuRegs>(cpu: &mut C, bus: &mut Bus, addr: u16) {
    let r = rmw_memory(cpu, bus, addr, |_, old| old.wrapping_add(1));
    update_zn(cpu, r);
}

#[inline]
pub(crate) fn dec_mem<C: CpuRegs>(cpu: &mut C, bus: &mut Bus, addr: u16) {
    let r = rmw_memory(cpu, bus, addr, |_, old| old.wrapping_sub(1));
    update_zn(cpu, r);
}

// ---------------------------------------------------------------------------
// Branch helpers (generic)
// ---------------------------------------------------------------------------

#[inline]
pub(crate) fn branch_offset<C: CpuRegs>(cpu: &mut C, offset: i8) {
    let new_pc = (cpu.pc() as i16).wrapping_add(offset as i16) as u16;
    cpu.set_pc(new_pc);
}

/// Fetch displacement, optionally apply branch, return extra cycles (1 or 2) if taken.
pub(crate) fn branch_cond<C: CpuRegs>(cpu: &mut C, bus: &mut Bus, take: bool) -> u32 {
    // Fetch displacement byte then advance PC.
    let pc = cpu.pc();
    let raw = bus.read(pc);
    cpu.advance_pc_one();
    let offset = raw as i8;

    if !take {
        return 0;
    }

    let old_pc = cpu.pc();
    branch_offset(cpu, offset);
    let mut extra = 1; // taken
    if (old_pc & 0xFF00) != (cpu.pc() & 0xFF00) {
        extra += 1;
    }
    extra
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cartridge::Cartridge;
    use crate::cpu::core::Cpu;
    use crate::test_utils::build_nrom_with_prg;

    fn setup() -> (Cpu, Bus) {
        let rom = build_nrom_with_prg(&[0xEA], 1, 1, None);
        let cart = Cartridge::from_ines_bytes(&rom).expect("parse");
        let mut bus = Bus::new();
        bus.attach_cartridge(cart);
        let mut cpu = Cpu::new();
        cpu.reset(&mut bus);
        (cpu, bus)
    }

    #[test]
    fn adc_overflow_and_carry() {
        let (mut cpu, mut _bus) = setup();
        cpu.set_a(0x50);
        adc(cpu.state_mut(), 0x50); // 0x50 + 0x50 = 0xA0 (signed overflow)
        assert!(get_flag(cpu.state(), OVERFLOW));
        assert!(!get_flag(cpu.state(), CARRY));
        cpu.set_a(0xF0);
        adc(cpu.state_mut(), 0x20); // 0xF0 + 0x20 = 0x110
        assert!(get_flag(cpu.state(), CARRY));
    }

    #[test]
    fn sbc_basic() {
        let (mut cpu, mut _bus) = setup();
        cpu.set_a(0x10);
        set_flag(cpu.state_mut(), CARRY, true); // Set for pure subtraction
        sbc(cpu.state_mut(), 0x01);
        assert_eq!(cpu.a(), 0x0F);
    }

    #[test]
    fn inc_mem_sequence() {
        let (mut cpu, mut bus) = setup();
        let addr = 0x0200;
        bus.write(addr, 0x0F);
        inc_mem(cpu.state_mut(), &mut bus, addr);
        assert_eq!(bus.read(addr), 0x10);
    }

    #[test]
    fn branch_cond_page_cross() {
        let (mut cpu, mut bus) = setup();
        bus.write(cpu.pc(), 0x02); // offset +2
        cpu.set_pc(0x80FF);
        let extra = branch_cond(cpu.state_mut(), &mut bus, true);
        assert_eq!(extra, 2);
        assert_eq!(cpu.pc(), 0x8101);
    }

    #[test]
    fn generic_register_ops() {
        // Exercise generic helpers on Cpu facade (CpuState implements CpuRegs).
        let (mut cpu, mut _bus) = setup();
        lda(cpu.state_mut(), 0x10);
        inx(cpu.state_mut()); // X still 0
        ldx(cpu.state_mut(), 0x01);
        inx(cpu.state_mut());
        assert_eq!(cpu.x(), 0x02);
        and(cpu.state_mut(), 0x00);
        assert_eq!(cpu.a(), 0x00);
        assert!(get_flag(cpu.state(), ZERO));
        ora(cpu.state_mut(), 0x80);
        assert_eq!(cpu.a(), 0x80);
        assert!(get_flag(cpu.state(), NEGATIVE));
        rol_acc(cpu.state_mut()); // 0x80 -> sets carry, A becomes 0x00
        assert_eq!(cpu.a(), 0x00);
        assert!(get_flag(cpu.state(), CARRY));
        assert!(get_flag(cpu.state(), ZERO));
    }
}
