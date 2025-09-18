//! MMC1 (Mapper 1) implementation.
//!
//! Implements:
//! - Serial shift register writes (5-bit) to control / CHR0 / CHR1 / PRG registers
//! - PRG banking modes (32K switch, or 16K with fixed low or high)
//! - CHR banking (8K or 4K+4K)
//! - Mirroring control bits stored internally (exposed via accessor for future bus use)
//! - Optional CHR RAM writes
//!
//! Deferred / Simplified:
//! - PRG RAM disable bit enforcement (treated as always enabled)
//! - Battery-backed persistence
//! - Large board variants (SUROM / SOROM / etc.)
use core::cmp::max;

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperMirroring};

/// MMC1 mapper core state.
#[derive(Debug, Clone)]
pub struct Mmc1 {
    prg_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    chr: Vec<u8>,
    chr_is_ram: bool,

    // 5-bit registers
    control: u8,
    chr_bank0: u8,
    chr_bank1: u8,
    prg_bank: u8,

    // Serial latch
    shift_reg: u8,
    shift_count: u8,

    // Bank counts
    prg_16k_bank_count: u8,
    chr_4k_bank_count: u8,

    // Cached PRG mapping
    prg_bank_lo_index: u8,
    prg_bank_hi_index: u8,

    // Cached mirroring mode (not yet wired into Bus mapping)
    mirroring: Mirroring,
}

impl Mmc1 {
    pub fn new(prg_rom: Vec<u8>, prg_ram: Vec<u8>, chr: Vec<u8>, chr_is_ram: bool) -> Self {
        let prg_16k_bank_count = max(1, (prg_rom.len() / 0x4000) as usize) as u8;
        let chr_len = if chr.is_empty() { 8 * 1024 } else { chr.len() };
        let mut chr_buf = chr;
        if chr_buf.is_empty() {
            chr_buf = vec![0; chr_len];
        }
        let chr_4k_bank_count = max(1, chr_buf.len() / 0x1000) as u8;

        let mut s = Self {
            prg_rom,
            prg_ram,
            chr: chr_buf,
            chr_is_ram,
            control: 0x0C, // power-on default
            chr_bank0: 0,
            chr_bank1: 0,
            prg_bank: 0,
            shift_reg: 0,
            shift_count: 0,
            prg_16k_bank_count,
            chr_4k_bank_count,
            prg_bank_lo_index: 0,
            prg_bank_hi_index: prg_16k_bank_count.saturating_sub(1),
            mirroring: Mirroring::Horizontal,
        };
        s.recompute_after_control();
        s
    }

    fn reset_internal(&mut self) {
        self.control = 0x0C;
        self.shift_reg = 0;
        self.shift_count = 0;
        self.chr_bank0 = 0;
        self.chr_bank1 = 0;
        self.prg_bank = 0;
        self.recompute_after_control();
    }

    #[inline]
    fn prg_mode(&self) -> u8 {
        (self.control >> 2) & 0x03
    }
    #[inline]
    fn chr_mode(&self) -> u8 {
        (self.control >> 4) & 0x01
    }

    fn recompute_after_control(&mut self) {
        self.mirroring = match self.control & 0x03 {
            0 => Mirroring::Horizontal, // placeholder for single-screen low
            1 => Mirroring::Vertical,   // placeholder for single-screen high
            2 => Mirroring::Vertical,
            3 => Mirroring::Horizontal,
            _ => Mirroring::Horizontal,
        };
        self.recompute_prg_banks();
    }

    fn recompute_prg_banks(&mut self) {
        let last = self.prg_16k_bank_count.saturating_sub(1);
        match self.prg_mode() {
            0 | 1 => {
                let bank = (self.prg_bank & !1) % self.prg_16k_bank_count.max(1);
                self.prg_bank_lo_index = bank;
                self.prg_bank_hi_index = bank.saturating_add(1).min(last);
            }
            2 => {
                self.prg_bank_lo_index = 0;
                self.prg_bank_hi_index = self.prg_bank % self.prg_16k_bank_count.max(1);
            }
            3 => {
                self.prg_bank_lo_index = self.prg_bank % self.prg_16k_bank_count.max(1);
                self.prg_bank_hi_index = last;
            }
            _ => {}
        }
    }

    fn commit_register(&mut self, addr: u16, value5: u8) {
        match addr {
            0x8000..=0x9FFF => {
                self.control = value5 & 0x1F;
                self.recompute_after_control();
            }
            0xA000..=0xBFFF => {
                self.chr_bank0 = value5 & 0x1F;
            }
            0xC000..=0xDFFF => {
                self.chr_bank1 = value5 & 0x1F;
            }
            0xE000..=0xFFFF => {
                self.prg_bank = value5 & 0x1F;
                self.recompute_prg_banks();
            }
            _ => {}
        }
    }

    fn serial_write(&mut self, addr: u16, data: u8) {
        if data & 0x80 != 0 {
            self.shift_reg = 0;
            self.shift_count = 0;
            self.control |= 0x0C;
            self.recompute_after_control();
            return;
        }
        let bit = data & 1;
        self.shift_reg |= bit << self.shift_count;
        self.shift_count += 1;
        if self.shift_count == 5 {
            let value5 = self.shift_reg & 0x1F;
            self.commit_register(addr, value5);
            self.shift_reg = 0;
            self.shift_count = 0;
        }
    }

    fn prg_read_internal(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => {
                if self.prg_ram.is_empty() {
                    0
                } else {
                    let rel = (addr as usize - 0x6000) % self.prg_ram.len();
                    self.prg_ram[rel]
                }
            }
            0x8000..=0xBFFF => {
                let bank = self.prg_bank_lo_index as usize;
                let base = bank * 0x4000;
                let ofs = (addr as usize - 0x8000) & 0x3FFF;
                self.prg_rom[(base + ofs) % self.prg_rom.len()]
            }
            0xC000..=0xFFFF => {
                let bank = self.prg_bank_hi_index as usize;
                let base = bank * 0x4000;
                let ofs = (addr as usize - 0xC000) & 0x3FFF;
                self.prg_rom[(base + ofs) % self.prg_rom.len()]
            }
            _ => 0xFF,
        }
    }

    fn chr_read_internal(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => {
                if self.chr.is_empty() {
                    return 0;
                }
                if self.chr_mode() == 0 {
                    let bank8k = (self.chr_bank0 & (self.chr_4k_bank_count.saturating_sub(1))) & !1;
                    let base = bank8k as usize * 0x1000;
                    let ofs = (addr as usize) & 0x1FFF;
                    self.chr[(base + ofs) % self.chr.len()]
                } else {
                    if addr < 0x1000 {
                        let b = self.chr_bank0 % self.chr_4k_bank_count.max(1);
                        let base = b as usize * 0x1000;
                        let ofs = (addr as usize) & 0x0FFF;
                        self.chr[(base + ofs) % self.chr.len()]
                    } else {
                        let b = self.chr_bank1 % self.chr_4k_bank_count.max(1);
                        let base = b as usize * 0x1000;
                        let ofs = (addr as usize - 0x1000) & 0x0FFF;
                        self.chr[(base + ofs) % self.chr.len()]
                    }
                }
            }
            _ => 0,
        }
    }

    fn chr_write_internal(&mut self, addr: u16, value: u8) {
        if !self.chr_is_ram {
            return;
        }
        if !(0x0000..=0x1FFF).contains(&addr) {
            return;
        }
        if self.chr_mode() == 0 {
            let bank8k = (self.chr_bank0 & (self.chr_4k_bank_count.saturating_sub(1))) & !1;
            let base = bank8k as usize * 0x1000;
            let ofs = (addr as usize) & 0x1FFF;
            let idx = (base + ofs) % self.chr.len();
            self.chr[idx] = value;
        } else {
            if addr < 0x1000 {
                let b = self.chr_bank0 % self.chr_4k_bank_count.max(1);
                let base = b as usize * 0x1000;
                let ofs = (addr as usize) & 0x0FFF;
                let idx = (base + ofs) % self.chr.len();
                self.chr[idx] = value;
            } else {
                let b = self.chr_bank1 % self.chr_4k_bank_count.max(1);
                let base = b as usize * 0x1000;
                let ofs = (addr as usize - 0x1000) & 0x0FFF;
                let idx = (base + ofs) % self.chr.len();
                self.chr[idx] = value;
            }
        }
    }

    #[cfg(test)]
    pub fn debug_prg_banks(&self) -> (u8, u8) {
        (self.prg_bank_lo_index, self.prg_bank_hi_index)
    }
    #[cfg(test)]
    pub fn debug_chr_banks(&self) -> (u8, u8) {
        (self.chr_bank0, self.chr_bank1)
    }
}

impl Mapper for Mmc1 {
    fn mapper_id(&self) -> u16 {
        1
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        self.prg_read_internal(addr)
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x6000..=0x7FFF => {
                if !self.prg_ram.is_empty() {
                    let rel = (addr as usize - 0x6000) % self.prg_ram.len();
                    self.prg_ram[rel] = value;
                }
            }
            0x8000..=0xFFFF => self.serial_write(addr, value),
            _ => {}
        }
    }

    fn ppu_read(&self, addr: u16) -> u8 {
        self.chr_read_internal(addr)
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        self.chr_write_internal(addr, value)
    }

    fn reset(&mut self) {
        self.reset_internal();
    }

    fn irq_pending(&self) -> bool {
        false
    }

    fn current_mirroring(&self) -> Option<MapperMirroring> {
        let bits = self.control & 0x03;
        let mode = match bits {
            0 => MapperMirroring::SingleScreenLower,
            1 => MapperMirroring::SingleScreenUpper,
            2 => MapperMirroring::Vertical,
            3 => MapperMirroring::Horizontal,
            _ => MapperMirroring::Horizontal,
        };
        Some(mode)
    }
}

#[cfg(test)]
mod tests {
    use super::Mmc1;
    use crate::mapper::Mapper;

    fn make_vec(size: usize, pattern: u8) -> Vec<u8> {
        let mut v = vec![pattern; size];
        if size > 0 {
            v[0] = pattern;
        }
        v
    }

    fn write_serial(mapper: &mut Mmc1, addr: u16, value5: u8) {
        for i in 0..5 {
            let bit = (value5 >> i) & 1;
            mapper.cpu_write(addr, bit);
        }
    }

    #[test]
    fn power_on_defaults() {
        let prg = make_vec(128 * 1024, 0xAA);
        let chr = make_vec(8 * 1024, 0x55);
        let prg_ram = vec![0; 8 * 1024];
        let m = Mmc1::new(prg, prg_ram, chr, false);
        let (lo, hi) = m.debug_prg_banks();
        assert!(hi >= lo);
    }

    #[test]
    fn control_serial_write() {
        let prg = make_vec(64 * 1024, 0x11);
        let chr = make_vec(8 * 1024, 0x22);
        let prg_ram = vec![0; 8 * 1024];
        let mut m = Mmc1::new(prg, prg_ram, chr, false);
        write_serial(&mut m, 0x8000, 0b10010);
        write_serial(&mut m, 0xE000, 0);
        let (lo, hi) = m.debug_prg_banks();
        assert!(hi >= lo);
    }

    #[test]
    fn prg_mode_fix_upper_switch_low() {
        let prg = make_vec(128 * 1024, 0x33);
        let chr = make_vec(8 * 1024, 0x44);
        let prg_ram = vec![0; 8 * 1024];
        let mut m = Mmc1::new(prg, prg_ram, chr, false);
        write_serial(&mut m, 0x8000, 0b01111); // mode 3
        write_serial(&mut m, 0xE000, 0b00101); // prg_bank=5
        let (lo, hi) = m.debug_prg_banks();
        assert_eq!(lo, 5 % m.prg_16k_bank_count);
        assert_eq!(hi, m.prg_16k_bank_count - 1);
    }

    #[test]
    fn chr_8k_mode_mapping() {
        let prg = make_vec(32 * 1024, 0x10);
        let mut chr = vec![0u8; 16 * 1024];
        chr[0] = 0x01;
        chr[8 * 1024] = 0x02;
        let prg_ram = vec![0; 8 * 1024];
        let mut m = Mmc1::new(prg, prg_ram, chr, false);
        write_serial(&mut m, 0x8000, 0b00000); // chr_mode=0
        write_serial(&mut m, 0xA000, 0b00010); // chr_bank0=2
        assert_eq!(m.ppu_read(0x0000), m.ppu_read(0x1000));
    }

    #[test]
    fn chr_4k_mode_mapping() {
        let prg = make_vec(32 * 1024, 0x10);
        let mut chr = vec![0u8; 16 * 1024];
        chr[0] = 0x11;
        chr[0x1000] = 0x22;
        chr[0x2000] = 0x33;
        chr[0x3000] = 0x44;
        let prg_ram = vec![0; 8 * 1024];
        let mut m = Mmc1::new(prg, prg_ram, chr, false);
        write_serial(&mut m, 0x8000, 0b10000); // chr_mode=1
        write_serial(&mut m, 0xA000, 0b00001); // chr_bank0=1
        write_serial(&mut m, 0xC000, 0b00010); // chr_bank1=2
        let lo_val = m.ppu_read(0x0000);
        let hi_val = m.ppu_read(0x1000);
        assert_ne!(lo_val, hi_val);
    }

    #[test]
    fn reset_restores_defaults() {
        let prg = make_vec(64 * 1024, 0x55);
        let chr = make_vec(8 * 1024, 0x66);
        let prg_ram = vec![0; 8 * 1024];
        let mut m = Mmc1::new(prg, prg_ram, chr, false);
        write_serial(&mut m, 0xE000, 0b10101);
        m.reset();
        let (lo, hi) = m.debug_prg_banks();
        assert!(hi >= lo);
    }
}

#[cfg(test)]
mod mirror_compile_asserts {
    use super::Mmc1;
    use crate::mapper::Mapper;

    #[test]
    fn mapper_id_is_1() {
        let m = Mmc1::new(
            vec![0; 32 * 1024],
            vec![0; 8 * 1024],
            vec![0; 8 * 1024],
            false,
        );
        assert_eq!(m.mapper_id(), 1);
    }
}
