//! DMA glue: trait impls wiring Bus and Ppu to DMA traits (CpuMemory, OamWriter)
/*!
DMA glue module

Purpose
- Host small trait implementations and glue needed for DMA to interact with the Bus and PPU
  without borrowing the entire Bus or introducing interior mutability.
- Centralize "who implements what" so the DMA controller (`DmaController`) can operate over
  minimal traits (`CpuMemory`, `OamWriter`) with concrete adapters.

Intended responsibilities (to be implemented in subsequent commits)
- Move the following trait impls from `bus_impl.rs` into this module:
  1) `impl crate::bus::dma::CpuMemory for crate::bus::bus_impl::Bus`
     - Delegates DMA source reads to the Bus CPU-visible read path.
     - This remains useful as a convenience/compat shim, but orchestrator code should prefer
       the field-borrowing `CpuMemoryView` adapter when possible.
  2) `impl crate::bus::dma::OamWriter for crate::ppu::Ppu`
     - Provides the minimal write interface to push one byte into PPU OAM (equivalent to writing $2004).

- Keep these impls thin and well-documented; they should forward to the same logic the Bus/PPU
  already expose (e.g., `Bus::read`, a small PPU helper to write OAMDATA).
- Optionally, add additional glue that remains small and focused (e.g., test-only adapters).

Integration plan (Phase B, step-by-step)
1. Introduce this module (skeleton) and add `mod dma_glue;` (or `pub mod dma_glue;`) in `bus/mod.rs`.
2. Move the trait impls listed above from `bus_impl.rs` into this file unchanged (copy/paste):
   - Add necessary `use` statements for `crate::bus::bus_impl::Bus`, `crate::ppu::Ppu`, and
     `crate::bus::dma::{CpuMemory, OamWriter}`.
3. Ensure visibility is appropriate:
   - Impl blocks are inherently visible if types are in scope; no need to export anything from here.
   - Do NOT publicly re-export from this module; these impls are side-effectful (they attach to types).
4. Run `cargo check` and `cargo test` to verify that the move is purely mechanical and safe.
5. Keep orchestrator paths using `CpuMemoryView` + `&mut Ppu` as the preferred borrowing pattern.

Notes
- `CpuMemoryView` (adapter struct) intentionally lives in `bus/dma.rs` alongside `DmaController`
  to keep the DMA subsystem cohesive. This module only hosts impls for existing concrete types.
- The glue here should remain minimal, stable, and well-documented.
- Unit and integration tests should continue to exercise DMA via the Bus orchestrator and PPU;
  this module does not require its own tests beyond ensuring trait impls compile and link.

*/
#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(unused_macros)]

impl crate::bus::dma::CpuMemory for crate::bus::bus_impl::Bus {
    #[inline]
    fn cpu_read(&mut self, addr: u16) -> u8 {
        self.read(addr)
    }
}

impl crate::bus::dma::OamWriter for crate::ppu::Ppu {
    #[inline]
    fn write_oam_data(&mut self, value: u8) {
        // Equivalent to CPU writing $2004 (OAMDATA)
        self.write_reg(0x2004, value);
    }
}
