/*!
rmw.rs - RMW / shift / increment / decrement opcode family handler

Overview
========
Implements all 6502 Read‑Modify‑Write (RMW) instructions, shifts / rotates, and
memory INC / DEC opcodes. Memory forms perform an internal read → dummy write
(old value) → write (new value) sequence inside the execute helpers. The
fallback dispatcher applies a global adjustment (subtract 2 from ticked cycles
for opcodes flagged by `is_rmw`) so the externally reported cycle count matches
expected 6502 timing while still ticking the bus once per instruction.

Covered Opcodes
---------------
Shifts / Rotates
  ASL: 0x0A (A), 0x06 (zp), 0x16 (zp,X), 0x0E (abs), 0x1E (abs,X)
  LSR: 0x4A (A), 0x46 (zp), 0x56 (zp,X), 0x4E (abs), 0x5E (abs,X)
  ROL: 0x2A (A), 0x26 (zp), 0x36 (zp,X), 0x2E (abs), 0x3E (abs,X)
  ROR: 0x6A (A), 0x66 (zp), 0x76 (zp,X), 0x6E (abs), 0x7E (abs,X)

Memory Increment / Decrement
  INC: 0xE6 (zp), 0xF6 (zp,X), 0xEE (abs), 0xFE (abs,X)
  DEC: 0xC6 (zp), 0xD6 (zp,X), 0xCE (abs), 0xDE (abs,X)

Responsibilities
================
- Decode only the listed opcodes.
- Use addressing helpers:
    * Zero Page       -> addr_zp
    * Zero Page,X     -> addr_zp_x
    * Absolute        -> addr_abs
    * Absolute,X      -> addr_abs_x  (no page‑cross penalty logic here)
- Invoke the appropriate execute helper (`asl_mem`, `rol_acc`, `inc_mem`, etc.).
- Return true if handled, false otherwise.

Non-Responsibilities
====================
- No bus ticking (the fallback dispatcher finalizes timing).
- No direct cycle arithmetic (RMW adjustment is centralized).
- No page-cross penalties (absolute,X RMW forms do not incur them).

Notes
=====
Accumulator forms do not perform memory addressing. Memory forms reuse the shared
addressing helpers; timing semantics rely on the dispatcher’s single tick +
adjustment strategy.

Testing
=======
Unit tests validate:
- Accumulator shift flag/result correctness.
- Memory INC/DEC behavior.
- Reported cycle parity vs `base_cycles(opcode)` (this handler does not modify cycles).

Future
======
Could be combined with a table-driven metadata layer once the fallback path is
fully supplanted for commonly executed opcodes.
*/

#![allow(dead_code)]

use crate::bus_impl::Bus;
use crate::cpu::regs::CpuRegs;

use crate::cpu::addressing::{addr_abs, addr_abs_x, addr_zp, addr_zp_x};
use crate::cpu::execute::{
    asl_acc, asl_mem, dec_mem, inc_mem, lsr_acc, lsr_mem, rol_acc, rol_mem, ror_acc, ror_mem,
};

/// Attempt to execute an RMW / shift / INC / DEC opcode.
/// Returns:
///   true  - if this family handled the opcode
///   false - otherwise (caller continues dispatch)
///
/// Contract:
/// - Caller has already: fetched opcode, advanced PC, set base cycles.
/// - This function MUST NOT tick the bus.
/// - This function MUST NOT modify the cycle count (dispatcher applies
///   RMW-specific tick adjustment globally).
pub(super) fn handle<C: CpuRegs>(
    opcode: u8,
    cpu: &mut C,
    bus: &mut Bus,
    _cycles: &mut u32,
) -> bool {
    match opcode {
        // -------- ASL --------
        0x0A => asl_acc(cpu),
        0x06 => {
            let a = addr_zp(cpu, bus);
            asl_mem(cpu, bus, a);
        }
        0x16 => {
            let a = addr_zp_x(cpu, bus);
            asl_mem(cpu, bus, a);
        }
        0x0E => {
            let a = addr_abs(cpu, bus);
            asl_mem(cpu, bus, a);
        }
        0x1E => {
            let a = addr_abs_x(cpu, bus);
            asl_mem(cpu, bus, a);
        }

        // -------- LSR --------
        0x4A => lsr_acc(cpu),
        0x46 => {
            let a = addr_zp(cpu, bus);
            lsr_mem(cpu, bus, a);
        }
        0x56 => {
            let a = addr_zp_x(cpu, bus);
            lsr_mem(cpu, bus, a);
        }
        0x4E => {
            let a = addr_abs(cpu, bus);
            lsr_mem(cpu, bus, a);
        }
        0x5E => {
            let a = addr_abs_x(cpu, bus);
            lsr_mem(cpu, bus, a);
        }

        // -------- ROL --------
        0x2A => rol_acc(cpu),
        0x26 => {
            let a = addr_zp(cpu, bus);
            rol_mem(cpu, bus, a);
        }
        0x36 => {
            let a = addr_zp_x(cpu, bus);
            rol_mem(cpu, bus, a);
        }
        0x2E => {
            let a = addr_abs(cpu, bus);
            rol_mem(cpu, bus, a);
        }
        0x3E => {
            let a = addr_abs_x(cpu, bus);
            rol_mem(cpu, bus, a);
        }

        // -------- ROR --------
        0x6A => ror_acc(cpu),
        0x66 => {
            let a = addr_zp(cpu, bus);
            ror_mem(cpu, bus, a);
        }
        0x76 => {
            let a = addr_zp_x(cpu, bus);
            ror_mem(cpu, bus, a);
        }
        0x6E => {
            let a = addr_abs(cpu, bus);
            ror_mem(cpu, bus, a);
        }
        0x7E => {
            let a = addr_abs_x(cpu, bus);
            ror_mem(cpu, bus, a);
        }

        // -------- INC (memory) --------
        0xE6 => {
            let a = addr_zp(cpu, bus);
            inc_mem(cpu, bus, a);
        }
        0xF6 => {
            let a = addr_zp_x(cpu, bus);
            inc_mem(cpu, bus, a);
        }
        0xEE => {
            let a = addr_abs(cpu, bus);
            inc_mem(cpu, bus, a);
        }
        0xFE => {
            let a = addr_abs_x(cpu, bus);
            inc_mem(cpu, bus, a);
        }

        // -------- DEC (memory) --------
        0xC6 => {
            let a = addr_zp(cpu, bus);
            dec_mem(cpu, bus, a);
        }
        0xD6 => {
            let a = addr_zp_x(cpu, bus);
            dec_mem(cpu, bus, a);
        }
        0xCE => {
            let a = addr_abs(cpu, bus);
            dec_mem(cpu, bus, a);
        }
        0xDE => {
            let a = addr_abs_x(cpu, bus);
            dec_mem(cpu, bus, a);
        }

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
    fn asl_accumulator_basic() {
        // LDA #$81; ASL A; BRK
        let (mut cpu, mut bus) = setup(&[0xA9, 0x81, 0x0A, 0x00]);
        assert_eq!(cpu.step(&mut bus), 2); // LDA
        let c = cpu.step(&mut bus); // ASL A
        assert_eq!(c, base_cycles(0x0A));
        assert_eq!(cpu.a(), 0x02);
    }

    #[test]
    fn inc_zeropage() {
        // LDA #$00; STA $10; INC $10; BRK
        let (mut cpu, mut bus) = setup(&[0xA9, 0x00, 0x85, 0x10, 0xE6, 0x10, 0x00]);
        assert_eq!(cpu.step(&mut bus), 2); // LDA
        assert_eq!(cpu.step(&mut bus), 3); // STA
        let cycles = cpu.step(&mut bus); // INC zp
        assert_eq!(cycles, base_cycles(0xE6));
        assert_eq!(bus.read(0x0010), 0x01);
    }

    #[test]
    fn dec_abs_x() {
        // LDX #$01; LDA #$05; STA $2000,X; DEC $2000,X; BRK
        let (mut cpu, mut bus) = setup(&[
            0xA2, 0x01, 0xA9, 0x05, 0x9D, 0x00, 0x20, 0xDE, 0x00, 0x20, 0x00,
        ]);
        assert_eq!(cpu.step(&mut bus), 2); // LDX
        assert_eq!(cpu.step(&mut bus), 2); // LDA
        assert_eq!(cpu.step(&mut bus), 5); // STA abs,X
        let c = cpu.step(&mut bus); // DEC abs,X
        assert_eq!(c, base_cycles(0xDE));
        assert_eq!(bus.read(0x2001), 0x04);
    }

    #[test]
    fn rmw_cycle_parity_example() {
        // Single INC zp instruction followed by BRK; confirm reported cycles match base_cycles.
        let (mut cpu, mut bus) = setup(&[0xE6, 0x20, 0x00]);
        let c = cpu.step(&mut bus);
        assert_eq!(c, base_cycles(0xE6));
    }
}
