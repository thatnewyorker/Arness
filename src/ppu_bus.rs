/*!
ppu_bus: Trait abstraction decoupling the PPU rendering code from the full
`Bus` implementation.

Rationale:
- The PPU background/sprite renderer only needs read access to the PPU
  address space (pattern tables, nametables, attribute tables, palette).
- Accepting a trait instead of a concrete `Bus`:
  * Improves testability (lightweight mocks without constructing CPU/APU/etc).
  * Reduces coupling, enabling future refactors (e.g. relocating VRAM/palette).
  * Allows instrumentation or alternative backends (e.g. cached read-only view).

Address Space Expectations (mirroring semantics left to implementor):
- 0x0000-0x1FFF : Pattern tables (CHR ROM/RAM via mapper)
- 0x2000-0x2FFF : Nametables (with mirroring rules)
- 0x3000-0x3EFF : Mirrors of 0x2000-0x2EFF
- 0x3F00-0x3F1F : Palette RAM (universal + sub-palettes; internal mirroring)
- 0x3F20-0x3FFF : Mirrors of 0x3F00-0x3F1F

The trait intentionally exposes ONLY a read method. If/when the renderer needs
to do side-effectful PPU fetch emulation (e.g., incrementing internal scroll
counters on certain cycles) that behavior should be internal to the concrete
`PpuBus` implementor (the real `Bus`). For tests, pure lookup is sufficient.

Future Extensions (add conservatively):
- Optional write hook (likely not needed for background rendering).
- Method to prefetch a whole 16-byte pattern row (micro-optimizations).
*/

/// Minimal interface the PPU renderer depends on for memory fetches.
pub trait PpuBus {
    /// Read a byte from the PPU-visible address space (with all NES mirroring
    /// and mapper translation applied). Must accept the full 14-bit address
    /// (0x0000-0x3FFF); callers are allowed to pass any value in that range.
    fn ppu_read(&self, addr: u16) -> u8;
}

impl PpuBus for crate::bus_impl::Bus {
    #[inline]
    fn ppu_read(&self, addr: u16) -> u8 {
        // Delegate to the Bus's public accessor (already handles mirroring + mapper).
        self.ppu_read(addr)
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
