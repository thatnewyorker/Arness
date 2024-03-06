// Define the status flags
const CARRY: u8 = 0b0000_0001;
const ZERO: u8 = 0b0000_0010;
const INTERRUPT_DISABLE: u8 = 0b0000_0100;
// const DECIMAL_MODE: u8 = 0b0000_1000; // Unused in NES
// const BREAK_COMMAND: u8 = 0b0001_0000; // Unused in NES
// const ONE: u8 = 0b0010_0000; // Unused in NES
const OVERFLOW: u8 = 0b0100_0000;
const NEGATIVE: u8 = 0b1000_0000;

// Define the CPU module and its implementation
pub struct Cpu6502 {
    // Registers
    pub a: u8, // Accumulator
    pub x: u8, // X Index Register
    pub y: u8, // Y Index Register
    pub sp: u8, // Stack Pointer
    pub pc: u16, // Program Counter
    pub status: u8, // Status Register

    // Memory (64KB)
    pub memory: Vec<u8>,
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
            memory: vec![0; 65536],
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
}

impl Cpu6502 {

// Arithmetic instructions
// Add with CARRY
pub fn adc(&mut self, value: u8) {
        let result = self.a as u16 + value as u16 + (self.status & CARRY) as u16;
        self.status &= !CARRY; // Clear CARRY flag
        self.status &= 0b1111_1110;
        if result > 0xFF {
            self.status |= CARRY; // Set CARRY flag
        }
        self.a = result as u8;
        self.update_zero_and_negative_flags(self.a);
    }

// Subtract with CARRY
pub fn sbc(&mut self, value: u8) {
        let value = value ^ 0xFF; // Two's complement
        let result = self.a as u16 + value as u16 + (self.status & CARRY) as u16;
        self.status &= !CARRY; // Clear CARRY flag
        self.status &= 0b1111_1110;
        if result > 0xFF {
            self.status |= CARRY; // Set CARRY flag
        }
        self.a = result as u8;
        self.update_zero_and_negative_flags(self.a);
    }

// Update the zero and negative flags based on the result
pub fn update_zero_and_negative_flags(&mut self, result: u8) {
        if result == 0 {
            self.status |= ZERO; // Set zero flag
        } else {
            self.status &= !ZERO; // Clear zero flag
            self.status &= 0b1111_1101;
        }

        if result & NEGATIVE != 0 {
            self.status |= NEGATIVE; // Set negative flag
        } else {
            self.status &= !NEGATIVE; // Clear negative flag
            self.status &= 0b0111_1111;
        }
    }
}

impl Cpu6502 {

// Stack Instructions
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
}

impl Cpu6502 {

// These functions are used to read and write to memory
//  Read a byte from memory
pub fn read(&self, addr: u16) -> u8 {
        *self.memory.get(addr as usize).unwrap_or(&0)
    }

// Write a byte to memory
pub fn write(&mut self, addr: u16, value: u8) {
        if addr as usize >= self.memory.len() {
            self.memory.resize(addr as usize + 1, 0);
    }
    self.memory[addr as usize] = value;
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
}

impl Cpu6502 {

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

}

impl Cpu6502 {

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
}

impl Cpu6502 {

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
}

impl Cpu6502 {

// Shifts and Rotates
// Arithmetic shift left
pub fn asl(&mut self, addr: u16) {
        let value = self.read(addr);
        let result = value << 1;
        self.write(addr, result);
        self.update_zero_and_negative_flags(result);
        if value & NEGATIVE != 0 {
            self.status |= CARRY; // Set CARRY flag
        } else {
            self.status &= !CARRY; // Clear CARRY flag
            self.status &= 0b1111_1110;
        }
    }

// Logical shift right
pub fn lsr(&mut self, addr: u16) {
        let value = self.read(addr);
        let result = value >> 1;
        self.write(addr, result);
        self.update_zero_and_negative_flags(result);
        if value & CARRY != 0 {
            self.status |= CARRY;
        } else {
            self.status &= !CARRY; // Clear CARRY flag
            self.status &= 0b1111_1110;
        }
    }

// Rotate left
// The CARRY flag is shifted into bit 0 and bit 7 is shifted into the CARRY flag
pub fn rol(&mut self, addr: u16) {
        let value = self.read(addr);
        let carry = self.status & CARRY;
        let result = (value << 1) | CARRY;
        self.write(addr, result);
        self.update_zero_and_negative_flags(result);
        if value & NEGATIVE != 0 {
            self.status |= CARRY;
        } else {
            self.status &= !CARRY; // Clear CARRY flag
            self.status &= 0b1111_1110;
        }
    }

// Rotate right
// The CARRY flag is shifted into bit 7 and bit 0 is shifted into the CARRY flag
pub fn ror(&mut self, addr: u16) {
        let value = self.read(addr);
        let carry = self.status & CARRY;
        let result = (value >> 1) | (CARRY << 7);
        self.write(addr, result);
        self.update_zero_and_negative_flags(result);
        if value & CARRY != 0 {
            self.status |= CARRY;
        } else {
            self.status &= !CARRY; // Clear CARRY flag
            self.status &= 0b1111_1110;
        }
    }
}

impl Cpu6502 {

// Flag operations
// Clear CARRY flag
pub fn clc(&mut self) {
        self.status &= !CARRY;
        self.status &= 0b1111_1110;
    }

/* Clear DECIMAL_MODE flag 
pub fn cld(&mut self) {
        self.status &= !DECIMAL_MODE;
        self.status &= 0b1111_0111;
    }
*/

/* Clear BREAK_COMMAND flag
 *pub fn clb(&mut self) {
        self.status &= !BREAK_COMMAND;
        self.status &= 0b1110_1111;
    }
*/

/* Clear ONE flag
 *pub fn clo(&mut self) {
        self.status &= !ONE;
        self.status &= 0b1101_1111;
    }
*/

// Clear INTERRUPT_DISABLE flag 
pub fn cli(&mut self) {
        self.status &= !INTERRUPT_DISABLE;
        self.status &= 0b1111_1011;
    }

// Clear OVERFLOW flag
pub fn clv(&mut self) {
        self.status &= !OVERFLOW;
        self.status &= 0b1011_1111;
    }

// Set CARRY flag to enable the CARRY
pub fn sec(&mut self) {
        self.status |= CARRY;
    }

/* Set DECIMAL_MODE flag to enter BCD
pub fn sed(&mut self) {
        self.status |= DECIMAL_MODE;
    }
*/

/* Set BREAK_COMMAND flag to enter BCD
 * pub fn set(&mut self) {
        self.status |= BREAK_COMMAND;
    }
*/

/* Set ONE flag to enter BCD
 * pub fn sev(&mut self) {
        self.status |= ONE;
    }
*/

// Set INTERRUPT_DISABLE flag to disable the maskable interrupt lin
pub fn sei(&mut self) {
        self.status |= INTERRUPT_DISABLE;
    }
}

impl Cpu6502 {

// Comparison instructions
// Compare the accumulator with a value
pub fn cmp(&mut self, value: u8) {
        if self.a >= value {
            self.status |= CARRY; // Set CARRY flag
        } else {
            self.status &= !CARRY; // Clear CARRY flag
            self.status &= 0b1111_1110;
        }
        let result = self.a.wrapping_sub(value);
        self.update_zero_and_negative_flags(result);
    }

// Compare the X register with a value
pub fn cpx(&mut self, value: u8) {
        if self.x >= value {
            self.status |= CARRY; // Set CARRY flag
        } else {
            self.status &= !CARRY; // Clear CARRY flag
            self.status &= 0b1111_1110;
        }
        let result = self.x.wrapping_sub(value);
        self.update_zero_and_negative_flags(result);
    }

// Compare the Y register with a value
pub fn cpy(&mut self, value: u8) {
        if self.y >= value {
            self.status |= CARRY; // Set CARRY flag
        } else {
            self.status &= !CARRY; // Clear CARRY flag
            self.status &= 0b1111_1110;
        }
        let result = self.y.wrapping_sub(value);
        self.update_zero_and_negative_flags(result);
        }
}

impl Cpu6502 {

// Branches
pub fn branch(&mut self, offset: u8) {
        let offset = offset as i8 as i16; // Convert to signed 16-bit integer
        self.pc = self.pc.wrapping_add(offset as u16) // Add the offset to the program counter
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
}

impl Cpu6502 {

// Jumps and Subroutines
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
}

impl Cpu6502 {

// Interrupts
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
}

impl Cpu6502 {

// These instructions perform bitwise operations on the accumulator and memory
pub fn bit(&mut self, value: u8) {
        if self.a & value == 0 {
            self.status |= ZERO; // Set zero flag
        } else {
            self.status &= !ZERO; // Clear zero flag
            self.status &= 0b1111_1101;
        }
        if value & NEGATIVE != 0 {
            self.status |= NEGATIVE; // Set negative flag
        } else {
            self.status &= !NEGATIVE; // Clear negative flag
            self.status &= 0b0111_1111;
        }
        if value & OVERFLOW != 0 {
            self.status |= OVERFLOW; // Set overflow flag
        } else {
            self.status &= !OVERFLOW; // Clear overflow flag
            self.status &= 0b1011_1111;
        }
    }
// No operation
pub fn nop(&mut self) {
    }
}
