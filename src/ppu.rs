const SCREEN_WIDTH: usize = 256;
const SCREEN_HEIGHT: usize = 240;

pub struct Ppu {
    ctrl: u8,
    mask: u8,
    status: u8,
    scroll_x: u8,
    scroll_y: u8,
    addr: u16,
    addr_latch: bool,
    data_buffer: u8,
    vram: [u8; 0x4000],
    oam: [u8; 256],
    palette: [u8; 32],
    cycles: usize,
    scanline: i32,
    dot: u32,
    frame_buffer: [u8; SCREEN_WIDTH * SCREEN_HEIGHT * 3],
}

impl Ppu {
    pub fn new() -> Self {
        Ppu {
            ctrl: 0,
            mask: 0,
            status: 0,
            scroll_x: 0,
            scroll_y: 0,
            addr: 0,
            addr_latch: false,
            data_buffer: 0,
            vram: [0; 0x4000],
            oam: [0; 256],
            palette: [0; 32],
            cycles: 0,
            scanline: -1,
            dot: 0,
            frame_buffer: [0; SCREEN_WIDTH * SCREEN_HEIGHT * 3],
        }
    }

    pub fn step(&mut self, cpu_cycles: usize) {
        let ppu_cycles = cpu_cycles * 3;
        self.cycles += ppu_cycles;

        for _ in 0..ppu_cycles {
            self.dot += 1;
            if self.dot > 340 {
                self.dot = 0;
                self.scanline += 1;
                if self.scanline > 261 {
                    self.scanline = -1;
                }
            }

            if self.scanline == 241 && self.dot == 1 {
                self.status |= 0x80;
            }
            if self.scanline == -1 && self.dot == 1 {
                self.status &= !0x80;
            }

            if self.scanline >= 0 && self.scanline < 240 && self.dot < 256 {
                let color = if self.mask & 0x08 != 0 { 0xFF } else { 0x00 };
                let idx = (self.scanline as usize * SCREEN_WIDTH + self.dot as usize) * 3;
                self.frame_buffer[idx] = color;
                self.frame_buffer[idx + 1] = color;
                self.frame_buffer[idx + 2] = color;
            }
        }
    }

pub fn read(&mut self, addr: u16) -> u8 {
    match addr & 0x2007 { // Mask to handle mirroring in CPU, but shown here for clarity
        0x2002 => {
            let value = self.status;
            self.status &= !0x80; // Clear VBLANK on read
            self.addr_latch = false;
            value
        }
        0x2007 => {
            let value = self.data_buffer;
            let new_data = self.read_vram(self.addr);
            self.data_buffer = if self.addr < 0x3F00 { new_data } else { value }; // Palette reads are immediate
            self.addr += if self.ctrl & 0x04 != 0 { 32 } else { 1 };
            value
        }
        _ => 0,
    }
}

    pub fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x2000 => self.ctrl = value,
            0x2001 => self.mask = value,
            0x2005 => {
                if self.addr_latch {
                    self.scroll_y = value;
                } else {
                    self.scroll_x = value;
                }
                self.addr_latch = !self.addr_latch;
            }
            0x2006 => {
                if self.addr_latch {
                    self.addr = (self.addr & 0xFF00) | value as u16;
                } else {
                    self.addr = (value as u16 & 0x3F) << 8;
                }
                self.addr_latch = !self.addr_latch;
            }
            0x2007 => {
                self.write_vram(self.addr, value);
                self.addr += if self.ctrl & 0x04 != 0 { 32 } else { 1 };
            }
            _ => {}
        }
    }

    fn read_vram(&self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x3EFF => self.vram[addr as usize],
            0x3F00..=0x3FFF => {
                let palette_addr = addr & 0x1F;
                if palette_addr == 0x10 || palette_addr == 0x14 || palette_addr == 0x18 || palette_addr == 0x1C {
                    self.palette[(palette_addr & 0x0F) as usize]
                } else {
                    self.palette[palette_addr as usize]
                }
            }
            _ => 0,
        }
    }

    fn write_vram(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x3EFF => self.vram[addr as usize] = value,
            0x3F00..=0x3FFF => {
                let palette_addr = addr & 0x1F;
                if palette_addr == 0x10 || palette_addr == 0x14 || palette_addr == 0x18 || palette_addr == 0x1C {
                    self.palette[(palette_addr & 0x0F) as usize] = value;
                } else {
                    self.palette[palette_addr as usize] = value;
                }
            }
            _ => {}
        }
    }

    pub fn get_frame_buffer(&self) -> &[u8] {
        &self.frame_buffer
    }
}
