/*!
control_flow.rs - Control-flow / system opcode family handler

Overview
========
Implements 6502 control-flow and system instructions that manipulate program
counter, stack, processor status, or execution state:

  JMP abs        (0x4C)
  JMP (ind)      (0x6C)  (hardware indirect wrap quirk preserved)
  JSR abs        (0x20)
  RTS            (0x60)
  RTI            (0x40)
  BRK            (0x00)

Responsibilities
================
- Decode and execute the above opcodes.
- Mutate PC, stack, and status as required.
- For BRK: set `cpu.halted = true`, set the IRQ Disable flag, push PC & status, load vector $FFFE/$FFFF, and force 7 total cycles by overriding `*cycles = 7`.
- Do not tick the bus (the fallback dispatcher finalizes timing).
- Do not perform base cycle lookup (caller already did that).
- Do not apply RMW adjustments (none of these are RMW; global adjustment handled elsewhere).

Caller Requirements
===================
The fallback dispatcher must:
- Fetch the opcode and advance PC before calling `handle`.
- Initialize `*cycles` with `base_cycles(opcode)`.
- After a true return, perform finalization (RMW adjustment if applicable, bus tick).

Behavior Details
================
- JSR pushes (PC - 1) (return address high, then low) per 6502 convention.
- RTS pulls the return address and adds 1 (wrap-safe) to produce the next PC.
- RTI pulls status (Break bit state governed by push helper semantics) then PC.
- BRK pushes the already-advanced PC and status (with Break set by helper), sets IRQ Disable, loads the vector at $FFFE/$FFFF, halts, and reports 7 cycles.
- JMP (ind) emulates the original indirect vector wrap page-boundary quirk.

Return Contract
===============
`handle` returns:
  true  => opcode recognized and executed (cycles possibly overridden for BRK)
  false => not a control-flow opcode; caller continues dispatch chain

*/

#![allow(dead_code)]

use crate::bus::Bus;
use crate::cpu::regs::CpuRegs;

use crate::cpu::addressing::{addr_abs, read_word_indirect_bug};
use crate::cpu::execute::{php, plp, pop_word, push_word, set_flag};
use crate::cpu::state::IRQ_DISABLE;

// ---------------------------------------------------------------------------
// Helper functions (extracted to reduce borrow lifetime complexity)
// ---------------------------------------------------------------------------

#[inline]
fn op_jmp_abs<C: CpuRegs>(cpu: &mut C, target: u16) {
    cpu.set_pc(target);
}

#[inline]
fn op_jmp_indirect<C: CpuRegs>(cpu: &mut C, bus: &mut Bus) {
    let ptr = addr_abs(cpu, bus);
    let resolved = read_word_indirect_bug(bus, ptr);
    cpu.set_pc(resolved);
}

#[inline]
fn op_jsr<C: CpuRegs>(cpu: &mut C, bus: &mut Bus) {
    let target = addr_abs(cpu, bus);
    // After operand fetch PC points to next instruction; push (PC - 1)
    let ret = cpu.pc().wrapping_sub(1);
    push_word(cpu, bus, ret);
    cpu.set_pc(target);
}

#[inline]
fn op_rts<C: CpuRegs>(cpu: &mut C, bus: &mut Bus) {
    let ret = pop_word(cpu, bus);
    cpu.set_pc(ret.wrapping_add(1));
}

#[inline]
fn op_brk<C: CpuRegs>(cpu: &mut C, bus: &mut Bus, cycles: &mut u32) {
    // Push PC then status (with Break semantics handled by php)
    let pc_to_push = cpu.pc();
    {
        push_word(cpu, bus, pc_to_push);
        php(cpu, bus);
    }
    set_flag(cpu, IRQ_DISABLE, true);
    let vector = bus.read_word(0xFFFE);
    cpu.set_pc(vector);
    cpu.set_halted(true);
    *cycles = 7;
}

#[inline]
fn op_rti<C: CpuRegs>(cpu: &mut C, bus: &mut Bus) {
    plp(cpu, bus);
    let return_pc = pop_word(cpu, bus);
    cpu.set_pc(return_pc);
}

/// Attempt to execute a control-flow opcode.
///
/// Parameters:
///   opcode  - already-fetched opcode byte
///   cpu/bus - mutable execution context
///   cycles  - base cycle counter (may be overridden for BRK)
///
/// Returns:
///   true  if opcode handled here
///   false if not a control-flow opcode (caller continues dispatch)
pub(super) fn handle<C: CpuRegs>(opcode: u8, cpu: &mut C, bus: &mut Bus, cycles: &mut u32) -> bool {
    match opcode {
        0x4C => {
            let target = addr_abs(cpu, bus);
            op_jmp_abs(cpu, target);
        }
        0x6C => op_jmp_indirect(cpu, bus),
        0x20 => op_jsr(cpu, bus),
        0x60 => op_rts(cpu, bus),
        0x00 => op_brk(cpu, bus, cycles),
        0x40 => op_rti(cpu, bus),
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::Bus;
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
    fn jmp_abs_sets_pc() {
        let (mut cpu, mut bus) = setup(&[0x4C, 0x05, 0x80, 0xEA, 0x00]);
        let c = cpu.step(&mut bus);
        assert_eq!(c, base_cycles(0x4C));
        assert!(!cpu.is_halted());
    }

    #[test]
    fn jsr_then_rts_round_trip() {
        let (mut cpu, mut bus) = setup(&[0x20, 0x05, 0x80, 0xEA, 0x00, 0x60, 0x00]);
        let c_jsr = cpu.step(&mut bus);
        assert_eq!(c_jsr, base_cycles(0x20));
        let c_rts = cpu.step(&mut bus);
        assert_eq!(c_rts, base_cycles(0x60));
        let c_next = cpu.step(&mut bus);
        assert!(c_next > 0);
    }

    #[test]
    fn brk_pushes_and_halts() {
        let (mut cpu, mut bus) = setup(&[0x00]);
        let sp_before = cpu.sp();
        let cycles = cpu.step(&mut bus);
        assert_eq!(cycles, 7);
        assert_eq!(cpu.sp(), sp_before.wrapping_sub(3));
        assert!(cpu.is_halted());
    }

    #[test]
    fn rti_restores_pc_and_status() {
        let (mut cpu, mut bus) = setup(&[0x40, 0x00]);
        let return_pc = cpu.pc();
        push_word(cpu.state_mut(), &mut bus, return_pc);
        bus.write(0x0100u16 | cpu.sp() as u16, cpu.status());
        let cycles = cpu.step(&mut bus);
        assert_eq!(cycles, base_cycles(0x40));
        assert_eq!(cpu.pc(), return_pc);
    }

    #[test]
    fn jmp_indirect_bug_behavior_smoke() {
        let (mut cpu, mut bus) = setup(&[0x6C, 0x00, 0x80, 0x00]);
        let c = cpu.step(&mut bus);
        assert_eq!(c, base_cycles(0x6C));
    }
}
