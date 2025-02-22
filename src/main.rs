mod cpu6502;
use cpu6502::Cpu6502;

fn main() {
    let mut cpu = Cpu6502::new();
    let program = [
        0xA9, 0x08,       // LDA #$08 (enable background in PPUMASK)
        0x8D, 0x01, 0x20, // STA $2001
        0x00,             // BRK
    ];
    cpu.load_program(&program, 0x8000);
    println!("Before: A={:#04x}, PPU Mask={:#04x}", cpu.a, cpu.ppu.mask);
    cpu.run(1000);
    println!(
        "After: A={:#04x}, PPU Mask={:#04x}, PPU Status={:#04x}",
        cpu.a, cpu.ppu.mask, cpu.ppu.status
    );
}
