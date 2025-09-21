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

pub(crate) mod fetch;
pub(crate) mod memory;
pub(crate) mod oam_eval;
pub(crate) mod registers;
pub(crate) mod renderer;
pub(crate) mod sprite;

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

    pub fn write_reg(&mut self, addr: u16, value: u8) {
        self.write_reg_inner(addr, value);
    }

    /// Read CPU-facing PPU register ($2000..$2007).
    pub fn read_reg(&mut self, addr: u16) -> u8 {
        self.read_reg_inner(addr)
    }

    /// OAM DMA copy (256 bytes).
    pub fn oam_dma_copy(&mut self, data: &[u8]) {
        self.oam_dma_copy_inner(data);
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
        self.peek_vram_inner(addr)
    }
    pub fn poke_vram(&mut self, addr: u16, value: u8) {
        self.poke_vram_inner(addr, value);
    }
    pub fn peek_oam(&self, idx: usize) -> u8 {
        self.peek_oam_inner(idx)
    }
    pub fn poke_oam(&mut self, idx: usize, value: u8) {
        self.poke_oam_inner(idx, value);
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
