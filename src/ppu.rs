use crate::rom::Mirroring;

const SCREEN_WIDTH: usize = 256;
const SCREEN_HEIGHT: usize = 240;

const PALETTE_RGB: [(u8, u8, u8); 64] = [
    (84, 84, 84), (0, 30, 116), (8, 16, 144), (48, 0, 136), (68, 0, 100), (92, 0, 48), (84, 4, 0), (60, 24, 0),
    (32, 42, 0), (8, 58, 0), (0, 64, 0), (0, 60, 0), (0, 50, 60), (0, 0, 0), (0, 0, 0), (0, 0, 0),
    (152, 150, 152), (8, 76, 196), (48, 50, 236), (92, 30, 228), (136, 20, 176), (160, 20, 100), (152, 34, 32), (120, 60, 0),
    (84, 90, 0), (40, 114, 0), (8, 124, 0), (0, 118, 40), (0, 102, 120), (0, 0, 0), (0, 0, 0), (0, 0, 0),
    (236, 238, 236), (76, 154, 236), (120, 124, 236), (176, 98, 236), (228, 84, 236), (236, 88, 180), (236, 106, 100), (212, 136, 32),
    (160, 170, 0), (116, 196, 0), (76, 208, 32), (56, 204, 108), (56, 180, 204), (60, 60, 60), (0, 0, 0), (0, 0, 0),
    (236, 238, 236), (168, 204, 236), (188, 188, 236), (212, 178, 236), (236, 174, 236), (236, 174, 212), (236, 180, 176), (228, 196, 144),
    (204, 210, 120), (180, 222, 120), (168, 226, 144), (152, 226, 180), (160, 214, 228), (160, 162, 160), (0, 0, 0), (0, 0, 0),
];

pub struct Ppu {
    ctrl: u8,
    mask: u8,
    status: u8,
    scroll_x: u8,
    scroll_y: u8,
    addr: u16,
    addr_latch: bool,
    data_buffer: u8,
    vram: Vec<u8>,
    vram_size: usize,
    pattern_size: usize,
    nametable_size: usize,
    is_chr_ram: bool,
    mirroring: Mirroring,
    oam: [u8; 256],
    palette: [u8; 32],
    cycles: u64,
    scanline: i32,
    dot: u32,
    frame_buffer: [u8; SCREEN_WIDTH * SCREEN_HEIGHT * 3],
    nmi_triggered: bool,
    bg_shift_low: u16,
    bg_shift_high: u16,
    bg_attr_shift: u8,
    tile_low: u8,
    tile_high: u8,
    attr_latch: u8,
}

impl Ppu {
    pub fn new(chr_size: usize, chr_ram_size: usize, mirroring: Mirroring) -> Self {
        let is_chr_ram = chr_size == 0;
        let pattern_size = if is_chr_ram { chr_ram_size.max(8 * 1024) } else { chr_size };
        let nametable_size = if mirroring == Mirroring::FourScreen { 4 * 1024 } else { 2 * 1024 };
        let vram_size = pattern_size + nametable_size;

        Ppu {
            ctrl: 0,
            mask: 0,
            status: 0,
            scroll_x: 0,
            scroll_y: 0,
            addr: 0,
            addr_latch: false,
            data_buffer: 0,
            vram: vec![0; vram_size],
            vram_size,
            pattern_size,
            nametable_size,
            is_chr_ram,
            mirroring,
            oam: [0; 256],
            palette: [0; 32],
            cycles: 0,
            scanline: -1,
            dot: 0,
            frame_buffer: [0; SCREEN_WIDTH * SCREEN_HEIGHT * 3],
            nmi_triggered: false,
            bg_shift_low: 0,
            bg_shift_high: 0,
            bg_attr_shift: 0,
            tile_low: 0,
            tile_high: 0,
            attr_latch: 0,
        }
    }

    pub fn load_chr_rom(&mut self, chr_data: &[u8]) -> Result<(), String> {
        if self.is_chr_ram {
            return Err("Cannot load CHR-ROM into CHR-RAM".to_string());
        }
        if chr_data.len() != self.pattern_size {
            return Err(format!(
                "CHR-ROM size {} does not match expected pattern table size {}",
                chr_data.len(),
                self.pattern_size
            ));
        }
        for (i, &byte) in chr_data.iter().enumerate() {
            self.vram[i] = byte;
        }
        Ok(())
    }

pub fn step(&mut self, cpu_cycles: usize) -> bool {
    let ppu_cycles = cpu_cycles * 3;
    self.cycles += ppu_cycles as u64;
    let mut nmi = false;

    println!("PPU Step - Scanline: {}, Dot: {}, Ctrl: {:#04x}, Mask: {:#04x}, Status: {:#04x}",
             self.scanline, self.dot, self.ctrl, self.mask, self.status);

    for _ in 0..ppu_cycles {
        self.dot += 1;
        if self.dot > 340 {
            self.dot = 0;
            self.scanline += 1;
            println!("Scanline advanced to: {}, Dot reset to: {}", self.scanline, self.dot);
            if self.scanline > 261 {
                self.scanline = -1;
                println!("Scanline wrapped to pre-render: -1");
            }
        }

        if self.scanline == 241 && self.dot == 1 {
            self.status |= 0x80;  // Set VBlank
            println!("VBlank started - Status: {:#04x}, Scanline: {}, Dot: {}", self.status, self.scanline, self.dot);
            if self.ctrl & 0x80 != 0 {
                nmi = true;
                self.nmi_triggered = true;
            }
        }

        if self.scanline == -1 && self.dot == 1 {
            self.status &= !(0x80 | 0x40 | 0x20);  // Clear VBlank, Sprite 0 Hit, Overflow
            println!("VBlank cleared - Status: {:#04x}, Scanline: {}, Dot: {}", self.status, self.scanline, self.dot);
            self.nmi_triggered = false;
        }

        if self.scanline >= 0 && self.scanline < 240 {
            if self.dot >= 1 && self.dot <= 256 {
                if self.mask & 0x08 != 0 {
                    self.render_background_pixel();
                    println!("Rendering background at Scanline: {}, Dot: {}, Pixel: {}",
                             self.scanline, self.dot, (((self.bg_shift_high >> 15) & 0x01) << 1 | ((self.bg_shift_low >> 15) & 0x01)) as u8);
                }
            }
            if self.dot >= 257 && self.dot <= 320 {
                self.evaluate_sprites();
            }
            if self.dot == 256 && self.mask & 0x10 != 0 {
                self.render_sprites();
                println!("Rendering sprites at Scanline: {}, Dot: 256", self.scanline);
            }
        }

        if self.scanline >= -1 && self.scanline < 240 && self.dot >= 1 && self.dot <= 336 && self.dot % 8 == 0 {
            self.fetch_background_tile();
            println!("Fetching background tile at Scanline: {}, Dot: {}, Tile_Low: {:#04x}, Tile_High: {:#04x}, Combined: {:#06x}, Attr: {:#04x}",
                     self.scanline, self.dot, self.tile_low, self.tile_high, (self.tile_low as u16) | ((self.tile_high as u16) << 8), self.attr_latch);
        }

        if self.scanline >= 0 && self.scanline < 240 && self.dot >= 1 && self.dot <= 256 {
            self.bg_shift_low <<= 1;
            self.bg_shift_high <<= 1;
            self.bg_attr_shift <<= 1;
        }
    }
    nmi
}

    fn render_background_pixel(&mut self) {
        let x = self.dot - 1;
        let y = self.scanline as usize;
        let idx = (y * SCREEN_WIDTH + x as usize) * 3;

        let pixel = (((self.bg_shift_high >> 15) & 0x01) << 1 | ((self.bg_shift_low >> 15) & 0x01)) as u8;
        if pixel != 0 {
            let palette_idx = ((self.bg_attr_shift >> 7) & 0x03) * 4 + pixel;
            let color = self.palette[palette_idx as usize] & 0x3F;
            let (r, g, b) = PALETTE_RGB[color as usize];
            self.frame_buffer[idx] = r;
            self.frame_buffer[idx + 1] = g;
            self.frame_buffer[idx + 2] = b;
        } else {
            println!("No pixel rendered at Scanline: {}, Dot: {}, Pixel: {}", self.scanline, self.dot, pixel);
        }
    }

    fn fetch_background_tile(&mut self) {
        let nametable_base = 0x2000 | ((self.ctrl & 0x03) as u16) << 10;
        let tile_x = (self.dot / 8 + self.scroll_x as u32 / 8) % 32;
        let tile_y = (self.scanline + self.scroll_y as i32) / 8;
        let nametable_addr = nametable_base + (tile_y as u16) * 32 + tile_x as u16;
        let tile_idx = self.read_vram(nametable_addr);

        let attr_x = tile_x / 4;
        let attr_y = tile_y / 4;
        let attr_addr = nametable_base + 0x3C0 + (attr_y as u16) * 8 + attr_x as u16;
        let attr = self.read_vram(attr_addr);
        let attr_shift = (((tile_y as u32 % 4) / 2 * 2 + (tile_x % 4) / 2) * 2) as u8;
        let palette_num = (attr >> attr_shift) & 0x03;

        let pattern_base = if self.ctrl & 0x10 != 0 { 0x1000 } else { 0x0000 };
        let pattern_addr = pattern_base + tile_idx as u16 * 16 + ((self.scanline % 8 + 8) % 8) as u16;
        self.tile_low = self.read_vram(pattern_addr);
        self.tile_high = self.read_vram(pattern_addr + 8);
        self.attr_latch = palette_num;

        self.bg_shift_low = (self.bg_shift_low & 0xFF00) | self.tile_low as u16;
        self.bg_shift_high = (self.bg_shift_high & 0xFF00) | self.tile_high as u16;
        self.bg_attr_shift = (self.bg_attr_shift & 0xF0) | (self.attr_latch << 2);

        println!("Fetching background tile at Scanline: {}, Dot: {}, Tile_Low: {:#04x}, Tile_High: {:#04x}, Combined: {:#06x}, Attr: {:#04x}",
                 self.scanline, self.dot, self.tile_low, self.tile_high, (self.tile_low as u16) | ((self.tile_high as u16) << 8), self.attr_latch);
    }

    fn evaluate_sprites(&mut self) {
        if self.scanline >= 0 && self.scanline < 240 {
            let sprite_y = self.oam[0] as i32;
            if sprite_y <= self.scanline && self.scanline < sprite_y + 8 {
                self.status |= 0x40;
            }
        }
    }

    fn render_sprites(&mut self) {
        for i in (0..256).step_by(4) {
            let y = self.oam[i] as i32;
            let tile = self.oam[i + 1];
            let attr = self.oam[i + 2];
            let x = self.oam[i + 3] as u32;

            if y <= self.scanline && self.scanline < y + 8 {
                let pattern_base = if self.ctrl & 0x08 != 0 { 0x1000 } else { 0x0000 };
                let pattern_addr = pattern_base + tile as u16 * 16 + (self.scanline - y) as u16;
                let low = self.read_vram(pattern_addr);
                let high = self.read_vram(pattern_addr + 8);
                let palette_num = (attr & 0x03) + 4;

                for bit in 0..8 {
                    let pixel_x = x + bit;
                    if pixel_x < 256 {
                        let pixel = ((high >> (7 - bit)) & 0x01) << 1 | ((low >> (7 - bit)) & 0x01);
                        if pixel != 0 {
                            let idx = (self.scanline as usize * SCREEN_WIDTH + pixel_x as usize) * 3;
                            let color = self.palette[palette_num as usize * 4 + pixel as usize] & 0x3F;
                            let (r, g, b) = PALETTE_RGB[color as usize];
                            self.frame_buffer[idx] = r;
                            self.frame_buffer[idx + 1] = g;
                            self.frame_buffer[idx + 2] = b;
                        } else {
                            println!("No sprite pixel rendered at Scanline: {}, Dot: {}, Pixel_X: {}, Pixel: {}", self.scanline, self.dot, pixel_x, pixel);
                        }
                    }
                }
            }
        }
    }

    pub fn read(&mut self, addr: u16) -> u8 {
        match addr & 0x2007 {
            0x2002 => {
                let value = self.status;
                self.status &= !0x80;
                self.addr_latch = false;
                value
            }
            0x2007 => {
                let value = self.data_buffer;
                let new_data = self.read_vram(self.addr);
                self.data_buffer = if self.addr < 0x3F00 { new_data } else { value };
                self.addr = self.addr.wrapping_add(if self.ctrl & 0x04 != 0 { 32 } else { 1 }) & 0x3FFF;
                value
            }
            _ => 0,
        }
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        match addr & 0x2007 {
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
                    self.addr = ((value as u16 & 0x3F) << 8) | (self.addr & 0x00FF);
                }
                self.addr_latch = !self.addr_latch;
            }
            0x2007 => {
                self.write_vram(self.addr, value);
                self.addr = self.addr.wrapping_add(if self.ctrl & 0x04 != 0 { 32 } else { 1 }) & 0x3FFF;
            }
            _ => {}
        }
    }

    pub fn read_vram(&self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        if addr as usize >= self.vram_size {
            return 0;
        }
        match addr {
            0x0000..=0x1FFF => self.vram[addr as usize],
            0x2000..=0x3EFF => {
                let nametable_offset = self.pattern_size;
                let mirrored_addr = addr & 0x0FFF;
                match self.mirroring {
                    Mirroring::Horizontal => {
                        let offset = if mirrored_addr < 0x0800 { mirrored_addr } else { mirrored_addr - 0x0800 };
                        self.vram[nametable_offset + offset as usize]
                    }
                    Mirroring::Vertical => {
                        let offset = mirrored_addr & 0x07FF;
                        self.vram[nametable_offset + offset as usize]
                    }
                    Mirroring::FourScreen => {
                        self.vram[nametable_offset + (mirrored_addr & 0x0FFF) as usize]
                    }
                }
            }
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

    // Changed from `fn` to `pub fn` to make it public
    pub fn write_vram(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if addr as usize >= self.vram_size {
            return;
        }
        match addr {
            0x0000..=0x1FFF => {
                if self.is_chr_ram {
                    self.vram[addr as usize] = value;
                }
            }
            0x2000..=0x3EFF => {
                let nametable_offset = self.pattern_size;
                let mirrored_addr = addr & 0x0FFF;
                match self.mirroring {
                    Mirroring::Horizontal => {
                        let offset = if mirrored_addr < 0x0800 { mirrored_addr } else { mirrored_addr - 0x0800 };
                        self.vram[nametable_offset + offset as usize] = value;
                    }
                    Mirroring::Vertical => {
                        let offset = mirrored_addr & 0x07FF;
                        self.vram[nametable_offset + offset as usize] = value;
                    }
                    Mirroring::FourScreen => {
                        self.vram[nametable_offset + (mirrored_addr & 0x0FFF) as usize] = value;
                    }
                }
            }
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

    pub fn is_nmi_triggered(&self) -> bool {
        self.nmi_triggered
    }

    pub fn ctrl(&self) -> u8 {
        self.ctrl
    }

    pub fn mask(&self) -> u8 {
        self.mask
    }

    pub fn status(&self) -> u8 {
        self.status
    }

    pub fn scanline(&self) -> i32 {
        self.scanline
    }

    pub fn dot(&self) -> u32 {
        self.dot
    }
}
