/*!
PPU stub exposing register read/write and OAM DMA hook, plus simple timing and NMI signaling.

Scope:
- Implements the CPU-visible PPU register interface ($2000..$2007) with basic,
  but useful, semantics:
  * $2000 PPUCTRL: stores control flags (incl. VRAM increment and NMI enable)
  * $2001 PPUMASK: stores mask flags (color emphasis, greyscale, show bg/sprites)
  * $2002 PPUSTATUS: exposes vblank/sprite flags; read clears vblank and write latch
  * $2003 OAMADDR: OAM address pointer for $2004
  * $2004 OAMDATA: read/write OAM at OAMADDR (writes increment OAMADDR)
  * $2005 PPUSCROLL: two writes form x/y scroll; affects internal write toggle
  * $2006 PPUADDR: two writes set VRAM address; read of $2002 resets the write toggle
  * $2007 PPUDATA: VRAM read/write at current VRAM address; auto-increment via $2000
- Provides a simple VRAM space (0x0000..0x3FFF mirrored) and OAM (256 bytes).
- Implements buffered reads for $2007 (non-palette addresses return delayed value).
- Exposes an OAM DMA helper for $4014 handling in the Bus.
- Adds simple dot/scanline/frame timing and signals NMI on vblank if enabled.

Notes:
- This is still a functional stub, not cycle-accurate. It aims to support CPU-side
  software interactions and provide a place to evolve full PPU behavior later.
- Palette mirroring and nametable mirroring are not fully modeled. VRAM is a flat
  16KB space to keep things simple at this stage.

Additions (display integration groundwork):
- A persistent RGBA framebuffer (`framebuffer`) sized 256x240x4 for integration
  with a future `pixels` / GPU-backed display layer.
- A `render_frame` method that (for now) produces a placeholder pattern using
  the canonical NES palette. This will be replaced with real background &
  sprite composition logic later.
*/

/// Width of the logical NES screen in pixels.
pub const NES_WIDTH: usize = 256;
/// Height of the logical NES screen in pixels.
pub const NES_HEIGHT: usize = 240;
/// Bytes per pixel (RGBA8888).
pub const BYTES_PER_PIXEL: usize = 4;

/// Canonical 64-entry NES palette (RGB; alpha supplied separately). Values are
/// a commonly used approximation; exact analog hardware output varied by TV.
/// Source palette adapted from public domain reference tables.
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
    // Registers (CPU visible)
    ctrl: u8,     // $2000 PPUCTRL
    mask: u8,     // $2001 PPUMASK
    status: u8,   // $2002 PPUSTATUS (bit7=vblank, bit6=sprite0 hit, bit5=sprite overflow)
    oam_addr: u8, // $2003 OAMADDR

    // Internal latches and toggles
    write_toggle: bool, // toggles on $2005/$2006 writes; reset on $2002 read
    scroll_x: u8,       // first write to $2005
    scroll_y: u8,       // second write to $2005

    // VRAM addressing
    vram_addr: u16,     // current VRAM address (15 bits used)
    vram_buffer: u8,    // read buffer for $2007 (non-palette)
    vram: [u8; 0x4000], // Simple VRAM stub (0x0000..0x3FFF)

    // OAM (Object Attribute Memory)
    oam: [u8; 256], // 256 bytes OAM

    // Timing
    dot: u16,      // 0..=340
    scanline: i16, // -1 (pre-render), 0..=260; 241..=260 vblank period
    frame_complete: bool,
    nmi_latch: bool,

    // New: RGBA framebuffer for display integration (lazy initialized).
    framebuffer: Vec<u8>,
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
        // Preserve framebuffer allocation to avoid realloc churn.
    }

    /// Access the current RGBA framebuffer (length = 256*240*4).
    pub fn framebuffer(&self) -> &[u8] {
        &self.framebuffer
    }

    /// Render the current PPU state into the internal RGBA framebuffer.
    ///
    /// Current implementation: background tile renderer (no fine scroll, no sprites).
    /// - Renders the 32x30 tile grid from nametable $2000.
    /// - Applies attribute table palette selection for each 2x2 tile group.
    /// - Uses background pattern table selected by PPUCTRL bit 4 (0=0x0000,1=0x1000).
    /// - Universal background color ($3F00) used when tile pixel color index == 0.
    /// Future enhancements: fine scroll, additional nametables, sprite layer, emphasis/greyscale.
    pub fn render_frame<B: crate::ppu_bus::PpuBus>(&mut self, bus: &B) {
        let needed = NES_WIDTH * NES_HEIGHT * BYTES_PER_PIXEL;
        if self.framebuffer.len() != needed {
            self.framebuffer.resize(needed, 0);
        }
        // Clear (optional – background fully overwritten, but keep for safety if future partial updates)
        // self.framebuffer.fill(0);

        // Determine background pattern table base (PPUCTRL bit 4)
        let pattern_base = if (self.ctrl & 0x10) != 0 {
            0x1000
        } else {
            0x0000
        };

        // Precompute RGBA palette table for quick lookup (cache inside function for now)
        // Convert NES palette indexes (0..63) into RGBA
        let mut rgba_cache = [[0u8; 4]; 64];
        for i in 0..64 {
            let rgb = NES_PALETTE[i];
            rgba_cache[i] = [rgb[0], rgb[1], rgb[2], 0xFF];
        }

        // Iterate tiles by scanline row (tile_y), then tile row (row_in_tile) for cache friendliness.
        for tile_y in 0..30 {
            // Attribute table row index components reused across 8-pixel rows in this tile row batch.
            let coarse_attr_y = tile_y / 4; // 0..7
            let attr_row_quadrant_y = (tile_y % 4) / 2; // 0 or 1 within attribute byte
            for row_in_tile in 0..8 {
                let pixel_y = tile_y * 8 + row_in_tile;
                if pixel_y >= NES_HEIGHT {
                    continue;
                }

                for tile_x in 0..32 {
                    let pixel_x_base = tile_x * 8;
                    if pixel_x_base >= NES_WIDTH {
                        continue;
                    }

                    // Fetch tile id from nametable $2000 + tile_y*32 + tile_x
                    let nametable_index = 0x2000 + (tile_y as u16) * 32 + tile_x as u16;
                    let tile_id = bus.ppu_read(nametable_index);

                    // Attribute table: base $23C0, index formed by (tile_y/4)*8 + (tile_x/4)
                    let attr_index = 0x23C0 + (coarse_attr_y as u16) * 8 + (tile_x as u16 / 4);
                    let attr_byte = bus.ppu_read(attr_index);

                    // Determine palette group (2 bits) based on quadrant inside 4x4 tile block
                    let attr_quadrant_x = (tile_x % 4) / 2; // 0 or 1
                    let quadrant = (attr_row_quadrant_y * 2) + attr_quadrant_x; // 0..3
                    let palette_group = (attr_byte >> (quadrant * 2)) & 0x03;

                    // Fetch pattern bytes for this tile row
                    let pattern_addr = pattern_base + (tile_id as u16) * 16 + row_in_tile as u16;
                    let low_plane = bus.ppu_read(pattern_addr);
                    let high_plane = bus.ppu_read(pattern_addr + 8);

                    for bit in 0..8 {
                        let x = pixel_x_base + bit;
                        if x >= NES_WIDTH {
                            break;
                        }

                        let shift = 7 - bit;
                        let lo = (low_plane >> shift) & 1;
                        let hi = (high_plane >> shift) & 1;
                        let color_index = (hi << 1) | lo; // 0..3

                        // Select final NES palette byte
                        let nes_palette_entry = if color_index == 0 {
                            // Universal background color ($3F00)
                            bus.ppu_read(0x3F00)
                        } else {
                            // Background subpalette: base = palette_group * 4
                            let pal_addr = 0x3F00 + (palette_group as u16) * 4 + color_index as u16;
                            bus.ppu_read(pal_addr)
                        } & 0x3F;

                        let rgba = rgba_cache[nes_palette_entry as usize];
                        let fb_index = (pixel_y * NES_WIDTH + x) * BYTES_PER_PIXEL;
                        self.framebuffer[fb_index] = rgba[0];
                        self.framebuffer[fb_index + 1] = rgba[1];
                        self.framebuffer[fb_index + 2] = rgba[2];
                        self.framebuffer[fb_index + 3] = 0xFF;
                    }
                }
            }
        }
    }

    // Advance PPU by one dot (called 3x per CPU cycle via Bus.tick).
    // Signals NMI via Bus at vblank start if enabled.
    pub fn tick(&mut self) {
        if self.dot == 0 {
            // Start of dot
        }

        self.dot = self.dot.wrapping_add(1);

        if self.dot == 1 {
            if self.scanline == 241 {
                self.set_vblank(true);
                if self.nmi_enabled() {
                    self.nmi_latch = true;
                }
            } else if self.scanline == -1 {
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

    // CPU write to PPU register in $2000..$2007
    pub fn write_reg(&mut self, addr: u16, value: u8) {
        let reg = 0x2000 + (addr & 0x0007);
        match reg {
            0x2000 => {
                self.ctrl = value;
            }
            0x2001 => {
                self.mask = value;
            }
            0x2002 => { /* ignored */ }
            0x2003 => {
                self.oam_addr = value;
            }
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
                let addr = (self.vram_addr & 0x3FFF) as usize;
                self.vram[addr] = value;
                let inc = if (self.ctrl & 0x04) != 0 { 32 } else { 1 };
                self.vram_addr = self.vram_addr.wrapping_add(inc) & 0x3FFF;
            }
            _ => {}
        }
    }

    // CPU read from PPU register in $2000..$2007
    pub fn read_reg(&mut self, addr: u16) -> u8 {
        let reg = 0x2000 + (addr & 0x0007);
        match reg {
            0x2000 => self.ctrl,
            0x2001 => self.mask,
            0x2002 => {
                let result = self.status;
                self.status &= !0x80;
                self.write_toggle = false;
                result
            }
            0x2003 => self.oam_addr,
            0x2004 => {
                let idx = self.oam_addr as usize;
                self.oam[idx]
            }
            0x2005 => 0,
            0x2006 => ((self.vram_addr >> 8) & 0xFF) as u8,
            0x2007 => {
                let addr = self.vram_addr & 0x3FFF;
                let value = self.vram[addr as usize];
                let ret = if addr < 0x3F00 {
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

    pub fn oam_dma_copy(&mut self, data: &[u8]) {
        let mut addr = self.oam_addr;
        for i in 0..256 {
            let byte = data.get(i).copied().unwrap_or(0);
            self.oam[addr as usize] = byte;
            addr = addr.wrapping_add(1);
        }
        self.oam_addr = addr;
    }

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

    pub fn get_ctrl(&self) -> u8 {
        self.ctrl
    }
    pub fn set_ctrl(&mut self, value: u8) {
        self.ctrl = value;
    }
    pub fn vram_increment_step(&self) -> u16 {
        if (self.ctrl & 0x04) != 0 { 32 } else { 1 }
    }

    pub fn get_vram_addr(&self) -> u16 {
        self.vram_addr
    }
    pub fn set_vram_addr(&mut self, addr: u16) {
        self.vram_addr = addr & 0x3FFF;
    }

    pub fn get_vram_buffer(&self) -> u8 {
        self.vram_buffer
    }
    pub fn set_vram_buffer(&mut self, value: u8) {
        self.vram_buffer = value;
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
        let mut ppu = Ppu::new();
        ppu.set_vblank(true);
        ppu.write_toggle = true;

        let s = ppu.read_reg(0x2002);
        assert_ne!(s & 0x80, 0);
        assert!(!ppu.vblank());
        assert!(!ppu.write_toggle);
    }

    #[test]
    fn ppudata_buffered_read_and_increment() {
        let mut ppu = Ppu::new();
        ppu.write_reg(0x2000, 0x00);

        ppu.vram[0x0000] = 0x11;
        ppu.vram[0x0001] = 0x22;

        ppu.write_reg(0x2006, 0x00);
        ppu.write_reg(0x2006, 0x00);

        assert_eq!(ppu.read_reg(0x2007), 0x00);
        assert_eq!(ppu.read_reg(0x2007), 0x11);
        assert_eq!(ppu.read_reg(0x2007), 0x22);
    }

    #[test]
    fn oam_dma_writes_wrap_and_update_oamaddr() {
        let mut ppu = Ppu::new();
        ppu.write_reg(0x2003, 0xFE);

        let mut buf = [0u8; 256];
        for i in 0..256 {
            buf[i] = i as u8;
        }
        ppu.oam_dma_copy(&buf);

        assert_eq!(ppu.peek_oam(0xFE), 0x00);
        assert_eq!(ppu.peek_oam(0xFF), 0x01);
        assert_eq!(ppu.peek_oam(0x00), 0x02);
        assert_eq!(ppu.peek_oam(0x01), 0x03);
        assert_eq!(ppu.read_reg(0x2003), 0xFE);
    }

    #[test]
    fn framebuffer_dimensions_background_renderer() {
        let mut ppu = Ppu::new();
        let bus = crate::bus::Bus::new();
        ppu.render_frame(&bus);
        assert_eq!(
            ppu.framebuffer.len(),
            NES_WIDTH * NES_HEIGHT * BYTES_PER_PIXEL
        );
    }

    #[test]
    fn background_single_tile_basic() {
        // Cartridge with CHR RAM (0 CHR banks) for writable pattern table
        let flags6 = 0u8;
        let flags7 = 0u8;
        let rom = crate::test_utils::build_ines(1, 0, flags6, flags7, 1, None);
        let cart = crate::cartridge::Cartridge::from_ines_bytes(&rom).expect("parse");
        let mut bus = crate::bus::Bus::new();
        bus.attach_cartridge(cart);

        // Tile 0 pattern: all pixels color index 1 (low plane=0xFF, high plane=0x00)
        for row in 0..8 {
            bus.ppu_write(0x0000 + row as u16, 0xFF); // low plane
            bus.ppu_write(0x0000 + 8 + row as u16, 0x00); // high plane
        }
        // Nametable entry (0,0) = tile 0
        bus.ppu_write(0x2000, 0x00);
        // Attribute table top-left entry = 0 (palette group 0)
        bus.ppu_write(0x23C0, 0x00);
        // Palette: universal background at $3F00 (ignored for index!=0), subpalette 0 color1 at $3F01
        bus.ppu_write(0x3F00, 0x00);
        bus.ppu_write(0x3F01, 0x03); // pick a known palette entry

        // Render then release mutable borrow before accessing framebuffer
        // Render and clone framebuffer while holding a single mutable borrow.
        bus.render_ppu_frame();
        let fb = bus.ppu().framebuffer();
        let expected_rgb = NES_PALETTE[0x03];
        assert_eq!(fb[0], expected_rgb[0]);
        assert_eq!(fb[1], expected_rgb[1]);
        assert_eq!(fb[2], expected_rgb[2]);
        assert_eq!(fb[3], 0xFF);
    }

    #[test]
    fn background_attribute_quadrants() {
        // CHR RAM cart
        let rom = crate::test_utils::build_ines(1, 0, 0, 0, 1, None);
        let cart = crate::cartridge::Cartridge::from_ines_bytes(&rom).expect("parse");
        let mut bus = crate::bus::Bus::new();
        bus.attach_cartridge(cart);

        // Tile pattern (tile 0) constant color index 1
        for row in 0..8 {
            bus.ppu_write(0x0000 + row as u16, 0xFF);
            bus.ppu_write(0x0000 + 8 + row as u16, 0x00);
        }
        // Place tile 0 at positions (0,0), (2,0), (0,2), (2,2) within the first 4x4 tile attribute block.
        let coords = [(0u16, 0u16), (2, 0), (0, 2), (2, 2)];
        for &(tx, ty) in &coords {
            let index = 0x2000 + ty * 32 + tx;
            bus.ppu_write(index, 0x00);
        }
        // Attribute byte: TL=palette0 (00), TR=palette1 (01), BL=palette2 (10), BR=palette3 (11)
        let attr_value = (3 << 6) | (2 << 4) | (1 << 2);
        bus.ppu_write(0x23C0, attr_value);

        // Palette entries for color index=1 in each subpalette
        bus.ppu_write(0x3F00, 0x00); // universal
        bus.ppu_write(0x3F01, 0x01); // palette0 color1
        bus.ppu_write(0x3F05, 0x02); // palette1 color1
        bus.ppu_write(0x3F09, 0x03); // palette2 color1
        bus.ppu_write(0x3F0D, 0x04); // palette3 color1

        bus.render_ppu_frame();
        let fb = bus.ppu().framebuffer();

        let check = |tile_x: usize, tile_y: usize, expect_palette_index: usize| {
            let x = tile_x * 8;
            let y = tile_y * 8;
            let idx = (y * NES_WIDTH + x) * BYTES_PER_PIXEL;
            let expected = NES_PALETTE[expect_palette_index];
            assert_eq!(fb[idx], expected[0]);
            assert_eq!(fb[idx + 1], expected[1]);
            assert_eq!(fb[idx + 2], expected[2]);
            assert_eq!(fb[idx + 3], 0xFF);
        };

        check(0, 0, 0x01); // TL palette0
        check(2, 0, 0x02); // TR palette1
        check(0, 2, 0x03); // BL palette2
        check(2, 2, 0x04); // BR palette3
    }
}
