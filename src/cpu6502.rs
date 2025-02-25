use crate::ppu::Ppu;

const CARRY: u8 = 0b0000_0001;
const ZERO: u8 = 0b0000_0010;
const INTERRUPT_DISABLE: u8 = 0b0000_0100;
const OVERFLOW: u8 = 0b0100_0000;
const NEGATIVE: u8 = 0b1000_0000;

pub struct Cpu6502 {
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub sp: u8,
    pub pc: u16,
    pub status: u8,
    pub memory: [u8; 65536],
    pub ppu: Ppu,
}

impl Cpu6502 {
    pub fn new(chr_size: usize, chr_ram_size: usize, mirroring: crate::rom::Mirroring) -> Self {
        Cpu6502 {
            a: 0,
            x: 0,
            y: 0,
            sp: 0xFD,
            pc: 0x8000,
            status: 0x24,
            memory: [0; 65536],
            ppu: Ppu::new(chr_size, chr_ram_size, mirroring),
        }
    }

    pub fn load_rom(&mut self, rom: &crate::rom::Rom) -> Result<(), String> {
        if rom.mapper() != 0 {
            return Err("Only mapper 0 (NROM) is supported for now".to_string());
        }

        let prg_data = rom.prg_rom();
        if prg_data.len() != 16 * 1024 && prg_data.len() != 32 * 1024 {
            return Err("PRG-ROM size must be 16 KB or 32 KB for NROM".to_string());
        }

        for (i, &byte) in prg_data.iter().enumerate() {
            self.memory[0x8000 + i] = byte;
        }

        if prg_data.len() == 16 * 1024 {
            for i in 0..16 * 1024 {
                self.memory[0xC000 + i] = self.memory[0x8000 + i];
            }
        }

        self.pc = self.read_word(0xFFFC);
        Ok(())
    }

    fn set_status_flag(&mut self, flag: u8) {
        self.status |= flag;
    }

    fn clear_status_flag(&mut self, flag: u8) {
        self.status &= flag ^ 0xFF;
    }

    fn is_status_flag_set(&self, flag: u8) -> bool {
        self.status & flag != 0
    }

    fn update_zero_and_negative_flags(&mut self, result: u8) {
        if result == 0 {
            self.set_status_flag(ZERO);
        } else {
            self.clear_status_flag(ZERO);
        }
        if result & NEGATIVE != 0 {
            self.set_status_flag(NEGATIVE);
        } else {
            self.clear_status_flag(NEGATIVE);
        }
    }

    fn immediate(&self) -> u8 {
        self.read_cpu(self.pc.wrapping_add(1))
    }

    fn absolute(&self) -> u16 {
        self.read_word_cpu(self.pc.wrapping_add(1))
    }

    fn zero_page(&self) -> u16 {
        self.read_cpu(self.pc.wrapping_add(1)) as u16
    }

    fn zero_page_x(&self) -> u16 {
        self.read_cpu(self.pc.wrapping_add(1)).wrapping_add(self.x) as u16
    }

    fn zero_page_y(&self) -> u16 {
        self.read_cpu(self.pc.wrapping_add(1)).wrapping_add(self.y) as u16
    }

    fn absolute_x(&self) -> u16 {
        self.read_word_cpu(self.pc.wrapping_add(1)).wrapping_add(self.x as u16)
    }

    fn absolute_y(&self) -> u16 {
        self.read_word_cpu(self.pc.wrapping_add(1)).wrapping_add(self.y as u16)
    }

    fn indirect_x(&self) -> u16 {
        let base = self.read_cpu(self.pc.wrapping_add(1)).wrapping_add(self.x);
        let lo = self.read_cpu(base as u16);
        let hi = self.read_cpu(base.wrapping_add(1) as u16);
        ((hi as u16) << 8) | (lo as u16)
    }

    fn indirect_y(&self) -> u16 {
        let base = self.read_cpu(self.pc.wrapping_add(1));
        let lo = self.read_cpu(base as u16);
        let hi = self.read_cpu(base.wrapping_add(1) as u16);
        (((hi as u16) << 8) | (lo as u16)).wrapping_add(self.y as u16)
    }

    fn indirect(&self) -> u16 {
        let ptr = self.read_word_cpu(self.pc.wrapping_add(1));
        let lo = self.read_cpu(ptr);
        let hi = self.read_cpu((ptr & 0xFF00) | ((ptr + 1) & 0x00FF));
        ((hi as u16) << 8) | (lo as u16)
    }

    pub fn lda_immediate(&mut self, value: u8) {
        self.a = value;
        self.update_zero_and_negative_flags(self.a);
    }

    fn lda_absolute(&mut self, addr: u16) {
        self.a = self.read(addr);
        self.update_zero_and_negative_flags(self.a);
    }

    fn lda_zero_page(&mut self, addr: u16) {
        self.a = self.read(addr);
        self.update_zero_and_negative_flags(self.a);
    }

    fn lda_zero_page_x(&mut self, addr: u16) {
        self.a = self.read(addr);
        self.update_zero_and_negative_flags(self.a);
    }

    fn lda_absolute_x(&mut self, addr: u16) {
        self.a = self.read(addr);
        self.update_zero_and_negative_flags(self.a);
    }

    fn lda_absolute_y(&mut self, addr: u16) {
        self.a = self.read(addr);
        self.update_zero_and_negative_flags(self.a);
    }

    fn lda_indirect_x(&mut self, addr: u16) {
        self.a = self.read(addr);
        self.update_zero_and_negative_flags(self.a);
    }

    fn lda_indirect_y(&mut self, addr: u16) {
        self.a = self.read(addr);
        self.update_zero_and_negative_flags(self.a);
    }

    pub fn ldx_immediate(&mut self, value: u8) {
        self.x = value;
        self.update_zero_and_negative_flags(self.x);
    }

    fn ldx_zero_page(&mut self, addr: u16) {
        self.x = self.read(addr);
        self.update_zero_and_negative_flags(self.x);
    }

    fn ldx_zero_page_y(&mut self, addr: u16) {
        self.x = self.read(addr);
        self.update_zero_and_negative_flags(self.x);
    }

    fn ldx_absolute(&mut self, addr: u16) {
        self.x = self.read(addr);
        self.update_zero_and_negative_flags(self.x);
    }

    fn ldx_absolute_y(&mut self, addr: u16) {
        self.x = self.read(addr);
        self.update_zero_and_negative_flags(self.x);
    }

    pub fn ldy_immediate(&mut self, value: u8) {
        self.y = value;
        self.update_zero_and_negative_flags(self.y);
    }

    fn ldy_zero_page(&mut self, addr: u16) {
        self.y = self.read(addr);
        self.update_zero_and_negative_flags(self.y);
    }

    fn ldy_zero_page_x(&mut self, addr: u16) {
        self.y = self.read(addr);
        self.update_zero_and_negative_flags(self.y);
    }

    fn ldy_absolute(&mut self, addr: u16) {
        self.y = self.read(addr);
        self.update_zero_and_negative_flags(self.y);
    }

    fn ldy_absolute_x(&mut self, addr: u16) {
        self.y = self.read(addr);
        self.update_zero_and_negative_flags(self.y);
    }

    pub fn sta(&mut self, addr: u16) {
        self.write(addr, self.a);
    }

    pub fn stx(&mut self, addr: u16) {
        self.write(addr, self.x);
    }

    pub fn sty(&mut self, addr: u16) {
        self.write(addr, self.y);
    }

    pub fn tax(&mut self) {
        self.x = self.a;
        self.update_zero_and_negative_flags(self.x);
    }

    pub fn tay(&mut self) {
        self.y = self.a;
        self.update_zero_and_negative_flags(self.y);
    }

    pub fn txa(&mut self) {
        self.a = self.x;
        self.update_zero_and_negative_flags(self.a);
    }

    pub fn tya(&mut self) {
        self.a = self.y;
        self.update_zero_and_negative_flags(self.a);
    }

    pub fn tsx(&mut self) {
        self.x = self.sp;
        self.update_zero_and_negative_flags(self.x);
    }

    pub fn txs(&mut self) {
        self.sp = self.x;
    }

    pub fn adc(&mut self, value: u8) {
        let a = self.a as u16;
        let v = value as u16;
        let c = (self.status & CARRY) as u16;
        let result = a + v + c;

        self.clear_status_flag(CARRY);
        if result > 0xFF {
            self.set_status_flag(CARRY);
        }

        let overflow = ((a ^ result) & (v ^ result) & 0x80) != 0;
        if overflow {
            self.set_status_flag(OVERFLOW);
        } else {
            self.clear_status_flag(OVERFLOW);
        }

        self.a = result as u8;
        self.update_zero_and_negative_flags(self.a);
    }

    pub fn sbc(&mut self, value: u8) {
        let a = self.a as u16;
        let v = value as u16;
        let c = (self.status & CARRY) as u16;
        let result = a.wrapping_sub(v).wrapping_sub(1 - c);

        self.clear_status_flag(CARRY);
        if result <= 0xFF {
            self.set_status_flag(CARRY);
        }

        let overflow = ((a ^ v) & (a ^ result) & 0x80) != 0;
        if overflow {
            self.set_status_flag(OVERFLOW);
        } else {
            self.clear_status_flag(OVERFLOW);
        }

        self.a = result as u8;
        self.update_zero_and_negative_flags(self.a);
    }

    pub fn pha(&mut self) {
        self.push(self.a);
    }

    pub fn pla(&mut self) {
        self.a = self.pop();
        self.update_zero_and_negative_flags(self.a);
    }

    pub fn php(&mut self) {
        let status_with_b_and_u_flags = self.status | 0b0011_0000;
        self.push(status_with_b_and_u_flags);
    }

    pub fn plp(&mut self) {
        let pulled_status = self.pop();
        let unused_flag_mask = !0b0010_0000;
        self.status = (self.status & unused_flag_mask) | (pulled_status & !unused_flag_mask);
    }

    fn read_cpu(&self, addr: u16) -> u8 {
        *self.memory.get(addr as usize).unwrap_or(&0)
    }

    pub fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x2000..=0x2007 => self.ppu.read(addr),
            _ => self.read_cpu(addr),
        }
    }

    pub fn write(&mut self, addr: u16, data: u8) {
        match addr {
            0x2000..=0x2007 => self.ppu.write(addr, data),
            _ => self.memory[addr as usize] = data,
        }
    }

    fn read_word_cpu(&self, addr: u16) -> u16 {
        let lo = self.read_cpu(addr) as u16;
        let hi = self.read_cpu(addr.wrapping_add(1)) as u16;
        (hi << 8) | lo
    }

    pub fn read_word(&mut self, addr: u16) -> u16 {
        let lo = self.read(addr) as u16;
        let hi = self.read(addr.wrapping_add(1)) as u16;
        (hi << 8) | lo
    }

    pub fn write_word(&mut self, addr: u16, data: u16) {
        let lo = data as u8;
        let hi = (data >> 8) as u8;
        self.write(addr, lo);
        self.write(addr.wrapping_add(1), hi);
    }

    pub fn push(&mut self, data: u8) {
        self.write(0x0100 + self.sp as u16, data);
        self.sp = self.sp.wrapping_sub(1);
    }

    pub fn pop(&mut self) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        self.read(0x0100 + self.sp as u16)
    }

    pub fn push_word(&mut self, data: u16) {
        self.push((data >> 8) as u8);
        self.push(data as u8);
    }

    pub fn pop_word(&mut self) -> u16 {
        let lo = self.pop() as u16;
        let hi = self.pop() as u16;
        (hi << 8) | lo
    }

    pub fn dec(&mut self, addr: u16) {
        let value = self.read(addr).wrapping_sub(1);
        self.write(addr, value);
        self.update_zero_and_negative_flags(value);
    }

    pub fn dex(&mut self) {
        self.x = self.x.wrapping_sub(1);
        self.update_zero_and_negative_flags(self.x);
    }

    pub fn dey(&mut self) {
        self.y = self.y.wrapping_sub(1);
        self.update_zero_and_negative_flags(self.y);
    }

    pub fn inc(&mut self, addr: u16) {
        let value = self.read(addr).wrapping_add(1);
        self.write(addr, value);
        self.update_zero_and_negative_flags(value);
    }

    pub fn inx(&mut self) {
        self.x = self.x.wrapping_add(1);
        self.update_zero_and_negative_flags(self.x);
    }

    pub fn iny(&mut self) {
        self.y = self.y.wrapping_add(1);
        self.update_zero_and_negative_flags(self.y);
    }

    pub fn and(&mut self, value: u8) {
        self.a &= value;
        self.update_zero_and_negative_flags(self.a);
    }

    pub fn eor(&mut self, value: u8) {
        self.a ^= value;
        self.update_zero_and_negative_flags(self.a);
    }

    pub fn ora(&mut self, value: u8) {
        self.a |= value;
        self.update_zero_and_negative_flags(self.a);
    }

    fn shift_left(&mut self, value: u8) -> u8 {
        let result = value << 1;
        if value & NEGATIVE != 0 {
            self.set_status_flag(CARRY);
        } else {
            self.clear_status_flag(CARRY);
        }
        self.update_zero_and_negative_flags(result);
        result
    }

    fn shift_right(&mut self, value: u8) -> u8 {
        let result = value >> 1;
        if value & CARRY != 0 {
            self.set_status_flag(CARRY);
        } else {
            self.clear_status_flag(CARRY);
        }
        self.update_zero_and_negative_flags(result);
        result
    }

    fn rotate_left(&mut self, value: u8) -> u8 {
        let carry = self.status & CARRY;
        let result = (value << 1) | carry;
        if value & NEGATIVE != 0 {
            self.set_status_flag(CARRY);
        } else {
            self.clear_status_flag(CARRY);
        }
        self.update_zero_and_negative_flags(result);
        result
    }

    fn rotate_right(&mut self, value: u8) -> u8 {
        let carry = self.status & CARRY;
        let result = (value >> 1) | (carry << 7);
        if value & CARRY != 0 {
            self.set_status_flag(CARRY);
        } else {
            self.clear_status_flag(CARRY);
        }
        self.update_zero_and_negative_flags(result);
        result
    }

    pub fn asl(&mut self, addr: u16) {
        let value = self.read(addr);
        let result = self.shift_left(value);
        self.write(addr, result);
    }

    pub fn asl_acc(&mut self) {
        self.a = self.shift_left(self.a);
    }

    pub fn lsr(&mut self, addr: u16) {
        let value = self.read(addr);
        let result = self.shift_right(value);
        self.write(addr, result);
    }

    pub fn lsr_acc(&mut self) {
        self.a = self.shift_right(self.a);
    }

    pub fn rol(&mut self, addr: u16) {
        let value = self.read(addr);
        let result = self.rotate_left(value);
        self.write(addr, result);
    }

    pub fn rol_acc(&mut self) {
        self.a = self.rotate_left(self.a);
    }

    pub fn ror(&mut self, addr: u16) {
        let value = self.read(addr);
        let result = self.rotate_right(value);
        self.write(addr, result);
    }

    pub fn ror_acc(&mut self) {
        self.a = self.rotate_right(self.a);
    }

    pub fn clc(&mut self) {
        self.clear_status_flag(CARRY);
    }

    pub fn cli(&mut self) {
        self.clear_status_flag(INTERRUPT_DISABLE);
    }

    pub fn clv(&mut self) {
        self.clear_status_flag(OVERFLOW);
    }

    pub fn sec(&mut self) {
        self.set_status_flag(CARRY);
    }

    pub fn sei(&mut self) {
        self.set_status_flag(INTERRUPT_DISABLE);
    }

    pub fn sed(&mut self) {
        self.set_status_flag(0b0000_1000);
    }

    pub fn cld(&mut self) {
        self.clear_status_flag(0b0000_1000);
    }

    pub fn cmp(&mut self, value: u8) {
        if self.a >= value {
            self.set_status_flag(CARRY);
        } else {
            self.clear_status_flag(CARRY);
        }
        let result = self.a.wrapping_sub(value);
        self.update_zero_and_negative_flags(result);
    }

    pub fn cpx(&mut self, value: u8) {
        if self.x >= value {
            self.set_status_flag(CARRY);
        } else {
            self.clear_status_flag(CARRY);
        }
        let result = self.x.wrapping_sub(value);
        self.update_zero_and_negative_flags(result);
    }

    pub fn cpy(&mut self, value: u8) {
        if self.y >= value {
            self.set_status_flag(CARRY);
        } else {
            self.clear_status_flag(CARRY);
        }
        let result = self.y.wrapping_sub(value);
        self.update_zero_and_negative_flags(result);
    }

    pub fn branch(&mut self, offset: u8) -> u8 {
        let offset = offset as i8 as i16;
        let old_pc = self.pc;
        self.pc = self.pc.wrapping_add(offset as u16);
        1 + if (old_pc & 0xFF00) != (self.pc & 0xFF00) { 1 } else { 0 }
    }

    pub fn bcc(&mut self, offset: u8) -> u8 {
        if !self.is_status_flag_set(CARRY) {
            self.branch(offset)
        } else {
            0
        }
    }

    pub fn bcs(&mut self, offset: u8) -> u8 {
        if self.is_status_flag_set(CARRY) {
            self.branch(offset)
        } else {
            0
        }
    }

    pub fn beq(&mut self, offset: u8) -> u8 {
        if self.is_status_flag_set(ZERO) {
            self.branch(offset)
        } else {
            0
        }
    }

    pub fn bmi(&mut self, offset: u8) -> u8 {
        if self.is_status_flag_set(NEGATIVE) {
            self.branch(offset)
        } else {
            0
        }
    }

    pub fn bne(&mut self, offset: u8) -> u8 {
        if !self.is_status_flag_set(ZERO) {
            self.branch(offset)
        } else {
            0
        }
    }

    pub fn bpl(&mut self, offset: u8) -> u8 {
        if !self.is_status_flag_set(NEGATIVE) {
            self.branch(offset)
        } else {
            0
        }
    }

    pub fn bvc(&mut self, offset: u8) -> u8 {
        if !self.is_status_flag_set(OVERFLOW) {
            self.branch(offset)
        } else {
            0
        }
    }

    pub fn bvs(&mut self, offset: u8) -> u8 {
        if self.is_status_flag_set(OVERFLOW) {
            self.branch(offset)
        } else {
            0
        }
    }

    pub fn jmp(&mut self, addr: u16) {
        self.pc = addr;
    }

    pub fn jsr(&mut self, addr: u16) {
        let return_addr = self.pc.wrapping_sub(1);
        self.push_word(return_addr);
        self.pc = addr;
    }

    pub fn rts(&mut self) {
        self.pc = self.pop_word().wrapping_add(1);
    }

    pub fn brk(&mut self) {
        self.push_word(self.pc);
        self.php();
        self.sei();
        self.pc = self.read_word(0xFFFE);
    }

    pub fn rti(&mut self) {
        self.plp();
        self.pc = self.pop_word();
    }

    pub fn nmi(&mut self) {
        println!("NMI triggered - PC: {:#06x}", self.pc);
        self.push_word(self.pc);
        self.php();
        self.sei();
        self.pc = self.read_word(0xFFFA);
    }

    pub fn irq(&mut self) {
        if !self.is_status_flag_set(INTERRUPT_DISABLE) {
            self.push_word(self.pc);
            self.php();
            self.sei();
            self.pc = self.read_word(0xFFFE);
        }
    }

    pub fn bit(&mut self, value: u8) {
        if self.a & value == 0 {
            self.set_status_flag(ZERO);
        } else {
            self.clear_status_flag(ZERO);
        }
        if value & NEGATIVE != 0 {
            self.set_status_flag(NEGATIVE);
        } else {
            self.clear_status_flag(NEGATIVE);
        }
        if value & OVERFLOW != 0 {
            self.set_status_flag(OVERFLOW);
        } else {
            self.clear_status_flag(OVERFLOW);
        }
    }

    pub fn nop(&mut self) {
        // Do nothing
    }

    pub fn step(&mut self) {
    let opcode = self.read_cpu(self.pc);
    
    println!(
        "PC: {:#06x}, Opcode: {:#04x}, A: {:#04x}, X: {:#04x}, Y: {:#04x}, SP: {:#04x}, Status: {:#04x}, PPU Ctrl: {:#04x}, Mask: {:#04x}, Scanline: {}, Dot: {}",
        self.pc, opcode, self.a, self.x, self.y, self.sp, self.status, self.ppu.ctrl(), self.ppu.mask(), self.ppu.scanline(), self.ppu.dot());
    let pc_increment;
    let cycles;
    match opcode {
        0xA9 => { let val = self.immediate(); self.lda_immediate(val); pc_increment = 2; cycles = 2; }
        0xA5 => { let addr = self.zero_page(); self.lda_zero_page(addr); pc_increment = 2; cycles = 3; }
        0xAD => { let addr = self.absolute(); self.lda_absolute(addr); pc_increment = 3; cycles = 4; }
        0xB5 => { let addr = self.zero_page_x(); self.lda_zero_page_x(addr); pc_increment = 2; cycles = 4; }
        0xBD => { let addr = self.absolute_x(); self.lda_absolute_x(addr); pc_increment = 3; cycles = 4; }
        0xB9 => { let addr = self.absolute_y(); self.lda_absolute_y(addr); pc_increment = 3; cycles = 4; }
        0xA1 => { let addr = self.indirect_x(); self.lda_indirect_x(addr); pc_increment = 2; cycles = 6; }
        0xB1 => { let addr = self.indirect_y(); self.lda_indirect_y(addr); pc_increment = 2; cycles = 5; }
        0xA2 => { let val = self.immediate(); self.ldx_immediate(val); pc_increment = 2; cycles = 2; }
        0xA6 => { let addr = self.zero_page(); self.ldx_zero_page(addr); pc_increment = 2; cycles = 3; }
        0xB6 => { let addr = self.zero_page_y(); self.ldx_zero_page_y(addr); pc_increment = 2; cycles = 4; }
        0xAE => { let addr = self.absolute(); self.ldx_absolute(addr); pc_increment = 3; cycles = 4; }
        0xBE => { let addr = self.absolute_y(); self.ldx_absolute_y(addr); pc_increment = 3; cycles = 4; }
        0xA0 => { let val = self.immediate(); self.ldy_immediate(val); pc_increment = 2; cycles = 2; }
        0xA4 => { let addr = self.zero_page(); self.ldy_zero_page(addr); pc_increment = 2; cycles = 3; }
        0xB4 => { let addr = self.zero_page_x(); self.ldy_zero_page_x(addr); pc_increment = 2; cycles = 4; }
        0xAC => { let addr = self.absolute(); self.ldy_absolute(addr); pc_increment = 3; cycles = 4; }
        0xBC => { let addr = self.absolute_x(); self.ldy_absolute_x(addr); pc_increment = 3; cycles = 4; }
        0x85 => { let addr = self.zero_page(); self.sta(addr); pc_increment = 2; cycles = 3; }
        0x8D => { let addr = self.absolute(); self.sta(addr); pc_increment = 3; cycles = 4; }
        0x95 => { let addr = self.zero_page_x(); self.sta(addr); pc_increment = 2; cycles = 4; }
        0x9D => { let addr = self.absolute_x(); self.sta(addr); pc_increment = 3; cycles = 5; }
        0x99 => { let addr = self.absolute_y(); self.sta(addr); pc_increment = 3; cycles = 5; }
        0x81 => { let addr = self.indirect_x(); self.sta(addr); pc_increment = 2; cycles = 6; }
        0x91 => { let addr = self.indirect_y(); self.sta(addr); pc_increment = 2; cycles = 6; }
        0x86 => { let addr = self.zero_page(); self.stx(addr); pc_increment = 2; cycles = 3; }
        0x96 => { let addr = self.zero_page_y(); self.stx(addr); pc_increment = 2; cycles = 4; }
        0x8E => { let addr = self.absolute(); self.stx(addr); pc_increment = 3; cycles = 4; }
        0x84 => { let addr = self.zero_page(); self.sty(addr); pc_increment = 2; cycles = 3; }
        0x94 => { let addr = self.zero_page_x(); self.sty(addr); pc_increment = 2; cycles = 4; }
        0x8C => { let addr = self.absolute(); self.sty(addr); pc_increment = 3; cycles = 4; }
        0xAA => { self.tax(); pc_increment = 1; cycles = 2; }
        0xA8 => { self.tay(); pc_increment = 1; cycles = 2; }
        0x8A => { self.txa(); pc_increment = 1; cycles = 2; }
        0x98 => { self.tya(); pc_increment = 1; cycles = 2; }
        0xBA => { self.tsx(); pc_increment = 1; cycles = 2; }
        0x9A => { self.txs(); pc_increment = 1; cycles = 2; }
        0x69 => { let val = self.immediate(); self.adc(val); pc_increment = 2; cycles = 2; }
        0x65 => { let addr = self.zero_page(); let val = self.read(addr); self.adc(val); pc_increment = 2; cycles = 3; }
        0x75 => { let addr = self.zero_page_x(); let val = self.read(addr); self.adc(val); pc_increment = 2; cycles = 4; }
        0x6D => { let addr = self.absolute(); let val = self.read(addr); self.adc(val); pc_increment = 3; cycles = 4; }
        0x7D => { let addr = self.absolute_x(); let val = self.read(addr); self.adc(val); pc_increment = 3; cycles = 4; }
        0x79 => { let addr = self.absolute_y(); let val = self.read(addr); self.adc(val); pc_increment = 3; cycles = 4; }
        0x61 => { let addr = self.indirect_x(); let val = self.read(addr); self.adc(val); pc_increment = 2; cycles = 6; }
        0x71 => { let addr = self.indirect_y(); let val = self.read(addr); self.adc(val); pc_increment = 2; cycles = 5; }
        0xE9 => { let val = self.immediate(); self.sbc(val); pc_increment = 2; cycles = 2; }
        0xE5 => { let addr = self.zero_page(); let val = self.read(addr); self.sbc(val); pc_increment = 2; cycles = 3; }
        0xF5 => { let addr = self.zero_page_x(); let val = self.read(addr); self.sbc(val); pc_increment = 2; cycles = 4; }
        0xED => { let addr = self.absolute(); let val = self.read(addr); self.sbc(val); pc_increment = 3; cycles = 4; }
        0xFD => { let addr = self.absolute_x(); let val = self.read(addr); self.sbc(val); pc_increment = 3; cycles = 4; }
        0xF9 => { let addr = self.absolute_y(); let val = self.read(addr); self.sbc(val); pc_increment = 3; cycles = 4; }
        0xE1 => { let addr = self.indirect_x(); let val = self.read(addr); self.sbc(val); pc_increment = 2; cycles = 6; }
        0xF1 => { let addr = self.indirect_y(); let val = self.read(addr); self.sbc(val); pc_increment = 2; cycles = 5; }
        0x48 => { self.pha(); pc_increment = 1; cycles = 3; }
        0x68 => { self.pla(); pc_increment = 1; cycles = 4; }
        0x08 => { self.php(); pc_increment = 1; cycles = 3; }
        0x28 => { self.plp(); pc_increment = 1; cycles = 4; }
        0xC6 => { let addr = self.zero_page(); self.dec(addr); pc_increment = 2; cycles = 5; }
        0xD6 => { let addr = self.zero_page_x(); self.dec(addr); pc_increment = 2; cycles = 6; }
        0xCE => { let addr = self.absolute(); self.dec(addr); pc_increment = 3; cycles = 6; }
        0xDE => { let addr = self.absolute_x(); self.dec(addr); pc_increment = 3; cycles = 7; }
        0xCA => { self.dex(); pc_increment = 1; cycles = 2; }
        0x88 => { self.dey(); pc_increment = 1; cycles = 2; }
        0xE6 => { let addr = self.zero_page(); self.inc(addr); pc_increment = 2; cycles = 5; }
        0xF6 => { let addr = self.zero_page_x(); self.inc(addr); pc_increment = 2; cycles = 6; }
        0xEE => { let addr = self.absolute(); self.inc(addr); pc_increment = 3; cycles = 6; }
        0xFE => { let addr = self.absolute_x(); self.inc(addr); pc_increment = 3; cycles = 7; }
        0xE8 => { self.inx(); pc_increment = 1; cycles = 2; }
        0xC8 => { self.iny(); pc_increment = 1; cycles = 2; }
        0x29 => { let val = self.immediate(); self.and(val); pc_increment = 2; cycles = 2; }
        0x25 => { let addr = self.zero_page(); let val = self.read(addr); self.and(val); pc_increment = 2; cycles = 3; }
        0x35 => { let addr = self.zero_page_x(); let val = self.read(addr); self.and(val); pc_increment = 2; cycles = 4; }
        0x2D => { let addr = self.absolute(); let val = self.read(addr); self.and(val); pc_increment = 3; cycles = 4; }
        0x3D => { let addr = self.absolute_x(); let val = self.read(addr); self.and(val); pc_increment = 3; cycles = 4; }
        0x39 => { let addr = self.absolute_y(); let val = self.read(addr); self.and(val); pc_increment = 3; cycles = 4; }
        0x21 => { let addr = self.indirect_x(); let val = self.read(addr); self.and(val); pc_increment = 2; cycles = 6; }
        0x31 => { let addr = self.indirect_y(); let val = self.read(addr); self.and(val); pc_increment = 2; cycles = 5; }
        0x49 => { let val = self.immediate(); self.eor(val); pc_increment = 2; cycles = 2; }
        0x45 => { let addr = self.zero_page(); let val = self.read(addr); self.eor(val); pc_increment = 2; cycles = 3; }
        0x55 => { let addr = self.zero_page_x(); let val = self.read(addr); self.eor(val); pc_increment = 2; cycles = 4; }
        0x4D => { let addr = self.absolute(); let val = self.read(addr); self.eor(val); pc_increment = 3; cycles = 4; }
        0x5D => { let addr = self.absolute_x(); let val = self.read(addr); self.eor(val); pc_increment = 3; cycles = 4; }
        0x59 => { let addr = self.absolute_y(); let val = self.read(addr); self.eor(val); pc_increment = 3; cycles = 4; }
        0x41 => { let addr = self.indirect_x(); let val = self.read(addr); self.eor(val); pc_increment = 2; cycles = 6; }
        0x51 => { let addr = self.indirect_y(); let val = self.read(addr); self.eor(val); pc_increment = 2; cycles = 5; }
        0x09 => { let val = self.immediate(); self.ora(val); pc_increment = 2; cycles = 2; }
        0x05 => { let addr = self.zero_page(); let val = self.read(addr); self.ora(val); pc_increment = 2; cycles = 3; }
        0x15 => { let addr = self.zero_page_x(); let val = self.read(addr); self.ora(val); pc_increment = 2; cycles = 4; }
        0x0D => { let addr = self.absolute(); let val = self.read(addr); self.ora(val); pc_increment = 3; cycles = 4; }
        0x1D => { let addr = self.absolute_x(); let val = self.read(addr); self.ora(val); pc_increment = 3; cycles = 4; }
        0x19 => { let addr = self.absolute_y(); let val = self.read(addr); self.ora(val); pc_increment = 3; cycles = 4; }
        0x01 => { let addr = self.indirect_x(); let val = self.read(addr); self.ora(val); pc_increment = 2; cycles = 6; }
        0x11 => { let addr = self.indirect_y(); let val = self.read(addr); self.ora(val); pc_increment = 2; cycles = 5; }
        0x0A => { self.asl_acc(); pc_increment = 1; cycles = 2; }
        0x06 => { let addr = self.zero_page(); self.asl(addr); pc_increment = 2; cycles = 5; }
        0x16 => { let addr = self.zero_page_x(); self.asl(addr); pc_increment = 2; cycles = 6; }
        0x0E => { let addr = self.absolute(); self.asl(addr); pc_increment = 3; cycles = 6; }
        0x1E => { let addr = self.absolute_x(); self.asl(addr); pc_increment = 3; cycles = 7; }
        0x4A => { self.lsr_acc(); pc_increment = 1; cycles = 2; }
        0x46 => { let addr = self.zero_page(); self.lsr(addr); pc_increment = 2; cycles = 5; }
        0x56 => { let addr = self.zero_page_x(); self.lsr(addr); pc_increment = 2; cycles = 6; }
        0x4E => { let addr = self.absolute(); self.lsr(addr); pc_increment = 3; cycles = 6; }
        0x5E => { let addr = self.absolute_x(); self.lsr(addr); pc_increment = 3; cycles = 7; }
        0x2A => { self.rol_acc(); pc_increment = 1; cycles = 2; }
        0x26 => { let addr = self.zero_page(); self.rol(addr); pc_increment = 2; cycles = 5; }
        0x36 => { let addr = self.zero_page_x(); self.rol(addr); pc_increment = 2; cycles = 6; }
        0x2E => { let addr = self.absolute(); self.rol(addr); pc_increment = 3; cycles = 6; }
        0x3E => { let addr = self.absolute_x(); self.rol(addr); pc_increment = 3; cycles = 7; }
        0x6A => { self.ror_acc(); pc_increment = 1; cycles = 2; }
        0x66 => { let addr = self.zero_page(); self.ror(addr); pc_increment = 2; cycles = 5; }
        0x76 => { let addr = self.zero_page_x(); self.ror(addr); pc_increment = 2; cycles = 6; }
        0x6E => { let addr = self.absolute(); self.ror(addr); pc_increment = 3; cycles = 6; }
        0x7E => { let addr = self.absolute_x(); self.ror(addr); pc_increment = 3; cycles = 7; }
        0x18 => { self.clc(); pc_increment = 1; cycles = 2; }
        0x58 => { self.cli(); pc_increment = 1; cycles = 2; }
        0xB8 => { self.clv(); pc_increment = 1; cycles = 2; }
        0x38 => { self.sec(); pc_increment = 1; cycles = 2; }
        0x78 => { self.sei(); pc_increment = 1; cycles = 2; }
        0xF8 => { self.sed(); pc_increment = 1; cycles = 2; }
        0xD8 => { self.cld(); pc_increment = 1; cycles = 2; }
        0xC9 => { let val = self.immediate(); self.cmp(val); pc_increment = 2; cycles = 2; }
        0xC5 => { let addr = self.zero_page(); let val = self.read(addr); self.cmp(val); pc_increment = 2; cycles = 3; }
        0xD5 => { let addr = self.zero_page_x(); let val = self.read(addr); self.cmp(val); pc_increment = 2; cycles = 4; }
        0xCD => { let addr = self.absolute(); let val = self.read(addr); self.cmp(val); pc_increment = 3; cycles = 4; }
        0xDD => { let addr = self.absolute_x(); let val = self.read(addr); self.cmp(val); pc_increment = 3; cycles = 4; }
        0xD9 => { let addr = self.absolute_y(); let val = self.read(addr); self.cmp(val); pc_increment = 3; cycles = 4; }
        0xC1 => { let addr = self.indirect_x(); let val = self.read(addr); self.cmp(val); pc_increment = 2; cycles = 6; }
        0xD1 => { let addr = self.indirect_y(); let val = self.read(addr); self.cmp(val); pc_increment = 2; cycles = 5; }
        0xE0 => { let val = self.immediate(); self.cpx(val); pc_increment = 2; cycles = 2; }
        0xE4 => { let addr = self.zero_page(); let val = self.read(addr); self.cpx(val); pc_increment = 2; cycles = 3; }
        0xEC => { let addr = self.absolute(); let val = self.read(addr); self.cpx(val); pc_increment = 3; cycles = 4; }
        0xC0 => { let val = self.immediate(); self.cpy(val); pc_increment = 2; cycles = 2; }
        0xC4 => { let addr = self.zero_page(); let val = self.read(addr); self.cpy(val); pc_increment = 2; cycles = 3; }
        0xCC => { let addr = self.absolute(); let val = self.read(addr); self.cpy(val); pc_increment = 3; cycles = 4; }
        0x90 => { let offset = self.immediate(); pc_increment = 2; cycles = 2 + self.bcc(offset); }
        0xB0 => { let offset = self.immediate(); pc_increment = 2; cycles = 2 + self.bcs(offset); }
        0xF0 => { let offset = self.immediate(); pc_increment = 2; cycles = 2 + self.beq(offset); }
        0x30 => { let offset = self.immediate(); pc_increment = 2; cycles = 2 + self.bmi(offset); }
        0xD0 => { let offset = self.immediate(); pc_increment = 2; cycles = 2 + self.bne(offset); }
        0x10 => { let offset = self.immediate(); pc_increment = 2; cycles = 2 + self.bpl(offset); }
        0x50 => { let offset = self.immediate(); pc_increment = 2; cycles = 2 + self.bvc(offset); }
        0x70 => { let offset = self.immediate(); pc_increment = 2; cycles = 2 + self.bvs(offset); }
        0x4C => { let addr = self.absolute(); self.jmp(addr); pc_increment = 3; cycles = 3; }
        0x6C => { let addr = self.indirect(); self.jmp(addr); pc_increment = 3; cycles = 5; }
        0x20 => { let addr = self.absolute(); self.jsr(addr); pc_increment = 3; cycles = 6; }
        0x00 => { self.brk(); pc_increment = 2; cycles = 7; }
        0x40 => { self.rti(); pc_increment = 1; cycles = 6; }
        0x60 => { self.rts(); pc_increment = 1; cycles = 6; }
        0x24 => { let addr = self.zero_page(); let val = self.read(addr); self.bit(val); pc_increment = 2; cycles = 3; }
        0x2C => { let addr = self.absolute(); let val = self.read(addr); self.bit(val); pc_increment = 3; cycles = 4; }
        0xEA => { self.nop(); pc_increment = 1; cycles = 2; }
        _ => { println!("Unimplemented opcode: {:#04x} at PC: {:#06x}", opcode, self.pc); pc_increment = 1; cycles = 2; }
    };
    self.pc = self.pc.wrapping_add(pc_increment);
    if self.ppu.step(cycles as usize) {
        self.nmi();
        println!("PPU NMI triggered, new PC: {:#06x}", self.pc);
    }
}

    pub fn run(&mut self, cycles: usize) {
        for _ in 0..cycles {
            self.step();
        }
    }

    pub fn load_program(&mut self, program: &[u8], start_addr: u16) {
        for (i, &byte) in program.iter().enumerate() {
            self.write(start_addr + i as u16, byte);
        }
        self.pc = start_addr;
    }
}
