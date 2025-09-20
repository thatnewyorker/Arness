/*!
branches.rs - Relative branch opcode handler (BPL/BMI/BVC/BVS/BCC/BCS/BNE/BEQ)

Overview
========
Executes all conditional relative branch instructions. Responsibilities:
- Compute the branch condition.
- Fetch the displacement and, if taken, update PC.
- Detect page boundary crossing and apply +1 (taken) / +2 (taken + cross) cycle adjustments
  by adding the value returned from `branch_cond` to *cycles.

Cycle Rules
===========
Base cost: 2 cycles.
If branch is taken: +1 cycle.
If branch is taken and target crosses a page boundary: +2 total (vs base).

Caller Requirements
===================
The fallback dispatcher must:
- Fetch the opcode and advance PC before invoking `handle`.
- Initialize *cycles using `base_cycles(opcode)` (2 for branch opcodes).
- Tick the bus after finalization (handlers never tick).

Return Contract
===============
`handle` returns:
- true  => opcode recognized, extra cycles already added to *cycles.
- false => not a branch opcode.

Notes
=====
No RMW behavior applies. All timing side-effects (bus tick, possible future profiling)
are handled centrally by the dispatcher.
*/

#![allow(dead_code)]

use crate::bus_impl::Bus;
use crate::cpu::execute::{branch_cond, get_flag};
use crate::cpu::regs::CpuRegs;
use crate::cpu::state::{CARRY, NEGATIVE, OVERFLOW, ZERO};

/// Attempt to execute a branch opcode.
/// Returns:
///   true  - opcode recognized and executed (extra cycles applied)
///   false - not a branch opcode
///
/// Behavior:
/// - Adds the value returned by `branch_cond` (0, 1, or 2) to *cycles.
/// - `branch_cond` internally fetches the displacement and updates PC if taken.
pub(super) fn handle<C: CpuRegs>(opcode: u8, cpu: &mut C, bus: &mut Bus, cycles: &mut u32) -> bool {
    let extra = match opcode {
        0x10 => branch_cond(cpu, bus, !get_flag(cpu, NEGATIVE)), // BPL
        0x30 => branch_cond(cpu, bus, get_flag(cpu, NEGATIVE)),  // BMI
        0x50 => branch_cond(cpu, bus, !get_flag(cpu, OVERFLOW)), // BVC
        0x70 => branch_cond(cpu, bus, get_flag(cpu, OVERFLOW)),  // BVS
        0x90 => branch_cond(cpu, bus, !get_flag(cpu, CARRY)),    // BCC
        0xB0 => branch_cond(cpu, bus, get_flag(cpu, CARRY)),     // BCS
        0xD0 => branch_cond(cpu, bus, !get_flag(cpu, ZERO)),     // BNE
        0xF0 => branch_cond(cpu, bus, get_flag(cpu, ZERO)),      // BEQ
        _ => return false,
    };
    *cycles += extra;
    true
}

#[cfg(test)]
mod tests {
    use crate::bus_impl::Bus;
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
    fn branch_not_taken_base_cycles() {
        // BCS (carry set?) but carry is clear after reset, so not taken.
        // Program: BCS +0x02; NOP; BRK
        let (mut cpu, mut bus) = setup(&[0xB0, 0x02, 0xEA, 0x00]);
        let c = cpu.step(&mut bus);
        assert_eq!(c, 2); // not taken => base cycles
    }

    #[test]
    fn branch_taken_no_page_cross() {
        // BCC (carry clear) small forward offset that does not cross boundary
        // Program: BCC +0x02; NOP (skipped if taken target points after); BRK
        let (mut cpu, mut bus) = setup(&[0x90, 0x02, 0xEA, 0x00]);
        let c = cpu.step(&mut bus);
        assert_eq!(c, 3); // taken, no cross => base (2) + 1
    }

    #[test]
    fn branch_taken_page_cross() {
        // Fill to near page end so branch target crosses.
        // Layout: 0x00..0xFE: NOP, 0xFF: BCC +0x01 (opcode + operand at boundary),
        // then target at next page (0x0101 relative position).
        let mut prg = vec![];
        prg.extend(std::iter::repeat(0xEA).take(0x00FF)); // NOP padding to byte 0x00FE
        prg.push(0x90); // BCC
        prg.push(0x01); // displacement -> cross to next page
        prg.push(0x00); // BRK at target (simplify)
        let (mut cpu, mut bus) = setup(&prg);
        // Consume the padding NOPs
        for _ in 0..0x00FF {
            assert_eq!(cpu.step(&mut bus), 2);
        }
        // Branch: taken + page cross => 4 cycles
        assert_eq!(cpu.step(&mut bus), 4);
    }

    #[test]
    fn branch_taken_sets_pc_correctly() {
        // BNE +0x02 (Z clear after reset) should skip over one byte (NOP) to BRK
        let (mut cpu, mut bus) = setup(&[0xD0, 0x02, 0xEA, 0x00]);
        let c = cpu.step(&mut bus);
        assert_eq!(c, 3); // taken, no cross
        // Next step should be BRK (0x00) not NOP
        let c2 = cpu.step(&mut bus);
        // BRK consumes 7 cycles (legacy semantics) and sets halted
        assert_eq!(c2, 7);
        assert!(cpu.is_halted());
    }
}
