#![doc = r#"
PPU renderer module

Responsibilities
- Orchestrates per-dot timing and scanline/frame progression.
- Hosts `Ppu::tick` (cycle-accurate entry) and `Ppu::render_frame` (frame renderer).
- Integrates background and sprite helpers at the correct dots.

Submodules (structure overview)
- `registers.rs` — CPU-visible register semantics ($2000–$2007)
- `memory.rs` — VRAM/OAM peek/poke and OAM DMA helpers
- `oam_eval.rs` — sprite evaluation phases (CLEAR/EVALUATE/FETCH)
- `fetch.rs` — per-dot background fetch, palette lookup, and bg_opaque updates
- `sprite.rs` — sprite shift registers and per-dot sprite pixel output
- `renderer.rs` — this module: timing orchestration and composition

Public API
- `Ppu::tick(&mut self, bus: &impl PpuBus)`: advance one PPU dot (invoked 3x per CPU cycle).
- `Ppu::render_frame(&mut self, bus: &impl PpuBus)`: non-cycle-accurate full frame renderer.

Notes
- Uses the canonical `crate::bus::interfaces::PpuBus` for bus-facing reads.
- Child modules implement inherent methods on `Ppu` and access private fields directly.
"#]

use super::*;

impl Ppu {
    /// Advance one PPU dot (invoked 3x per CPU cycle by the bus).
    ///
    /// Phase B sprite pipeline skeleton:
    /// - This replaces the old frame-level sprite overlay approach conceptually.
    /// - The actual sprite fetching / shift-register logic will be integrated here
    ///   in subsequent incremental edits (fields & evaluation state machine not yet added).
    /// - For now this preserves timing (dot/scanline/frame progression) and flag behavior
    ///   while providing placeholder phase demarcations for later implementation:
    ///     * CLEAR secondary OAM: dots   1- 64
    ///     * EVALUATE (primary OAM): dots 65-256
    ///     * FETCH sprite patterns: dots 257-320
    ///     * IDLE / BG prefetch: dots 321-340
    /// - Visible dots for pixel generation: 1-256 on scanlines 0-239.
    pub fn tick<B: crate::bus::interfaces::PpuBus>(&mut self, bus: &B) {
        self.dot = self.dot.wrapping_add(1);

        // Entering first dot of a scanline: handle vblank / pre-render housekeeping
        if self.dot == 1 {
            if self.scanline == 241 {
                // Entering VBlank
                self.set_vblank(true);
                if self.nmi_enabled() {
                    self.nmi_latch = true;
                }
            } else if self.scanline == -1 {
                // Pre-render line: clear status flags at start
                self.set_vblank(false);
                self.set_sprite_zero_hit(false);
                self.set_sprite_overflow(false);
                self.frame_complete = false;
            } else {
                // Start of a visible scanline: load sprite shift registers from fetched patterns
                if self.scanline >= 0 && self.scanline < NES_HEIGHT as i16 {
                    self.load_sprite_shift_registers();
                }
            }
        }

        let visible_scanline = self.scanline >= 0 && self.scanline < NES_HEIGHT as i16;
        let prerender_line = self.scanline == -1;

        if visible_scanline || prerender_line {
            // --- Sprite pipeline phases (placeholders; logic to be filled incrementally) ---
            if self.dot >= 1 && self.dot <= 64 {
                self.oam_clear_step();
            } else if self.dot >= 65 && self.dot <= 256 {
                self.oam_evaluate_step();
            } else if self.dot >= 257 && self.dot <= 320 {
                self.oam_fetch_step(bus);
            } else if self.dot >= 321 && self.dot <= 340 {
                // BG prefetch phase; sprite pipeline idle
            }

            // Per-dot background + sprite pixel production
            if visible_scanline && (1..=256).contains(&self.dot) {
                self.per_dot_background_pixel(bus);
                self.produce_sprite_pixel(bus, (self.dot - 1) as usize, self.scanline as usize);
            }
        }

        // End-of-scanline wrap
        if self.dot >= 341 {
            self.dot = 0;
            self.scanline += 1;
            if self.scanline > 260 {
                self.scanline = -1;
                self.frame_complete = true;
            }
        }
    }

    /// Render a full frame (background + sprite overlay).
    /// Non-cycle-accurate. Background first, then sprites.
    pub fn render_frame<B: crate::bus::interfaces::PpuBus>(&mut self, bus: &B) {
        let needed = NES_WIDTH * NES_HEIGHT * BYTES_PER_PIXEL;
        if self.framebuffer.len() != needed {
            self.framebuffer.resize(needed, 0);
        }
        if self.bg_opaque.len() != NES_WIDTH * NES_HEIGHT {
            self.bg_opaque.resize(NES_WIDTH * NES_HEIGHT, 0);
        } else {
            self.bg_opaque.fill(0);
        }

        // Background pattern table base (PPUCTRL bit 4)
        let bg_pattern_base = if (self.ctrl & 0x10) != 0 {
            0x1000
        } else {
            0x0000
        };

        // Palette RGBA cache
        let mut rgba_cache = [[0u8; 4]; 64];
        for i in 0..64 {
            let c = NES_PALETTE[i];
            rgba_cache[i] = [c[0], c[1], c[2], 0xFF];
        }

        // Background: iterate tiles then inside tile rows
        for tile_y in 0..30 {
            let coarse_attr_y = tile_y / 4;
            let attr_row_quad_y = (tile_y % 4) / 2;
            for row_in_tile in 0..8 {
                let py = tile_y * 8 + row_in_tile;
                if py >= NES_HEIGHT {
                    continue;
                }
                for tile_x in 0..32 {
                    let px_base = tile_x * 8;
                    if px_base >= NES_WIDTH {
                        continue;
                    }

                    let nt_index = 0x2000 + (tile_y as u16) * 32 + tile_x as u16;
                    let tile_id = bus.ppu_read(nt_index);

                    let attr_index = 0x23C0 + (coarse_attr_y as u16) * 8 + (tile_x as u16 / 4);
                    let attr_byte = bus.ppu_read(attr_index);
                    let attr_quad_x = (tile_x % 4) / 2;
                    let quadrant = attr_row_quad_y * 2 + attr_quad_x;
                    let palette_group = (attr_byte >> (quadrant * 2)) & 0x03;

                    let pattern_addr = bg_pattern_base + (tile_id as u16) * 16 + row_in_tile as u16;
                    let low_plane = bus.ppu_read(pattern_addr);
                    let high_plane = bus.ppu_read(pattern_addr + 8);

                    for bit in 0..8 {
                        let x = px_base + bit;
                        if x >= NES_WIDTH {
                            break;
                        }
                        let shift = 7 - bit;
                        let lo = (low_plane >> shift) & 1;
                        let hi = (high_plane >> shift) & 1;
                        let ci = (hi << 1) | lo; // 0..3
                        let nes_palette_entry = if ci == 0 {
                            bus.ppu_read(0x3F00)
                        } else {
                            let pal = 0x3F00 + (palette_group as u16) * 4 + ci as u16;
                            bus.ppu_read(pal)
                        } & 0x3F;

                        let fi = (py * NES_WIDTH + x) * BYTES_PER_PIXEL;
                        let rgba = rgba_cache[nes_palette_entry as usize];
                        self.framebuffer[fi] = rgba[0];
                        self.framebuffer[fi + 1] = rgba[1];
                        self.framebuffer[fi + 2] = rgba[2];
                        self.framebuffer[fi + 3] = 0xFF;

                        if ci != 0 {
                            self.bg_opaque[py * NES_WIDTH + x] = 1;
                        }
                    }
                }
            }
        }

        // Approximate sprite overflow (count per scanline > 8)
        let mut scan_counts = [0u8; NES_HEIGHT];
        let spr_h = if (self.ctrl & 0x20) != 0 { 16 } else { 8 };
        for s in 0..64 {
            let base = s * 4;
            let y = self.oam[base] as i16;
            let top = y as i32;
            let bottom = top + spr_h;
            if bottom <= 0 || top >= NES_HEIGHT as i32 {
                continue;
            }
            let start = top.max(0) as usize;
            let end = bottom.min(NES_HEIGHT as i32) as usize;
            for count in scan_counts[start..end].iter_mut() {
                if *count < 250 {
                    *count += 1;
                    if *count > 8 {
                        self.set_sprite_overflow(true);
                        break;
                    }
                }
            }
        }

        // If sprites disabled (PPUMASK bit 4), skip overlay
        if (self.mask & 0x10) == 0 {
            return;
        }

        // Sprite overlay: iterate in reverse so lower OAM index is drawn last (higher priority).
        for sprite_index in (0..64).rev() {
            let base = sprite_index * 4;
            let y = self.oam[base] as i16;
            let tile = self.oam[base + 1];
            let attr = self.oam[base + 2];
            let x = self.oam[base + 3] as i16;

            let flip_v = (attr & 0x80) != 0;
            let flip_h = (attr & 0x40) != 0;
            let priority_behind_bg = (attr & 0x20) != 0;
            let palette_index = (attr & 0x03) as u16;

            let sprite_height = if (self.ctrl & 0x20) != 0 { 16 } else { 8 };

            // Simple bounding reject
            if x >= NES_WIDTH as i16
                || y >= NES_HEIGHT as i16
                || x < -(sprite_height as i16)
                || y < -(sprite_height as i16)
            {
                continue;
            }

            for row in 0..sprite_height {
                let sy = y + row as i16;
                if sy < 0 || sy >= NES_HEIGHT as i16 {
                    continue;
                }

                // Pattern addressing (8x8 vs 8x16)
                let (addr_low, addr_high) = if sprite_height == 8 {
                    let base_sel = if (self.ctrl & 0x08) != 0 {
                        0x1000
                    } else {
                        0x0000
                    };
                    let eff_row = if flip_v { 7 - row } else { row } & 7;
                    let a = base_sel + (tile as u16) * 16 + eff_row as u16;
                    (a, a + 8)
                } else {
                    // 8x16
                    let table = (tile as u16 & 1) * 0x1000;
                    let base_tile = (tile & 0xFE) as u16;
                    let row_in_sprite = if flip_v { sprite_height - 1 - row } else { row };
                    let tile_select = if row_in_sprite < 8 { 0 } else { 1 };
                    let row_in_tile = row_in_sprite & 7;
                    let a = table + (base_tile + tile_select) * 16 + row_in_tile as u16;
                    (a, a + 8)
                };

                let low_plane = bus.ppu_read(addr_low);
                let high_plane = bus.ppu_read(addr_high);

                for col in 0..8 {
                    let sx = x + col as i16;
                    if sx < 0 || sx >= NES_WIDTH as i16 {
                        continue;
                    }
                    let bit_index = if flip_h { col } else { 7 - col };
                    let lo = (low_plane >> bit_index) & 1;
                    let hi = (high_plane >> bit_index) & 1;
                    let ci = (hi << 1) | lo;
                    if ci == 0 {
                        continue; // transparent
                    }

                    let bg_is_opaque = self.bg_opaque[sy as usize * NES_WIDTH + sx as usize] != 0;
                    if priority_behind_bg && bg_is_opaque {
                        continue;
                    }

                    let pal_addr = 0x3F10 + palette_index * 4 + ci as u16;
                    let nes_palette_entry = bus.ppu_read(pal_addr) & 0x3F;
                    let rgba = rgba_cache[nes_palette_entry as usize];
                    let fi = (sy as usize * NES_WIDTH + sx as usize) * BYTES_PER_PIXEL;
                    self.framebuffer[fi] = rgba[0];
                    self.framebuffer[fi + 1] = rgba[1];
                    self.framebuffer[fi + 2] = rgba[2];
                    self.framebuffer[fi + 3] = 0xFF;

                    if sprite_index == 0 && bg_is_opaque {
                        self.set_sprite_zero_hit(true);
                    }
                }
            }
        }
    }
}
