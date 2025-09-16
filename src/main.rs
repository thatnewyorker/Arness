use arness::{Bus, Cartridge, Cpu6502};

fn build_test_ines() -> Vec<u8> {
    // iNES header
    let mut header = Vec::with_capacity(16);
    header.extend_from_slice(b"NES\x1A");
    header.push(1); // 1 x 16KB PRG
    header.push(1); // 1 x 8KB CHR
    header.push(0); // flags6 (horizontal mirroring, no trainer, no battery)
    header.push(0); // flags7
    header.push(1); // PRG-RAM size in 8KB units (allocate 8KB)
    header.extend_from_slice(&[0u8; 7]); // padding

    // PRG ROM 16KB
    let mut prg = vec![0u8; 16 * 1024];

    // Program at $8000 (offset 0x0000 in PRG)
    let program: &[u8] = &[
        0xA9, 0x10, // LDA #$10
        0x69, 0x05, // ADC #$05 => A = 0x15
        0x8D, 0x00, 0x02, // STA $0200
        0xE8, // INX
        0xD0, 0xFD, // BNE -3 -> loop until X wraps to 0
        0x00, // BRK
    ];
    prg[..program.len()].copy_from_slice(program);

    // Vectors (NMI, RESET, IRQ) at top of 16KB bank mirrored to $C000-$FFFF:
    // $FFFA/$FFFB (NMI), $FFFC/$FFFD (RESET), $FFFE/$FFFF (IRQ/BRK)
    // For 16KB PRG, these map to offsets 0x3FFA..0x3FFF in the single bank
    let reset: u16 = 0x8000;
    let nmi: u16 = 0x8000;
    let irq: u16 = 0x8000;
    prg[0x3FFA] = (nmi & 0xFF) as u8;
    prg[0x3FFB] = (nmi >> 8) as u8;
    prg[0x3FFC] = (reset & 0xFF) as u8;
    prg[0x3FFD] = (reset >> 8) as u8;
    prg[0x3FFE] = (irq & 0xFF) as u8;
    prg[0x3FFF] = (irq >> 8) as u8;

    // CHR ROM 8KB (zeros)
    let chr = vec![0u8; 8 * 1024];

    let mut rom = header;
    rom.extend_from_slice(&prg);
    rom.extend_from_slice(&chr);
    rom
}

fn main() {
    // Build a simple NROM cartridge with our demo program
    let rom = build_test_ines();
    let cart = Cartridge::from_ines_bytes(&rom).expect("failed to parse iNES");

    // Create Bus and attach the cartridge
    let mut bus = Bus::new();
    bus.attach_cartridge(cart);

    // Create CPU and reset using reset vector from Bus
    let mut cpu = Cpu6502::new();
    cpu.reset(&mut bus);

    // Run until one PPU frame completes (with a safety cap)
    let mut instr_count: usize = 0;
    let max_instr: usize = 1_000_000;
    'frame: loop {
        let _cycles = cpu.step(&mut bus);
        if bus.ppu.take_frame_complete() {
            break 'frame;
        }
        instr_count += 1;
        if instr_count >= max_instr {
            break;
        }
    }

    // Inspect state
    let m0200 = bus.read(0x0200);
    println!("A: 0x{:02X}", cpu.a);
    println!("X: 0x{:02X}", cpu.x);
    println!("Y: 0x{:02X}", cpu.y);
    println!("SP: 0x{:02X}", cpu.sp);
    println!("PC: 0x{:04X}", cpu.pc);
    println!("P (flags): 0b{:08b}", cpu.status);
    println!("mem[0x0200]: 0x{:02X}", m0200);
}
