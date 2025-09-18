/*!
cycles.rs - Cycle timing helpers for the 6502 CPU core.

Purpose
=======
Provides:
  - `base_cycles(op)`  : The baseline cycle count for a single opcode
                         (not including conditional page‑cross or branch
                         penalties, nor +1 for taken branches or
                         penalties added dynamically in dispatch).
  - `is_rmw(op)`       : Identifies opcodes that perform a
                         Read‑Modify‑Write bus choreography so the
                         dispatcher can adjust how many cycles are
                         "ticked" vs. "returned".

Scope
=====
This module is intentionally minimal and purely data/logic. All dynamic
cycle adjustments (page crossing, branch taken, branch page cross) are
handled in the dispatcher layer. RMW microcycle ticks (dummy writes) are
initiated by the execution helpers; the fallback dispatcher subtracts 2 from
the ticked cycles for RMW instructions so the externally reported cycle
count matches expected 6502 timing.

Future Improvements
===================
- Replace the big `match` with a compact 256‑entry static array for
  constant‑time lookup (the compiler often optimizes the match into a
  jump table or lookup anyway; readability trade‑off).
- Add feature‑gated alternative tables for undocumented opcodes or
  cycle‑exact microcycle tracing.
- Potentially encode RMW / branch / page‑penalty flags alongside base
  cycles in a single packed table for the table‑driven dispatcher.

Design Notes
============
- Only documented opcodes implemented so far are enumerated. Unlisted
  opcodes default to 2 cycles (historical 6502-compatible assumption).
- This module does NOT attempt to validate legality of opcodes; unknown
  opcodes are the caller's responsibility to handle (e.g., halting).

*/

#![allow(dead_code)]

/// Return the base cycle count for a 6502 opcode (documented set).
/// Page-cross penalties (+1) and branch penalties (+1 taken, +1 page cross)
/// must be added by the dispatcher, not here.
pub(crate) fn base_cycles(op: u8) -> u32 {
    match op {
        // Loads
        0xA9 => 2, // LDA #imm
        0xA5 => 3,
        0xB5 => 4,
        0xAD => 4,
        0xBD => 4,
        0xB9 => 4,
        0xA1 => 6,
        0xB1 => 5, // LDA
        0xA2 => 2,
        0xA6 => 3,
        0xB6 => 4,
        0xAE => 4,
        0xBE => 4, // LDX
        0xA0 => 2,
        0xA4 => 3,
        0xB4 => 4,
        0xAC => 4,
        0xBC => 4, // LDY

        // Stores
        0x85 => 3,
        0x95 => 4,
        0x8D => 4,
        0x9D => 5,
        0x99 => 5,
        0x81 => 6,
        0x91 => 6, // STA
        0x86 => 3,
        0x96 => 4,
        0x8E => 4, // STX
        0x84 => 3,
        0x94 => 4,
        0x8C => 4, // STY

        // Transfers
        0xAA | 0xA8 | 0x8A | 0x98 | 0xBA | 0x9A => 2,

        // Stack
        0x48 => 3,
        0x68 => 4,
        0x08 => 3,
        0x28 => 4,

        // Increment / Decrement (register)
        0xE8 | 0xC8 | 0xCA | 0x88 => 2,

        // Inc/Dec memory
        0xE6 => 5,
        0xF6 => 6,
        0xEE => 6,
        0xFE => 7, // INC
        0xC6 => 5,
        0xD6 => 6,
        0xCE => 6,
        0xDE => 7, // DEC

        // Logical AND
        0x29 => 2,
        0x25 => 3,
        0x35 => 4,
        0x2D => 4,
        0x3D => 4,
        0x39 => 4,
        0x21 => 6,
        0x31 => 5,
        // ORA
        0x09 => 2,
        0x05 => 3,
        0x15 => 4,
        0x0D => 4,
        0x1D => 4,
        0x19 => 4,
        0x01 => 6,
        0x11 => 5,
        // EOR
        0x49 => 2,
        0x45 => 3,
        0x55 => 4,
        0x4D => 4,
        0x5D => 4,
        0x59 => 4,
        0x41 => 6,
        0x51 => 5,

        // BIT
        0x24 => 3,
        0x2C => 4,

        // Shifts / Rotates (accumulator / memory)
        0x0A => 2,
        0x06 => 5,
        0x16 => 6,
        0x0E => 6,
        0x1E => 7, // ASL
        0x4A => 2,
        0x46 => 5,
        0x56 => 6,
        0x4E => 6,
        0x5E => 7, // LSR
        0x2A => 2,
        0x26 => 5,
        0x36 => 6,
        0x2E => 6,
        0x3E => 7, // ROL
        0x6A => 2,
        0x66 => 5,
        0x76 => 6,
        0x6E => 6,
        0x7E => 7, // ROR

        // Flags
        0x18 | 0x38 | 0x58 | 0x78 | 0xD8 | 0xF8 | 0xB8 => 2,

        // Compare
        0xC9 => 2,
        0xC5 => 3,
        0xD5 => 4,
        0xCD => 4,
        0xDD => 4,
        0xD9 => 4,
        0xC1 => 6,
        0xD1 => 5, // CMP
        0xE0 => 2,
        0xE4 => 3,
        0xEC => 4, // CPX
        0xC0 => 2,
        0xC4 => 3,
        0xCC => 4, // CPY

        // Branches (base cycles only)
        0x10 | 0x30 | 0x50 | 0x70 | 0x90 | 0xB0 | 0xD0 | 0xF0 => 2,

        // Jumps / Subroutines / Returns
        0x4C => 3,
        0x6C => 5,
        0x20 => 6,
        0x60 => 6,

        // ADC
        0x69 => 2,
        0x65 => 3,
        0x75 => 4,
        0x6D => 4,
        0x7D => 4,
        0x79 => 4,
        0x61 => 6,
        0x71 => 5,
        // SBC
        0xE9 => 2,
        0xE5 => 3,
        0xF5 => 4,
        0xED => 4,
        0xFD => 4,
        0xF9 => 4,
        0xE1 => 6,
        0xF1 => 5,

        // Interrupts / BRK / RTI / NOP
        0x00 => 7,
        0x40 => 6,
        0xEA => 2,

        // Default fallback (unimplemented/undocumented)
        _ => 2,
    }
}

/// Return true if the opcode is a Read‑Modify‑Write (memory) instruction.
/// Used so dispatch can adjust ticked vs returned cycles while the RMW
/// helper itself performs the extra dummy write microcycles.
pub(crate) fn is_rmw(op: u8) -> bool {
    matches!(
        op,
        0x06 | 0x16 | 0x0E | 0x1E | // ASL zp,zpX,abs,absX
        0x46 | 0x56 | 0x4E | 0x5E | // LSR zp,zpX,abs,absX
        0x26 | 0x36 | 0x2E | 0x3E | // ROL zp,zpX,abs,absX
        0x66 | 0x76 | 0x6E | 0x7E | // ROR zp,zpX,abs,absX
        0xE6 | 0xF6 | 0xEE | 0xFE | // INC zp,zpX,abs,absX
        0xC6 | 0xD6 | 0xCE | 0xDE // DEC zp,zpX,abs,absX
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycles_basic_examples() {
        assert_eq!(base_cycles(0xA9), 2); // LDA #imm
        assert_eq!(base_cycles(0x9D), 5); // STA abs,X
        assert_eq!(base_cycles(0x4C), 3); // JMP abs
        assert_eq!(base_cycles(0x00), 7); // BRK
    }

    #[test]
    fn rmw_detection() {
        // ASL abs,X (RMW)
        assert!(is_rmw(0x1E));
        // LDA immediate (not RMW)
        assert!(!is_rmw(0xA9));
        // INC zp (RMW)
        assert!(is_rmw(0xE6));
    }

    #[test]
    fn fallback_default() {
        // An arbitrary undocumented opcode (e.g., 0x02) defaults to 2 cycles here.
        assert_eq!(base_cycles(0x02), 2);
    }
}
