/*!
integration_helpers: public compatibility wrappers and test ergonomics

Purpose
- Provide stable, test-friendly shims around the Bus façade without exposing internal structure.
- Keep external tests and examples simple and decoupled from internal layouts.

Scope
- Thin, readable wrappers that forward to existing Bus APIs (or read-only helpers).
- No new behavior and no side effects beyond the underlying Bus call.

Visibility and usage
- This module is public for integration/tests convenience.
- Prefer calling Bus methods directly in new code; use these helpers for clarity or when borrowing ergonomics are awkward.
- Internal/non-hot-path helpers live in `bus_helpers.rs` and are crate-internal (`pub(in crate::bus)`).

Performance
- Wrappers are tiny and annotated inline where helpful; they should not be used on hot paths if a direct call is clearer.

*/

use crate::apu::Apu;
use crate::bus::Bus;
use crate::cartridge::Cartridge;
use crate::controller::Controller;
use crate::ppu::Ppu;

//
// PPU helpers
//

/// Borrow the PPU immutably for read-only inspection (e.g., framebuffer reads in tests).
#[inline]
pub fn ppu(bus: &Bus) -> &Ppu {
    bus.ppu()
}

/// Borrow the PPU mutably for configuration or direct test manipulation.
#[inline]
pub fn ppu_mut(bus: &mut Bus) -> &mut Ppu {
    bus.ppu_mut()
}

/// Read a byte from the PPU-visible address space (0x0000-0x3FFF) using Bus mapping.
#[inline]
pub fn ppu_read(bus: &Bus, addr: u16) -> u8 {
    bus.ppu_read(addr)
}

/// Write a byte to the PPU-visible address space (0x0000-0x3FFF) using Bus mapping.
#[inline]
pub fn ppu_write(bus: &mut Bus, addr: u16, value: u8) {
    bus.ppu_write(addr, value)
}

//
// APU helpers
//

/// Borrow the APU mutably for configuration in tests or integration code.
#[inline]
pub fn apu_mut(bus: &mut Bus) -> &mut Apu {
    bus.apu_mut()
}

//
// Controller helpers
//

/// Borrow a controller by index (0 or 1) mutably, if present.
#[inline]
pub fn controller_mut(bus: &mut Bus, idx: usize) -> Option<&mut Controller> {
    bus.controller_mut(idx)
}

//
// Cartridge helpers
//

/// Borrow the inserted cartridge mutably, if present.
#[inline]
pub fn cartridge_mut(bus: &mut Bus) -> Option<&mut Cartridge> {
    bus.cartridge_mut()
}

/// Borrow the inserted cartridge immutably, if present.
#[inline]
pub fn cartridge_ref(bus: &Bus) -> Option<&Cartridge> {
    bus.cartridge_ref()
}

//
// DMA and timing helpers
//

/// True if OAM DMA is currently active (CPU is stalled).
#[inline]
pub fn dma_is_active(bus: &Bus) -> bool {
    bus.dma_is_active()
}

/// Remaining CPU stall cycles for the current DMA transfer (0 if idle).
#[inline]
pub fn dma_stall_remaining(bus: &Bus) -> u32 {
    bus.dma_stall_remaining()
}

/// Total CPU cycles elapsed as tracked by the Bus (useful in timing-sensitive tests).
#[inline]
pub fn total_ticks(bus: &Bus) -> u64 {
    bus.total_ticks()
}
