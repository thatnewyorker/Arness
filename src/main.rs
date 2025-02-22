mod cpu6502;
use cpu6502::Cpu6502;
mod ppu;
use minifb::{Key, Window, WindowOptions};

fn main() {
    // Initialize CPU
    let mut cpu = Cpu6502::new();

    // Test program: Toggle PPUMASK to enable/disable background
    let program = [
        0xA9, 0x08,       // LDA #$08 (enable background)
        0x8D, 0x01, 0x20, // STA $2001
        0xA9, 0x00,       // LDA #$00 (disable background)
        0x8D, 0x01, 0x20, // STA $2001
        0x4C, 0x00, 0x80, // JMP $8000 (loop indefinitely)
    ];
    cpu.load_program(&program, 0x8000);

    // Set up window
    let width = 256;
    let height = 240;
    let mut window = Window::new(
        "NES Emulator Test",
        width,
        height,
        WindowOptions::default(),
    ).unwrap_or_else(|e| panic!("Failed to create window: {}", e));

    // Limit to ~60 FPS using set_target_fps
    window.set_target_fps(60);

    // Main loop
    while window.is_open() && !window.is_key_down(Key::Escape) {
        // Run CPU for one frame (~29780 CPU cycles for NTSC)
        cpu.run(29780);

        // Convert PPU frame buffer from RGB to u32 RGBA for minifb
        let buffer: Vec<u32> = cpu.ppu.get_frame_buffer()
            .chunks(3)
            .map(|rgb| {
                let r = rgb[0] as u32;
                let g = rgb[1] as u32;
                let b = rgb[2] as u32;
                (r << 16) | (g << 8) | b // Alpha ignored by minifb
            })
            .collect();

        // Update window
        window.update_with_buffer(&buffer, width, height)
            .unwrap_or_else(|e| println!("Failed to update window: {}", e));
    }
}