#![doc = r#"
Rustendo library crate.

This crate exposes the emulator core modules for use by binaries and tests.

Modules:
- apu: APU register stub and basic frame IRQ behavior
- bus: Bus facade coordinating CPU/PPU/APU/Controllers and timing
- cartridge: iNES v1 loader and cartridge metadata; constructs a Mapper
- controller: NES controller abstraction
- cpu6502: 6502 CPU core with cycle-accurate timing for documented opcodes
- mapper: Mapper trait and NROM (mapper 0) implementation
- ppu: PPU register interface, OAM handling, simple timing and NMI latch

In tests, shared iNES builders are available under `crate::test_utils`.
"#]

// Core emulator modules
pub mod apu;
pub mod bus;
pub mod cartridge;
pub mod controller;
pub mod cpu6502;
pub mod mapper;
pub mod ppu;

// Re-export commonly used types at the crate root for convenience.
pub use bus::Bus;
pub use cartridge::Cartridge;
pub use cpu6502::Cpu6502;

// Shared test utilities (only compiled for tests)
#[cfg(test)]
pub mod test_utils;
