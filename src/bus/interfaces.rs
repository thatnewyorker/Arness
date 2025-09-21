/*!
interfaces: lightweight views and traits to decouple subsystems.

This module introduces `BusPpuView`, a read-only wrapper around `Bus` that
implements the `PpuBus` trait. It exposes only immutable PPU address-space
reads, allowing the PPU to operate with a read-only view of the bus to avoid
mutable borrow contention during ticking and rendering.

Usage:
- Prefer constructing `BusPpuView` with `BusPpuView::from_parts(bus.ppu_mem(), bus.cartridge.as_ref())` so only the required subfields are borrowed and the whole `Bus` is not immutably borrowed. This enables simultaneous `&mut Ppu` borrows during ticking and rendering.
- The `new(&Bus)` convenience is deprecated; use `from_parts(...)` to avoid whole-`Bus` borrows.

Rationale:
- The PPU needs read-only access to the PPU-visible address space for pattern,
  nametable, and palette fetches. This view avoids taking a mutable borrow of
  the entire `Bus` while still delegating to the exact same mapping logic.
*/

use crate::bus::Bus;
use crate::bus::ppu_space::{DynMirroring, HeaderMirroring, PpuAddressSpace};
use crate::cartridge::{Cartridge, Mirroring};
use crate::mapper::MapperMirroring;
use crate::ppu_bus::PpuBus;

/// Read-only view of the `Bus` that implements `PpuBus`.
///
/// This view forwards PPU address-space reads to `Bus::ppu_read`, preserving
/// cartridge/mapper and mirroring behavior, without exposing any mutable API.
#[derive(Clone, Copy)]
pub(in crate::bus) struct BusPpuView<'a> {
    ppu_mem: &'a PpuAddressSpace,
    cartridge: Option<&'a Cartridge>,
}

impl<'a> BusPpuView<'a> {
    /// Create a new read-only PPU view borrowing only the fields the PPU needs.
    #[deprecated(note = "Use BusPpuView::from_parts(...) to avoid borrowing the whole Bus")]
    #[inline]
    pub fn new(bus: &'a Bus) -> Self {
        Self {
            ppu_mem: bus.ppu_mem(),
            cartridge: bus.cartridge.as_ref(),
        }
    }

    /// Alternate constructor from explicit parts (avoids borrowing the full `Bus`).
    #[inline]
    pub fn from_parts(ppu_mem: &'a PpuAddressSpace, cartridge: Option<&'a Cartridge>) -> Self {
        Self { ppu_mem, cartridge }
    }
}

impl<'a> PpuBus for BusPpuView<'a> {
    #[inline]
    fn ppu_read(&self, addr: u16) -> u8 {
        let a = addr & 0x3FFF;
        match a {
            0x0000..=0x1FFF => {
                if let Some(cart) = self.cartridge {
                    cart.mapper.borrow().ppu_read(a)
                } else {
                    0
                }
            }
            0x2000..=0x3FFF => {
                // Resolve header mirroring
                let header_mode = if let Some(cart) = self.cartridge {
                    match cart.mirroring() {
                        Mirroring::Horizontal => HeaderMirroring::Horizontal,
                        Mirroring::Vertical => HeaderMirroring::Vertical,
                        Mirroring::FourScreen => HeaderMirroring::FourScreen,
                    }
                } else {
                    HeaderMirroring::Horizontal
                };
                // Resolve dynamic (mapper) mirroring unless header enforces FourScreen
                let dyn_mode = if let Some(cart) = self.cartridge {
                    if !matches!(cart.mirroring(), Mirroring::FourScreen) {
                        cart.mapper.borrow().current_mirroring().map(|m| match m {
                            MapperMirroring::SingleScreenLower => DynMirroring::SingleScreenLower,
                            MapperMirroring::SingleScreenUpper => DynMirroring::SingleScreenUpper,
                            MapperMirroring::Vertical => DynMirroring::Vertical,
                            MapperMirroring::Horizontal => DynMirroring::Horizontal,
                        })
                    } else {
                        None
                    }
                } else {
                    None
                };
                self.ppu_mem.ppu_read(a, header_mode, dyn_mode)
            }
            _ => 0,
        }
    }
}
