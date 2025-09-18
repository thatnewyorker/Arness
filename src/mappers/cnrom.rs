/*
CNROM (Mapper 3) implementation.

Characteristics:
- PRG: Fixed (16 KiB mirrored or 32 KiB direct) at $8000-$FFFF; no PRG banking.
- CHR: Switchable in 8 KiB banks via CPU writes to $8000-$FFFF (bank select register).
- Mirroring: Determined solely by iNES header (no dynamic control by the mapper).
- No IRQ generation.

Bank Select:
- Typical hardware uses the lower 2 bits of the value written to select the CHR bank.
- This implementation generalizes via modulo over the number of available 8 KiB CHR banks
  (robust in case of non-standard sizes).

CHR RAM:
- If `chr_is_ram` is true, writes to $0000-$1FFF modify the currently mapped CHR bank.

Reset Behavior:
- CHR bank is reset to 0.

Testing:
- Unit tests cover PRG mirroring/direct mapping, CHR bank switching, wrapping, RAM writes,
  and reset behavior.
*/

use crate::mapper::Mapper;

#[derive(Debug, Clone)]
pub struct Cnrom {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    chr_is_ram: bool,
    prg_is_16k: bool,
    chr_bank: u8,
    chr_bank_count: u8,
}

impl Cnrom {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, chr_is_ram: bool) -> Self {
        let prg_len = prg_rom.len();
        debug_assert!(
            prg_len == 16 * 1024 || prg_len == 32 * 1024,
            "CNROM PRG must be 16K or 32K"
        );
        let chr_len = chr.len();
        debug_assert!(
            chr_len % (8 * 1024) == 0 && chr_len > 0,
            "CNROM CHR must be a non-zero multiple of 8K"
        );
        let chr_bank_count = (chr_len / (8 * 1024)).max(1) as u8;
        Self {
            prg_rom,
            chr,
            chr_is_ram,
            prg_is_16k: prg_len == 16 * 1024,
            chr_bank: 0,
            chr_bank_count,
        }
    }

    #[cfg(test)]
    pub(crate) fn current_chr_bank(&self) -> u8 {
        self.chr_bank
    }
}

impl Mapper for Cnrom {
    fn mapper_id(&self) -> u16 {
        3
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xFFFF => {
                if self.prg_is_16k {
                    let rel = (addr - 0x8000) as usize & 0x3FFF;
                    self.prg_rom[rel]
                } else {
                    let rel = (addr - 0x8000) as usize & 0x7FFF;
                    self.prg_rom[rel]
                }
            }
            _ => 0xFF,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            // Bank select (modulo number of CHR banks)
            let bank = if self.chr_bank_count > 0 {
                value % self.chr_bank_count
            } else {
                0
            };
            self.chr_bank = bank;
        }
    }

    fn ppu_read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => {
                let bank_base = (self.chr_bank as usize) * 8 * 1024;
                let offset = (addr as usize) & 0x1FFF;
                self.chr[bank_base + offset]
            }
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        if self.chr_is_ram && (0x0000..=0x1FFF).contains(&addr) {
            let bank_base = (self.chr_bank as usize) * 8 * 1024;
            let offset = (addr as usize) & 0x1FFF;
            let idx = bank_base + offset;
            if idx < self.chr.len() {
                self.chr[idx] = value;
            }
        }
    }

    fn reset(&mut self) {
        self.chr_bank = 0;
    }

    fn irq_pending(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::Cnrom;
    use crate::mapper::Mapper;

    fn make_prg(size_k: usize, fill_a: u8, fill_b: u8) -> Vec<u8> {
        let size = size_k * 1024;
        let mut v = vec![fill_a; size];
        if size > 0 {
            v[size - 1] = fill_b;
        }
        v
    }

    #[test]
    fn prg_16k_mirroring() {
        let prg = make_prg(16, 0xAA, 0xBB);
        let chr = vec![0x11; 8 * 1024];
        let mut m = Cnrom::new(prg, chr, false);
        assert_eq!(m.cpu_read(0x8000), 0xAA);
        assert_eq!(m.cpu_read(0xBFFF), 0xBB);
        // Mirror: $C000-$FFFF same 16K
        assert_eq!(m.cpu_read(0xC000), 0xAA);
        assert_eq!(m.cpu_read(0xFFFF), 0xBB);
    }

    #[test]
    fn prg_32k_direct() {
        let prg = make_prg(32, 0x10, 0x20);
        let chr = vec![0x22; 8 * 1024];
        let mut m = Cnrom::new(prg, chr, false);
        assert_eq!(m.cpu_read(0x8000), 0x10);
        assert_eq!(m.cpu_read(0xFFFF), 0x20);
    }

    #[test]
    fn chr_bank_switching() {
        // Two banks (16K CHR)
        let mut chr = vec![0u8; 16 * 1024];
        chr[0] = 0x01; // bank 0 offset 0
        chr[8 * 1024] = 0x02; // bank 1 offset 0
        let prg = make_prg(16, 0, 0);
        let mut m = Cnrom::new(prg, chr, false);
        // Initial bank 0
        assert_eq!(m.ppu_read(0x0000), 0x01);
        m.cpu_write(0x8000, 0x01); // switch to bank 1
        assert_eq!(m.ppu_read(0x0000), 0x02);
    }

    #[test]
    fn chr_bank_wrap() {
        let chr = vec![0xAB; 16 * 1024]; // two banks
        let prg = make_prg(16, 0, 0);
        let mut m = Cnrom::new(prg, chr, false);
        m.cpu_write(0x8000, 5); // 5 % 2 = 1
        assert_eq!(m.current_chr_bank(), 1);
    }

    #[test]
    fn chr_ram_write_roundtrip() {
        let chr = vec![0x00; 16 * 1024];
        let prg = make_prg(16, 0, 0);
        let mut m = Cnrom::new(prg, chr, true);
        // Write into bank 0
        m.ppu_write(0x0002, 0x7F);
        assert_eq!(m.ppu_read(0x0002), 0x7F);
        // Switch to bank 1, unchanged there
        m.cpu_write(0x8000, 1);
        assert_eq!(m.ppu_read(0x0002), 0x00);
    }

    #[test]
    fn reset_restores_bank0() {
        let chr = vec![0x00; 16 * 1024];
        let prg = make_prg(16, 0, 0);
        let mut m = Cnrom::new(prg, chr, false);
        m.cpu_write(0x8000, 1);
        assert_eq!(m.current_chr_bank(), 1);
        m.reset();
        assert_eq!(m.current_chr_bank(), 0);
    }
}
