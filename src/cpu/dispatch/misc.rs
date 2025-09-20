/*!
misc.rs - Transfers / Stack / Flag opcode family handler

Overview
========
Handles small, fixed‑cycle instructions that move register values, manipulate
the stack, or set/clear individual processor status flags:

Transfers:
  TAX (0xAA), TAY (0xA8), TXA (0x8A), TYA (0x98), TSX (0xBA), TXS (0x9A)

Stack:
  PHA (0x48), PLA (0x68), PHP (0x08), PLP (0x28)

Flag operations:
  CLC (0x18), SEC (0x38),
  CLI (0x58), SEI (0x78),
  CLD (0xD8), SED (0xF8),
  CLV (0xB8)

Responsibilities
================
- Decode and execute the above opcodes.
- Update registers, stack, and flags via shared execute helpers.
- Return true when an opcode is handled so the fallback dispatcher can finalize timing.

Timing
======
- All listed opcodes have fixed base cycles (already set by the caller).
- This handler does not alter *cycles.
- No bus ticking occurs here; the fallback dispatcher performs a single tick after finalization.

Non-Responsibilities
====================
- No page-cross penalties (none apply).
- No RMW cycle adjustments (none of these are RMW instructions).
- No additional PC manipulation beyond what individual helpers inherently perform.

Return Contract
===============
handle(...) returns:
- true  => opcode recognized and executed
- false => not part of this family; dispatcher should continue with other handlers
*/

#![allow(dead_code)]

use crate::bus_impl::Bus;
use crate::cpu::regs::CpuRegs;

use crate::cpu::execute::{pha, php, pla, plp, set_flag, tax, tay, tsx, txa, txs, tya};
use crate::cpu::state::{CARRY, DECIMAL, IRQ_DISABLE, OVERFLOW};

/// Attempt to execute a miscellaneous (transfer / stack / flag) opcode.
///
/// Returns:
///   true  - opcode handled here
///   false - not part of this family; caller should continue dispatch
///
/// Contract:
/// - Caller has already advanced PC past opcode and set *cycles = base_cycles(opcode).
/// - This function must NOT tick the bus or change *cycles.
pub(super) fn handle<C: CpuRegs>(
    opcode: u8,
    cpu: &mut C,
    bus: &mut Bus,
    _cycles: &mut u32,
) -> bool {
    match opcode {
        // -------- Transfers --------
        0xAA => tax(cpu),
        0xA8 => tay(cpu),
        0x8A => txa(cpu),
        0x98 => tya(cpu),
        0xBA => tsx(cpu),
        0x9A => txs(cpu),

        // -------- Stack --------
        0x48 => pha(cpu, bus),
        0x68 => pla(cpu, bus),
        0x08 => php(cpu, bus),
        0x28 => plp(cpu, bus),

        // -------- Flags --------
        0x18 => set_flag(cpu, CARRY, false),       // CLC
        0x38 => set_flag(cpu, CARRY, true),        // SEC
        0x58 => set_flag(cpu, IRQ_DISABLE, false), // CLI
        0x78 => set_flag(cpu, IRQ_DISABLE, true),  // SEI
        0xD8 => set_flag(cpu, DECIMAL, false),     // CLD
        0xF8 => set_flag(cpu, DECIMAL, true),      // SED
        0xB8 => set_flag(cpu, OVERFLOW, false),    // CLV

        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use crate::bus_impl::Bus;
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
    fn transfers_sequence() {
        // LDA #$05; TAX; TAY; TXA; TYA; TSX; TXS; BRK
        let (mut cpu, mut bus) = setup(&[0xA9, 0x05, 0xAA, 0xA8, 0x8A, 0x98, 0xBA, 0x9A, 0x00]);
        // LDA
        assert_eq!(cpu.step(&mut bus), 2);
        // TAX
        assert_eq!(cpu.step(&mut bus), base_cycles(0xAA));
        assert_eq!(cpu.x(), 0x05);
        // TAY
        assert_eq!(cpu.step(&mut bus), base_cycles(0xA8));
        assert_eq!(cpu.y(), 0x05);
        // TXA
        assert_eq!(cpu.step(&mut bus), base_cycles(0x8A));
        assert_eq!(cpu.a(), 0x05);
        // TYA
        assert_eq!(cpu.step(&mut bus), base_cycles(0x98));
        // TSX
        assert_eq!(cpu.step(&mut bus), base_cycles(0xBA));
        // TXS
        assert_eq!(cpu.step(&mut bus), base_cycles(0x9A));
        // BRK
        assert_eq!(cpu.step(&mut bus), 7);
    }

    #[test]
    fn stack_push_pop() {
        // LDA #$AB; PHA; LDA #$00; PLA; BRK
        let (mut cpu, mut bus) = setup(&[0xA9, 0xAB, 0x48, 0xA9, 0x00, 0x68, 0x00]);
        assert_eq!(cpu.step(&mut bus), 2); // LDA #$AB
        let sp_after_lda = cpu.sp();
        assert_eq!(cpu.step(&mut bus), base_cycles(0x48)); // PHA
        assert!(cpu.sp() < sp_after_lda);
        assert_eq!(cpu.step(&mut bus), 2); // LDA #$00
        assert_eq!(cpu.a(), 0x00);
        assert_eq!(cpu.step(&mut bus), base_cycles(0x68)); // PLA
        assert_eq!(cpu.a(), 0xAB);
        // BRK
        assert_eq!(cpu.step(&mut bus), 7);
    }

    #[test]
    fn php_plp_round_trip_flags() {
        // SEC; PHP; CLC; PLP; BRK
        let (mut cpu, mut bus) = setup(&[0x38, 0x08, 0x18, 0x28, 0x00]);
        // SEC sets carry
        assert_eq!(cpu.step(&mut bus), base_cycles(0x38));
        // PHP pushes status
        assert_eq!(cpu.step(&mut bus), base_cycles(0x08));
        // CLC clears carry
        assert_eq!(cpu.step(&mut bus), base_cycles(0x18));
        // PLP restores status (carry should be set again)
        assert_eq!(cpu.step(&mut bus), base_cycles(0x28));
        // BRK
        assert_eq!(cpu.step(&mut bus), 7);
    }

    #[test]
    fn flag_ops_basic() {
        // SEC; CLC; SEI; CLI; SED; CLD; CLV; BRK
        let (mut cpu, mut bus) = setup(&[0x38, 0x18, 0x78, 0x58, 0xF8, 0xD8, 0xB8, 0x00]);
        assert_eq!(cpu.step(&mut bus), base_cycles(0x38)); // SEC
        assert_eq!(cpu.step(&mut bus), base_cycles(0x18)); // CLC
        assert_eq!(cpu.step(&mut bus), base_cycles(0x78)); // SEI
        assert_eq!(cpu.step(&mut bus), base_cycles(0x58)); // CLI
        assert_eq!(cpu.step(&mut bus), base_cycles(0xF8)); // SED
        assert_eq!(cpu.step(&mut bus), base_cycles(0xD8)); // CLD
        assert_eq!(cpu.step(&mut bus), base_cycles(0xB8)); // CLV
        assert_eq!(cpu.step(&mut bus), 7); // BRK
    }
}
