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
        // Keep framebuffer allocations (avoid churn)
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
            let bottom = top + spr_h as i32;
            if bottom <= 0 || top >= NES_HEIGHT as i32 {
                continue;
            }
            for sy in top.max(0) as usize..bottom.min(NES_HEIGHT as i32) as usize {
                if scan_counts[sy] < 250 {
                    scan_counts[sy] += 1;
                    if scan_counts[sy] > 8 {
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
                    let row_in_sprite = if flip_v {
                        (sprite_height - 1 - row)
                    } else {
                        row
                    };
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

    /// Advance one PPU dot (should be invoked 3x per CPU cycle by the bus).
    pub fn tick(&mut self) {
        self.dot = self.dot.wrapping_add(1);

        if self.dot == 1 {
            if self.scanline == 241 {
                // Entering vblank
                self.set_vblank(true);
                if self.nmi_enabled() {
                    self.nmi_latch = true;
                }
            } else if self.scanline == -1 {
                // Pre-render line: clear flags
                self.set_vblank(false);
                self.set_sprite_zero_hit(false);
                self.set_sprite_overflow(false);
                self.frame_complete = false;
            }
        }

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
        for i in 0..256 {
            buf[i] = i as u8;
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
            bus.ppu_write(0x0000 + row as u16, 0xFF);
            bus.ppu_write(0x0000 + 8 + row as u16, 0x00);
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
            bus.ppu_write(0x0000 + row as u16, 0xFF);
            bus.ppu_write(0x0000 + 8 + row as u16, 0x00);
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
            bus.ppu_write(0x0000 + row as u16, 0xFF);
            bus.ppu_write(0x0000 + 8 + row as u16, 0x00);
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
            bus.ppu_write(0x0000 + row as u16, 0xFF);
            bus.ppu_write(0x0000 + 8 + row as u16, 0x00);
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
            bus.ppu_write(0x0000 + row as u16, 0xFF);
            bus.ppu_write(0x0000 + 8 + row as u16, 0x00);
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
            p.oam[0] = 0;
            p.oam[1] = 1;
            p.oam[2] = 0x20; // behind background
            p.oam[3] = 0;
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
