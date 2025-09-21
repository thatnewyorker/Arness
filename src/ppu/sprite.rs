#![doc = r#"
PPU sprite helpers

Responsibilities
- Manage sprite shift registers for visible scanlines (per-slot load and x counters).
- Produce a sprite pixel per dot with priority handling and sprite-zero hit gating.
- Provide a bit-reversal helper for horizontal flip (`reverse8`).

Integration
- Implemented as inherent methods on `Ppu`, accessing private state from the parent module.
- Bus access uses the canonical `crate::bus::interfaces::PpuBus` trait for palette reads.

PPU submodule structure (overview)
- `registers.rs` — CPU-visible register semantics ($2000–$2007)
- `memory.rs` — VRAM/OAM peek/poke and OAM DMA helpers
- `oam_eval.rs` — sprite evaluation phases (CLEAR/EVALUATE/FETCH)
- `fetch.rs` — per-dot background fetch and `bg_opaque` updates
- `sprite.rs` — this module: sprite shift registers and per-dot sprite output
- `renderer.rs` — timing orchestration (`tick`) and frame renderer

Notes
- Legacy temporary shims were removed; call sites now use the inherent methods directly.
"#]

use super::*;

/// Inherent sprite methods on `Ppu`.
impl Ppu {
    /// Load fetched sprite pattern bytes into shift registers for the new visible scanline.
    /// Applies horizontal flip by reversing bit order at load time.
    #[inline]
    pub(in crate::ppu) fn load_sprite_shift_registers(&mut self) {
        for slot in 0..8 {
            let base = slot * 4;
            // Slot considered valid if secondary OAM Y != 0xFF (populated during evaluation)
            if self.secondary_oam[base] == 0xFF {
                self.sprite_slot_active[slot] = false;
                continue;
            }

            let attr = self.sprite_slot_attr[slot];
            let hflip = (attr & 0x40) != 0;

            let mut low = self.sprite_slot_pattern_low[slot];
            let mut high = self.sprite_slot_pattern_high[slot];

            if hflip {
                low = reverse8(low);
                high = reverse8(high);
            }

            self.sprite_slot_shift_low[slot] = low;
            self.sprite_slot_shift_high[slot] = high;
            self.sprite_slot_x_counter[slot] = self.sprite_slot_x[slot];
            self.sprite_slot_active[slot] = true;
        }
    }

    /// Produce a sprite pixel at (x,y) by evaluating up to 8 sprite slots with shift registers.
    /// Applies priority rules and sets sprite-zero hit when conditions are met.
    #[inline]
    pub(in crate::ppu) fn produce_sprite_pixel<B: crate::bus::interfaces::PpuBus>(
        &mut self,
        bus: &B,
        x: usize,
        y: usize,
    ) {
        // If sprites disabled (PPUMASK bit 4), skip.
        if (self.mask & 0x10) == 0 {
            return;
        }
        if x >= NES_WIDTH || y >= NES_HEIGHT {
            return;
        }

        // First, decrement X counters for active slots.
        for s in 0..8 {
            if self.sprite_slot_active[s] && self.sprite_slot_x_counter[s] > 0 {
                self.sprite_slot_x_counter[s] -= 1;
            }
        }

        // Select the first non-transparent sprite pixel among active slots whose x_counter == 0.
        let mut chosen_slot: Option<usize> = None;
        let mut chosen_ci: u8 = 0;
        for s in 0..8 {
            if !self.sprite_slot_active[s] || self.sprite_slot_x_counter[s] != 0 {
                continue;
            }
            let lo = (self.sprite_slot_shift_low[s] >> 7) & 1;
            let hi = (self.sprite_slot_shift_high[s] >> 7) & 1;
            let ci = (hi << 1) | lo;
            if ci != 0 {
                chosen_slot = Some(s);
                chosen_ci = ci;
                break;
            }
        }

        // Background opacity at this pixel
        let bg_is_opaque = self.bg_opaque[y * NES_WIDTH + x] != 0;

        // If we have a candidate, apply priority and draw if allowed.
        if let Some(s) = chosen_slot {
            let attr = self.sprite_slot_attr[s];
            let palette = (attr & 0x03) as u16;
            let priority_behind_bg = (attr & 0x20) != 0;

            if !(priority_behind_bg && bg_is_opaque) {
                // Sprite pixel visible: perform palette lookup in sprite palette space 0x3F10..0x3F1F
                let pal_addr = 0x3F10 + palette * 4 + chosen_ci as u16;
                let nes_palette_entry = bus.ppu_read(pal_addr) & 0x3F;
                let rgba = NES_PALETTE[nes_palette_entry as usize];
                let fi = (y * NES_WIDTH + x) * BYTES_PER_PIXEL;
                self.framebuffer[fi] = rgba[0];
                self.framebuffer[fi + 1] = rgba[1];
                self.framebuffer[fi + 2] = rgba[2];
                self.framebuffer[fi + 3] = 0xFF;

                // Sprite-zero hit: when sprite 0 overlaps non-zero background
                if self.sprite_slot_is_zero[s] && bg_is_opaque {
                    // Accurate sprite-zero gating: require BG+SPR enabled and respect left-8 masking
                    let bg_enabled = (self.mask & 0x08) != 0;
                    let spr_enabled = (self.mask & 0x10) != 0;
                    let show_bg_left = (self.mask & 0x02) != 0;
                    let show_spr_left = (self.mask & 0x04) != 0;
                    let in_left8 = x < 8;
                    if bg_enabled && spr_enabled && (!in_left8 || (show_bg_left && show_spr_left)) {
                        self.set_sprite_zero_hit(true);
                        self.sprite_zero_hit_scanline = self.scanline;
                        self.sprite_zero_hit_dot = self.dot;
                    }
                }
            }
        }

        // Shift registers for all active slots that have started outputting pixels.
        for s in 0..8 {
            if self.sprite_slot_active[s] && self.sprite_slot_x_counter[s] == 0 {
                self.sprite_slot_shift_low[s] <<= 1;
                self.sprite_slot_shift_high[s] <<= 1;
                // Optionally, slots could be marked inactive after 8 shifts; not strictly necessary.
            }
        }
    }
}

/// Bit-reversal helper for horizontal flip (crate-visible within `ppu`).
#[inline]
pub(in crate::ppu) fn reverse8(v: u8) -> u8 {
    let mut x = v;
    x = (x & 0xF0) >> 4 | (x & 0x0F) << 4;
    x = (x & 0xCC) >> 2 | (x & 0x33) << 2;
    x = (x & 0xAA) >> 1 | (x & 0x55) << 1;
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sprite_basic_overlay() {
        let rom = crate::test_utils::build_ines(1, 0, 0, 0, 1, None);
        let cart = crate::cartridge::Cartridge::from_ines_bytes(&rom).unwrap();
        let mut bus = crate::bus::Bus::new();
        bus.attach_cartridge(cart);

        // Background tile 0: color index 1
        for row in 0..8 {
            bus.ppu_write(row as u16, 0xFF);
            bus.ppu_write(8 + row as u16, 0x00);
        }
        bus.ppu_write(0x2000, 0x00);
        bus.ppu_write(0x23C0, 0x00);
        bus.ppu_write(0x3F00, 0x00);
        bus.ppu_write(0x3F01, 0x03);

        // Sprite tile 1: ci=3 (low AND high both 0xFF => bits produce 3)
        for row in 0..8 {
            bus.ppu_write(0x0010 + row as u16, 0xFF);
            bus.ppu_write(0x0010 + 8 + row as u16, 0xFF);
        }
        // Sprite palette entries (choose a distinct palette color)
        bus.ppu_write(0x3F10, 0x00);
        bus.ppu_write(0x3F11, 0x04);
        bus.ppu_write(0x3F12, 0x05);
        bus.ppu_write(0x3F13, 0x06);

        // Enable background & sprites
        bus.ppu_mut().mask = 0x18;

        {
            let p = bus.ppu_mut();
            p.oam[0] = 0; // Y
            p.oam[1] = 1; // tile index
            p.oam[2] = 0x00; // attributes
            p.oam[3] = 0; // X
        }

        bus.render_ppu_frame();
        let fb = bus.ppu().framebuffer();
        let expected = NES_PALETTE[0x06];
        assert_eq!(fb[0], expected[0]);
        assert_eq!(fb[1], expected[1]);
        assert_eq!(fb[2], expected[2]);
    }

    #[test]
    fn sprite_zero_hit_basic() {
        let rom = crate::test_utils::build_ines(1, 0, 0, 0, 1, None);
        let cart = crate::cartridge::Cartridge::from_ines_bytes(&rom).unwrap();
        let mut bus = crate::bus::Bus::new();
        bus.attach_cartridge(cart);

        // Background tile 0: color index 1
        for row in 0..8 {
            bus.ppu_write(row as u16, 0xFF);
            bus.ppu_write(8 + row as u16, 0x00);
        }
        bus.ppu_write(0x2000, 0x00);
        bus.ppu_write(0x23C0, 0x00);
        bus.ppu_write(0x3F00, 0x00);
        bus.ppu_write(0x3F01, 0x03);

        // Sprite tile 1: solid ci=1 (low=0xFF, high=0x00)
        for row in 0..8 {
            bus.ppu_write(0x0010 + row as u16, 0xFF);
            bus.ppu_write(0x0010 + 8 + row as u16, 0x00);
        }
        bus.ppu_write(0x3F10, 0x00);
        bus.ppu_write(0x3F11, 0x04);

        bus.ppu_mut().mask = 0x18;
        {
            let p = bus.ppu_mut();
            p.oam[0] = 0;
            p.oam[1] = 1;
            p.oam[2] = 0x00; // front
            p.oam[3] = 0;
        }
        bus.render_ppu_frame();
        assert!(bus.ppu().sprite_zero_hit());
    }

    #[test]
    fn sprite_priority_behind_background() {
        let rom = crate::test_utils::build_ines(1, 0, 0, 0, 1, None);
        let cart = crate::cartridge::Cartridge::from_ines_bytes(&rom).unwrap();
        let mut bus = crate::bus::Bus::new();
        bus.attach_cartridge(cart);

        // Background tile 0: color index 1
        for row in 0..8 {
            bus.ppu_write(row as u16, 0xFF);
            bus.ppu_write(8 + row as u16, 0x00);
        }
        bus.ppu_write(0x2000, 0x00);
        bus.ppu_write(0x23C0, 0x00);
        bus.ppu_write(0x3F00, 0x00);
        bus.ppu_write(0x3F01, 0x03);

        // Sprite tile 1: pattern doesn't matter (solid)
        for row in 0..8 {
            bus.ppu_write(0x0010 + row as u16, 0xFF);
            bus.ppu_write(0x0010 + 8 + row as u16, 0x00);
        }
        bus.ppu_write(0x3F10, 0x00);
        bus.ppu_write(0x3F11, 0x04);

        bus.ppu_mut().mask = 0x18;
        {
            let p = bus.ppu_mut();
            // Clear all other sprites to avoid unintended overlaps; Y=0xFF marks empty.
            p.oam.fill(0xFF);
            // Define a single sprite 0 behind background at (0,0) using tile 1
            p.oam[0] = 0; // Y
            p.oam[1] = 1; // tile
            p.oam[2] = 0x20; // behind background
            p.oam[3] = 0; // X
        }
        bus.render_ppu_frame();
        // Expect background color at (0,0)
        let fb = bus.ppu().framebuffer();
        let expected = NES_PALETTE[0x03];
        assert_eq!(fb[0], expected[0]);
        assert_eq!(fb[1], expected[1]);
        assert_eq!(fb[2], expected[2]);
    }

    #[test]
    fn reverse8_correctness() {
        assert_eq!(reverse8(0x00), 0x00);
        assert_eq!(reverse8(0xFF), 0xFF);
        assert_eq!(reverse8(0x01), 0x80);
        assert_eq!(reverse8(0x02), 0x40);
        assert_eq!(reverse8(0xAA), 0x55);
        assert_eq!(reverse8(0x96), 0x69);
        assert_eq!(reverse8(0x3C), 0x3C);
    }

    #[test]
    fn sprite_zero_hit_left8_masking() {
        // Case A: left-8 masked off
        let rom = crate::test_utils::build_ines(1, 0, 0, 0, 1, None);
        let cart = crate::cartridge::Cartridge::from_ines_bytes(&rom).unwrap();
        let mut bus = crate::bus::Bus::new();
        bus.attach_cartridge(cart);

        // Background tile 0: color index 1
        for row in 0..8 {
            bus.ppu_write(row as u16, 0xFF);
            bus.ppu_write(8 + row as u16, 0x00);
        }
        bus.ppu_write(0x2000, 0x00);
        bus.ppu_write(0x23C0, 0x00);
        bus.ppu_write(0x3F00, 0x00);
        bus.ppu_write(0x3F01, 0x03);

        // Sprite tile 1: solid ci=1 (low=0xFF, high=0x00)
        for row in 0..8 {
            bus.ppu_write(0x0010 + row as u16, 0xFF);
            bus.ppu_write(0x0010 + 8 + row as u16, 0x00);
        }
        bus.ppu_write(0x3F10, 0x00);
        bus.ppu_write(0x3F11, 0x04);

        // Enable BG+SPR but disable left-8 for both
        bus.ppu_mut().mask = 0x18;

        {
            let p = bus.ppu_mut();
            p.oam.fill(0xFF);
            p.oam[0] = 0;
            p.oam[1] = 1;
            p.oam[2] = 0x00;
            p.oam[3] = 0;
        }

        // Advance into first visible scanline, leftmost 8 pixels
        loop {
            let (sl, dot) = bus.ppu()._test_scanline_dot();
            if sl == 0 && dot == 8 {
                break;
            }
            bus.tick(1);
        }

        // Left-8 masking should prevent sprite-zero hit
        assert!(
            !bus.ppu().sprite_zero_hit(),
            "sprite-zero hit should be masked off in left 8 pixels when mask bits 1/2 are clear"
        );

        // Case B: left-8 enabled for BG and SPR
        let rom2 = crate::test_utils::build_ines(1, 0, 0, 0, 1, None);
        let cart2 = crate::cartridge::Cartridge::from_ines_bytes(&rom2).unwrap();
        let mut bus2 = crate::bus::Bus::new();
        bus2.attach_cartridge(cart2);

        // Background tile 0: color index 1
        for row in 0..8 {
            bus2.ppu_write(row as u16, 0xFF);
            bus2.ppu_write(8 + row as u16, 0x00);
        }
        bus2.ppu_write(0x2000, 0x00);
        bus2.ppu_write(0x23C0, 0x00);
        bus2.ppu_write(0x3F00, 0x00);
        bus2.ppu_write(0x3F01, 0x03);

        // Sprite tile 1: solid ci=1 (low=0xFF, high=0x00)
        for row in 0..8 {
            bus2.ppu_write(0x0010 + row as u16, 0xFF);
            bus2.ppu_write(0x0010 + 8 + row as u16, 0x00);
        }
        bus2.ppu_write(0x3F10, 0x00);
        bus2.ppu_write(0x3F11, 0x04);

        // Enable BG+SPR and left-8 for both
        bus2.ppu_mut().mask = 0x1E;

        {
            let p = bus2.ppu_mut();
            p.oam.fill(0xFF);
            p.oam[0] = 0;
            p.oam[1] = 1;
            p.oam[2] = 0x00;
            p.oam[3] = 0;
        }

        // Advance into first visible scanline, leftmost 8 pixels
        loop {
            let (sl, dot) = bus2.ppu()._test_scanline_dot();
            if sl == 0 && dot == 8 {
                break;
            }
            bus2.tick(1);
        }

        // With left-8 enabled for both BG and SPR, hit should be set
        assert!(
            bus2.ppu().sprite_zero_hit(),
            "sprite-zero hit should be set in left 8 pixels when mask bits 1/2 are set"
        );
    }
}

// -----------------------------------------------------------------------------
// Temporary shims: thin adapters to maintain compatibility during migration
// -----------------------------------------------------------------------------
