mod cpu6502;
mod ppu;
mod rom;
use cpu6502::Cpu6502;
use rom::Rom;
use minifb::{Key, Window, WindowOptions};

fn main() {
    // Load ROM
    let rom = Rom::load_from_file("/home/gerard/ROMs/NES/Donkey_Kong/dk.nes").expect("Failed to load ROM");
    println!("ROM loaded: PRG size = {}, CHR size = {}, Mapper = {}", 
             rom.prg_size(), rom.chr_size(), rom.mapper());  // Use method calls

    // Initialize CPU and PPU
    let mut cpu = Cpu6502::new(rom.chr_size(), rom.chr_ram_size(), rom.mirroring());
    cpu.load_rom(&rom).expect("Failed to load ROM");
    cpu.ppu.load_chr_rom(rom.chr_rom()).expect("Failed to load CHR-ROM");

    // Quick fix: Force-enable PPU rendering
    cpu.ppu.write(0x2000, 0x80); // PPUCTRL: Enable NMI
    cpu.ppu.write(0x2001, 0x1E); // PPUMASK: Show background and sprites, no clipping

    // Force nametable and palette initialization (temporary)
    for i in 0x2000..0x23FF {  // Fill nametable 0 with a checkerboard pattern
        cpu.ppu.write_vram(i, if (i / 32 + i % 32) % 2 == 0 { 0x00 } else { 0x01 });
    }
    for i in 0x3F00..0x3F10 {  // Set simple palette (black and gray)
        cpu.ppu.write_vram(i, if i == 0x3F00 { 0x0F } else { 0x20 });
    }

    // Print reset vector for confirmation
    println!("Reset vector: {:#06x}", cpu.pc);

    // Set up window
    let width = 256;
    let height = 240;
    let mut window = Window::new(
        "NES Emulator Test",
        width,
        height,
        WindowOptions::default(),
    ).unwrap_or_else(|e| panic!("Failed to create window: {}", e));
    window.set_target_fps(60);

    // Main loop with debug prints
    let mut frame_count = 0;  // Move this inside main, before the while loop
    while window.is_open() && !window.is_key_down(Key::Escape) {
        cpu.run(32000);  // Increase to ensure full NTSC frame (29780.5 cycles)
        frame_count += 1;

        println!("Frame: {}, PC: {:#06x}, A: {:#04x}, X: {:#04x}, Y: {:#04x}, Status: {:#04x}, PPU Ctrl: {:#04x}, Mask: {:#04x}, Scanline: {}, Dot: {}",
                 frame_count, cpu.pc, cpu.a, cpu.x, cpu.y, cpu.status, cpu.ppu.ctrl(), cpu.ppu.mask(), cpu.ppu.scanline(), cpu.ppu.dot());

        let buffer: Vec<u32> = cpu.ppu.get_frame_buffer()
            .chunks(3)
            .map(|rgb| (rgb[0] as u32) << 16 | (rgb[1] as u32) << 8 | (rgb[2] as u32))
            .collect();

        window.update_with_buffer(&buffer, width, height)
            .unwrap_or_else(|e| println!("Failed to update window: {}", e));
    }
}
