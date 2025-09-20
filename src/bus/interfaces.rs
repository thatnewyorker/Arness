/*!
interfaces: lightweight views and traits to decouple subsystems.

This module introduces `BusPpuView`, a read-only wrapper around `Bus` that
implements the `PpuBus` trait. It exposes only immutable PPU address-space
reads, allowing the PPU to operate with a read-only view of the bus to avoid
mutable borrow contention during ticking and rendering.

Usage:
- Construct a `BusPpuView` with `BusPpuView::new(&bus)` and pass it where a
  `PpuBus` is required (e.g., `Ppu::tick` or rendering helpers).

Rationale:
- The PPU needs read-only access to the PPU-visible address space for pattern,
  nametable, and palette fetches. This view avoids taking a mutable borrow of
  the entire `Bus` while still delegating to the exact same mapping logic.
*/

use crate::bus_impl::Bus;
use crate::ppu_bus::PpuBus;

/// Read-only view of the `Bus` that implements `PpuBus`.
///
/// This view forwards PPU address-space reads to `Bus::ppu_read`, preserving
/// cartridge/mapper and mirroring behavior, without exposing any mutable API.
#[derive(Clone, Copy)]
pub struct BusPpuView<'a> {
    bus: &'a Bus,
}

impl<'a> BusPpuView<'a> {
    /// Create a new read-only PPU view over the provided `Bus`.
    #[inline]
    pub fn new(bus: &'a Bus) -> Self {
        Self { bus }
    }

    /// Access the underlying `Bus` reference if needed for non-PPU read-only queries.
    #[inline]
    pub fn bus(&self) -> &'a Bus {
        self.bus
    }
}

impl<'a> PpuBus for BusPpuView<'a> {
    #[inline]
    fn ppu_read(&self, addr: u16) -> u8 {
        self.bus.ppu_read(addr)
    }
}
