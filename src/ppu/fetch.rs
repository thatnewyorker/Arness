#![doc = r#"
PPU fetch helpers

Responsibilities
- Per-dot background fetch used by the timing loop to paint one background pixel
- Nametable byte (tile id), attribute quadrant (palette group), pattern low/high bitplanes
- Palette lookup and RGBA write into the framebuffer
- Maintain `bg_opaque` for sprite priority and sprite-0 hit tests

Integration
- Implemented as inherent methods on `Ppu` to keep state private in `mod.rs` while exposing focused behavior.
- Bus access uses the canonical `crate::bus::interfaces::PpuBus` trait.

Submodules (PPU structure overview)
- `registers.rs` — CPU-visible register semantics ($2000–$2007)
- `memory.rs` — VRAM/OAM peek/poke and OAM DMA helpers
- `oam_eval.rs` — sprite evaluation phases (CLEAR/EVALUATE/FETCH)
- `fetch.rs` — this module: per-dot background fetch and `bg_opaque` updates
- `sprite.rs` — sprite shift registers and per-dot sprite pixel output
- `renderer.rs` — timing orchestration (`tick`) and frame renderer

Notes
- Scrolling is not yet implemented; this fetch uses a fixed origin (0,0).
- The PPU mask bit 3 (show background) is honored; when disabled, no pixel is written.
"#]

use super::*;

impl Ppu {
    /// Produce a single background pixel for the current (scanline, dot).
    /// Simplified per-dot background fetch mirroring logic (no fine scroll/wrapping).
    pub(in crate::ppu) fn per_dot_background_pixel<B: crate::bus::interfaces::PpuBus>(
        &mut self,
        bus: &B,
    ) {
        // Allocate framebuffer lazily at first visible pixel if needed.
        let needed = NES_WIDTH * NES_HEIGHT * BYTES_PER_PIXEL;
        if self.framebuffer.len() != needed {
            self.framebuffer.resize(needed, 0);
        }
        if self.bg_opaque.len() != NES_WIDTH * NES_HEIGHT {
            self.bg_opaque.resize(NES_WIDTH * NES_HEIGHT, 0);
        }

        let x = (self.dot - 1) as usize;
        let y = self.scanline as usize;
        if x >= NES_WIDTH || y >= NES_HEIGHT {
            return;
        }

        // Background enable check (mask bit 3). If disabled, leave pixel as transparent/unwritten.
        if (self.mask & 0x08) == 0 {
            return;
        }

        // Derive tile coordinates (no scrolling yet).
        let tile_x = x / 8;
        let tile_y = y / 8;
        let row_in_tile = y & 7;

        // Fetch tile id from nametable 0.
        let nt_index = 0x2000 + (tile_y as u16) * 32 + tile_x as u16;
        let tile_id = bus.ppu_read(nt_index);

        // Attribute fetch (same quadrant logic as frame renderer)
        let coarse_attr_y = tile_y / 4;
        let attr_row_quad_y = (tile_y % 4) / 2;
        let attr_index = 0x23C0 + (coarse_attr_y as u16) * 8 + (tile_x as u16 / 4);
        let attr_byte = bus.ppu_read(attr_index);
        let attr_quad_x = (tile_x % 4) / 2;
        let quadrant = attr_row_quad_y * 2 + attr_quad_x;
        let palette_group = (attr_byte >> (quadrant * 2)) & 0x03;

        // Pattern fetch (row within tile)
        let bg_pattern_base = if (self.ctrl & 0x10) != 0 {
            0x1000
        } else {
            0x0000
        };
        let pattern_addr = bg_pattern_base + (tile_id as u16) * 16 + row_in_tile as u16;
        let low_plane = bus.ppu_read(pattern_addr);
        let high_plane = bus.ppu_read(pattern_addr + 8);

        // Extract the bit for the current pixel within the 8-pixel tile row
        let bit = x & 7;
        let shift = 7 - bit;
        let lo = (low_plane >> shift) & 1;
        let hi = (high_plane >> shift) & 1;
        let ci = (hi << 1) | lo; // 0..3

        // Palette lookup: universal BG color for ci==0, otherwise the quadrant's sub-palette
        let nes_palette_entry = if ci == 0 {
            bus.ppu_read(0x3F00)
        } else {
            let pal = 0x3F00 + (palette_group as u16) * 4 + ci as u16;
            bus.ppu_read(pal)
        } & 0x3F;

        let rgba = NES_PALETTE[nes_palette_entry as usize];
        let fi = (y * NES_WIDTH + x) * BYTES_PER_PIXEL;
        self.framebuffer[fi] = rgba[0];
        self.framebuffer[fi + 1] = rgba[1];
        self.framebuffer[fi + 2] = rgba[2];
        self.framebuffer[fi + 3] = 0xFF;

        if ci != 0 {
            self.bg_opaque[y * NES_WIDTH + x] = 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_single_tile_basic() {
        // Build cart with CHR RAM (0 CHR banks => writable pattern table)
        let rom = crate::test_utils::build_ines(1, 0, 0, 0, 1, None);
        let cart = crate::cartridge::Cartridge::from_ines_bytes(&rom).unwrap();
        let mut bus = crate::bus::Bus::new();
        bus.attach_cartridge(cart);

        // Tile 0 pattern: all pixels color index 1 (low=0xFF, high=0x00)
        for row in 0..8 {
            bus.ppu_write(row as u16, 0xFF);
            bus.ppu_write(8 + row as u16, 0x00);
        }
        // Name table top-left tile = 0
        bus.ppu_write(0x2000, 0x00);
        bus.ppu_write(0x23C0, 0x00); // attributes
        // Palette: universal + color1
        bus.ppu_write(0x3F00, 0x00);
        bus.ppu_write(0x3F01, 0x03);

        bus.render_ppu_frame();
        let fb = bus.ppu().framebuffer();
        let expected = NES_PALETTE[0x03];
        assert_eq!(fb[0], expected[0]);
        assert_eq!(fb[1], expected[1]);
        assert_eq!(fb[2], expected[2]);
        assert_eq!(fb[3], 0xFF);
    }

    #[test]
    fn background_attribute_quadrants() {
        let rom = crate::test_utils::build_ines(1, 0, 0, 0, 1, None);
        let cart = crate::cartridge::Cartridge::from_ines_bytes(&rom).unwrap();
        let mut bus = crate::bus::Bus::new();
        bus.attach_cartridge(cart);

        // Tile pattern 0: color index 1
        for row in 0..8 {
            bus.ppu_write(row as u16, 0xFF);
            bus.ppu_write(8 + row as u16, 0x00);
        }
        // Place tile 0 at four quadrants within first attribute block
        for &(tx, ty) in &[(0u16, 0u16), (2, 0), (0, 2), (2, 2)] {
            let nt = 0x2000 + ty * 32 + tx;
            bus.ppu_write(nt, 0x00);
        }
        // Attribute: TL=0, TR=1, BL=2, BR=3
        let attr = (3 << 6) | (2 << 4) | (1 << 2);
        bus.ppu_write(0x23C0, attr);

        // Palettes
        bus.ppu_write(0x3F00, 0x00);
        bus.ppu_write(0x3F01, 0x01); // palette0 color1
        bus.ppu_write(0x3F05, 0x02); // palette1 color1
        bus.ppu_write(0x3F09, 0x03); // palette2 color1
        bus.ppu_write(0x3F0D, 0x04); // palette3 color1

        bus.render_ppu_frame();
        let fb = bus.ppu().framebuffer();

        let check = |tile_x: usize, tile_y: usize, pal_idx: usize| {
            let x = tile_x * 8;
            let y = tile_y * 8;
            let pix = (y * NES_WIDTH + x) * BYTES_PER_PIXEL;
            let expected = NES_PALETTE[pal_idx];
            assert_eq!(fb[pix], expected[0]);
            assert_eq!(fb[pix + 1], expected[1]);
            assert_eq!(fb[pix + 2], expected[2]);
            assert_eq!(fb[pix + 3], 0xFF);
        };
        check(0, 0, 0x01);
        check(2, 0, 0x02);
        check(0, 2, 0x03);
        check(2, 2, 0x04);
    }
}
