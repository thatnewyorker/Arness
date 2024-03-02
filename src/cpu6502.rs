// Define the CPU module
pub struct Cpu6502 {
    // Registers
    pub a: u8, // Accumulator
    pub x: u8, // X Index Register
    pub y: u8, // Y Index Register
    pub sp: u8, // Stack Pointer
    pub pc: u16, // Program Counter
    pub status: u8, // Status Register

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
            sp: 0xFD, // Initialized to 0xFD as per 6502's power-up state
            pc: 0x8000, // Commonly used starting address for programs
            status: 0x24, // Default status flags
            memory: [0; 65536],
        }
    }

// Reset the CPU
// The reset vector is read from memory location 0xFFFC
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


// Aritmetic instructions

// Add with carry
pub fn adc(&mut self, value: u8) {
        let result = self.a as u16 + value as u16 + (self.status & 0b0000_0001) as u16;
        self.status &= !0b0000_0001; // Clear carry flag
        if result > 0xFF {
            self.status |= 0b0000_0001; // Set carry flag
        }
        self.a = result as u8;
        self.update_zero_and_negative_flags(self.a);
    }

// Subtract with carry
pub fn sbc(&mut self, value: u8) {
        let value = value ^ 0xFF; // Two's complement
        let result = self.a as u16 + value as u16 + (self.status & 0b0000_0001) as u16;
        self.status &= !0b0000_0001; // Clear carry flag
        if result > 0xFF {
            self.status |= 0b0000_0001; // Set carry flag
        }
        self.a = result as u8;
        self.update_zero_and_negative_flags(self.a);
    }


// Update the zero and negative flags based on the result
pub fn update_zero_and_negative_flags(&mut self, result: u8) {
        if result == 0 {
            self.status |= 0b0000_0010; // Set zero flag
        } else {
            self.status &= !0b0000_0010; // Clear zero flag
        }

        if result & 0b1000_0000 != 0 {
            self.status |= 0b1000_0000; // Set negative flag
        } else {
            self.status &= !0b1000_0000; // Clear negative flag
        }
    }


// Stack Instructions
// These instructions transfer data between the stack and the accumulator or the index registers
pub fn pha(&mut self) {
        self.push(self.a);
    }

pub fn pla(&mut self) {
        self.a = self.pop();
        self.update_zero_and_negative_flags(self.a);
    }

pub fn php(&mut self) {
        self.push(self.status | 0b0011_0000); // Set the break and unused flags
    }

pub fn plp(&mut self) {
        self.status = self.pop() | 0b0011_0000; // Set the break and unused flags
    }

// These functions are used to read and write to memory

// Read a byte from memory
pub fn read(&self, addr: u16) -> u8 { 
        self.memory[addr as usize] 
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

// Push a byte to the stack
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

}

impl Cpu6502 {

// Decrements and Increments
// These instructions are used to decrement or increment the value of a memory location or a register

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
// These instructions perform logical operations on the accumulator and memory

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
        if value & 0b1000_0000 != 0 {
            self.status |= 0b0000_0001; // Set carry flag
        } else {
            self.status &= !0b0000_0001; // Clear carry flag
        }
    }

// Logical shift right
pub fn lsr(&mut self, addr: u16) {
        let value = self.read(addr);
        let result = value >> 1;
        self.write(addr, result);
        self.update_zero_and_negative_flags(result);
        if value & 0b0000_0001 != 0 {
            self.status |= 0b0000_0001;
        } else {
            self.status &= !0b0000_0001; 
        }
    }

// Rotate left
// The carry flag is shifted into bit 0 and bit 7 is shifted into the carry flag
pub fn rol(&mut self, addr: u16) {
        let value = self.read(addr);
        let carry = self.status & 0b0000_0001;
        let result = (value << 1) | carry;
        self.write(addr, result);
        self.update_zero_and_negative_flags(result);
        if value & 0b1000_0000 != 0 {
            self.status |= 0b0000_0001;
        } else {
            self.status &= !0b0000_0001;
        }
    }

// Rotate right
// The carry flag is shifted into bit 7 and bit 0 is shifted into the carry flag
pub fn ror(&mut self, addr: u16) {
        let value = self.read(addr);
        let carry = self.status & 0b0000_0001;
        let result = (value >> 1) | (carry << 7);
        self.write(addr, result);
        self.update_zero_and_negative_flags(result);
        if value & 0b0000_0001 != 0 {
            self.status |= 0b0000_0001; 
        } else {
            self.status &= !0b0000_0001; 
        }
    }

// Flag operations

// Clear carry flag
pub fn clc(&mut self) {
        self.status &= !0b0000_0001;
    }

// Clear decimal mode
pub fn cld(&mut self) {
        self.status &= !0b0000_1000;
    }

// Clear interrupt disable
pub fn cli(&mut self) {
        self.status &= !0b0000_0100;
    }

// Clear overflow flag
pub fn clv(&mut self) {
        self.status &= !0b0100_0000;
    }

// Set carry flag
pub fn sec(&mut self) {
        self.status |= 0b0000_0001;
    }

// Set decimal mode
pub fn sed(&mut self) {
        self.status |= 0b0000_1000;
    }

// Set interrupt disable
pub fn sei(&mut self) {
        self.status |= 0b0000_0100;
    }

// Comparison instructions
// These instructions compare the value in the accumulator with another value and set the zero and negative flags based on the result
// They also set the carry flag if the comparison is true

// Compare the accumulator with a value
pub fn cmp(&mut self, value: u8) {
        if self.a >= value {
            self.status |= 0b0000_0001; // Set carry flag
        } else {
            self.status &= !0b0000_0001; // Clear carry flag
        }
        let result = self.a.wrapping_sub(value);
        self.update_zero_and_negative_flags(result);
    }

// Compare the X register with a value
pub fn cpx(&mut self, value: u8) {
        if self.x >= value {
            self.status |= 0b0000_0001; // Set carry flag
        } else {
            self.status &= !0b0000_0001; // Clear carry flag
        }
        let result = self.x.wrapping_sub(value);
        self.update_zero_and_negative_flags(result);
    }

// Compare the Y register with a value
pub fn cpy(&mut self, value: u8) {
        if self.y >= value {
            self.status |= 0b0000_0001; // Set carry flag
        } else {
            self.status &= !0b0000_0001; // Clear carry flag
        }
        let result = self.y.wrapping_sub(value);
        self.update_zero_and_negative_flags(result);
        }
}


// Conditional Branches
// These instructions are used to change the flow of the program based on the status of the status register
// They are used to implement loops, if-else statements, and other control structures

impl Cpu6502 {

pub fn branch(&mut self, offset: u8) {
        let offset = offset as i8;
        let pc = self.pc as i16;
        self.pc = (pc + offset as i16) as u16;
    }

pub fn bcc(&mut self, offset: u8) {
        if self.status & 0b0000_0001 == 0 {
            self.branch(offset);
        }
    }

pub fn bcs(&mut self, offset: u8) {
        if self.status & 0b0000_0001 != 0 {
            self.branch(offset);
        }
    }

pub fn beq(&mut self, offset: u8) {
        if self.status & 0b0000_0010 != 0 {
            self.branch(offset);
        }
    }

pub fn bmi(&mut self, offset: u8) {
        if self.status & 0b1000_0000 != 0 {
            self.branch(offset);
        }
    }

pub fn bne(&mut self, offset: u8) {
        if self.status & 0b0000_0010 == 0 {
            self.branch(offset);
        }
    }

pub fn bpl(&mut self, offset: u8) {
        if self.status & 0b1000_0000 == 0 {
            self.branch(offset);
        }
    }

pub fn bvc(&mut self, offset: u8) {
        if self.status & 0b0100_0000 == 0 {
            self.branch(offset);
        }
    }

pub fn bvs(&mut self, offset: u8) {
        if self.status & 0b0100_0000 != 0 {
            self.branch(offset);
        }
    }

// Jumps and Subroutines
// These instructions are used to change the flow of the program by jumping to a different location in memory

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
// The BRK instruction is used to generate an IRQ
// The RTI instruction is used to return from an interrupt
// The NMI instruction is used to generate a non-maskable interrupt
// The IRQ instruction is used to generate an IRQ

pub fn brk(&mut self) {
        self.push_word(self.pc);
        self.php();
        self.sei();
        self.pc = self.read_word(0xFFFE)
    }

pub fn rti(&mut self) {
        self.pull_status();
        self.pc = self.pop_word();
    }

pub fn nmi(&mut self) {
        self.push_word(self.pc);
        self.php();
        self.sei();
        self.pc = self.read_word(0xFFFA);
    }

pub fn irq(&mut self) {
        self.push_word(self.pc);
        self.php();
        self.sei();
        self.pc = self.read_word(0xFFFE);
    }

// These instructions perform bitwise operations on the accumulator and memory
// The bit instruction is used to test a bit in memory
pub fn bit(&mut self, value: u8) {
        if self.a & value == 0 {
            self.status |= 0b0000_0010; // Set zero flag
        } else {
            self.status &= !0b0000_0010; // Clear zero flag
        }
        if value & 0b1000_0000 != 0 {
            self.status |= 0b1000_0000; // Set negative flag
        } else {
            self.status &= !0b1000_0000; // Clear negative flag
        }
        if value & 0b0100_0000 != 0 {
            self.status |= 0b0100_0000; // Set overflow flag
        } else {
            self.status &= !0b0100_0000; // Clear overflow flag
        }
    }

pub fn nop(&mut self) {
        // Do nothing
    }
}
