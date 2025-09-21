#![doc = r#"
PPU memory submodule

Responsibilities
- Centralize direct VRAM/OAM accessors and the OAM DMA copy as inherent methods on `Ppu`.
- Provide a single place to evolve mirroring/aliasing and side-effect rules without cluttering `mod.rs`.

Integration
- Implemented as inherent methods on `Ppu` to keep fields private while allowing direct access.
- Called by thin public wrappers in `ppu::mod.rs` (e.g., `peek_vram`, `poke_vram`, `oam_dma_copy`).
- Cooperates with:
  * `registers.rs` for CPU-visible register semantics (PPUDATA buffered behavior, PPUADDR/PPUSCROLL latches).
  * `oam_eval.rs` which consumes OAM; DMA populates OAM used by evaluation.
  * `renderer.rs` timing loop, which orchestrates per-dot phases and frame progression.
- Bus reads for pattern/nametable/palette during rendering use the canonical `crate::bus::interfaces::PpuBus`
  and are intentionally out of scope for this module.

Scope
- VRAM peek/poke (with address masking to 14 bits).
- OAM peek/poke (with 256-entry wrapping).
- OAM DMA inner copy helper (copy 256 bytes, starting at current OAMADDR and wrapping).

Design
- These are inner helpers (`*_inner`) intended to be called from public wrappers in `mod.rs`
  (following the pattern used by registers: `read_reg_inner` / `write_reg_inner`).
- Keeping `Ppu` fields private and implementing these in a child module preserves encapsulation
  while allowing direct access from the PPU implementation.

Testing
- Unit tests live in this file to validate masking/wrapping and DMA behavior end-to-end using `Ppu`.
"#]

use super::Ppu;

impl Ppu {
    /// OAM DMA inner copy helper (256 bytes).
    ///
    /// Semantics:
    /// - Copies 256 bytes from `data` into OAM, starting at the current `OAMADDR` ($2003).
    /// - Writes wrap around OAM (256 bytes total).
    /// - Updates `OAMADDR` to its final post-increment value (wrap-aware).
    ///
    /// Notes:
    /// - Callers should ensure `data` contains at least 256 bytes; any missing bytes are treated as 0.
    /// - CPU stall timing and bus-cycle details are handled at a higher level (DMA controller/bus).
    pub(in crate::ppu) fn oam_dma_copy_inner(&mut self, data: &[u8]) {
        let mut oam_ptr = self.oam_addr;
        for i in 0..256 {
            let b = data.get(i).copied().unwrap_or(0);
            self.oam[oam_ptr as usize] = b;
            oam_ptr = oam_ptr.wrapping_add(1);
        }
        self.oam_addr = oam_ptr;
    }

    /// Read a byte from VRAM (PPU address space), masked to 14 bits.
    ///
    /// This is a raw peek that does not emulate PPUDATA buffering or palette mirroring
    /// side-effects. Use register-level helpers for CPU-visible behavior.
    #[inline]
    pub(in crate::ppu) fn peek_vram_inner(&self, addr: u16) -> u8 {
        self.vram[(addr as usize) & 0x3FFF]
    }

    /// Write a byte to VRAM (PPU address space), masked to 14 bits.
    ///
    /// This is a raw poke that does not emulate palette mirroring peculiarities.
    /// Use register-level helpers for CPU-visible behavior through PPUDATA.
    #[inline]
    pub(in crate::ppu) fn poke_vram_inner(&mut self, addr: u16, value: u8) {
        self.vram[(addr as usize) & 0x3FFF] = value;
    }

    /// Read a byte from primary OAM (256 bytes), index wraps to 0x00..=0xFF.
    ///
    /// This is a raw read; it does not model the active evaluation windows or OAMDATA port
    /// side-effects. Use register-level helpers to mirror CPU-visible semantics.
    #[inline]
    pub(in crate::ppu) fn peek_oam_inner(&self, idx: usize) -> u8 {
        self.oam[idx & 0xFF]
    }

    /// Write a byte to primary OAM (256 bytes), index wraps to 0x00..=0xFF.
    ///
    /// This is a raw write; it does not model the OAMDATA port increment behavior which
    /// is handled by the register write path.
    #[inline]
    pub(in crate::ppu) fn poke_oam_inner(&mut self, idx: usize, value: u8) {
        self.oam[idx & 0xFF] = value;
    }
}

#[cfg(test)]
mod tests {
    use crate::ppu::Ppu;

    #[test]
    fn vram_peek_poke_masking() {
        let mut p = Ppu::new();
        // 0x4000 aliases 0x0000 due to 14-bit masking
        p.poke_vram(0x4000, 0xAA);
        assert_eq!(p.peek_vram(0x0000), 0xAA);

        // Highest masked address and its mirrors
        p.poke_vram(0x7FFF, 0x55); // 0x7FFF -> 0x3FFF
        assert_eq!(p.peek_vram(0x3FFF), 0x55);
        assert_eq!(p.peek_vram(0xBFFF), 0x55); // 0xBFFF -> 0x3FFF
    }

    #[test]
    fn oam_peek_poke_wrapping() {
        let mut p = Ppu::new();
        // Indices wrap to 0x00..=0xFF
        p.poke_oam(0x1FF, 0x11); // -> 0xFF
        assert_eq!(p.peek_oam(0xFF), 0x11);
        p.poke_oam(0x100, 0x22); // -> 0x00
        assert_eq!(p.peek_oam(0x00), 0x22);
    }

    #[test]
    fn oam_dma_copy_wraps_and_updates_oamaddr() {
        let mut p = Ppu::new();
        // Set OAMADDR to 0xFE
        p.write_reg(0x2003, 0xFE);

        // Prepare DMA buffer [0,1,2,...,255]
        let mut buf = [0u8; 256];
        for (i, b) in buf.iter_mut().enumerate() {
            *b = i as u8;
        }

        // Perform DMA copy; writes should wrap and OAMADDR should end at 0xFE
        p.oam_dma_copy(&buf);
        assert_eq!(p.peek_oam(0xFE), 0x00);
        assert_eq!(p.peek_oam(0xFF), 0x01);
        assert_eq!(p.peek_oam(0x00), 0x02);
        assert_eq!(p.peek_oam(0x01), 0x03);
        assert_eq!(p.read_reg(0x2003), 0xFE);
    }
}
