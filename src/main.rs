mod cpu6502;    // Import the cpu module

// Import the Cpu6502 struct from the cpu module
fn main() {
    let mut cpu6502 = cpu6502::Cpu6502::new();

    // Example usage: Load the value 0x10 into the accumulator
    cpu6502.lda_immediate(0x10);
    println!("Accumulator: {}", cpu6502.a);
}
