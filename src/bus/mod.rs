#![doc = r#"
Bus module: modular façade and submodules.

Overview
- This directory contains the modularized Bus façade and focused submodules. It replaces
  the previous single-file `src/bus.rs` incrementally while preserving behavior.

Modules and responsibilities
- Bus: public façade implemented directly in this module (migrated from legacy).
- cpu_interface: CPU-visible address decoder and helpers (read/write, read_word); delegates to devices.
- ppu_registers: CPU-visible PPU register window (0x2000-0x3FFF, with mirroring and PPUDATA semantics).
- ppu_space: PPU address-space mapping (nametables, palette, mirroring) via `PpuAddressSpace` and pure helpers.
- dma: OAM DMA controller (`DmaController`) and minimal traits (`CpuMemory`, `OamWriter`) for decoupling.
- clock: tick/scheduler orchestration (advance CPU, step PPU 3x, DMA micro-step, latch NMI, step APU, aggregate IRQ).
- interfaces: lightweight views/traits to reduce borrowing friction (e.g., `BusPpuView` for read-only PPU access).
- integration_helpers: convenience wrappers and compatibility accessors used by tests and integration code.
- nametable_mapper: optional pure mapping logic for advanced mirroring strategies.

Migration notes
- The public Bus façade remains stable; internal responsibilities are delegated to these submodules.
- Submodules can evolve independently and are designed for isolated testing.
"#]
#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(unused_macros)]
#![allow(unused_mut)]

// Inlined Bus implementation (migrated from legacy.rs) wrapped into a module to accept inner doc comments
pub mod bus_impl {
    include!("bus_impl.rs");
}
pub use bus_impl::Bus;

/// CPU-visible memory map and helpers (dispatcher for address ranges).
pub mod cpu_interface;

/// PPU registers handler (CPU-visible 0x2000-0x3FFF).
pub mod ppu_registers;
/// PPU address-space mapping: nametables, palette, mirroring rules.
pub mod ppu_space;

/// Optional pure nametable mapping component if rules grow complex.
pub mod nametable_mapper {
    //! Optional pure component for nametable address mapping.
    //!
    //! Responsibilities (optional):
    //! - Resolve logical nametable address to physical index based on mirroring mode.
}

/// APU register window handler (0x4000–0x4017 subset).
pub mod apu_registers;
/// Controller ($4016/$4017) handler.
pub mod controller_registers;
pub mod dma;
/// Cycle-accurate OAM DMA controller.
pub mod ram;

/// Tick/scheduler orchestration for CPU/PPU/APU/DMA and interrupts.
pub mod clock;

/// Small traits/interfaces to decouple modules.
pub mod interfaces;

/// Integration helpers, accessors, and compatibility shims for tests.
pub mod integration_helpers {
    //! Backwards-compatibility and test helpers.
    //!
    //! Responsibilities (to be migrated here):
    //! - `ppu_mut`, `apu_mut`, `cartridge_mut`, etc.
    //! - Frame rendering helpers if needed during migration.
}

// Public re-exports for consumers. As functionality migrates into the submodules,
// these `pub use` lines ensure the public surface remains discoverable from `bus`.
pub use apu_registers::*;
pub use clock::*;
pub use controller_registers::*;
pub use cpu_interface::*;
pub use dma::*;
pub use integration_helpers::*;
pub use interfaces::BusPpuView;
pub use interfaces::*;
pub use nametable_mapper::*;
pub use ppu_registers::*;
pub use ppu_space::*;
pub use ram::*;
