#![doc = r#"
PPU registers handler

Purpose
- Provide a focused entry point for CPU-visible PPU register access (0x2000-0x3FFF).
- Delegate register semantics to `Ppu` while handling PPUDATA ($2007) via the bus
  so that nametable/palette mapping, buffering, and VRAM address increment are
  applied consistently.

Usage
- Intended to be called by the CPU memory dispatcher for addresses in
  0x2000..=0x3FFF. Mirroring to the 8-byte register set is handled internally.

Notes
- Mirroring: addresses 0x2008..=0x3FFF mirror 0x2000..=0x2007.
- PPUDATA ($2007) special cases:
  - Read: uses bus PPU memory mapping and PPU's buffered read semantics.
  - Write: writes via bus PPU mapping and increments VRAM address according to PPUCTRL.
"#]

use crate::bus::Bus;

/// Handler for CPU-visible PPU register reads/writes.
pub struct PpuRegisters;

impl PpuRegisters {
    /// Read from a CPU-visible PPU register address (0x2000..=0x3FFF).
    /// Applies register mirroring and delegates to PPU or bus as appropriate.
    pub fn read(bus: &mut Bus, addr: u16) -> u8 {
        let reg = mirror_ppu_reg(addr);

        match reg {
            0x2002 => {
                // PPUSTATUS: PPU owns the side-effects (vblank clear, toggle reset)
                bus.ppu.read_reg(reg)
            }
            0x2007 => {
                // PPUDATA read via bus mapping with PPU buffering + VRAM increment
                let vaddr = bus.ppu.get_vram_addr() & 0x3FFF;
                let value = bus.ppu_read(vaddr);

                // Buffered read behavior: below $3F00 returns delayed buffered byte
                // and updates buffer with the just-read value; palette region returns direct.
                let ret = if vaddr < 0x3F00 {
                    let out = bus.ppu.get_vram_buffer();
                    bus.ppu.set_vram_buffer(value);
                    out
                } else {
                    value
                };

                // Increment VRAM address by the PPU-configured step
                let inc = bus.ppu.vram_increment_step();
                bus.ppu.set_vram_addr(vaddr.wrapping_add(inc) & 0x3FFF);

                ret
            }
            _ => {
                // Other registers: defer to PPU
                bus.ppu.read_reg(reg)
            }
        }
    }

    /// Write to a CPU-visible PPU register address (0x2000..=0x3FFF).
    /// Applies register mirroring and delegates to PPU or bus as appropriate.
    pub fn write(bus: &mut Bus, addr: u16, value: u8) {
        let reg = mirror_ppu_reg(addr);

        match reg {
            0x2007 => {
                // PPUDATA write via bus mapping + VRAM increment
                let vaddr = bus.ppu.get_vram_addr() & 0x3FFF;
                bus.ppu_write(vaddr, value);

                let inc = bus.ppu.vram_increment_step();
                bus.ppu.set_vram_addr(vaddr.wrapping_add(inc) & 0x3FFF);
            }
            _ => {
                // Other registers: defer to PPU (PPUCTRL, PPUSCROLL, PPUADDR, etc)
                bus.ppu.write_reg(reg, value);
            }
        }
    }
}

#[inline]
fn mirror_ppu_reg(addr: u16) -> u16 {
    0x2000 | (addr & 0x0007)
}
