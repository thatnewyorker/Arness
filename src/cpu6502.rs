mod ppu; // Import the PPU module
use ppu::Ppu;

// Define the status flags
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
    pub ppu: Ppu, // PPU instance
}

impl Cpu6502 {
    pub fn new() -> Self {
        Cpu6502 {
            a: 0,
            x: 0,
            y: 0,
            sp: 0xFD,
            pc: 0x8000,
            status: 0x24,
            memory: [0; 65536],
            ppu: Ppu::new(),
        }
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

    // Addressing modes
    fn immediate(&mut self) -> u8 {
        let value = self.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        value
    }

    fn absolute(&mut self) -> u16 {
        let addr = self.read_word(self.pc);
        self.pc = self.pc.wrapping_add(2);
        addr
    }

    fn zero_page(&mut self) -> u16 {
        let addr = self.read(self.pc) as u16;
        self.pc = self.pc.wrapping_add(1);
        addr
    }

    // Load instructions
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

    pub fn ldx_immediate(&mut self, value: u8) {
        self.x = value;
        self.update_zero_and_negative_flags(self.x);
    }

    pub fn ldy_immediate(&mut self, value: u8) {
        self.y = value;
        self.update_zero_and_negative_flags(self.y);
    }

    // Store instructions
    pub fn sta(&mut self, addr: u16) {
        self.write(addr, self.a);
    }

    pub fn stx(&mut self, addr: u16) {
        self.write(addr, self.x);
    }

    pub fn sty(&mut self, addr: u16) {
        self.write(addr, self.y);
    }

    // Transfer instructions
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

    // Arithmetic instructions
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

    // Stack instructions
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

    // Memory access
    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0x2000..=0x2007 => self.ppu.read(addr),
            _ => *self.memory.get(addr as usize).unwrap_or(&0),
        }
    }

    pub fn write(&mut self, addr: u16, data: u8) {
        match addr {
            0x2000..=0x2007 => self.ppu.write(addr, data),
            _ => self.memory[addr as usize] = data,
        }
    }

    pub fn read_word(&self, addr: u16) -> u16 {
        let lo = self.read(addr) as u16;
        let hi = self.read(addr + 1) as u16;
        (hi << 8) | lo
    }

    pub fn write_word(&mut self, addr: u16, data: u16) {
        let lo = data as u8;
        let hi = (data >> 8) as u8;
        self.write(addr, lo);
        self.write(addr + 1, hi);
    }

    // Stack operations
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

    // Increment and decrement
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

    // Logical instructions
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

    // Shifts and rotates
    pub fn asl(&mut self, addr: u16) {
        let value = self.read(addr);
        let result = value << 1;
        self.write(addr, result);
        self.update_zero_and_negative_flags(result);
        if value & NEGATIVE != 0 {
            self.set_status_flag(CARRY);
        } else {
            self.clear_status_flag(CARRY);
        }
    }

    pub fn lsr(&mut self, addr: u16) {
        let value = self.read(addr);
        let result = value >> 1;
        self.write(addr, result);
        self.update_zero_and_negative_flags(result);
        if value & CARRY != 0 {
            self.set_status_flag(CARRY);
        } else {
            self.clear_status_flag(CARRY);
        }
    }

    pub fn rol(&mut self, addr: u16) {
        let value = self.read(addr);
        let carry = self.status & CARRY;
        let result = (value << 1) | carry;
        self.write(addr, result);
        self.update_zero_and_negative_flags(result);
        if value & NEGATIVE != 0 {
            self.set_status_flag(CARRY);
        } else {
            self.clear_status_flag(CARRY);
        }
    }

    pub fn ror(&mut self, addr: u16) {
        let value = self.read(addr);
        let carry = self.status & CARRY;
        let result = (value >> 1) | (carry << 7);
        self.write(addr, result);
        self.update_zero_and_negative_flags(result);
        if value & CARRY != 0 {
            self.set_status_flag(CARRY);
        } else {
            self.clear_status_flag(CARRY);
        }
    }

    // Flag operations
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

    // Comparison instructions
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

    // Branches
    pub fn branch(&mut self, offset: u8) {
        let offset = offset as i8 as i16;
        self.pc = self.pc.wrapping_add(offset as u16);
    }

    pub fn bcc(&mut self, offset: u8) {
        if self.status & CARRY == 0 {
            self.branch(offset);
        }
    }

    pub fn bcs(&mut self, offset: u8) {
        if self.status & CARRY != 0 {
            self.branch(offset);
        }
    }

    pub fn beq(&mut self, offset: u8) {
        if self.status & ZERO != 0 {
            self.branch(offset);
        }
    }

    pub fn bmi(&mut self, offset: u8) {
        if self.status & NEGATIVE != 0 {
            self.branch(offset);
        }
    }

    pub fn bne(&mut self, offset: u8) {
        if self.status & ZERO == 0 {
            self.branch(offset);
        }
    }

    pub fn bpl(&mut self, offset: u8) {
        if self.status & NEGATIVE == 0 {
            self.branch(offset);
        }
    }

    pub fn bvc(&mut self, offset: u8) {
        if self.status & OVERFLOW == 0 {
            self.branch(offset);
        }
    }

    pub fn bvs(&mut self, offset: u8) {
        if self.status & OVERFLOW != 0 {
            self.branch(offset);
        }
    }

    // Jumps and subroutines
    pub fn jmp(&mut self, addr: u16) {
        self.pc = addr;
    }

    pub fn jsr(&mut self, addr: u16) {
        let return_addr = self.pc - 1;
        self.push_word(return_addr);
        self.pc = addr;
    }

    pub fn rts(&mut self) {
        self.pc = self.pop_word() + 1;
    }

    // Interrupts
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

    // Bit test
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
        let opcode = self.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        match opcode {
            0xA9 => {
                let value = self.immediate();
                self.lda_immediate(value);
            }
            0xAD => {
                let addr = self.absolute();
                self.lda_absolute(addr);
            }
            0xA5 => {
                let addr = self.zero_page();
                self.lda_zero_page(addr);
            }
            0x69 => {
                let value = self.immediate();
                self.adc(value);
            }
            0x6D => {
                let addr = self.absolute();
                let value = self.read(addr);
                self.adc(value);
            }
            0x8D => {
                let addr = self.absolute();
                self.sta(addr);
            }
            0x85 => {
                let addr = self.zero_page();
                self.sta(addr);
            }
            0x4C => {
                let addr = self.absolute();
                self.jmp(addr);
            }
            0x00 => self.brk(),
            0xEA => self.nop(),
            _ => println!("Unimplemented opcode: {:#04x} at PC: {:#06x}", opcode, self.pc - 1),
        }
        self.ppu.step(1);
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
