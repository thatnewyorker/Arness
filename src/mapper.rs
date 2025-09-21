/*!
Mapper subsystem: trait definition and NROM (mapper 0) implementation scaffold.

Purpose:
- Decouple CPU/PPU address mapping from the `Cartridge` so additional mappers can be added.
- Provide a stable interface the Bus can call for CPU/PPU memory transactions in cartridge space.

Integration plan (high-level):
- Cartridge parses iNES and instantiates a concrete mapper (e.g., `Nrom`) with PRG/CHR data.
- Bus forwards CPU $6000..=$7FFF and $8000..=$FFFF to `mapper.cpu_*`.
- Bus forwards PPU $0000..=$1FFF (pattern tables) to `mapper.ppu_*`.
- Later, extend Bus to consult the mapper for PPU nametable mirroring if/when a mapper provides it.

This file intentionally avoids dependencies on other modules to keep the trait minimal and portable.
*/

/// Common interface all cartridge mappers must implement.
///
/// Semantics:
/// - All read/write methods take full CPU or PPU addresses (unmasked).
/// - Implementations decide mapping/banking and handle out-of-range accesses reasonably.
/// - `reset()` allows mapper-specific state to be reinitialized on power/reset.
/// - `irq_pending()` returns whether the mapper asserts an IRQ line (used by Bus/CPU).
///   Dynamic mirroring modes a mapper may produce at runtime. When a mapper
///   returns `Some(MapperMirroring)` from `current_mirroring`, it overrides the
///   static cartridge header mirroring (except in four‑screen cases).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MapperMirroring {
    SingleScreenLower,
    SingleScreenUpper,
    Vertical,
    Horizontal,
}

pub trait Mapper {
    /// Mapper numeric identifier (e.g., 0 for NROM).
    fn mapper_id(&self) -> u16;

    /// CPU-visible read at $6000..=$FFFF (Bus should only forward these ranges).
    fn cpu_read(&mut self, addr: u16) -> u8;

    /// CPU-visible write at $6000..=$FFFF (Bus should only forward these ranges).
    fn cpu_write(&mut self, addr: u16, value: u8);

    /// PPU-visible read at $0000..=$1FFF (pattern table region).
    fn ppu_read(&self, addr: u16) -> u8;

    /// PPU-visible write at $0000..=$1FFF (pattern table region).
    fn ppu_write(&mut self, addr: u16, value: u8);

    /// Reset/Power-on mapper state (bank registers, IRQ state, etc).
    fn reset(&mut self) {}

    /// Whether this mapper is asserting its IRQ output line at the moment.
    fn irq_pending(&self) -> bool {
        false
    }

    /// Optional dynamic nametable mirroring override. Mappers that can
    /// change mirroring (e.g., MMC1) return `Some(mode)`. Others return
    /// `None` so the Bus uses the cartridge header mirroring.
    fn current_mirroring(&self) -> Option<MapperMirroring> {
        None
    }
}

/// NROM (mapper 0) implementation.
///
/// Features:
/// - PRG ROM: either 16 KiB (NROM-128) mirrored or 32 KiB (NROM-256) direct at $8000..=$FFFF.
/// - PRG RAM: optional (commonly 8 KiB) at $6000..=$7FFF, read/write if present.
/// - CHR: either ROM or RAM (8 KiB). If CHR RAM, PPU writes are allowed; otherwise ignored.
#[derive(Clone, Debug)]
pub struct Nrom {
    prg_rom: Vec<u8>, // 16 KiB or 32 KiB (commonly)
    prg_ram: Vec<u8>, // often 8 KiB; can be empty
    chr: Vec<u8>,     // 8 KiB (ROM or RAM)
    chr_is_ram: bool, // true => PPU writes update CHR
}

impl Nrom {
    /// Create a new NROM mapper.
    ///
    /// - `prg_rom`: PRG ROM bytes (16 KiB or 32 KiB typical)
    /// - `chr`: CHR ROM bytes, or CHR RAM buffer if `chr_is_ram` is true (typically 8 KiB)
    /// - `chr_is_ram`: whether CHR is writable RAM (true) or ROM (false)
    /// - `prg_ram_size`: size of PRG RAM in bytes (0 to disable)
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, chr_is_ram: bool, prg_ram_size: usize) -> Self {
        Self {
            prg_rom,
            prg_ram: if prg_ram_size > 0 {
                vec![0; prg_ram_size]
            } else {
                Vec::new()
            },
            chr,
            chr_is_ram,
        }
    }

    #[inline]
    fn prg_rom_read(&self, addr: u16) -> u8 {
        if self.prg_rom.is_empty() {
            return 0xFF;
        }
        // Map $8000..=$FFFF into PRG ROM. 16 KiB -> mirror across 32 KiB; 32 KiB -> direct.
        let rel = addr.wrapping_sub(0x8000) as usize;
        let len = self.prg_rom.len();
        if len == 16 * 1024 {
            self.prg_rom[rel & 0x3FFF]
        } else if len == 32 * 1024 {
            self.prg_rom[rel & 0x7FFF]
        } else if len.is_power_of_two() {
            self.prg_rom[rel & (len - 1)]
        } else {
            self.prg_rom[rel % len.max(1)]
        }
    }

    #[inline]
    fn prg_ram_read(&self, addr: u16) -> u8 {
        if self.prg_ram.is_empty() {
            return 0;
        }
        let rel = (addr as usize).saturating_sub(0x6000);
        self.prg_ram[rel % self.prg_ram.len()]
    }

    #[inline]
    fn prg_ram_write(&mut self, addr: u16, value: u8) {
        if self.prg_ram.is_empty() {
            return;
        }
        let rel = (addr as usize).saturating_sub(0x6000);
        let idx = rel % self.prg_ram.len();
        self.prg_ram[idx] = value;
    }

    #[inline]
    fn chr_read(&self, addr: u16) -> u8 {
        if self.chr.is_empty() {
            return 0;
        }
        self.chr[(addr as usize) & 0x1FFF] // 8 KiB mask for CHR region
    }

    #[inline]
    fn chr_write(&mut self, addr: u16, value: u8) {
        if !self.chr_is_ram || self.chr.is_empty() {
            return;
        }
        let idx = (addr as usize) & 0x1FFF;
        self.chr[idx] = value;
    }

    /// Returns true if this is an NROM-128 (16 KiB PRG) ROM.
    pub fn is_nrom_128(&self) -> bool {
        self.prg_rom.len() == 16 * 1024
    }

    /// Returns true if this is an NROM-256 (32 KiB PRG) ROM.
    pub fn is_nrom_256(&self) -> bool {
        self.prg_rom.len() == 32 * 1024
    }

    /// Returns true if PRG RAM is present.
    pub fn has_prg_ram(&self) -> bool {
        !self.prg_ram.is_empty()
    }

    /// Returns true if CHR is RAM (writable).
    pub fn chr_is_ram(&self) -> bool {
        self.chr_is_ram
    }
}

impl Mapper for Nrom {
    #[inline]
    fn mapper_id(&self) -> u16 {
        0
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => self.prg_ram_read(addr),
            0x8000..=0xFFFF => self.prg_rom_read(addr),
            _ => 0xFF, // not mapped by mapper (Bus should not forward other ranges)
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x6000..=0x7FFF => self.prg_ram_write(addr, value),
            0x8000..=0xFFFF => {
                // NROM has no PRG ROM registers; ignore writes.
            }
            _ => { /* ignore */ }
        }
    }

    fn ppu_read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => self.chr_read(addr),
            _ => 0, // mapper does not handle $2000+
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF => self.chr_write(addr, value),
            _ => { /* ignore */ }
        }
    }

    fn reset(&mut self) {
        // No dynamic banks or IRQs to reset in NROM.
    }

    fn irq_pending(&self) -> bool {
        false
    }
}

// Small helper because stable core lacked is_power_of_two for usize on older compilers.
// Keep a local polyfill to maintain compatibility.

#[cfg(test)]
mod tests {
    use super::{Mapper, Nrom};

    #[test]
    fn nrom_32k_prg_basic() {
        let prg = vec![0xAA; 32 * 1024];
        let chr = vec![0xCC; 8 * 1024];
        let mut nrom = Nrom::new(prg, chr, false, 8 * 1024);

        // PRG ROM reads
        assert_eq!(nrom.cpu_read(0x8000), 0xAA);
        assert_eq!(nrom.cpu_read(0xFFFF), 0xAA);

        // PRG RAM read/write
        nrom.cpu_write(0x6000, 0x42);
        assert_eq!(nrom.cpu_read(0x6000), 0x42);

        // CHR ROM read (write ignored)
        assert_eq!(nrom.ppu_read(0x0000), 0xCC);
        nrom.ppu_write(0x0000, 0x11);
        assert_eq!(nrom.ppu_read(0x0000), 0xCC);
    }

    #[test]
    fn nrom_16k_prg_mirroring() {
        // Make the two 16 KiB halves distinguishable when mirrored.
        let mut prg = vec![0x00; 16 * 1024];
        prg[0] = 0x12; // at $8000
        prg[0x3FFF] = 0x34; // at $BFFF
        let chr = vec![0; 8 * 1024];
        let mut nrom = Nrom::new(prg, chr, true, 0);

        // $8000-$BFFF: direct
        assert_eq!(nrom.cpu_read(0x8000), 0x12);
        assert_eq!(nrom.cpu_read(0xBFFF), 0x34);

        // $C000-$FFFF: mirror of first 16 KiB
        assert_eq!(nrom.cpu_read(0xC000), 0x12);
        assert_eq!(nrom.cpu_read(0xFFFF), 0x34);
    }

    #[test]
    fn chr_ram_is_writable() {
        let prg = vec![0xAA; 32 * 1024];
        let chr = vec![0x00; 8 * 1024];
        let mut nrom = Nrom::new(prg, chr, true, 0);

        assert_eq!(nrom.ppu_read(0x0001), 0x00);
        nrom.ppu_write(0x0001, 0x77);
        assert_eq!(nrom.ppu_read(0x0001), 0x77);
    }
}
