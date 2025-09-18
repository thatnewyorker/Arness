#![doc = r#"
Rustendo library crate.

This crate exposes the emulator core modules for use by binaries and tests.

Modules:
- apu: APU register stub and basic frame IRQ behavior
- bus: Bus facade coordinating CPU/PPU/APU/Controllers and timing
- cartridge: iNES v1 loader and cartridge metadata; constructs a Mapper
- controller: NES controller abstraction
- cpu: 6502 CPU core (modular refactor façade; preferred)
- cpu6502: Legacy monolithic 6502 core (temporary; slated for removal after refactor)
- mapper: Mapper trait and NROM (mapper 0) implementation
- ppu: PPU register interface, OAM handling, simple timing and NMI latch
- ppu_bus: Trait abstraction for PPU memory reads (decouples PPU from full Bus)

In tests, shared iNES builders are available under `crate::test_utils`.
"#]

// Core emulator modules
pub mod apu;
pub mod bus;
pub mod cartridge;
pub mod controller;
pub mod cpu;
// Legacy monolithic 6502 core (temporary; slated for removal after refactor)
// Note: prefer `cpu::Cpu6502`; this legacy module remains available during migration.
pub mod cpu6502;
pub mod mapper;
pub mod mappers;
pub mod ppu;
pub mod ppu_bus;

// Re-export commonly used types at the crate root for convenience.
pub use bus::Bus;
pub use cartridge::Cartridge;
pub use cpu::Cpu6502;

// Shared test utilities (only compiled for tests)
#[cfg(test)]
pub mod test_utils;
