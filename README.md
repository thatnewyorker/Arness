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
  - bus/ — Bus façade and modular submodules
    - mod.rs — Bus façade with inlined Bus implementation and submodule declarations
    - cpu_interface.rs — CPU-visible address dispatcher and helpers
    - ppu_registers.rs — CPU-visible PPU register window (0x2000–0x3FFF)
    - ppu_space.rs — PPU address-space mapping (nametables, palette, mirroring)
    - ppu_memory.rs — Bus-level PPU memory helpers (mirroring resolution, delegates to mapper/PPU RAM)
    - dma.rs — DmaController + CpuMemory/OamWriter traits and DMA tests
    - dma_glue.rs — DMA glue trait impls (CpuMemory for Bus, OamWriter for Ppu)
    - clock.rs — Orchestrator (PPU x3 per CPU cycle, DMA micro-step, APU, IRQ/NMI)
    - interfaces.rs — Lightweight views (e.g., BusPpuView) for borrow-safe access
    - ram.rs — 2 KiB CPU RAM with mirroring
    - ram_helpers.rs — CPU RAM mirrored access wrappers (internal helpers)
  - cartridge.rs — iNES v1 loader, NROM cartridge
  - controller.rs — NES controller logic
  - cpu/ — CPU core (facade, state, dispatch, execute, addressing modules)
  - lib.rs — Library exports
  - main.rs — Demo runner (builds an in-memory NROM program and runs until one PPU frame)
  - mapper.rs — Mapper trait + NROM implementation
  - ppu.rs — PPU core (registers, OAM, basic rendering/timing)
  - ppu_bus.rs — PpuBus trait
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

## Changelog

- Refactor: Introduced field-based views to eliminate moves of subcomponents:
  - Added CpuMemoryView (internal to bus module) to provide DMA with a borrow-safe CPU-memory adapter over RAM, cartridge, and controllers.
  - Extended BusPpuView to support construction from parts (BusPpuView::from_parts), borrowing only PPU-related subfields.
  - Removed mem::replace usage in timing/orchestrator paths (PPU stepping and DMA micro-steps) in favor of non-overlapping borrows.
- Deprecation: BusPpuView::new(&Bus) is deprecated in favor of BusPpuView::from_parts(...) to avoid whole-Bus immutable borrows in orchestrator code.
- API surface: BusPpuView and CpuMemoryView are internal to the bus module; downstream consumers should use the Bus façade and PpuBus trait where needed.
- Reorg: Extracted PPU mapping helpers to bus/ppu_memory.rs, moved DMA glue impls to bus/dma_glue.rs, and added bus/ram_helpers.rs; Bus methods delegate to these helpers to keep the Bus façade (in bus/mod.rs) focused while preserving behavior.
- Tests: Added unit tests covering DMA alignment behavior (513/514 cycles), read/write alternation, RAM mirroring via CpuMemoryView, and end-to-end OAM DMA correctness.

## Library usage

You can integrate the emulator core into your own program and drive it from a host loop. Example:

    use arness::{Bus, Cartridge};
    use arness::cpu::core::Cpu;

    fn main() -> Result<(), String> {
        // Load an iNES v1 ROM (NROM/mapper 0 supported for now)
        let cart = Cartridge::from_ines_file("path/to/game.nes")?;

        // Create bus and attach cartridge
        let mut bus = Bus::new();
        bus.attach_cartridge(cart);

        // Create CPU and reset (using the new facade)
        let mut cpu = Cpu::new();
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