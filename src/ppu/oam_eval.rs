#![doc = r#"
PPU OAM evaluation submodule

Responsibilities
- Provide per-dot, phase-specific helpers for the cycle-accurate sprite pipeline:
  * CLEAR:      secondary OAM clear (dots 1..64)
  * EVALUATE:   primary OAM evaluation/selection (dots 65..256)
  * FETCH:      sprite pattern fetches for selected slots (dots 257..320)
- Populate `secondary_oam` and per-slot metadata/pattern buffers used by the sprite pipeline.

Integration
- Implemented as inherent methods on `Ppu`, keeping all PPU state private within `mod.rs`.
- Called from the timing loop in `renderer.rs` at the appropriate dots on visible and pre-render
  scanlines:
  * `oam_clear_step()` during dots 1..=64
  * `oam_evaluate_step()` during dots 65..=256
  * `oam_fetch_step()` during dots 257..=320
- Cooperation with other PPU submodules:
  * `sprite.rs`: consumes latched per-slot metadata and pattern bytes to load shift registers at
    the start of visible scanlines and to produce per-dot sprite pixels.
  * `fetch.rs`: per-dot background fetch runs alongside these phases for visible dots.
  * `memory.rs`: centralizes VRAM/OAM peek/poke and OAM DMA helpers used by register and test paths.
- Bus access during FETCH uses the canonical `crate::bus::interfaces::PpuBus` to read pattern data.

Behavioral notes
- CLEAR writes 0xFF to secondary OAM and resets evaluation counters at dot 64.
- EVALUATE:
  - Iterates OAM entries (up to 64 sprites), choosing up to 8 that intersect the next scanline.
  - Copies four bytes (Y, tile, attr, X) per selected sprite into secondary OAM.
  - Sets sprite overflow when encountering a 9th in-range sprite at the Y-byte boundary.
- FETCH:
  - For each of the 8 slots, latches metadata and reads low/high pattern bytes based on sprite
    height and vertical flip, using the scanline+1 row for addressing (preparing next scanline).

Visibility
- `pub(in crate::ppu)` ensures these helpers are available to sibling PPU submodules only.
"#]

use super::Ppu;

impl Ppu {
    /// CLEAR phase: dots 1..=64 on visible and pre-render scanlines
    ///
    /// Behavior:
    /// - Writes 0xFF into secondary OAM (1 byte per dot).
    /// - At the end of dot 64, resets evaluation counters for the upcoming EVALUATE phase.
    #[inline]
    pub(in crate::ppu) fn oam_clear_step(&mut self) {
        if self.dot >= 1 && self.dot <= 64 {
            let idx = (self.dot - 1) as usize;
            if idx < self.secondary_oam.len() {
                self.secondary_oam[idx] = 0xFF;
            }
            if self.dot == 64 {
                // Prepare EVALUATE phase
                self.oam_eval_oam_index = 0;
                self.oam_eval_byte_index = 0;
                self.oam_eval_sprite_count = 0;
                self.oam_secondary_write_index = 0;
                // Overflow tracking remains until frame end to aid debugging, but we clear latch
                // of the boolean each scanline to track first occurrence per frame if desired.
                self.oam_eval_overflow_detected = false;
            }
        }
    }

    /// EVALUATE phase: dots 65..=256 on visible and pre-render scanlines
    ///
    /// Behavior:
    /// - Scans primary OAM to select up to 8 sprites intersecting `next_scanline = scanline + 1`.
    /// - Copies four bytes (Y, tile, attr, X) for each selected sprite into secondary OAM.
    /// - On encountering a 9th in-range sprite at the Y-byte boundary, sets overflow flag.
    #[inline]
    pub(in crate::ppu) fn oam_evaluate_step(&mut self) {
        if !(self.dot >= 65 && self.dot <= 256) {
            return;
        }
        // Continue evaluating sprites until we run out of OAM entries
        if self.oam_eval_oam_index >= 64 {
            return;
        }

        let sprite_index = self.oam_eval_oam_index as usize;
        let byte_index = self.oam_eval_byte_index as usize;
        let base = sprite_index * 4;

        // Sprite height: 8 or 16 based on PPUCTRL bit 5 (0x20)
        let sprite_height = if (self.ctrl & 0x20) != 0 { 16 } else { 8 };

        // The pipeline evaluates for the next scanline
        let next_scanline = self.scanline + 1;

        // Primary OAM Y check (raw semantics)
        let y_byte = self.oam[base] as i16;
        let in_range = next_scanline >= y_byte && next_scanline < y_byte + sprite_height as i16;

        // Overflow detection: ninth in-range sprite on its Y byte
        if in_range
            && self.oam_eval_sprite_count >= 8
            && self.oam_eval_byte_index == 0
            && !self.oam_eval_overflow_detected
        {
            self.set_sprite_overflow(true);
            self.oam_eval_overflow_detected = true;
            self.oam_eval_overflow_scanline = self.scanline;
            self.oam_eval_overflow_dot = self.dot;
        }

        // Copy current sprite into secondary OAM if in-range and we still have room (<8).
        let copying = in_range && self.oam_eval_sprite_count < 8;
        let value = self.oam[base + byte_index];

        if copying {
            // Initialize internal write pointer at start of a new sprite copy.
            if self.oam_eval_byte_index == 0 {
                self.oam_secondary_write_index = self.oam_eval_sprite_count.saturating_mul(4);
            }
            let slot = self.oam_eval_sprite_count as usize;

            // Track original OAM index for priority and sprite-zero hit.
            self.sprite_slot_orig_index[slot] = self.oam_eval_oam_index;
            self.sprite_slot_is_zero[slot] = self.oam_eval_oam_index == 0;

            let dest = self.oam_secondary_write_index as usize + byte_index;
            if dest < self.secondary_oam.len() {
                self.secondary_oam[dest] = value;
            }
        }

        // Advance to next byte within the current sprite. After byte 3, move to next sprite.
        self.oam_eval_byte_index = self.oam_eval_byte_index.wrapping_add(1);
        if self.oam_eval_byte_index >= 4 {
            self.oam_eval_byte_index = 0;
            if copying {
                self.oam_eval_sprite_count = self.oam_eval_sprite_count.saturating_add(1);
                self.oam_secondary_write_index = self.oam_secondary_write_index.wrapping_add(4);
            }
            self.oam_eval_oam_index = self.oam_eval_oam_index.wrapping_add(1);
        }
    }

    /// FETCH phase: dots 257..=320 on visible and pre-render scanlines
    ///
    /// Behavior:
    /// - For each of 8 sprite slots, perform sub-steps to:
    ///   * sub 0: latch metadata (Y, tile, attr, X) from secondary OAM into slot fields.
    ///   * sub 5: fetch low pattern byte for the sprite row used by next scanline.
    ///   * sub 6: fetch high pattern byte for the sprite row used by next scanline.
    ///
    /// Addressing rules:
    /// - 8x8 sprites: base selected by PPUCTRL bit 3 (0x08) => 0x0000 or 0x1000.
    /// - 8x16 sprites: pattern table selected by tile LSB (bit 0), base tile = tile & 0xFE,
    ///   top/bottom tile selected by row(0..15) < 8, row_in_tile = row & 7.
    /// - Vertical flip (attr bit 7) reverses the row index within the sprite.
    #[inline]
    pub(in crate::ppu) fn oam_fetch_step<B: crate::bus::interfaces::PpuBus>(&mut self, bus: &B) {
        if !(self.dot >= 257 && self.dot <= 320) {
            return;
        }

        // Compute slot and sub-cycle for this dot.
        let fetch_idx = self.dot - 257; // 0..63
        let slot = (fetch_idx / 8) as usize; // 0..7
        let sub = fetch_idx % 8; // 0..7

        if slot >= 8 {
            return;
        }

        // Validate slot by checking secondary OAM Y != 0xFF
        let base = slot * 4;
        let y = self.secondary_oam[base];
        if y == 0xFF {
            return;
        }

        if sub == 0 {
            // Latch sprite metadata from secondary OAM for this slot
            let tile = self.secondary_oam[base + 1];
            let attr = self.secondary_oam[base + 2];
            let x = self.secondary_oam[base + 3];
            self.sprite_slot_y[slot] = y;
            self.sprite_slot_tile[slot] = tile;
            self.sprite_slot_attr[slot] = attr;
            self.sprite_slot_x[slot] = x;
            return;
        }

        if sub == 5 || sub == 6 {
            let tile = self.secondary_oam[base + 1];
            let attr = self.secondary_oam[base + 2];

            // Sprite height: 8 or 16 based on PPUCTRL bit 5 (0x20)
            let sprite_height = if (self.ctrl & 0x20) != 0 { 16 } else { 8 };
            let next_scanline = self.scanline + 1;

            // Row within sprite for next scanline; in-range guaranteed by evaluation
            let mut row_in_sprite = (next_scanline - y as i16) as u16; // 0..(H-1)
            // Apply vertical flip
            if (attr & 0x80) != 0 {
                row_in_sprite = (sprite_height as u16 - 1) - row_in_sprite;
            }

            let (addr_low, addr_high) = if sprite_height == 8 {
                // 8x8 sprites: pattern base from PPUCTRL bit 3 (0x08)
                let base_sel = if (self.ctrl & 0x08) != 0 {
                    0x1000
                } else {
                    0x0000
                };
                let row = row_in_sprite & 7;
                let a = base_sel + (tile as u16) * 16 + row;
                (a, a + 8)
            } else {
                // 8x16 sprites: table from tile LSB; tile index even/odd select top/bottom
                let table = (tile as u16 & 1) * 0x1000;
                let base_tile = (tile & 0xFE) as u16;
                let row = row_in_sprite; // 0..15
                let tile_select = if row < 8 { 0 } else { 1 };
                let row_in_tile = row & 7;
                let a = table + (base_tile + tile_select) * 16 + row_in_tile;
                (a, a + 8)
            };

            if sub == 5 {
                let low = bus.ppu_read(addr_low);
                self.sprite_slot_pattern_low[slot] = low;
            } else {
                let high = bus.ppu_read(addr_high);
                self.sprite_slot_pattern_high[slot] = high;
            }
        }
    }
}

#[cfg(test)]
impl Ppu {
    pub(in crate::ppu) fn _test_secondary_oam(&self) -> &[u8; 32] {
        &self.secondary_oam
    }
    pub(in crate::ppu) fn _test_oam_eval_sprite_count(&self) -> u8 {
        self.oam_eval_sprite_count
    }
    pub(in crate::ppu) fn _test_sprite_slot_patterns(&self) -> (&[u8; 8], &[u8; 8]) {
        (
            &self.sprite_slot_pattern_low,
            &self.sprite_slot_pattern_high,
        )
    }
    pub(in crate::ppu) fn _test_sprite_slot_meta(
        &self,
    ) -> (&[u8; 8], &[u8; 8], &[u8; 8], &[u8; 8]) {
        (
            &self.sprite_slot_y,
            &self.sprite_slot_tile,
            &self.sprite_slot_attr,
            &self.sprite_slot_x,
        )
    }
    pub(in crate::ppu) fn _test_scanline_dot(&self) -> (i16, u16) {
        (self.scanline, self.dot)
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn oam_clear_phase_secondary_oam_filled() {
        let rom = crate::test_utils::build_ines(1, 0, 0, 0, 1, None);
        let cart = crate::cartridge::Cartridge::from_ines_bytes(&rom).unwrap();
        let mut bus = crate::bus::Bus::new();
        bus.attach_cartridge(cart);

        // Advance until scanline 0 dot 64 (end of CLEAR phase for first visible line).
        loop {
            let (sl, dot) = bus.ppu()._test_scanline_dot();
            if sl == 0 && dot == 64 {
                break;
            }
            bus.tick(1);
        }

        let so = bus.ppu()._test_secondary_oam();
        for (i, b) in so.iter().enumerate() {
            assert_eq!(*b, 0xFF, "secondary_oam[{}] not cleared", i);
        }
    }

    #[test]
    fn oam_evaluate_phase_copies_up_to_8_in_range_sprites() {
        // Build a minimal cart; CHR not relevant for evaluation.
        let rom = crate::test_utils::build_ines(1, 0, 0, 0, 1, None);
        let cart = crate::cartridge::Cartridge::from_ines_bytes(&rom).unwrap();
        let mut bus = crate::bus::Bus::new();
        bus.attach_cartridge(cart);

        {
            // Prepare 10 sprites whose Y=0 so they are all in range for next_scanline=1 during scanline 0 evaluation.
            // Each sprite i has bytes: Y=0, tile=i, attr=0, X=i
            let ppu = bus.ppu_mut();
            for i in 0..10u8 {
                let base = (i as usize) * 4;
                ppu.poke_oam(base + 0, 0); // Y
                ppu.poke_oam(base + 1, i); // tile
                ppu.poke_oam(base + 2, 0x00); // attributes
                ppu.poke_oam(base + 3, i); // X
            }
        }

        // Advance to the end of evaluation window of first visible scanline:
        // Reach scanline 0, dot 256
        loop {
            let (sl, dot) = bus.ppu()._test_scanline_dot();
            if sl == 0 && dot == 256 {
                break;
            }
            bus.tick(1);
        }

        let ppu_ref = bus.ppu();
        // Expect exactly 8 accepted sprites
        assert_eq!(
            ppu_ref._test_oam_eval_sprite_count(),
            8,
            "Expected 8 sprites selected"
        );

        // Verify the 8 copied sprites match OAM entries 0..7
        let so = ppu_ref._test_secondary_oam();
        for s in 0..8usize {
            assert_eq!(so[s * 4 + 0], 0, "Sprite {} Y mismatch", s);
            assert_eq!(so[s * 4 + 1], s as u8, "Sprite {} tile mismatch", s);
            assert_eq!(so[s * 4 + 2], 0x00, "Sprite {} attr mismatch", s);
            assert_eq!(so[s * 4 + 3], s as u8, "Sprite {} X mismatch", s);
        }

        // Overflow should be set because more than 8 sprites were in-range.
        assert!(
            ppu_ref.sprite_overflow(),
            "Expected sprite overflow when more than 8 sprites are in-range"
        );
    }

    #[test]
    fn oam_fetch_phase_fetches_pattern_low_high_for_slot0() {
        let rom = crate::test_utils::build_ines(1, 0, 0, 0, 1, None);
        let cart = crate::cartridge::Cartridge::from_ines_bytes(&rom).unwrap();
        let mut bus = crate::bus::Bus::new();
        bus.attach_cartridge(cart);

        // Configure PPU: 8x8 sprites, sprite pattern table at $1000 (PPUCTRL bit 3)
        {
            let ppu = bus.ppu_mut();
            let ctrl = ppu.get_ctrl();
            let new_ctrl = (ctrl & !0x20) | 0x08; // clear 8x16, set sprite pattern base 0x1000
            ppu.set_ctrl(new_ctrl);
            // Sprite 0 at Y=0, tile=2, attr=0, X=0 so it's in-range for pre-render evaluation (next_scanline=0)
            ppu.poke_oam(0, 0); // Y
            ppu.poke_oam(1, 2); // tile index
            ppu.poke_oam(2, 0x00); // attributes
            ppu.poke_oam(3, 0); // X
        }

        // Program CHR pattern bytes for tile 2, row 0 at pattern base $1000
        let tile: u8 = 2;
        let low: u8 = 0xA5;
        let high: u8 = 0x5A;
        bus.ppu_write(0x1000 + (tile as u16) * 16, low);
        bus.ppu_write(0x1000 + (tile as u16) * 16 + 8, high);

        // Advance to the end of FETCH window on pre-render line (-1, dots 257..320)
        loop {
            let (sl, dot) = bus.ppu()._test_scanline_dot();
            if sl == -1 && dot == 320 {
                break;
            }
            bus.tick(1);
        }

        let ppu_ref = bus.ppu();
        let (pat_lo, pat_hi) = ppu_ref._test_sprite_slot_patterns();
        assert_eq!(pat_lo[0], low, "low plane mismatch for slot 0");
        assert_eq!(pat_hi[0], high, "high plane mismatch for slot 0");

        let (ys, tiles, attrs, xs) = ppu_ref._test_sprite_slot_meta();
        assert_eq!(ys[0], 0, "slot 0 Y mismatch");
        assert_eq!(tiles[0], tile, "slot 0 tile mismatch");
        assert_eq!(attrs[0], 0x00, "slot 0 attr mismatch");
        assert_eq!(xs[0], 0, "slot 0 X mismatch");
    }

    #[test]
    fn oam_fetch_8x16_vertical_flip_uses_bottom_tile_last_row() {
        let rom = crate::test_utils::build_ines(1, 0, 0, 0, 1, None);
        let cart = crate::cartridge::Cartridge::from_ines_bytes(&rom).unwrap();
        let mut bus = crate::bus::Bus::new();
        bus.attach_cartridge(cart);

        // Enable 8x16 sprites (PPUCTRL bit 5) and set a vertically flipped sprite 0 at Y=0.
        {
            let ppu = bus.ppu_mut();
            let ctrl = ppu.get_ctrl();
            ppu.set_ctrl(ctrl | 0x20); // 8x16 on
            // Sprite 0: tile=2 (even => pattern table 0x0000), attr=VFLIP, X=0
            ppu.poke_oam(0, 0); // Y
            ppu.poke_oam(1, 2); // tile index (base_tile=2, bottom tile=3)
            ppu.poke_oam(2, 0x80); // attributes (vertical flip)
            ppu.poke_oam(3, 0); // X
        }

        // With vertical flip and next_scanline=0 (pre-render line), row_in_sprite = 15.
        // That selects the bottom tile (base_tile+1) and row_in_tile=7.
        let low: u8 = 0x3C;
        let high: u8 = 0xC3;
        let base_tile: u16 = 2;
        let bottom_tile = base_tile + 1; // 3
        let addr = 0x0000 + bottom_tile * 16 + 7; // table=0x0000 (tile LSB=0), row=7
        bus.ppu_write(addr, low);
        bus.ppu_write(addr + 8, high);

        // Advance to end of FETCH window on pre-render line (-1, dots 257..320)
        loop {
            let (sl, dot) = bus.ppu()._test_scanline_dot();
            if sl == -1 && dot == 320 {
                break;
            }
            bus.tick(1);
        }

        let ppu_ref = bus.ppu();
        let (pat_lo, pat_hi) = ppu_ref._test_sprite_slot_patterns();
        assert_eq!(
            pat_lo[0], low,
            "low plane mismatch for vertically flipped 8x16"
        );
        assert_eq!(
            pat_hi[0], high,
            "high plane mismatch for vertically flipped 8x16"
        );

        let (ys, tiles, attrs, xs) = ppu_ref._test_sprite_slot_meta();
        assert_eq!(ys[0], 0, "slot 0 Y mismatch");
        assert_eq!(
            tiles[0], 2,
            "slot 0 tile mismatch (should reflect OAM tile index)"
        );
        assert_eq!(attrs[0], 0x80, "slot 0 attr mismatch (vertical flip)");
        assert_eq!(xs[0], 0, "slot 0 X mismatch");
    }

    #[test]
    fn vblank_and_nmi_latch_on_scanline_241_dot_1() {
        let rom = crate::test_utils::build_ines(1, 0, 0, 0, 1, None);
        let cart = crate::cartridge::Cartridge::from_ines_bytes(&rom).unwrap();
        let mut bus = crate::bus::Bus::new();
        bus.attach_cartridge(cart);

        // Enable NMI on VBlank
        {
            let ppu = bus.ppu_mut();
            let ctrl = ppu.get_ctrl();
            ppu.set_ctrl(ctrl | 0x80);
        }

        // Advance until (scanline=241, dot=1) which is where VBlank should be set.
        loop {
            let (sl, dot) = bus.ppu()._test_scanline_dot();
            if sl == 241 && dot == 1 {
                break;
            }
            bus.tick(1);
        }

        // VBlank bit should be set and NMI latch should be raised once.
        assert!(bus.ppu().vblank(), "VBlank flag should be set at 241,1");
        assert!(
            bus.nmi_pending,
            "NMI request should be latched when NMI enabled at VBlank start"
        );
    }
}
