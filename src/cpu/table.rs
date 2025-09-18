/*!
table.rs - Feature-gated table driven opcode metadata + lightweight dispatcher.

Purpose
=======
This module houses a compact, data-driven description of (currently a
subset of) 6502 opcodes plus a helper that can execute supported
instructions. It is an incremental extraction from the legacy
earlier monolithic implementation (now removed). The intent is to grow
coverage until the large match-based legacy dispatcher can be retired.

Design Goals
------------
1. Keep the table self-contained behind the `table_dispatch` feature.
2. Depend only on modular helpers (`addressing`, `execute`) for
   operand resolution and instruction semantics.
3. Provide a single entry point (`try_table_step`) returning
   `Option<cycles>`: `None` indicates the opcode is not yet table
   backed and the caller must fall back to the legacy dispatcher.
4. Accurately model page-cross penalties for the supported opcodes.

Current Scope
-------------
Implements a small subset (LDA addressing family + a few flag/NOP
instructions) mirroring the earlier prototype found in the legacy file.

Future Steps
------------
- Gradually add the remaining opcodes (populate EXEC_TABLE).
- Add RMW and branch handling (rmw + branch flags already present in
  OpInfo for future use).
- Once coverage complete, the legacy dispatcher becomes a secondary
  fallback only for undocumented opcodes (if desired) and eventually
  can be removed (with an optional feature for undocumented ops).

Cycle Handling
--------------
Base cycles + (optional) +1 on page cross (for indexed modes that
incur a penalty) are applied here. RMW internal micro-cycle adjustments
are not yet needed for the migrated subset (rmw=false entries).

Safety / Parity
---------------
Logic mirrors the semantics already exercised by tests for the
legacy path. Parity can be validated by executing the same short
programs with table_dispatch enabled and disabled.

*/

#![allow(dead_code)]

use crate::bus::Bus;
use crate::cpu::regs::CpuRegs;

#[cfg(feature = "table_dispatch")]
use crate::cpu::{
    addressing::{
        addr_abs, addr_abs_x_pc, addr_abs_y_pc, addr_ind_x, addr_ind_y_pc, addr_zp, addr_zp_x,
        fetch_byte,
    },
    execute::{lda, set_flag},
    state::{CARRY, ZERO},
};

/// Public (crate) entry point: attempt a table-dispatch of `opcode`.
/// Returns Some(cycles_consumed) if handled; None if the caller
/// should fall back to the legacy match dispatcher.
#[cfg(feature = "table_dispatch")]
pub(crate) fn try_table_step<C: CpuRegs>(cpu: &mut C, bus: &mut Bus, opcode: u8) -> Option<u32> {
    let entry = &EXEC_TABLE[opcode as usize];
    if matches!(entry.kind, ExecKind::Fallback) {
        return None;
    }

    // Base cycles
    let mut cycles = entry.base as u32;

    // Resolve operand and track page cross when table row indicates it might matter.
    let mut page_crossed = false;

    // Operand fetch / addressing
    let operand_kind = entry.mode;
    let resolved_value: Option<u8>; // For immediate loads
    let mut effective_addr: Option<u16> = None;

    use AddrMode::*;
    resolved_value = match operand_kind {
        Implied => None,
        Acc => None,
        Imm => Some(fetch_byte(cpu, bus)),
        Zp => {
            effective_addr = Some(addr_zp(cpu, bus));
            None
        }
        ZpX => {
            effective_addr = Some(addr_zp_x(cpu, bus));
            None
        }
        ZpY => {
            // Not yet used in migrated subset (future)
            effective_addr = Some({
                // Reuse zp_x logic placeholder if needed later; not migrating LDY zp,Y yet.
                addr_zp_x(cpu, bus) // placeholder; real zp,y variant exists if imported
            });
            None
        }
        Abs => {
            effective_addr = Some(addr_abs(cpu, bus));
            None
        }
        AbsX => {
            let (a, crossed) = addr_abs_x_pc(cpu, bus);
            effective_addr = Some(a);
            page_crossed = crossed;
            None
        }
        AbsY => {
            let (a, crossed) = addr_abs_y_pc(cpu, bus);
            effective_addr = Some(a);
            page_crossed = crossed;
            None
        }
        Ind => {
            // Not used in current subset (JMP (ind) not migrated yet)
            return None;
        }
        IndX => {
            effective_addr = Some(addr_ind_x(cpu, bus));
            None
        }
        IndY => {
            let (a, crossed) = addr_ind_y_pc(cpu, bus);
            effective_addr = Some(a);
            page_crossed = crossed;
            None
        }
        Rel => {
            // Branch operands not yet table-migrated
            return None;
        }
    };

    // Page-cross penalty
    if entry.page_cross_penalty && page_crossed {
        cycles += 1;
    }

    // Execute semantics
    use ExecKind::*;
    match entry.kind {
        Lda => {
            if let Some(v) = resolved_value {
                lda(cpu, v);
            } else if let Some(addr) = effective_addr {
                let v = bus.read(addr);
                lda(cpu, v);
            } else {
                return None; // malformed row for LDA
            }
        }
        Clc => set_flag(cpu, CARRY, false),
        Sec => set_flag(cpu, CARRY, true),
        Nop => { /* no-op */ }
        // The subset intentionally excludes other kinds for now:
        _ => return None,
    }

    // Tick total cycles (table path handles its own timing)
    bus.tick(cycles);
    Some(cycles)
}

#[cfg(not(feature = "table_dispatch"))]
pub(crate) fn try_table_step<C: CpuRegs>(_cpu: &mut C, _bus: &mut Bus, _opcode: u8) -> Option<u32> {
    None
}

#[cfg(feature = "table_dispatch")]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum AddrMode {
    Implied,
    Acc,
    Imm,
    Zp,
    ZpX,
    ZpY,
    Abs,
    AbsX,
    AbsY,
    Ind,
    IndX,
    IndY,
    Rel,
}

#[cfg(feature = "table_dispatch")]
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum ExecKind {
    Lda,
    Ldx,
    Ldy,
    Sta,
    Stx,
    Sty,
    And,
    Ora,
    Eor,
    Bit,
    Adc,
    Sbc,
    Inx,
    Iny,
    Dex,
    Dey,
    Tax,
    Tay,
    Txa,
    Tya,
    Tsx,
    Txs,
    Asl,
    Lsr,
    Rol,
    Ror,
    Inc,
    Dec,
    CmpA,
    CmpX,
    CmpY,
    Clc,
    Sec,
    Cli,
    Sei,
    Cld,
    Sed,
    Clv,
    Branch,
    JmpAbs,
    JmpInd,
    Jsr,
    Rts,
    Brk,
    Rti,
    Nop,
    Fallback,
}

#[cfg(feature = "table_dispatch")]
#[derive(Copy, Clone, Debug)]
struct OpInfo {
    mode: AddrMode,
    kind: ExecKind,
    base: u8,
    page_cross_penalty: bool,
    rmw: bool,
    branch: bool,
}

#[cfg(feature = "table_dispatch")]
impl OpInfo {
    const fn new(
        mode: AddrMode,
        kind: ExecKind,
        base: u8,
        page_cross_penalty: bool,
        rmw: bool,
        branch: bool,
    ) -> Self {
        Self {
            mode,
            kind,
            base,
            page_cross_penalty,
            rmw,
            branch,
        }
    }
    const fn fb() -> Self {
        Self::new(
            AddrMode::Implied,
            ExecKind::Fallback,
            2,
            false,
            false,
            false,
        )
    }
}

#[cfg(feature = "table_dispatch")]
static EXEC_TABLE: [OpInfo; 256] = {
    use AddrMode::*;
    use ExecKind::*;
    let mut t: [OpInfo; 256] = [OpInfo::fb(); 256];

    // LDA variants
    t[0xA9] = OpInfo::new(Imm, Lda, 2, false, false, false);
    t[0xA5] = OpInfo::new(Zp, Lda, 3, false, false, false);
    t[0xB5] = OpInfo::new(ZpX, Lda, 4, false, false, false);
    t[0xAD] = OpInfo::new(Abs, Lda, 4, false, false, false);
    t[0xBD] = OpInfo::new(AbsX, Lda, 4, true, false, false); // +1 if page cross
    t[0xB9] = OpInfo::new(AbsY, Lda, 4, true, false, false);
    t[0xA1] = OpInfo::new(IndX, Lda, 6, false, false, false);
    t[0xB1] = OpInfo::new(IndY, Lda, 5, true, false, false);

    // Flag ops sample
    t[0x18] = OpInfo::new(Implied, Clc, 2, false, false, false);
    t[0x38] = OpInfo::new(Implied, Sec, 2, false, false, false);

    // NOP
    t[0xEA] = OpInfo::new(Implied, Nop, 2, false, false, false);

    t
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cartridge::Cartridge;
    use crate::cpu::core::Cpu;
    use crate::test_utils::build_nrom_with_prg;

    // Only run when feature enabled.
    #[cfg(feature = "table_dispatch")]
    fn setup(prg: &[u8]) -> (Cpu, Bus) {
        let rom = build_nrom_with_prg(prg, 1, 1, None);
        let cart = Cartridge::from_ines_bytes(&rom).unwrap();
        let mut bus = Bus::new();
        bus.attach_cartridge(cart);
        let mut cpu = Cpu::new();
        cpu.reset(&mut bus);
        (cpu, bus)
    }

    #[test]
    #[cfg(feature = "table_dispatch")]
    fn lda_imm_table_cycles() {
        // Program: LDA #$10; NOP
        let (mut cpu, mut bus) = setup(&[0xA9, 0x10, 0xEA, 0x00]);
        let op1 = bus.read(cpu.pc()); // peek
        assert_eq!(op1, 0xA9);
        let c = try_table_step(&mut cpu, &mut bus, op1).unwrap();
        assert_eq!(c, 2);
        assert_eq!(cpu.a(), 0x10);
    }

    #[test]
    #[cfg(feature = "table_dispatch")]
    fn lda_abs_x_page_cross_penalty() {
        // LDX #$01; (manually set X after first opcode) then LDA $80FF,X crossing to $8100
        // We'll construct a short program and manually set X so second fetch crosses.
        let (mut cpu, mut bus) = setup(&[0xEA, 0xBD, 0xFF, 0x80, 0x00]); // NOP; LDA $80FF,X; BRK
        // Execute NOP via table
        let _ = try_table_step(&mut cpu, &mut bus, bus.read(cpu.pc())).unwrap();
        cpu.set_x(0x01);
        let lda_opcode = bus.read(cpu.pc());
        assert_eq!(lda_opcode, 0xBD);
        let cycles = try_table_step(&mut cpu, &mut bus, lda_opcode).unwrap();
        // Base 4 + 1 page cross
        assert_eq!(cycles, 5);
    }

    #[test]
    #[cfg(not(feature = "table_dispatch"))]
    fn table_disabled_returns_none() {
        let mut dummy_cpu = Cpu::new();
        // Minimal bus: construct empty bus with default cartridge-less state.
        let mut bus = Bus::new();
        assert!(try_table_step(&mut dummy_cpu, &mut bus, 0xA9).is_none());
    }
}
