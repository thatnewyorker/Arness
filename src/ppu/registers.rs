#![doc = r#"
PPU registers module

Purpose
- Focused home for CPU-visible PPU register semantics: read_reg/write_reg behavior,
  buffering, and address mirroring rules.

Notes
- Addresses 0x2000..=0x3FFF mirror to the 8-byte register window 0x2000..=0x2007.
- PPUDATA ($2007) read uses buffered behavior below $3F00 and increments VRAMADDR
  by 1 or 32 depending on PPUCTRL bit 2.
- This module operates directly on `Ppu` internal state; bus-level PPUDATA mapping
  (nametables/palette mirroring) is handled elsewhere for CPU accesses.

Migration
- These are inherent methods on `Ppu`, moved here from helper-style functions to
  colocate behavior with a focused module. During the staged migration, call
  sites in `mod.rs` may still delegate to wrappers; those will be removed
  after all impls are in place.
"#]

use super::Ppu;

impl Ppu {
    /// CPU-visible register read ($2000..$2007) with mirroring and side-effects.
    ///
    /// Mirrors any address in 0x2000..=0x3FFF to 0x2000..=0x2007 and applies
    /// PPUSTATUS side-effects (clear vblank, reset write toggle) as appropriate.
    ///
    /// PPUDATA ($2007) uses buffered read semantics for addresses below $3F00 and
    /// increments VRAMADDR according to PPUCTRL bit 2 (1 or 32).
    pub(in crate::ppu) fn read_reg_inner(&mut self, addr: u16) -> u8 {
        let reg = 0x2000 + (addr & 0x7);
        match reg {
            0x2000 => self.ctrl,
            0x2001 => self.mask,
            0x2002 => {
                let v = self.status;
                self.status &= !0x80; // Clear vblank
                self.write_toggle = false;
                v
            }
            0x2003 => self.oam_addr,
            0x2004 => self.oam[self.oam_addr as usize],
            0x2005 => 0,
            0x2006 => ((self.vram_addr >> 8) & 0xFF) as u8,
            0x2007 => {
                let a = self.vram_addr & 0x3FFF;
                let value = self.vram[a as usize];
                let ret = if a < 0x3F00 {
                    // buffered read
                    let out = self.vram_buffer;
                    self.vram_buffer = value;
                    out
                } else {
                    value
                };
                let inc = if (self.ctrl & 0x04) != 0 { 32 } else { 1 };
                self.vram_addr = self.vram_addr.wrapping_add(inc) & 0x3FFF;
                ret
            }
            _ => 0,
        }
    }

    /// CPU-visible register write ($2000..$2007) with mirroring and side-effects.
    ///
    /// Mirrors any address in 0x2000..=0x3FFF to 0x2000..=0x2007 and applies
    /// PPUADDR/PPUSCROLL two-write latch behavior. PPUDATA writes do a raw write
    /// into the internal VRAM buffer and increment VRAMADDR according to PPUCTRL.
    pub(in crate::ppu) fn write_reg_inner(&mut self, addr: u16, value: u8) {
        let reg = 0x2000 + (addr & 0x7);
        match reg {
            0x2000 => {
                self.ctrl = value;
            }
            0x2001 => self.mask = value,
            0x2002 => { /* read-only */ }
            0x2003 => self.oam_addr = value,
            0x2004 => {
                let idx = self.oam_addr as usize;
                self.oam[idx] = value;
                self.oam_addr = self.oam_addr.wrapping_add(1);
            }
            0x2005 => {
                if !self.write_toggle {
                    self.scroll_x = value;
                    self.write_toggle = true;
                } else {
                    self.scroll_y = value;
                    self.write_toggle = false;
                }
            }
            0x2006 => {
                if !self.write_toggle {
                    self.vram_addr = (self.vram_addr & 0x00FF) | (((value as u16) & 0x3F) << 8);
                    self.write_toggle = true;
                } else {
                    self.vram_addr = (self.vram_addr & 0x7F00) | (value as u16);
                    self.vram_addr &= 0x3FFF;
                    self.write_toggle = false;
                }
            }
            0x2007 => {
                let a = (self.vram_addr & 0x3FFF) as usize;
                self.vram[a] = value;
                let inc = if (self.ctrl & 0x04) != 0 { 32 } else { 1 };
                self.vram_addr = self.vram_addr.wrapping_add(inc) & 0x3FFF;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_read_clears_vblank_and_write_toggle() {
        let mut p = Ppu::new();
        p.set_vblank(true);
        p.set_write_toggle(true);
        let s = p.read_reg(0x2002);
        assert_ne!(
            s & 0x80,
            0,
            "PPUSTATUS read should return VBlank=1 when it was set"
        );
        assert!(!p.vblank(), "PPUSTATUS read must clear VBlank (bit 7)");
        assert!(
            !p.get_write_toggle(),
            "PPUSTATUS read must clear the write toggle"
        );
    }

    #[test]
    fn ppudata_buffered_read_and_increment() {
        let mut p = Ppu::new();
        // PPUCTRL increment = 1
        p.write_reg(0x2000, 0x00);

        // Seed VRAM values via direct poke helpers (PPU address space)
        p.poke_vram(0x0000, 0x11);
        p.poke_vram(0x0001, 0x22);

        // Set VRAMADDR to $0000 via PPUADDR high/low writes
        p.write_reg(0x2006, 0x00);
        p.write_reg(0x2006, 0x00);

        // PPUDATA buffered read behavior:
        // - first read returns old buffer (0), loads buffer with $0000 (0x11)
        // - next read returns 0x11, loads buffer with $0001 (0x22)
        // - next read returns 0x22, and so on
        assert_eq!(p.read_reg(0x2007), 0x00);
        assert_eq!(p.read_reg(0x2007), 0x11);
        assert_eq!(p.read_reg(0x2007), 0x22);
    }

    #[test]
    fn ppuctrl_increment_32_on_ppudata_write() {
        let mut p = Ppu::new();
        // Set VRAM increment to 32 (PPUCTRL bit 2)
        p.write_reg(0x2000, 0x04);

        // Set VRAMADDR to $2000 via PPUADDR high/low writes
        p.write_reg(0x2006, 0x20);
        p.write_reg(0x2006, 0x00);
        assert_eq!(p.get_vram_addr(), 0x2000);

        // PPUDATA write should increment by 32
        p.write_reg(0x2007, 0xAA);
        assert_eq!(p.get_vram_addr(), 0x2020);

        // Another write increments again by 32
        p.write_reg(0x2007, 0xBB);
        assert_eq!(p.get_vram_addr(), 0x2040);
    }
}
