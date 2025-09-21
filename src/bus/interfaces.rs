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
#[cfg(test)]
pub use tests::MockPpuBus;
/// Minimal interface the PPU renderer depends on for memory fetches.
///
/// Placed here to centralize bus-facing interfaces.
pub trait PpuBus {
    /// Read a byte from the PPU-visible address space (0x0000-0x3FFF).
    fn ppu_read(&self, addr: u16) -> u8;
}

impl PpuBus for crate::bus::Bus {
    #[inline]
    fn ppu_read(&self, addr: u16) -> u8 {
        // Delegate to the Bus's public accessor (already handles mirroring + mapper).
        self.ppu_read(addr)
    }
}

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

#[cfg(test)]
mod tests {
    use super::PpuBus;

    /// A lightweight in-memory mock implementing the basic PPU address space
    /// layout sufficient for deterministic unit tests of rendering logic.
    ///
    /// Features:
    /// - Pattern table region: 0x0000-0x1FFF stored in `pattern`
    /// - Single 4 KiB nametable backing array (assumes simple single-screen)
    /// - Palette RAM with required mirroring rules
    ///
    /// This is intentionally minimal; extend only when a test proves a need.
    pub struct MockPpuBus {
        pattern: Vec<u8>,        // 8 KiB
        nametable: [u8; 0x1000], // 4 KiB covers $2000-$2FFF (and $3000 mirror)
        palette: [u8; 32],       // $3F00-$3F1F
    }

    impl Default for MockPpuBus {
        fn default() -> Self {
            Self {
                pattern: vec![0; 0x2000],
                nametable: [0; 0x1000],
                palette: [0; 32],
            }
        }
    }

    impl MockPpuBus {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn write_pattern(&mut self, addr: u16, value: u8) {
            let idx = (addr as usize) & 0x1FFF;
            self.pattern[idx] = value;
        }

        pub fn write_nametable(&mut self, addr: u16, value: u8) {
            let base = 0x2000;
            let masked = (addr as usize & 0x0FFF).min(self.nametable.len() - 1);
            if (addr as usize) >= base {
                self.nametable[masked] = value;
            }
        }

        pub fn write_palette(&mut self, addr: u16, value: u8) {
            let a = (addr - 0x3F00) & 0x3FFF;
            let mut idx = (a & 0x1F) as usize;
            if idx >= 16 && (idx & 0x03) == 0 {
                idx -= 16;
            }
            self.palette[idx] = value;
        }
    }

    impl PpuBus for MockPpuBus {
        fn ppu_read(&self, addr: u16) -> u8 {
            let a = addr & 0x3FFF;
            match a {
                0x0000..=0x1FFF => self.pattern[(a as usize) & 0x1FFF],
                0x2000..=0x3EFF => {
                    // Mirror $3000-$3EFF to $2000-$2EFF
                    let base = 0x2000 | (a & 0x0FFF);
                    let idx = (base as usize) & 0x0FFF;
                    self.nametable[idx]
                }
                0x3F00..=0x3FFF => {
                    let mut idx = ((a - 0x3F00) & 0x1F) as usize;
                    if idx >= 16 && (idx & 0x03) == 0 {
                        idx -= 16;
                    }
                    self.palette[idx]
                }
                _ => 0,
            }
        }
    }

    #[test]
    fn mock_basic_pattern_and_palette_reads() {
        let mut mock = MockPpuBus::new();
        mock.write_pattern(0x0002, 0xAA);
        mock.write_palette(0x3F01, 0x1C);

        assert_eq!(mock.ppu_read(0x0002), 0xAA);
        assert_eq!(mock.ppu_read(0x3F01), 0x1C);
    }

    #[test]
    fn palette_mirror_handling() {
        let mut mock = MockPpuBus::new();
        // Write to $3F00 and ensure $3F10 mirrors (after mirroring adjustments).
        mock.write_palette(0x3F00, 0x09);
        assert_eq!(mock.ppu_read(0x3F00), 0x09);
        assert_eq!(mock.ppu_read(0x3F10), 0x09);
    }

    #[test]
    fn nametable_mirror_into_3000_region() {
        let mut mock = MockPpuBus::new();
        mock.write_nametable(0x2000, 0x55);
        // $3000 mirrors $2000 region
        assert_eq!(mock.ppu_read(0x2000), 0x55);
        assert_eq!(mock.ppu_read(0x3000), 0x55);
    }
}
