mod cpu6502;
use cpu6502::Cpu6502;
use minifb::{Key, Window, WindowOptions};

fn main() {
    // Initialize CPU and load a test program
    let mut cpu = Cpu6502::new();
    let program = [
        0xA9, 0x08,       // LDA #$08 (enable background in PPUMASK)
        0x8D, 0x01, 0x20, // STA $2001
        0x00,             // BRK
    ];
    cpu.load_program(&program, 0x8000);

    // Set up window
    let width = 256;
    let height = 240;
    let mut window = Window::new(
        "NES Emulator",
        width,
        height,
        WindowOptions::default(),
    ).unwrap_or_else(|e| panic!("Failed to create window: {}", e));

    // Limit frame rate to ~60 FPS
    window.limit_update_rate(Some(std::time::Duration::from_micros(16600)));

    // Main loop
    while window.is_open() && !window.is_key_down(Key::Escape) {
        // Run CPU for a frame’s worth of cycles (roughly 29780 CPU cycles for NTSC)
        cpu.run(29780);

        // Convert PPU frame buffer to u32 format for minifb (RGBA)
        let buffer: Vec<u32> = cpu.ppu.get_frame_buffer()
            .chunks(3)
            .map(|rgb| {
                let r = rgb[0] as u32;
                let g = rgb[1] as u32;
                let b = rgb[2] as u32;
                (r << 16) | (g << 8) | b // Pack into RGBA (minifb ignores A)
            })
            .collect();

        // Update window with frame buffer
        window.update_with_buffer(&buffer, width, height)
            .unwrap_or_else(|e| println!("Failed to update window: {}", e));
    }
}
