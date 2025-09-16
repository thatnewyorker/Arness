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
*/

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
    }

    // Advance PPU by one dot (called 3x per CPU cycle via Bus.tick).
    // Signals NMI via Bus at vblank start if enabled.
    pub fn tick(&mut self) {
        // On the first dot of specific scanlines, handle state transitions.
        if self.dot == 0 {
            // Start of a new dot; when we move to dot 1, check events.
        }

        // Increment dot
        self.dot = self.dot.wrapping_add(1);

        // At dot 1 of a scanline, perform events
        if self.dot == 1 {
            if self.scanline == 241 {
                // Entering vblank
                self.set_vblank(true);
                if self.nmi_enabled() {
                    self.nmi_latch = true;
                }
            } else if self.scanline == -1 {
                // Pre-render line: clear vblank and sprite flags
                self.set_vblank(false);
                self.set_sprite_zero_hit(false);
                self.set_sprite_overflow(false);
                self.frame_complete = false;
            }
        }

        // End of scanline
        if self.dot >= 341 {
            self.dot = 0;
            self.scanline += 1;

            if self.scanline > 260 {
                // Wrap to pre-render line
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
                // PPUCTRL
                self.ctrl = value;
            }
            0x2001 => {
                // PPUMASK
                self.mask = value;
            }
            0x2002 => {
                // PPUSTATUS is read-only; writes are ignored.
            }
            0x2003 => {
                // OAMADDR
                self.oam_addr = value;
            }
            0x2004 => {
                // OAMDATA write; write and increment OAMADDR
                let idx = self.oam_addr as usize;
                self.oam[idx] = value;
                self.oam_addr = self.oam_addr.wrapping_add(1);
            }
            0x2005 => {
                // PPUSCROLL: two writes
                if !self.write_toggle {
                    self.scroll_x = value;
                    self.write_toggle = true;
                } else {
                    self.scroll_y = value;
                    self.write_toggle = false;
                }
            }
            0x2006 => {
                // PPUADDR: two writes set VRAM address (15-bit)
                if !self.write_toggle {
                    // High byte (only lower 6 bits used on hardware; we mask to 15 bits)
                    self.vram_addr = (self.vram_addr & 0x00FF) | (((value as u16) & 0x3F) << 8);
                    self.write_toggle = true;
                } else {
                    // Low byte
                    self.vram_addr = (self.vram_addr & 0x7F00) | (value as u16);
                    self.vram_addr &= 0x3FFF; // mirror to 0x0000..0x3FFF
                    self.write_toggle = false;
                }
            }
            0x2007 => {
                // PPUDATA write: write to VRAM, then increment by 1 or 32 depending on $2000 bit 2
                let addr = (self.vram_addr & 0x3FFF) as usize;
                self.vram[addr] = value;
                let inc = if (self.ctrl & 0x04) != 0 { 32 } else { 1 };
                self.vram_addr = self.vram_addr.wrapping_add(inc) & 0x3FFF;
            }
            _ => { /* unreachable by construction */ }
        }
    }

    // CPU read from PPU register in $2000..$2007
    pub fn read_reg(&mut self, addr: u16) -> u8 {
        let reg = 0x2000 + (addr & 0x0007);
        match reg {
            0x2000 => {
                // PPUCTRL usually not readable; return stored value for visibility.
                self.ctrl
            }
            0x2001 => {
                // PPUMASK usually not readable; return stored value for visibility.
                self.mask
            }
            0x2002 => {
                // PPUSTATUS: reading clears vblank flag (bit 7) and resets write toggle
                let result = self.status;
                // Clear vblank on read
                self.status &= !0x80;
                // Reset $2005/$2006 write toggle
                self.write_toggle = false;
                result
            }
            0x2003 => {
                // Return current OAMADDR
                self.oam_addr
            }
            0x2004 => {
                // OAMDATA read at OAMADDR. On hardware, reads do not increment OAMADDR.
                let idx = self.oam_addr as usize;
                self.oam[idx]
            }
            0x2005 => {
                // PPUSCROLL is write-only; return 0 for simplicity.
                0
            }
            0x2006 => {
                // PPUADDR is write-only; return high byte for visibility.
                ((self.vram_addr >> 8) & 0xFF) as u8
            }
            0x2007 => {
                // PPUDATA read: buffered read for non-palette addresses
                let addr = self.vram_addr & 0x3FFF;
                let value = self.vram[addr as usize];

                let ret = if addr < 0x3F00 {
                    // Return buffered value and update buffer
                    let out = self.vram_buffer;
                    self.vram_buffer = value;
                    out
                } else {
                    // Palette range returns immediate; buffer updated (simple model)
                    value
                };

                let inc = if (self.ctrl & 0x04) != 0 { 32 } else { 1 };
                self.vram_addr = self.vram_addr.wrapping_add(inc) & 0x3FFF;
                ret
            }
            _ => 0,
        }
    }

    // Bulk OAM DMA: copies 256 bytes into OAM starting at current OAMADDR, wrapping as needed.
    // The Bus should call this when $4014 is written.
    pub fn oam_dma_copy(&mut self, data: &[u8]) {
        let mut addr = self.oam_addr;
        for i in 0..256 {
            let byte = data.get(i).copied().unwrap_or(0);
            self.oam[addr as usize] = byte;
            addr = addr.wrapping_add(1);
        }
        self.oam_addr = addr;
    }

    // Helpers to inspect/modify status flags for integration (e.g., frame/vblank signaling).
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

    // Whether NMI on vblank is enabled (PPUCTRL bit 7).
    pub fn nmi_enabled(&self) -> bool {
        (self.ctrl & 0x80) != 0
    }

    // Expose minimal VRAM access for tests/integration if needed.
    pub fn peek_vram(&self, addr: u16) -> u8 {
        self.vram[(addr as usize) & 0x3FFF]
    }
    pub fn poke_vram(&mut self, addr: u16, value: u8) {
        self.vram[(addr as usize) & 0x3FFF] = value;
    }

    // OAM inspection for tests/integration.
    pub fn peek_oam(&self, idx: usize) -> u8 {
        self.oam[idx & 0xFF]
    }
    pub fn poke_oam(&mut self, idx: usize, value: u8) {
        self.oam[idx & 0xFF] = value;
    }

    // Frame completion handling.
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

    // ----- Accessors for Bus integration (single source of truth in PPU) -----

    // PPUCTRL accessors and VRAM increment step (bit 2 of PPUCTRL)
    pub fn get_ctrl(&self) -> u8 {
        self.ctrl
    }
    pub fn set_ctrl(&mut self, value: u8) {
        self.ctrl = value;
    }
    pub fn vram_increment_step(&self) -> u16 {
        if (self.ctrl & 0x04) != 0 { 32 } else { 1 }
    }

    // VRAM address accessors (mask to 14 bits)
    pub fn get_vram_addr(&self) -> u16 {
        self.vram_addr
    }
    pub fn set_vram_addr(&mut self, addr: u16) {
        self.vram_addr = addr & 0x3FFF;
    }

    // PPUDATA buffered read value accessors
    pub fn get_vram_buffer(&self) -> u8 {
        self.vram_buffer
    }
    pub fn set_vram_buffer(&mut self, value: u8) {
        self.vram_buffer = value;
    }

    // $2005/$2006 write toggle accessors
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
        // Set VRAM increment to 1
        ppu.write_reg(0x2000, 0x00);

        // Write VRAM at 0x0000 and 0x0001
        ppu.vram[0x0000] = 0x11;
        ppu.vram[0x0001] = 0x22;

        // Set address to 0x0000
        ppu.write_reg(0x2006, 0x00);
        ppu.write_reg(0x2006, 0x00);

        // First read returns buffer (initially 0)
        assert_eq!(ppu.read_reg(0x2007), 0x00);
        // Second read returns previous value at 0x0000, buffer updated to 0x22
        assert_eq!(ppu.read_reg(0x2007), 0x11);
        // Third read returns 0x22
        assert_eq!(ppu.read_reg(0x2007), 0x22);
    }

    #[test]
    fn oam_dma_writes_wrap_and_update_oamaddr() {
        let mut ppu = Ppu::new();
        ppu.write_reg(0x2003, 0xFE); // OAMADDR = 0xFE

        let mut buf = [0u8; 256];
        for i in 0..256 {
            buf[i] = i as u8;
        }
        ppu.oam_dma_copy(&buf);

        assert_eq!(ppu.peek_oam(0xFE), 0x00);
        assert_eq!(ppu.peek_oam(0xFF), 0x01);
        assert_eq!(ppu.peek_oam(0x00), 0x02);
        assert_eq!(ppu.peek_oam(0x01), 0x03);
        // OAMADDR advanced by 256 (wraps), so +0 from 0xFE -> 0xFE
        assert_eq!(ppu.read_reg(0x2003), 0xFE);
    }
}
