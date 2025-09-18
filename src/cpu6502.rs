/*!
Bus-integrated 6502 CPU core
NOTE: This file is in the process of being decomposed into a modular
`cpu/` directory (see `src/cpu/`). New development should prefer the
facade `crate::cpu::Cpu6502` and avoid adding fresh logic here. Once
the migration (state, addressing, execute, dispatch, table decode)
reaches parity, this legacy monolithic file will be removed.

Features:
- 64KiB CPU address space accessed only via Bus
- Full register set and flags
- Reset vector handling via Bus
- Fetch-Decode-Execute via `step(&mut self, &mut Bus)`
- Broad opcode coverage with addressing modes:
  - Immediate, Implied, Accumulator
  - Zero Page, Zero Page,X, Zero Page,Y
  - Absolute, Absolute,X, Absolute,Y
  - (Indirect,X), (Indirect),Y, and JMP (Indirect) with 6502 page bug
  - Relative (branches)
- Correct BRK/RTI/IRQ/NMI stack/flag push semantics (B/U bits)
- Helper APIs: reset, run, irq, nmi, and a compatibility `lda_immediate`

Note:
- No cycle counting/timing in this revision.
*/

use crate::bus::Bus;

#[allow(dead_code)]
/* ---------------------------------------------------------------------------
   Legacy 6502 CPU Core (cpu6502.rs)
   ---------------------------------------------------------------------------
   STATUS: DEPRECATED (in migration)
   This monolithic implementation is being refactored into the modular
   hierarchy under `src/cpu/`:
       cpu/
         mod.rs            - public façade (re-exports Cpu6502)
         state.rs          - CPU registers & flag helpers
         addressing.rs     - addressing mode / operand resolution
         execute.rs        - instruction semantics (ALU, stack, RMW)
         (future) dispatch_legacy.rs / table.rs / dispatch.rs
   MIGRATION PLAN:
   - New code should live in the `cpu` module; do NOT add new functionality
     here unless strictly necessary to keep existing behavior during the
     transition.
   - Incrementally port opcode families to the table-driven dispatcher
     (feature flag: `table_dispatch`) until full parity is achieved, then
     remove this file.
   - Tests referencing crate::cpu6502::Cpu6502 should be updated to use
     crate::cpu::Cpu6502 (the façade) to avoid depending on this legacy path.
   WARNING:
   - This file will be removed once all documented opcodes plus timing,
     interrupts, and page-cross penalties are validated in the modular core.
   - Keep changes here minimal (bug fixes only) to reduce merge churn.
--------------------------------------------------------------------------- */
pub struct Cpu6502 {
    // Registers
    pub a: u8,      // Accumulator
    pub x: u8,      // X Index
    pub y: u8,      // Y Index
    pub sp: u8,     // Stack Pointer
    pub pc: u16,    // Program Counter
    pub status: u8, // Processor Status (NV-BDIZC) (bit 5 always set)

    // Control
    halted: bool,
}

// Processor status flags (bit positions)
const CARRY: u8 = 0b0000_0001; // C
const ZERO: u8 = 0b0000_0010; // Z
const IRQ_DISABLE: u8 = 0b0000_0100; // I
const DECIMAL: u8 = 0b0000_1000; // D (on NES CPU, ADC/SBC are binary regardless)
const BREAK: u8 = 0b0001_0000; // B (special on push)
const UNUSED: u8 = 0b0010_0000; // U (always set)
const OVERFLOW: u8 = 0b0100_0000; // V
const NEGATIVE: u8 = 0b1000_0000; // N

// ================= Table-Dispatch (feature-gated) =================
//
// The following types and static table are defined at module scope so that
// they are not nested inside the Cpu6502 impl (which caused earlier
// compilation errors). The Cpu6502::step method can optionally use these
// when the "table_dispatch" Cargo feature is enabled. Incomplete entries
// fall back to the legacy match-based dispatcher.
//
// Migration plan:
// 1. Gradually populate EXEC_TABLE with opcode metadata.
// 2. Add executor coverage for each ExecKind.
// 3. Remove legacy match once parity is achieved.
//

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

    // LDA set
    t[0xA9] = OpInfo::new(Imm, Lda, 2, false, false, false);
    t[0xA5] = OpInfo::new(Zp, Lda, 3, false, false, false);
    t[0xB5] = OpInfo::new(ZpX, Lda, 4, false, false, false);
    t[0xAD] = OpInfo::new(Abs, Lda, 4, false, false, false);
    t[0xBD] = OpInfo::new(AbsX, Lda, 4, true, false, false);
    t[0xB9] = OpInfo::new(AbsY, Lda, 4, true, false, false);
    t[0xA1] = OpInfo::new(IndX, Lda, 6, false, false, false);
    t[0xB1] = OpInfo::new(IndY, Lda, 5, true, false, false);

    // Flag ops (examples)
    t[0x18] = OpInfo::new(Implied, Clc, 2, false, false, false);
    t[0x38] = OpInfo::new(Implied, Sec, 2, false, false, false);
    t[0xEA] = OpInfo::new(Implied, Nop, 2, false, false, false);

    t
};

#[inline]
fn table_dispatch_enabled() -> bool {
    #[cfg(feature = "table_dispatch")]
    {
        true
    }
    #[cfg(not(feature = "table_dispatch"))]
    {
        false
    }
}

#[cfg(feature = "table_dispatch")]
enum Operand {
    None,
    Immediate(u8),
    Address(u16, bool), // (address, page_crossed)
}

#[cfg(feature = "table_dispatch")]
impl Cpu6502 {
    fn resolve_operand(&mut self, bus: &mut Bus, mode: AddrMode) -> Operand {
        use AddrMode::*;
        match mode {
            Implied | Acc => Operand::None,
            Imm => Operand::Immediate(self.fetch_byte(bus)),
            Zp => Operand::Address(self.fetch_byte(bus) as u16, false),
            ZpX => Operand::Address(self.fetch_byte(bus).wrapping_add(self.x) as u16, false),
            ZpY => Operand::Address(self.fetch_byte(bus).wrapping_add(self.y) as u16, false),
            Abs => Operand::Address(self.fetch_word(bus), false),
            AbsX => {
                let base = self.fetch_word(bus);
                let eff = base.wrapping_add(self.x as u16);
                Operand::Address(eff, (base & 0xFF00) != (eff & 0xFF00))
            }
            AbsY => {
                let base = self.fetch_word(bus);
                let eff = base.wrapping_add(self.y as u16);
                Operand::Address(eff, (base & 0xFF00) != (eff & 0xFF00))
            }
            IndX => {
                let zp = self.fetch_byte(bus).wrapping_add(self.x);
                let lo = bus.read(zp as u16) as u16;
                let hi = bus.read(((zp as u16 + 1) & 0x00FF) as u16) as u16;
                Operand::Address((hi << 8) | lo, false)
            }
            IndY => {
                let zp = self.fetch_byte(bus);
                let lo = bus.read(zp as u16) as u16;
                let hi = bus.read(((zp as u16 + 1) & 0x00FF) as u16) as u16;
                let base = (hi << 8) | lo;
                let eff = base.wrapping_add(self.y as u16);
                Operand::Address(eff, (base & 0xFF00) != (eff & 0xFF00))
            }
            Ind => Operand::None, // (Indirect JMP not yet table-migrated)
            Rel => Operand::None, // (Branches handled later)
        }
    }
    // Execute via table; returns (cycles, consumed_flag)
    fn exec_via_table(&mut self, bus: &mut Bus, opcode: u8) -> (u32, bool) {
        use ExecKind::*;
        let entry = &EXEC_TABLE[opcode as usize];
        if let ExecKind::Fallback = entry.kind {
            return (0, false);
        }
        let mut cycles = entry.base as u32;
        let operand = self.resolve_operand(bus, entry.mode);
        // Page-cross penalty (centralized)
        if entry.page_cross_penalty {
            if let Operand::Address(_, crossed) = operand {
                if crossed {
                    cycles += 1;
                }
            }
        }
        match entry.kind {
            Lda => match operand {
                Operand::Immediate(v) => self.lda(v),
                Operand::Address(addr, _) => self.lda(bus.read(addr)),
                Operand::None => return (0, false),
            },
            Clc => self.set_flag(CARRY, false),
            Sec => self.set_flag(CARRY, true),
            Nop => {}
            _ => {
                // Not yet migrated
                return (0, false);
            }
        }
        // RMW not applicable for migrated subset yet.
        bus.tick(cycles);
        (cycles, true)
    }
}

impl Cpu6502 {
    pub fn new() -> Self {
        Cpu6502 {
            a: 0,
            x: 0,
            y: 0,
            sp: 0xFD,
            pc: 0x0000,
            status: IRQ_DISABLE | UNUSED, // typical power-up (I and U set)
            halted: false,
        }
    }

    /// Reset CPU registers and load PC from the reset vector at $FFFC/$FFFD via Bus.
    pub fn reset(&mut self, bus: &mut Bus) {
        self.a = 0;
        self.x = 0;
        self.y = 0;
        self.sp = 0xFD;
        self.status = IRQ_DISABLE | UNUSED;
        self.pc = bus.read_word(0xFFFC);
        self.halted = false;
    }

    /// Run up to `max_instructions` steps or until halted.
    pub fn run(&mut self, bus: &mut Bus, max_instructions: usize) {
        for _ in 0..max_instructions {
            if self.halted {
                break;
            }
            self.step(bus);
        }
    }

    /// Fetch-Decode-Execute one instruction.
    pub fn step(&mut self, bus: &mut Bus) -> u32 {
        // Handle DMA stall while OAM DMA is active: consume one CPU cycle without executing an opcode
        if bus.dma_is_active() {
            bus.tick(1);
            return 1;
        }

        // Service NMI if pending (before fetching next opcode)
        if bus.nmi_pending {
            // Push PC and P (B=0 on push), set I, and jump to NMI vector
            self.push_word(self.pc, bus);
            self.push_status_with_break(false, bus);
            self.set_flag(IRQ_DISABLE, true);
            self.pc = bus.read_word(0xFFFA);
            bus.nmi_pending = false;

            bus.tick(7);
            return 7;
        }

        // Service IRQ if asserted and I flag clear (before fetching next opcode)
        if bus.irq_line && !self.get_flag(IRQ_DISABLE) {
            self.push_word(self.pc, bus);
            self.push_status_with_break(false, bus);
            self.set_flag(IRQ_DISABLE, true);
            self.pc = bus.read_word(0xFFFE);

            bus.tick(7);
            return 7;
        }

        let opcode = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        let mut cycles: u32 = Self::base_cycles(opcode);

        // If feature table_dispatch is active, attempt table path first.
        if table_dispatch_enabled() {
            #[cfg(feature = "table_dispatch")]
            {
                let (table_cycles, consumed) = self.exec_via_table(bus, opcode);
                if consumed {
                    return table_cycles;
                }
            }
        }

        match opcode {
            // --------- Load/Store ---------
            // LDA
            0xA9 => {
                let v = self.fetch_byte(bus);
                self.lda(v);
            } // imm
            0xA5 => {
                let a = self.addr_zp(bus);
                self.lda(bus.read(a));
            } // zp
            0xB5 => {
                let a = self.addr_zp_x(bus);
                self.lda(bus.read(a));
            } // zp,X
            0xAD => {
                let a = self.addr_abs(bus);
                self.lda(bus.read(a));
            } // abs
            0xBD => {
                let (a, crossed) = self.addr_abs_x_pc(bus);
                self.lda(bus.read(a));
                self.add_page_cross_penalty(&mut cycles, crossed);
            } // abs,X
            0xB9 => {
                let (a, crossed) = self.addr_abs_y_pc(bus);
                self.lda(bus.read(a));
                self.add_page_cross_penalty(&mut cycles, crossed);
            } // abs,Y
            0xA1 => {
                let a = self.addr_ind_x(bus);
                self.lda(bus.read(a));
            } // (ind,X)
            0xB1 => {
                let (a, crossed) = self.addr_ind_y_pc(bus);
                self.lda(bus.read(a));
                self.add_page_cross_penalty(&mut cycles, crossed);
            } // (ind),Y

            // LDX
            0xA2 => {
                let v = self.fetch_byte(bus);
                self.ldx(v);
            } // imm
            0xA6 => {
                let a = self.addr_zp(bus);
                self.ldx(bus.read(a));
            } // zp
            0xB6 => {
                let a = self.addr_zp_y(bus);
                self.ldx(bus.read(a));
            } // zp,Y
            0xAE => {
                let a = self.addr_abs(bus);
                self.ldx(bus.read(a));
            } // abs
            0xBE => {
                let (a, crossed) = self.addr_abs_y_pc(bus);
                self.ldx(bus.read(a));
                self.add_page_cross_penalty(&mut cycles, crossed);
            } // abs,Y

            // LDY
            0xA0 => {
                let v = self.fetch_byte(bus);
                self.ldy(v);
            } // imm
            0xA4 => {
                let a = self.addr_zp(bus);
                self.ldy(bus.read(a));
            } // zp
            0xB4 => {
                let a = self.addr_zp_x(bus);
                self.ldy(bus.read(a));
            } // zp,X
            0xAC => {
                let a = self.addr_abs(bus);
                self.ldy(bus.read(a));
            } // abs
            0xBC => {
                let (a, crossed) = self.addr_abs_x_pc(bus);
                self.ldy(bus.read(a));
                self.add_page_cross_penalty(&mut cycles, crossed);
            } // abs,X

            // STA
            0x85 => {
                let a = self.addr_zp(bus);
                bus.write(a, self.a);
            }
            0x95 => {
                let a = self.addr_zp_x(bus);
                bus.write(a, self.a);
            }
            0x8D => {
                let a = self.addr_abs(bus);
                bus.write(a, self.a);
            }
            0x9D => {
                let a = self.addr_abs_x(bus);
                bus.write(a, self.a);
            }
            0x99 => {
                let a = self.addr_abs_y(bus);
                bus.write(a, self.a);
            }
            0x81 => {
                let a = self.addr_ind_x(bus);
                bus.write(a, self.a);
            }
            0x91 => {
                let a = self.addr_ind_y(bus);
                bus.write(a, self.a);
            }

            // STX
            0x86 => {
                let a = self.addr_zp(bus);
                bus.write(a, self.x);
            }
            0x96 => {
                let a = self.addr_zp_y(bus);
                bus.write(a, self.x);
            }
            0x8E => {
                let a = self.addr_abs(bus);
                bus.write(a, self.x);
            }

            // STY
            0x84 => {
                let a = self.addr_zp(bus);
                bus.write(a, self.y);
            }
            0x94 => {
                let a = self.addr_zp_x(bus);
                bus.write(a, self.y);
            }
            0x8C => {
                let a = self.addr_abs(bus);
                bus.write(a, self.y);
            }

            // --------- Transfers ---------
            0xAA => self.tax(),
            0xA8 => self.tay(),
            0x8A => self.txa(),
            0x98 => self.tya(),
            0xBA => self.tsx(),
            0x9A => self.txs(),

            // --------- Stack ---------
            0x48 => self.pha(bus),
            0x68 => self.pla(bus),
            0x08 => self.php(bus), // B set on push
            0x28 => self.plp(bus),

            // --------- Increment/Decrement ---------
            0xE8 => self.inx(),
            0xC8 => self.iny(),
            0xCA => self.dex(),
            0x88 => self.dey(),

            0xE6 => {
                let a = self.addr_zp(bus);
                self.inc_mem(a, bus);
            }
            0xF6 => {
                let a = self.addr_zp_x(bus);
                self.inc_mem(a, bus);
            }
            0xEE => {
                let a = self.addr_abs(bus);
                self.inc_mem(a, bus);
            }
            0xFE => {
                let a = self.addr_abs_x(bus);
                self.inc_mem(a, bus);
            }

            0xC6 => {
                let a = self.addr_zp(bus);
                self.dec_mem(a, bus);
            }
            0xD6 => {
                let a = self.addr_zp_x(bus);
                self.dec_mem(a, bus);
            }
            0xCE => {
                let a = self.addr_abs(bus);
                self.dec_mem(a, bus);
            }
            0xDE => {
                let a = self.addr_abs_x(bus);
                self.dec_mem(a, bus);
            }

            // --------- Logical (AND/ORA/EOR/BIT) ---------
            // AND
            0x29 => {
                let v = self.fetch_byte(bus);
                self.and(v);
            }
            0x25 => {
                let a = self.addr_zp(bus);
                self.and(bus.read(a));
            }
            0x35 => {
                let a = self.addr_zp_x(bus);
                self.and(bus.read(a));
            }
            0x2D => {
                let a = self.addr_abs(bus);
                self.and(bus.read(a));
            }
            0x3D => {
                let (a, crossed) = self.addr_abs_x_pc(bus);
                self.and(bus.read(a));
                if crossed {
                    cycles += 1;
                }
            }
            0x39 => {
                let (a, crossed) = self.addr_abs_y_pc(bus);
                self.and(bus.read(a));
                self.add_page_cross_penalty(&mut cycles, crossed);
            }
            0x21 => {
                let a = self.addr_ind_x(bus);
                self.and(bus.read(a));
            }
            0x31 => {
                let (a, crossed) = self.addr_ind_y_pc(bus);
                self.and(bus.read(a));
                self.add_page_cross_penalty(&mut cycles, crossed);
            }

            // ORA
            0x09 => {
                let v = self.fetch_byte(bus);
                self.ora(v);
            }
            0x05 => {
                let a = self.addr_zp(bus);
                self.ora(bus.read(a));
            }
            0x15 => {
                let a = self.addr_zp_x(bus);
                self.ora(bus.read(a));
            }
            0x0D => {
                let a = self.addr_abs(bus);
                self.ora(bus.read(a));
            }
            0x1D => {
                let (a, crossed) = self.addr_abs_x_pc(bus);
                self.ora(bus.read(a));
                self.add_page_cross_penalty(&mut cycles, crossed);
            }
            0x19 => {
                let (a, crossed) = self.addr_abs_y_pc(bus);
                self.ora(bus.read(a));
                self.add_page_cross_penalty(&mut cycles, crossed);
            }
            0x01 => {
                let a = self.addr_ind_x(bus);
                self.ora(bus.read(a));
            }
            0x11 => {
                let (a, crossed) = self.addr_ind_y_pc(bus);
                self.ora(bus.read(a));
                self.add_page_cross_penalty(&mut cycles, crossed);
            }

            // EOR
            0x49 => {
                let v = self.fetch_byte(bus);
                self.eor(v);
            }
            0x45 => {
                let a = self.addr_zp(bus);
                self.eor(bus.read(a));
            }
            0x55 => {
                let a = self.addr_zp_x(bus);
                self.eor(bus.read(a));
            }
            0x4D => {
                let a = self.addr_abs(bus);
                self.eor(bus.read(a));
            }
            0x5D => {
                let (a, crossed) = self.addr_abs_x_pc(bus);
                self.eor(bus.read(a));
                self.add_page_cross_penalty(&mut cycles, crossed);
            }
            0x59 => {
                let (a, crossed) = self.addr_abs_y_pc(bus);
                self.eor(bus.read(a));
                self.add_page_cross_penalty(&mut cycles, crossed);
            }
            0x41 => {
                let a = self.addr_ind_x(bus);
                self.eor(bus.read(a));
            }
            0x51 => {
                let (a, crossed) = self.addr_ind_y_pc(bus);
                self.eor(bus.read(a));
                self.add_page_cross_penalty(&mut cycles, crossed);
            }

            // BIT
            0x24 => {
                let a = self.addr_zp(bus);
                self.bit(bus.read(a));
            }
            0x2C => {
                let a = self.addr_abs(bus);
                self.bit(bus.read(a));
            }

            // --------- Shifts and Rotates ---------
            // ASL
            0x0A => self.asl_acc(),
            0x06 => {
                let a = self.addr_zp(bus);
                self.asl_mem(a, bus);
            }
            0x16 => {
                let a = self.addr_zp_x(bus);
                self.asl_mem(a, bus);
            }
            0x0E => {
                let a = self.addr_abs(bus);
                self.asl_mem(a, bus);
            }
            0x1E => {
                let a = self.addr_abs_x(bus);
                self.asl_mem(a, bus);
            }

            // LSR
            0x4A => self.lsr_acc(),
            0x46 => {
                let a = self.addr_zp(bus);
                self.lsr_mem(a, bus);
            }
            0x56 => {
                let a = self.addr_zp_x(bus);
                self.lsr_mem(a, bus);
            }
            0x4E => {
                let a = self.addr_abs(bus);
                self.lsr_mem(a, bus);
            }
            0x5E => {
                let a = self.addr_abs_x(bus);
                self.lsr_mem(a, bus);
            }

            // ROL
            0x2A => self.rol_acc(),
            0x26 => {
                let a = self.addr_zp(bus);
                self.rol_mem(a, bus);
            }
            0x36 => {
                let a = self.addr_zp_x(bus);
                self.rol_mem(a, bus);
            }
            0x2E => {
                let a = self.addr_abs(bus);
                self.rol_mem(a, bus);
            }
            0x3E => {
                let a = self.addr_abs_x(bus);
                self.rol_mem(a, bus);
            }

            // ROR
            0x6A => self.ror_acc(),
            0x66 => {
                let a = self.addr_zp(bus);
                self.ror_mem(a, bus);
            }
            0x76 => {
                let a = self.addr_zp_x(bus);
                self.ror_mem(a, bus);
            }
            0x6E => {
                let a = self.addr_abs(bus);
                self.ror_mem(a, bus);
            }
            0x7E => {
                let a = self.addr_abs_x(bus);
                self.ror_mem(a, bus);
            }

            // --------- Flags ---------
            0x18 => self.set_flag(CARRY, false),       // CLC
            0x38 => self.set_flag(CARRY, true),        // SEC
            0x58 => self.set_flag(IRQ_DISABLE, false), // CLI
            0x78 => self.set_flag(IRQ_DISABLE, true),  // SEI
            0xD8 => self.set_flag(DECIMAL, false),     // CLD
            0xF8 => self.set_flag(DECIMAL, true),      // SED (binary ADC/SBC on NES)
            0xB8 => self.set_flag(OVERFLOW, false),    // CLV

            // --------- Compare ---------
            // CMP
            0xC9 => {
                let v = self.fetch_byte(bus);
                self.cmp(self.a, v);
            }
            0xC5 => {
                let a = self.addr_zp(bus);
                self.cmp(self.a, bus.read(a));
            }
            0xD5 => {
                let a = self.addr_zp_x(bus);
                self.cmp(self.a, bus.read(a));
            }
            0xCD => {
                let a = self.addr_abs(bus);
                self.cmp(self.a, bus.read(a));
            }
            0xDD => {
                let (a, crossed) = self.addr_abs_x_pc(bus);
                self.cmp(self.a, bus.read(a));
                self.add_page_cross_penalty(&mut cycles, crossed);
            }
            0xD9 => {
                let (a, crossed) = self.addr_abs_y_pc(bus);
                self.cmp(self.a, bus.read(a));
                self.add_page_cross_penalty(&mut cycles, crossed);
            }
            0xC1 => {
                let a = self.addr_ind_x(bus);
                self.cmp(self.a, bus.read(a));
            }
            0xD1 => {
                let (a, crossed) = self.addr_ind_y_pc(bus);
                self.cmp(self.a, bus.read(a));
                self.add_page_cross_penalty(&mut cycles, crossed);
            }

            // CPX
            0xE0 => {
                let v = self.fetch_byte(bus);
                self.cmp(self.x, v);
            }
            0xE4 => {
                let a = self.addr_zp(bus);
                self.cmp(self.x, bus.read(a));
            }
            0xEC => {
                let a = self.addr_abs(bus);
                self.cmp(self.x, bus.read(a));
            }

            // CPY
            0xC0 => {
                let v = self.fetch_byte(bus);
                self.cmp(self.y, v);
            }
            0xC4 => {
                let a = self.addr_zp(bus);
                self.cmp(self.y, bus.read(a));
            }
            0xCC => {
                let a = self.addr_abs(bus);
                self.cmp(self.y, bus.read(a));
            }

            // --------- Branches ---------
            0x10 => {
                cycles += self.branch_cond(bus, !self.get_flag(NEGATIVE));
            } // BPL
            0x30 => {
                cycles += self.branch_cond(bus, self.get_flag(NEGATIVE));
            } // BMI
            0x50 => {
                cycles += self.branch_cond(bus, !self.get_flag(OVERFLOW));
            } // BVC
            0x70 => {
                cycles += self.branch_cond(bus, self.get_flag(OVERFLOW));
            } // BVS
            0x90 => {
                cycles += self.branch_cond(bus, !self.get_flag(CARRY));
            } // BCC
            0xB0 => {
                cycles += self.branch_cond(bus, self.get_flag(CARRY));
            } // BCS
            0xD0 => {
                cycles += self.branch_cond(bus, !self.get_flag(ZERO));
            } // BNE
            0xF0 => {
                cycles += self.branch_cond(bus, self.get_flag(ZERO));
            } // BEQ

            // --------- Jumps/Subroutines/Returns ---------
            0x4C => {
                let a = self.addr_abs(bus);
                self.pc = a;
            } // JMP abs
            0x6C => {
                let a = self.addr_abs(bus);
                self.pc = self.read_word_indirect_bug(bus, a);
            } // JMP (ind)
            0x20 => {
                let a = self.addr_abs(bus);
                let ret = self.pc.wrapping_sub(1);
                self.push_word(ret, bus);
                self.pc = a;
            } // JSR
            0x60 => {
                self.pc = self.pop_word(bus).wrapping_add(1);
            } // RTS

            // --------- ADC/SBC ---------
            // ADC
            0x69 => {
                let v = self.fetch_byte(bus);
                self.adc(v);
            } // imm
            0x65 => {
                let a = self.addr_zp(bus);
                self.adc(bus.read(a));
            }
            0x75 => {
                let a = self.addr_zp_x(bus);
                self.adc(bus.read(a));
            }
            0x6D => {
                let a = self.addr_abs(bus);
                self.adc(bus.read(a));
            }
            0x7D => {
                let (a, crossed) = self.addr_abs_x_pc(bus);
                self.adc(bus.read(a));
                self.add_page_cross_penalty(&mut cycles, crossed);
            }
            0x79 => {
                let (a, crossed) = self.addr_abs_y_pc(bus);
                self.adc(bus.read(a));
                if crossed {
                    cycles += 1;
                }
            }
            0x61 => {
                let a = self.addr_ind_x(bus);
                self.adc(bus.read(a));
            }
            0x71 => {
                let (a, crossed) = self.addr_ind_y_pc(bus);
                self.adc(bus.read(a));
                if crossed {
                    cycles += 1;
                }
            }

            // SBC
            0xE9 => {
                let v = self.fetch_byte(bus);
                self.sbc(v);
            } // imm
            0xE5 => {
                let a = self.addr_zp(bus);
                self.sbc(bus.read(a));
            }
            0xF5 => {
                let a = self.addr_zp_x(bus);
                self.sbc(bus.read(a));
            }
            0xED => {
                let a = self.addr_abs(bus);
                self.sbc(bus.read(a));
            }
            0xFD => {
                let (a, crossed) = self.addr_abs_x_pc(bus);
                self.sbc(bus.read(a));
                self.add_page_cross_penalty(&mut cycles, crossed);
            }
            0xF9 => {
                let (a, crossed) = self.addr_abs_y_pc(bus);
                self.sbc(bus.read(a));
                self.add_page_cross_penalty(&mut cycles, crossed);
            }
            0xE1 => {
                let a = self.addr_ind_x(bus);
                self.sbc(bus.read(a));
            }
            0xF1 => {
                let (a, crossed) = self.addr_ind_y_pc(bus);
                self.sbc(bus.read(a));
                self.add_page_cross_penalty(&mut cycles, crossed);
            }

            // --------- Interrupts ---------
            // BRK: push PC, push P with B set, set I, jump to $FFFE (here also halt demo-run)
            0x00 => {
                let pc_to_push = self.pc;
                self.push_word(pc_to_push, bus);
                self.php(bus); // B set
                self.set_flag(IRQ_DISABLE, true);
                self.pc = bus.read_word(0xFFFE);
                self.halted = true;
                cycles = 7;
            }
            0x40 => {
                self.rti(bus);
            } // RTI

            // --------- NOP ---------
            0xEA => { /* NOP */ }

            // Unknown/unimplemented: halt to avoid undefined execution
            _ => {
                self.halted = true;
            }
        }
        let ret_cycles = cycles;
        let tick_cycles = if Self::is_rmw(opcode) {
            cycles.saturating_sub(2)
        } else {
            cycles
        };
        bus.tick(tick_cycles);
        ret_cycles
    }

    // Base cycles table (scaffolding) and penalty hooks
    #[inline]
    fn base_cycles(op: u8) -> u32 {
        match op {
            // Loads
            0xA9 => 2, // LDA #imm
            0xA5 => 3,
            0xB5 => 4,
            0xAD => 4,
            0xBD => 4,
            0xB9 => 4,
            0xA1 => 6,
            0xB1 => 5,
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
            0xAA => 2,
            0xA8 => 2,
            0x8A => 2,
            0x98 => 2,
            0xBA => 2,
            0x9A => 2,

            // Stack
            0x48 => 3,
            0x68 => 4,
            0x08 => 3,
            0x28 => 4,

            // Inc/Dec
            0xE8 => 2,
            0xC8 => 2,
            0xCA => 2,
            0x88 => 2,
            0xE6 => 5,
            0xF6 => 6,
            0xEE => 6,
            0xFE => 7,
            0xC6 => 5,
            0xD6 => 6,
            0xCE => 6,
            0xDE => 7,

            // Logical
            0x29 => 2,
            0x25 => 3,
            0x35 => 4,
            0x2D => 4,
            0x3D => 4,
            0x39 => 4,
            0x21 => 6,
            0x31 => 5, // AND
            0x09 => 2,
            0x05 => 3,
            0x15 => 4,
            0x0D => 4,
            0x1D => 4,
            0x19 => 4,
            0x01 => 6,
            0x11 => 5, // ORA
            0x49 => 2,
            0x45 => 3,
            0x55 => 4,
            0x4D => 4,
            0x5D => 4,
            0x59 => 4,
            0x41 => 6,
            0x51 => 5, // EOR
            0x24 => 3,
            0x2C => 4, // BIT

            // Shifts/Rotates (accumulator / memory)
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
            0x18 => 2,
            0x38 => 2,
            0x58 => 2,
            0x78 => 2,
            0xD8 => 2,
            0xF8 => 2,
            0xB8 => 2,

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

            // Branches (base cycles; add penalties when taken/page-cross)
            0x10 => 2,
            0x30 => 2,
            0x50 => 2,
            0x70 => 2,
            0x90 => 2,
            0xB0 => 2,
            0xD0 => 2,
            0xF0 => 2,

            // Jumps/Subroutines
            0x4C => 3,
            0x6C => 5,
            0x20 => 6,
            0x60 => 6,

            // ADC/SBC
            0x69 => 2,
            0x65 => 3,
            0x75 => 4,
            0x6D => 4,
            0x7D => 4,
            0x79 => 4,
            0x61 => 6,
            0x71 => 5, // ADC
            0xE9 => 2,
            0xE5 => 3,
            0xF5 => 4,
            0xED => 4,
            0xFD => 4,
            0xF9 => 4,
            0xE1 => 6,
            0xF1 => 5, // SBC

            // Interrupts / NOP
            0x00 => 7,
            0x40 => 6,
            0xEA => 2,

            // Temporary default; fill remaining opcodes as needed.
            _ => 2,
        }
    }

    // (table-dispatch definitions moved to module scope above)

    // (legacy exec_via_table implementation removed from impl; now provided at module scope via specialized impl when feature enabled)

    // ------------------------
    // Public helper API
    // ------------------------

    /// Compatibility helper: directly load A with an immediate value and update flags.
    pub fn lda_immediate(&mut self, value: u8) {
        self.lda(value);
    }

    /// Trigger an IRQ (if not masked).
    pub fn irq(&mut self, bus: &mut Bus) {
        if !self.get_flag(IRQ_DISABLE) {
            self.push_word(self.pc, bus);
            self.push_status_with_break(false, bus); // B=0 for IRQ/NMI pushes
            self.set_flag(IRQ_DISABLE, true);
            self.pc = bus.read_word(0xFFFE);
        }
    }

    /// Trigger an NMI (non-maskable interrupt).
    pub fn nmi(&mut self, bus: &mut Bus) {
        self.push_word(self.pc, bus);
        self.push_status_with_break(false, bus); // B=0 for IRQ/NMI pushes
        self.set_flag(IRQ_DISABLE, true);
        self.pc = bus.read_word(0xFFFA);
    }

    /// Return from interrupt.
    pub fn rti(&mut self, bus: &mut Bus) {
        self.plp(bus);
        self.pc = self.pop_word(bus);
    }

    // ------------------------
    // Addressing helpers
    // ------------------------

    #[inline]
    fn fetch_byte(&mut self, bus: &mut Bus) -> u8 {
        let v = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        v
    }

    #[inline]
    fn fetch_word(&mut self, bus: &mut Bus) -> u16 {
        let lo = self.fetch_byte(bus) as u16;
        let hi = self.fetch_byte(bus) as u16;
        (hi << 8) | lo
    }

    #[inline]
    fn addr_zp(&mut self, bus: &mut Bus) -> u16 {
        self.fetch_byte(bus) as u16
    }

    #[inline]
    fn addr_zp_x(&mut self, bus: &mut Bus) -> u16 {
        self.fetch_byte(bus).wrapping_add(self.x) as u16
    }

    #[inline]
    fn addr_zp_y(&mut self, bus: &mut Bus) -> u16 {
        self.fetch_byte(bus).wrapping_add(self.y) as u16
    }

    #[inline]
    fn addr_abs(&mut self, bus: &mut Bus) -> u16 {
        self.fetch_word(bus)
    }

    #[inline]
    fn addr_abs_x(&mut self, bus: &mut Bus) -> u16 {
        self.fetch_word(bus).wrapping_add(self.x as u16)
    }

    #[inline]
    fn addr_abs_y(&mut self, bus: &mut Bus) -> u16 {
        self.fetch_word(bus).wrapping_add(self.y as u16)
    }

    #[inline]
    fn addr_ind_x(&mut self, bus: &mut Bus) -> u16 {
        let zp = self.fetch_byte(bus).wrapping_add(self.x);
        self.read_word_zp(bus, zp)
    }

    #[inline]
    fn addr_ind_y(&mut self, bus: &mut Bus) -> u16 {
        let zp = self.fetch_byte(bus);
        self.read_word_zp(bus, zp).wrapping_add(self.y as u16)
    }

    #[inline]
    fn addr_abs_x_pc(&mut self, bus: &mut Bus) -> (u16, bool) {
        let base = self.fetch_word(bus);
        let addr = base.wrapping_add(self.x as u16);
        let crossed = (base & 0xFF00) != (addr & 0xFF00);
        (addr, crossed)
    }

    #[inline]
    fn addr_abs_y_pc(&mut self, bus: &mut Bus) -> (u16, bool) {
        let base = self.fetch_word(bus);
        let addr = base.wrapping_add(self.y as u16);
        let crossed = (base & 0xFF00) != (addr & 0xFF00);
        (addr, crossed)
    }

    #[inline]
    fn addr_ind_y_pc(&mut self, bus: &mut Bus) -> (u16, bool) {
        let zp = self.fetch_byte(bus);
        let base = self.read_word_zp(bus, zp);
        let addr = base.wrapping_add(self.y as u16);
        let crossed = (base & 0xFF00) != (addr & 0xFF00);
        (addr, crossed)
    }

    #[inline]
    fn read_word_zp(&mut self, bus: &mut Bus, ptr: u8) -> u16 {
        let lo = bus.read(ptr as u16) as u16;
        let hi = bus.read(((ptr as u16 + 1) & 0x00FF) as u16) as u16;
        (hi << 8) | lo
    }

    // 6502 JMP (indirect) page-boundary bug
    #[inline]
    fn read_word_indirect_bug(&mut self, bus: &mut Bus, addr: u16) -> u16 {
        let lo = bus.read(addr) as u16;
        let hi_addr = (addr & 0xFF00) | ((addr + 1) & 0x00FF);
        let hi = bus.read(hi_addr) as u16;
        (hi << 8) | lo
    }

    #[inline]
    fn branch(&mut self, offset: i8) {
        let new_pc = (self.pc as i16).wrapping_add(offset as i16) as u16;
        self.pc = new_pc;
    }

    // ------------------------
    // Stack operations
    // ------------------------
    #[inline]
    fn push(&mut self, v: u8, bus: &mut Bus) {
        let addr = 0x0100u16 | self.sp as u16;
        bus.write(addr, v);
        self.sp = self.sp.wrapping_sub(1);
    }

    #[inline]
    fn pop(&mut self, bus: &mut Bus) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        let addr = 0x0100u16 | self.sp as u16;
        bus.read(addr)
    }

    #[inline]
    fn push_word(&mut self, v: u16, bus: &mut Bus) {
        self.push((v >> 8) as u8, bus);
        self.push((v & 0xFF) as u8, bus);
    }

    #[inline]
    fn pop_word(&mut self, bus: &mut Bus) -> u16 {
        let lo = self.pop(bus) as u16;
        let hi = self.pop(bus) as u16;
        (hi << 8) | lo
    }

    // Push P with control over Break bit on push (for IRQ/NMI/BRK differences).
    fn push_status_with_break(&mut self, set_break_on_push: bool, bus: &mut Bus) {
        let mut v = self.status | UNUSED;
        if set_break_on_push {
            v |= BREAK;
        } else {
            v &= !BREAK;
        }
        self.push(v, bus);
    }

    fn php(&mut self, bus: &mut Bus) {
        self.push_status_with_break(true, bus);
    }

    fn plp(&mut self, bus: &mut Bus) {
        let v = self.pop(bus);
        // Ensure bit 5 is set and bit 4 (B) is cleared in P (only set when pushed)
        self.status = (v | UNUSED) & !BREAK;
    }

    fn pha(&mut self, bus: &mut Bus) {
        self.push(self.a, bus);
    }

    fn pla(&mut self, bus: &mut Bus) {
        self.a = self.pop(bus);
        self.update_zn(self.a);
    }

    // ------------------------
    // Flags
    // ------------------------
    #[inline]
    fn set_flag(&mut self, mask: u8, on: bool) {
        if on {
            self.status |= mask;
        } else {
            self.status &= !mask;
        }
    }

    #[inline]
    fn get_flag(&self, mask: u8) -> bool {
        (self.status & mask) != 0
    }

    #[inline]
    fn update_zn(&mut self, v: u8) {
        self.set_flag(ZERO, v == 0);
        self.set_flag(NEGATIVE, (v & 0x80) != 0);
    }

    // ------------------------
    // Core ALU and operations
    // ------------------------
    fn lda(&mut self, v: u8) {
        self.a = v;
        self.update_zn(self.a);
    }

    fn ldx(&mut self, v: u8) {
        self.x = v;
        self.update_zn(self.x);
    }

    fn ldy(&mut self, v: u8) {
        self.y = v;
        self.update_zn(self.y);
    }

    fn tax(&mut self) {
        self.x = self.a;
        self.update_zn(self.x);
    }
    fn tay(&mut self) {
        self.y = self.a;
        self.update_zn(self.y);
    }
    fn txa(&mut self) {
        self.a = self.x;
        self.update_zn(self.a);
    }
    fn tya(&mut self) {
        self.a = self.y;
        self.update_zn(self.a);
    }
    fn tsx(&mut self) {
        self.x = self.sp;
        self.update_zn(self.x);
    }
    fn txs(&mut self) {
        self.sp = self.x;
    } // flags unaffected

    fn and(&mut self, v: u8) {
        self.a &= v;
        self.update_zn(self.a);
    }
    fn ora(&mut self, v: u8) {
        self.a |= v;
        self.update_zn(self.a);
    }
    fn eor(&mut self, v: u8) {
        self.a ^= v;
        self.update_zn(self.a);
    }

    fn bit(&mut self, v: u8) {
        // Z set if (A & M) == 0; N and V taken from M
        self.set_flag(ZERO, (self.a & v) == 0);
        self.set_flag(NEGATIVE, (v & 0x80) != 0);
        self.set_flag(OVERFLOW, (v & 0x40) != 0);
    }

    fn adc(&mut self, v: u8) {
        let a = self.a;
        let carry_in = if self.get_flag(CARRY) { 1 } else { 0 };
        let sum16 = a as u16 + v as u16 + carry_in as u16;
        let result = sum16 as u8;

        self.set_flag(CARRY, sum16 > 0xFF);
        self.set_flag(OVERFLOW, ((!(a ^ v)) & (a ^ result) & 0x80) != 0);

        self.a = result;
        self.update_zn(self.a);
    }

    fn sbc(&mut self, v: u8) {
        // 6502 SBC is ADC of one's complement of v
        self.adc(v ^ 0xFF);
    }

    fn cmp(&mut self, reg: u8, v: u8) {
        self.set_flag(CARRY, reg >= v);
        let r = reg.wrapping_sub(v);
        self.update_zn(r);
    }

    // Increments/Decrements
    fn inx(&mut self) {
        self.x = self.x.wrapping_add(1);
        self.update_zn(self.x);
    }
    fn iny(&mut self) {
        self.y = self.y.wrapping_add(1);
        self.update_zn(self.y);
    }
    fn dex(&mut self) {
        self.x = self.x.wrapping_sub(1);
        self.update_zn(self.x);
    }
    fn dey(&mut self) {
        self.y = self.y.wrapping_sub(1);
        self.update_zn(self.y);
    }

    fn inc_mem(&mut self, addr: u16, bus: &mut Bus) {
        let v = self.rmw(bus, addr, |_, old| old.wrapping_add(1));
        self.update_zn(v);
    }
    fn dec_mem(&mut self, addr: u16, bus: &mut Bus) {
        let v = self.rmw(bus, addr, |_, old| old.wrapping_sub(1));
        self.update_zn(v);
    }

    // Shifts/Rotates - Accumulator
    fn asl_acc(&mut self) {
        let v = self.a;
        self.set_flag(CARRY, (v & 0x80) != 0);
        self.a = v << 1;
        self.update_zn(self.a);
    }
    fn lsr_acc(&mut self) {
        let v = self.a;
        self.set_flag(CARRY, (v & 0x01) != 0);
        self.a = v >> 1;
        self.update_zn(self.a);
    }
    fn rol_acc(&mut self) {
        let v = self.a;
        let carry_in = if self.get_flag(CARRY) { 1 } else { 0 };
        self.set_flag(CARRY, (v & 0x80) != 0);
        self.a = (v << 1) | carry_in;
        self.update_zn(self.a);
    }
    fn ror_acc(&mut self) {
        let v = self.a;
        let carry_in = if self.get_flag(CARRY) { 0x80 } else { 0 };
        self.set_flag(CARRY, (v & 0x01) != 0);
        self.a = (v >> 1) | carry_in;
        self.update_zn(self.a);
    }

    // Shifts/Rotates - Memory
    fn asl_mem(&mut self, addr: u16, bus: &mut Bus) {
        // RMW sequence: read -> dummy write old -> write new, with micro-cycle ticks
        let v = bus.read(addr);
        bus.tick(1);
        bus.write(addr, v); // dummy write of old value
        bus.tick(1);
        self.set_flag(CARRY, (v & 0x80) != 0);
        let r = v << 1;
        bus.write(addr, r);
        self.update_zn(r);
    }
    fn lsr_mem(&mut self, addr: u16, bus: &mut Bus) {
        // RMW sequence: read -> dummy write old -> write new, with micro-cycle ticks
        let v = bus.read(addr);
        bus.tick(1);
        bus.write(addr, v); // dummy write of old value
        bus.tick(1);
        self.set_flag(CARRY, (v & 0x01) != 0);
        let r = v >> 1;
        bus.write(addr, r);
        self.update_zn(r);
    }
    fn rol_mem(&mut self, addr: u16, bus: &mut Bus) {
        // RMW sequence: read -> dummy write old -> write new, with micro-cycle ticks
        let v = bus.read(addr);
        bus.tick(1);
        bus.write(addr, v); // dummy write of old value
        bus.tick(1);
        let carry_in = if self.get_flag(CARRY) { 1 } else { 0 };
        self.set_flag(CARRY, (v & 0x80) != 0);
        let r = (v << 1) | carry_in;
        bus.write(addr, r);
        self.update_zn(r);
    }
    fn ror_mem(&mut self, addr: u16, bus: &mut Bus) {
        let r = self.rmw(bus, addr, |cpu, old| {
            let carry_in = if cpu.get_flag(CARRY) { 0x80 } else { 0 };
            cpu.set_flag(CARRY, (old & 0x01) != 0);
            (old >> 1) | carry_in
        });
        self.update_zn(r);
    }

    // ----- CPU helpers for penalties and RMW choreography -----

    #[inline]
    fn add_page_cross_penalty(&self, cycles: &mut u32, crossed: bool) {
        if crossed {
            *cycles += 1;
        }
    }

    /// Branch helper: fetches displacement, applies branch if taken, and returns extra cycles (1 or 2).
    fn branch_cond(&mut self, bus: &mut Bus, take: bool) -> u32 {
        let offset = self.fetch_byte(bus) as i8;
        if !take {
            return 0;
        }
        let old_pc = self.pc;
        self.branch(offset);
        let mut extra = 1;
        if (old_pc & 0xFF00) != (self.pc & 0xFF00) {
            extra += 1;
        }
        extra
    }

    /// Read-Modify-Write bus choreography: read -> dummy write old -> write new. Returns final value.
    fn rmw<F>(&mut self, bus: &mut Bus, addr: u16, transform: F) -> u8
    where
        F: FnOnce(&mut Self, u8) -> u8,
    {
        let old = bus.read(addr);
        bus.tick(1);
        bus.write(addr, old);
        bus.tick(1);
        let newv = transform(self, old);
        bus.write(addr, newv);
        newv
    }

    #[inline]
    fn is_rmw(opcode: u8) -> bool {
        matches!(
            opcode,
            0x06 | 0x16 | 0x0E | 0x1E | // ASL zp,zpX,abs,absX
            0x46 | 0x56 | 0x4E | 0x5E | // LSR zp,zpX,abs,absX
            0x26 | 0x36 | 0x2E | 0x3E | // ROL zp,zpX,abs,absX
            0x66 | 0x76 | 0x6E | 0x7E | // ROR zp,zpX,abs,absX
            0xE6 | 0xF6 | 0xEE | 0xFE | // INC zp,zpX,abs,absX
            0xC6 | 0xD6 | 0xCE | 0xDE // DEC zp,zpX,abs,absX
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::Bus;
    use crate::cartridge::Cartridge;
    use crate::test_utils::build_nrom_with_prg;

    // Use shared test utility for building NROM images with PRG and vectors

    // Run a single instruction and return cycles consumed.
    fn step_once(cpu: &mut Cpu6502, bus: &mut Bus) -> u32 {
        cpu.step(bus)
    }

    #[test]
    fn cycles_lda_abs_x_no_page_cross_is_4() {
        // Program: LDX #$00; LDA $12FF,X; BRK
        let prg = vec![0xA2, 0x00, 0xBD, 0xFF, 0x12, 0x00];
        let rom = build_nrom_with_prg(&prg, 1, 1, None);
        let cart = Cartridge::from_ines_bytes(&rom).expect("parse");
        let mut bus = Bus::new();
        bus.attach_cartridge(cart);

        // Put a value at $12FF that LDA will read
        bus.write(0x12FF, 0x42);

        let mut cpu = Cpu6502::new();
        cpu.reset(&mut bus);

        // LDX #$00
        let c1 = step_once(&mut cpu, &mut bus);
        assert_eq!(c1, 2);

        // LDA $12FF,X with X=0 (no page cross)
        let c2 = step_once(&mut cpu, &mut bus);
        assert_eq!(c2, 4);

        // BRK
        let c3 = step_once(&mut cpu, &mut bus);
        assert_eq!(c3, 7);
    }

    #[test]
    fn cycles_lda_abs_x_page_cross_is_5() {
        // Program: LDX #$01; LDA $12FF,X; BRK
        let prg = vec![0xA2, 0x01, 0xBD, 0xFF, 0x12, 0x00];
        let rom = build_nrom_with_prg(&prg, 1, 1, None);
        let cart = Cartridge::from_ines_bytes(&rom).expect("parse");
        let mut bus = Bus::new();
        bus.attach_cartridge(cart);

        // Value at $1300 (since $12FF+X crosses page)
        bus.write(0x1300, 0x99);

        let mut cpu = Cpu6502::new();
        cpu.reset(&mut bus);

        // LDX #$01
        let c1 = step_once(&mut cpu, &mut bus);
        assert_eq!(c1, 2);

        // LDA $12FF,X with X=1 (page cross)
        let c2 = step_once(&mut cpu, &mut bus);
        assert_eq!(c2, 5);

        // BRK
        let c3 = step_once(&mut cpu, &mut bus);
        assert_eq!(c3, 7);
    }

    #[test]
    fn cycles_sta_abs_x_no_penalty_is_5() {
        // Program: LDA #$10; LDX #$01; STA $12FF,X; BRK
        let prg = vec![0xA9, 0x10, 0xA2, 0x01, 0x9D, 0xFF, 0x12, 0x00];
        let rom = build_nrom_with_prg(&prg, 1, 1, None);
        let cart = Cartridge::from_ines_bytes(&rom).expect("parse");
        let mut bus = Bus::new();
        bus.attach_cartridge(cart);

        let mut cpu = Cpu6502::new();
        cpu.reset(&mut bus);

        // LDA #$10
        let c1 = step_once(&mut cpu, &mut bus);
        assert_eq!(c1, 2);

        // LDX #$01
        let c2 = step_once(&mut cpu, &mut bus);
        assert_eq!(c2, 2);

        // STA $12FF,X (abs,X store has fixed 5 cycles, no +1 penalty)
        let c3 = step_once(&mut cpu, &mut bus);
        assert_eq!(c3, 5);

        // BRK
        let c4 = step_once(&mut cpu, &mut bus);
        assert_eq!(c4, 7);

        // Value landed at $1300
        assert_eq!(bus.read(0x1300), 0x10);
    }

    #[test]
    fn branch_cycles_taken_and_page_cross() {
        // We'll place a branch at $80FF so a small positive offset crosses to $8101.
        // Construct PRG with padding NOPs so that BCC resides at $80FF.
        let mut prg = vec![];
        // Fill up to $80FF - $8000 = 0x00FF bytes with NOPs
        prg.extend(std::iter::repeat(0xEA).take(0x00FF));
        // At $80FF: CLC (clear carry) so BCC will be taken
        prg.push(0x18);
        // Next at $8100: BCC +0x01 -> target $8103 (crosses page: from $8101 after fetch+inc)
        prg.push(0x90);
        prg.push(0x01);
        // Filler then BRK at target
        prg.push(0xEA); // at $8102
        prg.push(0x00); // BRK at $8103

        let rom = build_nrom_with_prg(&prg, 1, 1, None);
        let cart = Cartridge::from_ines_bytes(&rom).expect("parse");
        let mut bus = Bus::new();
        bus.attach_cartridge(cart);

        let mut cpu = Cpu6502::new();
        cpu.reset(&mut bus);

        // Advance through NOP padding
        for _ in 0..0x00FF {
            let c = step_once(&mut cpu, &mut bus);
            assert_eq!(c, 2);
        }

        // CLC
        let c_clc = step_once(&mut cpu, &mut bus);
        assert_eq!(c_clc, 2);

        // BCC taken across page: expect 4 cycles (2 base +1 taken +1 page cross)
        let c_bcc = step_once(&mut cpu, &mut bus);
        assert_eq!(c_bcc, 4);

        // BRK at target (after a NOP)
        let _c_nop = step_once(&mut cpu, &mut bus);
        let c_brk = step_once(&mut cpu, &mut bus);
        assert_eq!(c_brk, 7);
    }
}
