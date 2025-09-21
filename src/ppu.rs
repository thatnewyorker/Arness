/*!
PPU implementation (Phase A) providing:
- CPU-visible register interface ($2000..$2007)
- OAM (sprite) memory and DMA hook
- Basic timing (scanline/dot counters) with vblank + NMI signaling
- Background tile rendering into an RGBA framebuffer
- Frame-level (non cycle-accurate) sprite rendering overlay (Phase A)
- Approximate sprite zero hit & sprite overflow flag behavior

NOTES / LIMITATIONS (Phase A):
- No fine-scroll or scrolling across multiple nametables (only base $2000 nametable).
- Sprite rendering is done after the full background pass (not per-scanline).
- Sprite zero hit timing is approximate: flag is set if a non-transparent sprite-0 pixel
  overlays a non-zero background pixel in the final composed frame.
- Sprite overflow is approximated: if more than 8 sprites overlap any scanline, flag is set.
- PPUSTATUS bits for sprite zero (bit 6) and overflow (bit 5) are thus approximate.
- Pattern table / palette mirroring behavior is simplified.
- Future Phase B should introduce per-scanline evaluation, cycle-level timing, and precise flags.

STRUCTURE:
- `Ppu` holds all state (register mirrors, VRAM, OAM, frame buffer, background opacity mask).
- `render_frame` performs a full background draw + sprite overlay.
- `tick` advances coarse timing (3 ticks per CPU cycle expected via bus integration).
*/

/// Screen width in pixels.
pub const NES_WIDTH: usize = 256;
/// Screen height in pixels.
pub const NES_HEIGHT: usize = 240;
/// RGBA bytes per pixel.
pub const BYTES_PER_PIXEL: usize = 4;

/// Canonical (approximate) NES master palette (RGB; alpha always 0xFF when rendered).
const NES_PALETTE: [[u8; 3]; 64] = [
    [0x75, 0x75, 0x75],
    [0x27, 0x1B, 0x8F],
    [0x00, 0x00, 0xAB],
    [0x47, 0x00, 0x9F],
    [0x8F, 0x00, 0x77],
    [0xAB, 0x00, 0x13],
    [0xA7, 0x00, 0x00],
    [0x7F, 0x0B, 0x00],
    [0x43, 0x2F, 0x00],
    [0x00, 0x47, 0x00],
    [0x00, 0x51, 0x00],
    [0x00, 0x3F, 0x17],
    [0x1B, 0x3F, 0x5F],
    [0x00, 0x00, 0x00],
    [0x00, 0x00, 0x00],
    [0x00, 0x00, 0x00],
    [0xBC, 0xBC, 0xBC],
    [0x00, 0x73, 0xEF],
    [0x23, 0x3B, 0xEF],
    [0x83, 0x00, 0xF3],
    [0xBF, 0x00, 0xBF],
    [0xE7, 0x00, 0x5B],
    [0xDB, 0x2B, 0x00],
    [0xCB, 0x4F, 0x0F],
    [0x8B, 0x73, 0x00],
    [0x00, 0x97, 0x00],
    [0x00, 0xAB, 0x00],
    [0x00, 0x93, 0x3B],
    [0x00, 0x83, 0x8B],
    [0x00, 0x00, 0x00],
    [0x00, 0x00, 0x00],
    [0x00, 0x00, 0x00],
    [0xFF, 0xFF, 0xFF],
    [0x3F, 0xBF, 0xFF],
    [0x5F, 0x97, 0xFF],
    [0xA7, 0x8B, 0xFD],
    [0xF7, 0x7B, 0xFF],
    [0xFF, 0x77, 0xB7],
    [0xFF, 0x77, 0x63],
    [0xFF, 0x9B, 0x3B],
    [0xF3, 0xBF, 0x3F],
    [0x83, 0xD3, 0x13],
    [0x4F, 0xDF, 0x4B],
    [0x58, 0xF8, 0x98],
    [0x00, 0xEB, 0xDB],
    [0x00, 0x00, 0x00],
    [0x00, 0x00, 0x00],
    [0x00, 0x00, 0x00],
    [0xFF, 0xFF, 0xFF],
    [0xAB, 0xE7, 0xFF],
    [0xC7, 0xD7, 0xFF],
    [0xD7, 0xCB, 0xFF],
    [0xFF, 0xC7, 0xFF],
    [0xFF, 0xC7, 0xDB],
    [0xFF, 0xBF, 0xB3],
    [0xFF, 0xDB, 0xAB],
    [0xFF, 0xE7, 0xA3],
    [0xE3, 0xFF, 0xA3],
    [0xAB, 0xF3, 0xBF],
    [0xB3, 0xFF, 0xCF],
    [0x9F, 0xFF, 0xF3],
    [0x00, 0x00, 0x00],
    [0x00, 0x00, 0x00],
    [0x00, 0x00, 0x00],
];

#[derive(Clone, Debug)]
pub struct Ppu {
    // CPU-visible register mirrors
    ctrl: u8,     // $2000
    mask: u8,     // $2001
    status: u8,   // $2002 (bit7=vblank, bit6=sprite0 hit, bit5=sprite overflow)
    oam_addr: u8, // $2003

    // Write toggle + scroll latches
    write_toggle: bool,
    scroll_x: u8,
    scroll_y: u8,

    // VRAM addressing & buffered read
    vram_addr: u16,
    vram_buffer: u8,
    vram: [u8; 0x4000], // Simple 16KB VRAM (pattern + nametable + palette space mirrored)

    // OAM (Object Attribute Memory) 64 sprites * 4 bytes
    oam: [u8; 256],

    // Timing
    dot: u16,
    scanline: i16,
    frame_complete: bool,
    nmi_latch: bool,

    // Output framebuffer (RGBA)
    framebuffer: Vec<u8>,

    // Background opacity per pixel (1 if bg color index != 0)
    bg_opaque: Vec<u8>,

    // --- Phase B sprite pipeline state (cycle-accurate preparation) ---
    // Secondary OAM (32 bytes: up to 8 sprites * 4 bytes) for next scanline evaluation.
    secondary_oam: [u8; 32],

    // Per-sprite slot latched attributes (for the 8 sprites selected for the upcoming scanline).
    sprite_slot_y: [u8; 8],
    sprite_slot_tile: [u8; 8],
    sprite_slot_attr: [u8; 8],
    sprite_slot_x: [u8; 8],
    // Pattern bytes fetched during the fetch phase (before loading into shift registers).
    sprite_slot_pattern_low: [u8; 8],
    sprite_slot_pattern_high: [u8; 8],

    // Active shift registers (loaded at start of visible scanline).
    sprite_slot_shift_low: [u8; 8],
    sprite_slot_shift_high: [u8; 8],
    // X counters (delay before sprite becomes active on the scanline).
    sprite_slot_x_counter: [u8; 8],
    // Shift bookkeeping: number of pixels shifted this scanline and whether the slot is loaded
    sprite_slot_shift_count: [u8; 8],
    sprite_slot_loaded: [bool; 8],

    // Original OAM index per slot (for priority & sprite-zero detection).
    sprite_slot_orig_index: [u8; 8],
    sprite_slot_is_zero: [bool; 8],
    sprite_slot_active: [bool; 8],

    // OAM evaluation state
    oam_eval_oam_index: u8,
    oam_eval_byte_index: u8,
    oam_eval_sprite_count: u8,

    // Sprite pattern fetch sequencing (which slot we are currently fetching).
    sprite_fetch_slot: u8,

    // Master enable for the cycle-accurate sprite pipeline (allows fallback to legacy path if false).
    sprite_pipeline_enabled: bool,

    // Overflow detection and evaluation write-pointer state
    oam_secondary_write_index: u8,
    oam_eval_overflow_detected: bool,
    oam_eval_overflow_scanline: i16,
    oam_eval_overflow_dot: u16,

    // Debug: record sprite-zero hit (scanline, dot) when set
    sprite_zero_hit_scanline: i16,
    sprite_zero_hit_dot: u16,
}

impl Default for Ppu {
    fn default() -> Self {
        Self::new()
    }
}

impl Ppu {
    pub fn new() -> Self {
        Self {
            ctrl: 0,
            mask: 0,
            status: 0,
            oam_addr: 0,
            write_toggle: false,
            scroll_x: 0,
            scroll_y: 0,
            vram_addr: 0,
            vram_buffer: 0,
            vram: [0; 0x4000],
            oam: [0; 256],
            dot: 0,
            scanline: -1,
            frame_complete: false,
            nmi_latch: false,
            framebuffer: Vec::new(),
            bg_opaque: Vec::new(),
            // Phase B sprite pipeline fields
            secondary_oam: [0xFF; 32],
            sprite_slot_y: [0; 8],
            sprite_slot_tile: [0; 8],
            sprite_slot_attr: [0; 8],
            sprite_slot_x: [0; 8],
            sprite_slot_pattern_low: [0; 8],
            sprite_slot_pattern_high: [0; 8],
            sprite_slot_shift_low: [0; 8],
            sprite_slot_shift_high: [0; 8],
            sprite_slot_x_counter: [0; 8],
            sprite_slot_shift_count: [0; 8],
            sprite_slot_loaded: [false; 8],
            sprite_slot_orig_index: [0; 8],
            sprite_slot_is_zero: [false; 8],
            sprite_slot_active: [false; 8],
            oam_eval_oam_index: 0,
            oam_eval_byte_index: 0,
            oam_eval_sprite_count: 0,
            sprite_fetch_slot: 0,
            sprite_pipeline_enabled: false,
            oam_secondary_write_index: 0,
            oam_eval_overflow_detected: false,
            oam_eval_overflow_scanline: 0,
            oam_eval_overflow_dot: 0,
            sprite_zero_hit_scanline: 0,
            sprite_zero_hit_dot: 0,
        }
    }

    pub fn reset(&mut self) {
        self.ctrl = 0;
        self.mask = 0;
        self.status = 0;
        self.oam_addr = 0;
        self.write_toggle = false;
        self.scroll_x = 0;
        self.scroll_y = 0;
        self.vram_addr = 0;
        self.vram_buffer = 0;
        self.vram.fill(0);
        self.oam.fill(0);
        self.dot = 0;
        self.scanline = -1;
        self.frame_complete = false;
        self.nmi_latch = false;
        // Reset sprite pipeline state
        self.secondary_oam.fill(0xFF);
        self.sprite_slot_y = [0; 8];
        self.sprite_slot_tile = [0; 8];
        self.sprite_slot_attr = [0; 8];
        self.sprite_slot_x = [0; 8];
        self.sprite_slot_pattern_low = [0; 8];
        self.sprite_slot_pattern_high = [0; 8];
        self.sprite_slot_shift_low = [0; 8];
        self.sprite_slot_shift_high = [0; 8];
        self.sprite_slot_x_counter = [0; 8];
        self.sprite_slot_shift_count = [0; 8];
        self.sprite_slot_loaded = [false; 8];
        self.sprite_slot_orig_index = [0; 8];
        self.sprite_slot_is_zero = [false; 8];
        self.sprite_slot_active = [false; 8];
        self.oam_eval_oam_index = 0;
        self.oam_eval_byte_index = 0;
        self.oam_eval_sprite_count = 0;
        self.sprite_fetch_slot = 0;
        // Keep disabled until full per-dot implementation replaces legacy sprite overlay
        self.sprite_pipeline_enabled = false;

        // Overflow detection and evaluation write-pointer state
        self.oam_secondary_write_index = 0;
        self.oam_eval_overflow_detected = false;
        self.oam_eval_overflow_scanline = 0;
        self.oam_eval_overflow_dot = 0;
        self.sprite_zero_hit_scanline = 0;
        self.sprite_zero_hit_dot = 0;
    }

    /// Read-only framebuffer slice (RGBA).
    pub fn framebuffer(&self) -> &[u8] {
        &self.framebuffer
    }

    /// Render a full frame (background + sprite overlay).
    /// Non-cycle-accurate. Background first, then sprites.
    pub fn render_frame<B: crate::ppu_bus::PpuBus>(&mut self, bus: &B) {
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
    /// - Visible dots for pixel generation: 1-256 on scanlines 0-239 (not yet producing per-dot pixels here).
    pub fn tick<B: crate::ppu_bus::PpuBus>(&mut self, bus: &B) {
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
                // Reset (future) sprite evaluation state
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
                // CLEAR phase: write one 0xFF byte of secondary OAM per dot (0..63)
                let idx = (self.dot - 1) as usize;
                if idx < self.secondary_oam.len() {
                    self.secondary_oam[idx] = 0xFF;
                }
                // When CLEAR completes (dot 64), reset evaluation counters for upcoming EVALUATE phase
                if self.dot == 64 {
                    self.oam_eval_oam_index = 0;
                    self.oam_eval_byte_index = 0;
                    self.oam_eval_sprite_count = 0;
                }
            } else if self.dot >= 65 && self.dot <= 256 {
                // PRIMARY OAM EVALUATION: internal write-pointer model; detect overflow at ninth in-range sprite (Y byte).
                // One OAM byte is processed per dot. We continue scanning all 64 sprites even after 8 are selected.
                if self.oam_eval_oam_index < 64 {
                    let sprite_index = self.oam_eval_oam_index as usize;
                    let byte_index = self.oam_eval_byte_index as usize;
                    let base = sprite_index * 4;
                    let y_byte = self.oam[base];
                    let next_scanline = self.scanline + 1;
                    let sprite_height = if (self.ctrl & 0x20) != 0 { 16 } else { 8 };
                    // Use raw OAM Y semantics consistent with Phase A tests.
                    let in_range = next_scanline >= y_byte as i16
                        && next_scanline < y_byte as i16 + sprite_height as i16;

                    // Overflow detection: when we encounter the ninth in-range sprite on its Y byte, set overflow immediately.
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

                    // Copy current sprite into secondary OAM only if in-range and we still have room (<8).
                    let copying = in_range && self.oam_eval_sprite_count < 8;
                    let value = self.oam[base + byte_index];
                    if copying {
                        // Initialize internal write pointer at start of a new sprite copy.
                        if self.oam_eval_byte_index == 0 {
                            self.oam_secondary_write_index =
                                self.oam_eval_sprite_count.saturating_mul(4);
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

                    // Advance to next byte; after byte 3, finish copy (if any) and move to next sprite.
                    self.oam_eval_byte_index = self.oam_eval_byte_index.wrapping_add(1);
                    if self.oam_eval_byte_index >= 4 {
                        self.oam_eval_byte_index = 0;
                        if copying {
                            self.oam_eval_sprite_count =
                                self.oam_eval_sprite_count.saturating_add(1);
                            self.oam_secondary_write_index =
                                self.oam_secondary_write_index.wrapping_add(4);
                        }
                        self.oam_eval_oam_index = self.oam_eval_oam_index.wrapping_add(1);
                    }
                }
            } else if self.dot >= 257 && self.dot <= 320 {
                // SPRITE PATTERN FETCH: for each of 8 slots over 64 dots, compute row and fetch pattern low/high
                let fetch_idx = self.dot - 257; // 0..63
                let slot = (fetch_idx / 8) as usize; // 0..7
                let sub = fetch_idx % 8; // 0..7 (sub-cycle within slot)
                if slot < 8 {
                    // Validate slot by secondary OAM Y != 0xFF
                    let base = slot * 4;
                    let y = self.secondary_oam[base];
                    if y != 0xFF {
                        if sub == 0 {
                            // Latch sprite metadata from secondary OAM for this slot
                            let tile = self.secondary_oam[base + 1];
                            let attr = self.secondary_oam[base + 2];
                            let x = self.secondary_oam[base + 3];
                            self.sprite_slot_y[slot] = y;
                            self.sprite_slot_tile[slot] = tile;
                            self.sprite_slot_attr[slot] = attr;
                            self.sprite_slot_x[slot] = x;
                            // orig index flags were set during evaluation; no change here
                        } else if sub == 5 || sub == 6 {
                            // Compute pattern addresses and fetch low/high planes
                            let tile = self.secondary_oam[base + 1];
                            let attr = self.secondary_oam[base + 2];
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

    /// Write CPU-facing PPU register ($2000..$2007).
    /// Produce a single background pixel for the current (scanline, dot).
    /// Simplified per-dot background fetch mirroring logic in original frame renderer.
    /// Does not yet emulate fine/coarse scroll or horizontal/vertical wrapping; uses (0,0) origin.
    fn per_dot_background_pixel<B: crate::ppu_bus::PpuBus>(&mut self, bus: &B) {
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

        let bg_pattern_base = if (self.ctrl & 0x10) != 0 {
            0x1000
        } else {
            0x0000
        };
        let pattern_addr = bg_pattern_base + (tile_id as u16) * 16 + row_in_tile as u16;
        let low_plane = bus.ppu_read(pattern_addr);
        let high_plane = bus.ppu_read(pattern_addr + 8);

        let bit = x & 7;
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

    // Load fetched sprite pattern bytes into shift registers for the new visible scanline.
    // Applies horizontal flip by reversing bit order at load time.
    fn load_sprite_shift_registers(&mut self) {
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
                low = Self::reverse8(low);
                high = Self::reverse8(high);
            }

            self.sprite_slot_shift_low[slot] = low;
            self.sprite_slot_shift_high[slot] = high;
            self.sprite_slot_x_counter[slot] = self.sprite_slot_x[slot];
            self.sprite_slot_active[slot] = true;
        }
    }

    // Produce a sprite pixel at (x,y) by evaluating up to 8 sprite slots with shift registers.
    // Applies priority rules and sets sprite-zero hit when conditions are met.
    fn produce_sprite_pixel<B: crate::ppu_bus::PpuBus>(&mut self, bus: &B, x: usize, y: usize) {
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

    // Bit-reversal helper for horizontal flip
    #[inline]
    fn reverse8(v: u8) -> u8 {
        let mut x = v;
        x = (x & 0xF0) >> 4 | (x & 0x0F) << 4;
        x = (x & 0xCC) >> 2 | (x & 0x33) << 2;
        x = (x & 0xAA) >> 1 | (x & 0x55) << 1;
        x
    }

    pub fn write_reg(&mut self, addr: u16, value: u8) {
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

    /// Read CPU-facing PPU register ($2000..$2007).
    pub fn read_reg(&mut self, addr: u16) -> u8 {
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

    /// OAM DMA copy (256 bytes).
    pub fn oam_dma_copy(&mut self, data: &[u8]) {
        let mut oam_ptr = self.oam_addr;
        for i in 0..256 {
            let b = data.get(i).copied().unwrap_or(0);
            self.oam[oam_ptr as usize] = b;
            oam_ptr = oam_ptr.wrapping_add(1);
        }
        self.oam_addr = oam_ptr;
    }

    // Flag setters
    pub fn set_vblank(&mut self, on: bool) {
        if on {
            self.status |= 0x80;
        } else {
            self.status &= !0x80;
        }
    }
    pub fn set_sprite_zero_hit(&mut self, on: bool) {
        if on {
            self.status |= 0x40;
        } else {
            self.status &= !0x40;
        }
    }
    pub fn set_sprite_overflow(&mut self, on: bool) {
        if on {
            self.status |= 0x20;
        } else {
            self.status &= !0x20;
        }
    }

    // Flag queries
    pub fn vblank(&self) -> bool {
        (self.status & 0x80) != 0
    }
    pub fn sprite_zero_hit(&self) -> bool {
        (self.status & 0x40) != 0
    }
    pub fn sprite_overflow(&self) -> bool {
        (self.status & 0x20) != 0
    }
    pub fn nmi_enabled(&self) -> bool {
        (self.ctrl & 0x80) != 0
    }

    // VRAM/OAM convenience
    pub fn peek_vram(&self, addr: u16) -> u8 {
        self.vram[(addr as usize) & 0x3FFF]
    }
    pub fn poke_vram(&mut self, addr: u16, value: u8) {
        self.vram[(addr as usize) & 0x3FFF] = value;
    }
    pub fn peek_oam(&self, idx: usize) -> u8 {
        self.oam[idx & 0xFF]
    }
    pub fn poke_oam(&mut self, idx: usize, value: u8) {
        self.oam[idx & 0xFF] = value;
    }

    // Frame completion & NMI latch
    pub fn frame_complete(&self) -> bool {
        self.frame_complete
    }
    pub fn take_frame_complete(&mut self) -> bool {
        let was = self.frame_complete;
        self.frame_complete = false;
        was
    }
    pub fn take_nmi_request(&mut self) -> bool {
        let was = self.nmi_latch;
        self.nmi_latch = false;
        was
    }

    // Misc register access helpers (used elsewhere in crate)
    pub fn get_ctrl(&self) -> u8 {
        self.ctrl
    }
    pub fn set_ctrl(&mut self, v: u8) {
        self.ctrl = v;
    }
    pub fn vram_increment_step(&self) -> u16 {
        if (self.ctrl & 0x04) != 0 { 32 } else { 1 }
    }
    pub fn get_vram_addr(&self) -> u16 {
        self.vram_addr
    }
    pub fn set_vram_addr(&mut self, a: u16) {
        self.vram_addr = a & 0x3FFF;
    }
    pub fn get_vram_buffer(&self) -> u8 {
        self.vram_buffer
    }
    pub fn set_vram_buffer(&mut self, v: u8) {
        self.vram_buffer = v;
    }
    pub fn get_write_toggle(&self) -> bool {
        self.write_toggle
    }
    pub fn set_write_toggle(&mut self, on: bool) {
        self.write_toggle = on;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sprite_clear_phase_secondary_oam_filled() {
        let rom = crate::test_utils::build_ines(1, 0, 0, 0, 1, None);
        let cart = crate::cartridge::Cartridge::from_ines_bytes(&rom).unwrap();
        let mut bus = crate::bus::Bus::new();
        bus.attach_cartridge(cart);
        // Advance until scanline 0 dot 64 (end of CLEAR phase for first visible line).
        while !(bus.ppu().scanline == 0 && bus.ppu().dot == 64) {
            bus.tick(1);
        }
        for i in 0..32 {
            assert_eq!(
                bus.ppu().secondary_oam[i],
                0xFF,
                "secondary_oam[{}] not cleared",
                i
            );
        }
    }

    #[test]
    fn sprite_evaluate_phase_copies_up_to_8_in_range_sprites() {
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
                ppu.oam[base] = 0; // Y
                ppu.oam[base + 1] = i; // tile
                ppu.oam[base + 2] = 0x00; // attributes
                ppu.oam[base + 3] = i; // X
            }
        }

        // Advance to (end of) evaluation window of first visible scanline:
        // We need to reach scanline 0 after CLEAR (dot 64) and past some evaluation dots.
        while !(bus.ppu().scanline == 0 && bus.ppu().dot == 256) {
            bus.tick(1);
        }

        let ppu_ref = bus.ppu();
        // Expect exactly 8 accepted sprites
        assert_eq!(
            ppu_ref.oam_eval_sprite_count, 8,
            "Expected 8 sprites selected, got {}",
            ppu_ref.oam_eval_sprite_count
        );
        // Verify the 8 copied sprites match OAM entries 0..7
        for s in 0..8usize {
            let so = &ppu_ref.secondary_oam[s * 4..s * 4 + 4];
            assert_eq!(so[0], 0, "Sprite {} Y mismatch", s);
            assert_eq!(so[1], s as u8, "Sprite {} tile mismatch", s);
            assert_eq!(so[2], 0x00, "Sprite {} attr mismatch", s);
            assert_eq!(so[3], s as u8, "Sprite {} X mismatch", s);
        }
        // Overflow should be set because more than 8 sprites were in-range.
        assert!(
            ppu_ref.sprite_overflow(),
            "Expected sprite overflow when more than 8 sprites are in-range"
        );
    }

    #[test]
    fn sprite_fetch_phase_fetches_pattern_low_high_for_slot0() {
        let rom = crate::test_utils::build_ines(1, 0, 0, 0, 1, None);
        let cart = crate::cartridge::Cartridge::from_ines_bytes(&rom).unwrap();
        let mut bus = crate::bus::Bus::new();
        bus.attach_cartridge(cart);

        // Configure PPU: 8x8 sprites, sprite pattern table at $1000 (PPUCTRL bit 3)
        {
            let ppu = bus.ppu_mut();
            ppu.ctrl = (ppu.ctrl & !0x20) | 0x08; // clear 8x16, set sprite pattern base 0x1000
            // Sprite 0 at Y=0, tile=2, attr=0, X=0 so it's in-range for pre-render evaluation (next_scanline=0)
            ppu.oam[0] = 0; // Y
            ppu.oam[1] = 2; // tile index
            ppu.oam[2] = 0x00; // attributes
            ppu.oam[3] = 0; // X
        }

        // Program CHR pattern bytes for tile 2, row 0 at pattern base $1000
        let tile: u8 = 2;
        let low: u8 = 0xA5;
        let high: u8 = 0x5A;
        bus.ppu_write(0x1000 + (tile as u16) * 16, low);
        bus.ppu_write(0x1000 + (tile as u16) * 16 + 8, high);

        // Advance to the end of FETCH window on pre-render line (-1, dots 257..320)
        while !(bus.ppu().scanline == -1 && bus.ppu().dot == 320) {
            bus.tick(1);
        }

        let ppu_ref = bus.ppu();
        assert_eq!(
            ppu_ref.sprite_slot_pattern_low[0], low,
            "low plane mismatch for slot 0"
        );
        assert_eq!(
            ppu_ref.sprite_slot_pattern_high[0], high,
            "high plane mismatch for slot 0"
        );
        assert_eq!(ppu_ref.sprite_slot_y[0], 0, "slot 0 Y mismatch");
        assert_eq!(ppu_ref.sprite_slot_tile[0], tile, "slot 0 tile mismatch");
        assert_eq!(ppu_ref.sprite_slot_attr[0], 0x00, "slot 0 attr mismatch");
        assert_eq!(ppu_ref.sprite_slot_x[0], 0, "slot 0 X mismatch");
    }

    #[test]
    fn status_read_clears_vblank_and_write_toggle() {
        let mut p = Ppu::new();
        p.set_vblank(true);
        p.write_toggle = true;
        let s = p.read_reg(0x2002);
        assert_ne!(s & 0x80, 0);
        assert!(!p.vblank());
        assert!(!p.get_write_toggle());
    }

    #[test]
    fn ppudata_buffered_read_and_increment() {
        let mut p = Ppu::new();
        p.write_reg(0x2000, 0x00);
        p.vram[0x0000] = 0x11;
        p.vram[0x0001] = 0x22;
        p.write_reg(0x2006, 0x00);
        p.write_reg(0x2006, 0x00);
        assert_eq!(p.read_reg(0x2007), 0x00); // buffered
        assert_eq!(p.read_reg(0x2007), 0x11);
        assert_eq!(p.read_reg(0x2007), 0x22);
    }

    #[test]
    fn oam_dma_writes_wrap_and_update_oamaddr() {
        let mut p = Ppu::new();
        p.write_reg(0x2003, 0xFE);
        let mut buf = [0u8; 256];
        for (i, b) in buf.iter_mut().enumerate() {
            *b = i as u8;
        }
        p.oam_dma_copy(&buf);
        assert_eq!(p.peek_oam(0xFE), 0x00);
        assert_eq!(p.peek_oam(0xFF), 0x01);
        assert_eq!(p.peek_oam(0x00), 0x02);
        assert_eq!(p.peek_oam(0x01), 0x03);
        assert_eq!(p.read_reg(0x2003), 0xFE);
    }

    #[test]
    fn framebuffer_dimensions_background_renderer() {
        let mut p = Ppu::new();
        let bus = crate::bus::Bus::new();
        p.render_frame(&bus);
        assert_eq!(
            p.framebuffer().len(),
            NES_WIDTH * NES_HEIGHT * BYTES_PER_PIXEL
        );
    }

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
}
