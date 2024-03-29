// Define the status flags
const CARRY: u8 = 0b0000_0001;
const ZERO: u8 = 0b0000_0010;
const INTERRUPT_DISABLE: u8 = 0b0000_0100;
const OVERFLOW: u8 = 0b0100_0000;
const NEGATIVE: u8 = 0b1000_0000;

// Define the CPU module and its implementation
pub struct Cpu6502 {
    // Registers
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub sp: u8,
    pub pc: u16,
    pub status: u8,

    // Memory (64KB)
    pub memory: [u8; 65536],
}

// Implementation of the CPU
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
        }
    }

    // Set a status flag
    fn set_status_flag(&mut self, flag: u8) {
        self.status |= flag;
    }

    // Clear a status flag
    fn clear_status_flag(&mut self, flag: u8) {
        self.status &= flag ^ 0xFF;
    }

    // Check if a status flag is set
    fn is_status_flag_set(&self, flag: u8) -> bool {
        self.status & flag != 0
    }

    // Update the zero and negative flags based on the result
    fn update_zero_and_negative_flags(&mut self, result: u8) {
        if result == 0 {
            self.set_status_flag(ZERO);
        } else {
            self.clear_status_flag(ZERO);
        }

        // Set or clear the negative flag by checking the most significant bit of the result
        if result & NEGATIVE != 0 {
            self.set_status_flag(NEGATIVE);
        } else {
            self.clear_status_flag(NEGATIVE);
        }
    }

    // Load the accumulator with a value
    pub fn lda_immediate(&mut self, value: u8) {
        self.a = value;
        self.update_zero_and_negative_flags(self.a);
    }

    // Load the X register with a value
    pub fn ldx_immediate(&mut self, value: u8) {
        self.x = value;
        self.update_zero_and_negative_flags(self.x);
    }

    // Load the Y register with a value
    pub fn ldy_immediate(&mut self, value: u8) {
        self.y = value;
        self.update_zero_and_negative_flags(self.y);
    }

    // Store the accumulator in memory
    pub fn sta(&mut self, addr: u16) {
        self.write(addr, self.a);
    }

    // Store the X register in memory
    pub fn stx(&mut self, addr: u16) {
        self.write(addr, self.x);
    }

    // Store the Y register in memory
    pub fn sty(&mut self, addr: u16) {
        self.write(addr, self.y);
    }

    // Transfer the accumulator to the X register
    pub fn tax(&mut self) {
        self.x = self.a;
        self.update_zero_and_negative_flags(self.x);
    }

    // Transfer the accumulator to the Y register
    pub fn tay(&mut self) {
        self.y = self.a;
        self.update_zero_and_negative_flags(self.y);
    }

    // Transfer the X register to the accumulator
    pub fn txa(&mut self) {
        self.a = self.x;
        self.update_zero_and_negative_flags(self.a);
    }

    // Transfer the Y register to the accumulator
    pub fn tya(&mut self) {
        self.a = self.y;
        self.update_zero_and_negative_flags(self.a);
    }

    // Transfer the stack pointer to the X register
    pub fn tsx(&mut self) {
        self.x = self.sp;
        self.update_zero_and_negative_flags(self.x);
    }

    // Transfer the X register to the stack pointer
    pub fn txs(&mut self) {
        self.sp = self.x;
    }

    // Arithmetic instructions
    // Add with CARRY
    pub fn adc(&mut self, value: u8) {
        let result = self.a as u16 + value as u16 + (self.status & CARRY) as u16;
        self.clear_status_flag(CARRY);
        if result > 0xFF {
            self.set_status_flag(CARRY);
        }
        self.a = result as u8;
        self.update_zero_and_negative_flags(self.a);
    }

    // Subtract with CARRY
    pub fn sbc(&mut self, value: u8) {
        let value = value ^ 0xFF;
        let result = self.a as u16 + value as u16 + (self.status & CARRY) as u16;
        self.clear_status_flag(CARRY);
        if result > 0xFF {
            self.set_status_flag(CARRY);
        }
        self.a = result as u8;
        self.update_zero_and_negative_flags(self.a);
    }

    // Stack Instructions
    // Push a byte to the stack
    pub fn pha(&mut self) {
        self.push(self.a);
    }

    //
    pub fn pla(&mut self) {
        self.a = self.pop();
        self.update_zero_and_negative_flags(self.a);
    }

    pub fn php(&mut self) {
        let status_with_b_and_u_flags = self.status | 0b0011_0000;
        // bit 4 and 5 set
        self.push(status_with_b_and_u_flags);
    }

    // Pull the status register from the stack
    pub fn plp(&mut self) {
        let pulled_status = self.pop();
        let unused_flag_mask = !0b0010_0000;
        self.status = (self.status & unused_flag_mask) | (pulled_status & !unused_flag_mask);
    }

    // These functions are used to read and write to memory
    //  Read a byte from memory
    pub fn read(&self, addr: u16) -> u8 {
        *self.memory.get(addr as usize).unwrap_or(&0)
    }

    // Write a byte to memory
    pub fn write(&mut self, addr: u16, data: u8) {
        self.memory[addr as usize] = data;
    }

    // Read a 16-bit word from memory
    pub fn read_word(&self, addr: u16) -> u16 {
        let lo = self.read(addr) as u16;
        let hi = self.read(addr + 1) as u16;
        (hi << 8) | lo
    }

    // Write a 16-bit word to memory
    pub fn write_word(&mut self, addr: u16, data: u16) {
        let lo = data as u8;
        let hi = (data >> 8) as u8;
        self.write(addr, lo);
        self.write(addr + 1, hi);
    }

    // Stack operations (the stack is located at 0x0100-0x01FF)
    pub fn push(&mut self, data: u8) {
        self.write(0x0100 + self.sp as u16, data);
        self.sp = self.sp.wrapping_sub(1);
    }

    // Pop a byte from the stack
    pub fn pop(&mut self) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        self.read(0x0100 + self.sp as u16)
    }

    // Push a 16-bit word to the stack
    pub fn push_word(&mut self, data: u16) {
        self.push((data >> 8) as u8);
        self.push(data as u8);
    }

    // Pop a 16-bit word from the stack
    pub fn pop_word(&mut self) -> u16 {
        let lo = self.pop() as u16;
        let hi = self.pop() as u16;
        (hi << 8) | lo
    }

    // Status register operations
    pub fn pull_status(&mut self) {
        self.status = self.pop();
    }

    // Increment and Decrement
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

    // Shifts and Rotates
    // Arithmetic shift left
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

    // Logical shift right
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

    // Rotate left
    // The CARRY flag is shifted into bit 0 and bit 7 is shifted into the CARRY flag
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

    // Rotate right
    // The CARRY flag is shifted into bit 7 and bit 0 is shifted into the CARRY flag
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
    // Clear CARRY flag
    pub fn clc(&mut self) {
        self.clear_status_flag(CARRY);
    }

    // Clear INTERRUPT_DISABLE flag
    pub fn cli(&mut self) {
        self.clear_status_flag(INTERRUPT_DISABLE);
    }

    // Clear OVERFLOW flag
    pub fn clv(&mut self) {
        self.clear_status_flag(OVERFLOW);
    }

    // Set CARRY flag to enable the CARRY
    pub fn sec(&mut self) {
        self.set_status_flag(CARRY);
    }

    // Set INTERRUPT_DISABLE flag to disable the maskable interrupt lin
    pub fn sei(&mut self) {
        self.status |= INTERRUPT_DISABLE;
    }

    // Comparison instructions
    // Compare the accumulator with a value
    pub fn cmp(&mut self, value: u8) {
        if self.a >= value {
            self.set_status_flag(CARRY);
        } else {
            self.clear_status_flag(CARRY);
        }
        let result = self.a.wrapping_sub(value);
        self.update_zero_and_negative_flags(result);
    }

    // Compare the X register with a value
    pub fn cpx(&mut self, value: u8) {
        if self.x >= value {
            self.set_status_flag(CARRY);
        } else {
            self.clear_status_flag(CARRY);
        }
        let result = self.x.wrapping_sub(value);
        self.update_zero_and_negative_flags(result);
    }

    // Compare the Y register with a value
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
        self.pc = self.pc.wrapping_add(offset as u16)
    }

    // Branch on carry clear
    pub fn bcc(&mut self, offset: u8) {
        if self.status & CARRY == 0 {
            self.branch(offset);
        }
    }

    // Branch on carry set
    pub fn bcs(&mut self, offset: u8) {
        if self.status & CARRY != 0 {
            self.branch(offset);
        }
    }

    // Branch on equal
    pub fn beq(&mut self, offset: u8) {
        if self.status & ZERO != 0 {
            self.branch(offset);
        }
    }

    // Branch on minus
    pub fn bmi(&mut self, offset: u8) {
        if self.status & NEGATIVE != 0 {
            self.branch(offset);
        }
    }

    // Branch on not equal
    pub fn bne(&mut self, offset: u8) {
        if self.status & ZERO == 0 {
            self.branch(offset);
        }
    }

    // Branch on plus
    pub fn bpl(&mut self, offset: u8) {
        if self.status & NEGATIVE == 0 {
            self.branch(offset);
        }
    }

    // Branch on overflow clear
    pub fn bvc(&mut self, offset: u8) {
        if self.status & OVERFLOW == 0 {
            self.branch(offset);
        }
    }

    // Branch on overflow set
    pub fn bvs(&mut self, offset: u8) {
        if self.status & OVERFLOW != 0 {
            self.branch(offset);
        }
    }

    // Jumps and Subroutines
    pub fn jmp(&mut self, addr: u16) {
        self.pc = addr;
    }

    // Jump to subroutine
    pub fn jsr(&mut self, addr: u16) {
        let return_addr = self.pc - 1;
        self.push_word(return_addr);
        self.pc = addr;
    }

    // Return from subroutine
    pub fn rts(&mut self) {
        self.pc = self.pop_word() + 1;
    }

    // Interrupts
    pub fn brk(&mut self) {
        self.push_word(self.pc);
        self.php();
        self.sei();
        self.pc = self.read_word(0xFFFE)
    }

    // Return from interrupt
    pub fn rti(&mut self) {
        self.pull_status();
        self.pc = self.pop_word();
    }

    // Non-Maskable Interrupt
    pub fn nmi(&mut self) {
        self.push_word(self.pc);
        self.php();
        self.sei();
        self.pc = self.read_word(0xFFFA);
    }

    // Interrupt Request
    pub fn irq(&mut self) {
        self.push_word(self.pc);
        self.php();
        self.sei();
        self.pc = self.read_word(0xFFFE);
    }

    // These instructions perform bitwise operations on the accumulator and memory
    // AND memory with accumulator
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
    // No operation
    pub fn nop(&mut self) {
        // Do nothing
    }
}
