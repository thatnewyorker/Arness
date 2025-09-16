# Arness: NES Emulator Core in Rust (WIP)

Arness is a work-in-progress Nintendo Entertainment System (NES) emulator core written in Rust. The current focus is on correctness of the core components (CPU, bus, cartridge/mapper, basic PPU and APU stubs) with unit tests and a minimal demo runner. There is no graphics, audio, or input backend yet.

## Status

Implemented
- CPU: 6502 core with cycle counting for documented opcodes; targeted cycle and behavior tests
- Bus: CPU memory map, RAM mirroring, PPU/APU register access, controllers, OAM DMA, simple timing (PPU ticks 3x per CPU tick)
- PPU (stub): CPU-visible registers ($2000–$2007), simple VRAM + buffered reads, OAM, basic dot/scanline/frame timing, NMI on vblank
- APU (stub): Register mirror, $4015 status, simple frame IRQ approximation; no audio output
- Controllers: $4016/$4017 strobe and serial-read behavior
- Cartridge: iNES v1 loader; NROM (mapper 0) only; CHR RAM allocated when CHR size is 0
- Mapper: NROM implementation with PRG ROM mirroring and PRG RAM support
- Shared test utils: iNES builders for consistent unit tests across modules

Not yet implemented
- Rendering or audio output
- Input backends
- Additional mappers beyond NROM
- CLI to load external ROMs
- Save states, configuration, or UI

## Repository layout

- src/
  - apu.rs — APU register/status stub + frame IRQ approximation
  - bus.rs — Bus and timing glue
  - cartridge.rs — iNES v1 loader, NROM cartridge
  - controller.rs — NES controller logic
  - cpu6502.rs — CPU core
  - lib.rs — Library exports
  - main.rs — Demo runner (builds an in-memory NROM program and runs until one PPU frame)
  - mapper.rs — Mapper trait + NROM implementation
  - test_utils/ — Shared iNES builder helpers for tests
- Cargo.toml

## Prerequisites

- Rust (latest stable recommended)

No external libraries (e.g., SDL2) are required at this stage.

## Build

From the repository root:

    cd Arness
    cargo build

Or from the crate directory (this folder):

    cargo build

## Run the demo

The included demo binary constructs an in-memory NROM image and runs the CPU/Bus until a single PPU frame (with a safety cap), then prints CPU registers and a memory location to stdout.

From the crate directory:

    cargo run -q

Expected: a short printout of CPU registers, flags, PC, SP, and a memory byte (e.g., mem[0x0200]).

## Run tests

Unit tests cover CPU cycle counts, bus behavior (mirroring, DMA, registers), PPU/Controller semantics, mapper behavior, and cartridge parsing.

    cargo test -q

## Library usage

You can integrate the emulator core into your own program and drive it from a host loop. Example:

    use arness::{Bus, Cartridge, Cpu6502};

    fn main() -> Result<(), String> {
        // Load an iNES v1 ROM (NROM/mapper 0 supported for now)
        let cart = Cartridge::from_ines_file("path/to/game.nes")?;

        // Create bus and attach cartridge
        let mut bus = Bus::new();
        bus.attach_cartridge(cart);

        // Create CPU and reset
        let mut cpu = Cpu6502::new();
        cpu.reset(&mut bus);

        // Run until one PPU frame (or a safety cap)
        let mut instr_count = 0usize;
        let max_instr = 1_000_000;
        while instr_count < max_instr {
            let _cycles = cpu.step(&mut bus);

            // The Bus drives PPU/APU timing; break when a frame completes
            if bus.ppu.take_frame_complete() {
                break;
            }
            instr_count += 1;
        }

        Ok(())
    }

Notes
- iNES 2.0 is detected and currently rejected.
- Only NROM/mapper 0 is supported.
- No rendering/audio/backends are included yet; add your own UI loop and use PPU state to render.

## Roadmap (planned)

- PPU: background/sprite rendering and accurate memory/mirroring behavior
- APU: audio generation and accurate frame sequencer
- Input: keyboard/gamepad backends
- CLI: load ROMs, debugging helpers
- Mappers: add common mappers beyond NROM
- Save states and configuration
- Performance tuning and accuracy improvements

## Contributing

Contributions are welcome:
- Expand instruction coverage and timing tests
- Improve PPU/APU fidelity
- Add mappers and test ROM coverage
- Documentation and developer experience

Please open an issue or PR to discuss changes.

## License

MIT — see LICENSE.

## Acknowledgments

Thanks to the Rust community and emulator authors for publicly available documentation and test ROM insights.

## Disclaimer

This project does not include Nintendo software or games. Use only legally obtained ROMs.