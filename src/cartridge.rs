/*!
Cartridge with iNES (v1) loader and Mapper integration (NROM/mapper 0).

Features:
- Parse iNES (v1) header from bytes or file path
- Extract PRG ROM, CHR (ROM or allocate CHR RAM when CHR size == 0), and PRG RAM size
- Determine mirroring, battery-backed RAM, mapper ID (supports mapper 0)
- Construct a concrete Mapper (NROM) and delegate CPU/PPU mapping through it

Notes:
- iNES 2.0 is detected and currently rejected with an error.
- PRG RAM allocation policy:
  - If header byte 8 (PRG-RAM size in 8 KiB units) is 0, allocate 8 KiB by convention.
  - Otherwise allocate size_in_units * 8 KiB.
- NROM mapping rules (via Mapper):
  - 16 KiB PRG (NROM-128): $8000-$BFFF maps to the single 16 KiB bank; $C000-$FFFF mirrors it.
  - 32 KiB PRG (NROM-256): $8000-$FFFF maps directly to 32 KiB.
*/

use std::cell::RefCell;
use std::fs;
use std::path::Path;

use crate::mapper::{Mapper, Nrom};
use crate::mappers::{Cnrom, Mmc1, Mmc3};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Mirroring {
    Horizontal,
    Vertical,
    FourScreen,
}

pub struct Cartridge {
    // Mapper trait object; interior mutability to allow read methods to delegate.
    pub mapper: RefCell<Box<dyn Mapper>>,

    // Metadata
    mapper_id: u16,
    mirroring: Mirroring,
    battery: bool,
    has_trainer: bool,
    pub ines_version: InesVersion,

    // Size metadata for convenience accessors
    prg_rom_len: usize,
    chr_len: usize,
    prg_ram_len: usize,
    chr_is_ram: bool,
}

// Debug implemented manually for Cartridge below
impl std::fmt::Debug for Cartridge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cartridge")
            .field("mapper_id", &self.mapper_id)
            .field("mirroring", &self.mirroring)
            .field("battery", &self.battery)
            .field("has_trainer", &self.has_trainer)
            .field("ines_version", &self.ines_version)
            .field("prg_rom_len", &self.prg_rom_len)
            .field("chr_len", &self.chr_len)
            .field("prg_ram_len", &self.prg_ram_len)
            .field("chr_is_ram", &self.chr_is_ram)
            .finish()
    }
}
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum InesVersion {
    Ines1,
    Ines2, // currently unsupported
}

impl Cartridge {
    // -------------- Construction --------------

    /// Load a cartridge from raw iNES bytes and construct a Mapper (currently NROM only).
    pub fn from_ines_bytes(data: &[u8]) -> Result<Self, String> {
        if data.len() < 16 {
            return Err("Data too small for iNES header".into());
        }

        // Header: 16 bytes
        // 0-3: 'N', 'E', 'S', 0x1A
        if &data[0..4] != b"NES\x1A" {
            return Err("Invalid iNES header magic (expected NES<1A>)".into());
        }

        let prg_rom_16k_units = data[4] as usize;
        let chr_rom_8k_units = data[5] as usize;
        let flags6 = data[6];
        let flags7 = data[7];
        let prg_ram_8k_units = data.get(8).copied().unwrap_or(0) as usize;

        // Detect iNES 2.0
        // NES 2.0 if (flags7 & 0x0C) == 0x08. We reject for now.
        let is_ines2 = (flags7 & 0x0C) == 0x08;
        let version = if is_ines2 {
            InesVersion::Ines2
        } else {
            InesVersion::Ines1
        };
        if is_ines2 {
            return Err("NES 2.0 format is not supported yet".into());
        }

        // Mapper ID: high nibble from flags7 and low nibble from flags6
        let mapper_low = (flags6 >> 4) as u16;
        let mapper_high = (flags7 & 0xF0) as u16;
        let mapper_id = mapper_high | mapper_low;

        // Mirroring and battery/trainer flags
        let four_screen = (flags6 & 0b0000_1000) != 0;
        let vertical_mirroring = (flags6 & 0b0000_0001) != 0;
        let mirroring = if four_screen {
            Mirroring::FourScreen
        } else if vertical_mirroring {
            Mirroring::Vertical
        } else {
            Mirroring::Horizontal
        };
        let battery = (flags6 & 0b0000_0010) != 0;
        let has_trainer = (flags6 & 0b0000_0100) != 0;

        // Offset to PRG ROM data
        let mut offset = 16usize;
        if has_trainer {
            // Trainer is 512 bytes right after header
            if data.len() < offset + 512 {
                return Err("Data too small for iNES trainer".into());
            }
            offset += 512;
        }

        // PRG ROM size in bytes
        let prg_rom_len = prg_rom_16k_units
            .checked_mul(16 * 1024)
            .ok_or_else(|| "PRG ROM size overflow".to_string())?;
        // CHR size in bytes; if zero, allocate CHR RAM (8 KiB)
        let (chr_len, chr_is_ram) = if chr_rom_8k_units == 0 {
            (8 * 1024, true)
        } else {
            (
                chr_rom_8k_units
                    .checked_mul(8 * 1024)
                    .ok_or_else(|| "CHR ROM size overflow".to_string())?,
                false,
            )
        };

        if data.len() < offset + prg_rom_len {
            return Err("Data too small for PRG ROM".into());
        }
        let prg_rom = data[offset..offset + prg_rom_len].to_vec();
        offset += prg_rom_len;

        let chr = if chr_is_ram {
            // Allocate CHR RAM (8 KiB)
            vec![0; chr_len]
        } else {
            if data.len() < offset + chr_len {
                return Err("Data too small for CHR ROM".into());
            }
            data[offset..offset + chr_len].to_vec()
        };

        // PRG RAM size: if 0, allocate 8KiB by convention
        let prg_ram_len = if prg_ram_8k_units == 0 {
            8 * 1024
        } else {
            prg_ram_8k_units
                .checked_mul(8 * 1024)
                .ok_or_else(|| "PRG RAM size overflow".to_string())?
        };

        // Mapper factory:
        // 0: NROM implemented
        // 3: CNROM implemented (CHR bank switching)
        // 1,4: placeholders (MMC1 / MMC3) pending implementation
        let mapper: Box<dyn Mapper> = match mapper_id {
            0 => Box::new(Nrom::new(prg_rom, chr, chr_is_ram, prg_ram_len)),
            1 => {
                let prg_ram = vec![0; prg_ram_len];
                Box::new(Mmc1::new(prg_rom, prg_ram, chr, chr_is_ram))
            }
            3 => Box::new(Cnrom::new(prg_rom, chr, chr_is_ram)),
            4 => {
                let prg_ram = vec![0; prg_ram_len];
                Box::new(Mmc3::new(prg_rom, prg_ram, chr, chr_is_ram))
            }
            _ => return Err(format!("Unsupported mapper id: {}", mapper_id)),
        };

        Ok(Self {
            mapper: RefCell::new(mapper),
            mapper_id,
            mirroring,
            battery,
            has_trainer,
            ines_version: version,
            prg_rom_len,
            chr_len,
            prg_ram_len,
            chr_is_ram,
        })
    }

    /// Load a cartridge from an iNES file (.nes).
    pub fn from_ines_file<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let bytes = fs::read(path).map_err(|e| format!("Failed to read iNES file: {e}"))?;
        Self::from_ines_bytes(&bytes)
    }

    // -------------- CPU PRG mapping convenience (delegating to Mapper) --------------

    /// Read a byte from PRG ROM space ($8000..=$FFFF) via the mapper.
    pub fn cpu_read_prg_rom(&self, addr: u16) -> u8 {
        self.mapper.borrow_mut().cpu_read(addr)
    }

    /// Writes to PRG ROM space ($8000..=$FFFF), delegated to the mapper (ignored by NROM).
    pub fn cpu_write_prg_rom(&mut self, addr: u16, value: u8) {
        self.mapper.get_mut().cpu_write(addr, value);
    }

    /// Read a byte from PRG RAM space ($6000..=$7FFF) via the mapper.
    /// Normalizes any input address to wrap within the PRG RAM window.
    pub fn cpu_read_prg_ram(&self, addr: u16) -> u8 {
        if self.prg_ram_len == 0 {
            return 0;
        }
        let base = 0x6000u16;
        let rel = (addr as usize).saturating_sub(base as usize);
        let idx = rel % self.prg_ram_len;
        let eff = base.wrapping_add(idx as u16);
        self.mapper.borrow_mut().cpu_read(eff)
    }

    /// Write a byte to PRG RAM space ($6000..=$7FFF) via the mapper.
    /// Normalizes any input address to wrap within the PRG RAM window.
    pub fn cpu_write_prg_ram(&mut self, addr: u16, value: u8) {
        if self.prg_ram_len == 0 {
            return;
        }
        let base = 0x6000u16;
        let rel = (addr as usize).saturating_sub(base as usize);
        let idx = rel % self.prg_ram_len;
        let eff = base.wrapping_add(idx as u16);
        self.mapper.get_mut().cpu_write(eff, value);
    }

    // -------------- Accessors --------------

    pub fn mapper_id(&self) -> u16 {
        self.mapper_id
    }

    pub fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    pub fn battery_backed(&self) -> bool {
        self.battery
    }

    pub fn has_prg_ram(&self) -> bool {
        self.prg_ram_len > 0
    }

    pub fn prg_rom_len(&self) -> usize {
        self.prg_rom_len
    }

    pub fn chr_len(&self) -> usize {
        self.chr_len
    }

    pub fn prg_ram_len(&self) -> usize {
        self.prg_ram_len
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::build_ines;

    // Using shared test utils: build_ines from crate::test_utils

    #[test]
    fn parse_simple_nrom_32k_chr8k() {
        // mapper 0, vertical mirroring, no trainer, battery off
        let flags6 = 0b0000_0001; // vertical mirroring
        let flags7 = 0u8;
        let data = build_ines(2, 1, flags6, flags7, 1, None);
        let cart = Cartridge::from_ines_bytes(&data).expect("parse");

        assert_eq!(cart.mapper_id(), 0);
        assert_eq!(cart.mirroring(), Mirroring::Vertical);
        assert!(cart.has_prg_ram());
        assert_eq!(cart.prg_rom_len(), 32 * 1024);
        assert_eq!(cart.chr_len(), 8 * 1024);

        // Test PRG ROM mapping at ends
        assert_eq!(cart.cpu_read_prg_rom(0x8000), 0xAA);
        assert_eq!(cart.cpu_read_prg_rom(0xFFFF), 0xAA);
    }

    #[test]
    fn parse_nrom_16k_chr_ram() {
        // mapper 0, horizontal mirroring, no trainer, PRG RAM 0 (allocate 8K)
        let flags6 = 0b0000_0000; // horizontal
        let flags7 = 0u8;
        let data = build_ines(1, 0, flags6, flags7, 0, None);
        let cart = Cartridge::from_ines_bytes(&data).expect("parse");

        assert_eq!(cart.mapper_id(), 0);
        assert_eq!(cart.mirroring(), Mirroring::Horizontal);
        assert!(cart.has_prg_ram());
        assert_eq!(cart.prg_rom_len(), 16 * 1024);
        assert_eq!(cart.chr_len(), 8 * 1024); // allocated CHR RAM

        // NROM-128 mirroring check
        let first_half = cart.cpu_read_prg_rom(0x8000);
        let second_half = cart.cpu_read_prg_rom(0xC000);
        assert_eq!(first_half, second_half);
    }

    #[test]
    fn trainer_moves_data_offset() {
        // Include a trainer and ensure parsing doesn't panic (we don't store trainer).
        let mut trainer = [0u8; 512];
        for (i, b) in trainer.iter_mut().enumerate() {
            *b = (i & 0xFF) as u8;
        }
        let flags6 = 0b0000_0100; // trainer present
        let flags7 = 0u8;
        let data = build_ines(1, 1, flags6, flags7, 1, Some(&trainer));
        let cart = Cartridge::from_ines_bytes(&data).expect("parse");
        assert_eq!(cart.mapper_id(), 0);
        assert!(cart.has_prg_ram());
    }

    #[test]
    fn ines2_rejected() {
        // flags7 indicates iNES2 (bits 2..3 = 0b10)
        let flags6 = 0u8;
        let flags7 = 0b0000_1000;
        let data = build_ines(1, 1, flags6, flags7, 1, None);
        let err = Cartridge::from_ines_bytes(&data).unwrap_err();
        assert!(err.contains("NES 2.0"));
    }

    #[test]
    fn prg_ram_read_write() {
        let flags6 = 0u8;
        let flags7 = 0u8;
        let data = build_ines(2, 1, flags6, flags7, 1, None);
        let mut cart = Cartridge::from_ines_bytes(&data).expect("parse");

        // Write to PRG RAM and read back
        cart.cpu_write_prg_ram(0x6000, 0x42);
        assert_eq!(cart.cpu_read_prg_ram(0x6000), 0x42);

        // Wrap within PRG RAM size
        let len = cart.prg_ram_len();
        if len > 0 {
            cart.cpu_write_prg_ram(0x6000 + (len as u16), 0x99);
            assert_eq!(cart.cpu_read_prg_ram(0x6000), 0x99);
        }
    }
}
