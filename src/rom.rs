use std::fs::File;
use std::io::Read;

#[derive(Debug)]
pub struct Rom {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    mapper: u16,
    submapper: u8,
    prg_size: usize,
    chr_size: usize,
    prg_ram_size: usize,
    chr_ram_size: usize,
    mirroring: Mirroring,
    has_trainer: bool,
    is_nes_2_0: bool,
}

#[derive(Debug, PartialEq, Clone)]
pub enum Mirroring {
    Horizontal,
    Vertical,
    FourScreen,
}

impl Rom {
    pub fn load_from_file(path: &str) -> Result<Self, String> {
        let mut file = File::open(path).map_err(|e| format!("Failed to open ROM file: {}", e))?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer).map_err(|e| format!("Failed to read ROM file: {}", e))?;

        if buffer.len() < 16 || &buffer[0..4] != b"NES\x1A" {
            return Err("Invalid NES file format".to_string());
        }

        let is_nes_2_0 = (buffer[7] & 0x0C) == 0x08;
        let (prg_size, chr_size) = if is_nes_2_0 && buffer.len() > 9 {
            let prg_low = buffer[4] as u32;
            let chr_low = buffer[5] as u32;
            let byte_9 = buffer[9] as u32;
            let prg_high = (byte_9 & 0x0F) << 8;
            let chr_high = (byte_9 & 0xF0) << 4;
            (
                if prg_low == 0xFF && prg_high > 0 {
                    let exponent = (prg_high & 0x07) + 1;
                    let mantissa = (prg_high >> 3) + 1;
                    ((mantissa * (1 << exponent)) * 1024) as usize
                } else {
                    ((prg_high | prg_low) * 16 * 1024) as usize
                },
                if chr_low == 0xFF && chr_high > 0 {
                    let exponent = (chr_high & 0x07) + 1;
                    let mantissa = (chr_high >> 3) + 1;
                    ((mantissa * (1 << exponent)) * 1024) as usize
                } else {
                    ((chr_high | chr_low) * 8 * 1024) as usize
                }
            )
        } else {
            (buffer[4] as usize * 16 * 1024, buffer[5] as usize * 8 * 1024)
        };

        let flags_6 = buffer[6];
        let flags_7 = buffer[7];
        let mapper_low = flags_6 >> 4;
        let mapper_high = flags_7 & 0xF0;
        let mapper = if is_nes_2_0 && buffer.len() > 8 {
            let mapper_upper = (buffer[8] & 0x0F) as u16;
            (mapper_upper << 8) | (mapper_high as u16) | (mapper_low as u16)
        } else {
            (mapper_high | mapper_low) as u16
        };
        let submapper = if is_nes_2_0 && buffer.len() > 8 {
            buffer[8] >> 4
        } else {
            0
        };

        let has_trainer = flags_6 & 0x04 != 0;
        let mirroring = if flags_6 & 0x08 != 0 {
            Mirroring::FourScreen
        } else if flags_6 & 0x01 != 0 {
            Mirroring::Vertical
        } else {
            Mirroring::Horizontal
        };

        let (prg_ram_size, chr_ram_size) = if is_nes_2_0 && buffer.len() > 11 {
            let prg_ram_shift = buffer[10] & 0x0F;
            let chr_ram_shift = buffer[11] & 0x0F;
            (
                if prg_ram_shift > 0 { 64 << prg_ram_shift } else { 0 },
                if chr_ram_shift > 0 { 64 << chr_ram_shift } else { 0 },
            )
        } else {
            (0, 0)
        };

        let mut offset = 16;
        if has_trainer {
            offset += 512;
        }
        let prg_start = offset;
        let prg_end = prg_start + prg_size;
        let chr_start = prg_end;
        let chr_end = chr_start + chr_size;

        if buffer.len() < chr_end {
            return Err("ROM file too small for specified PRG/CHR sizes".to_string());
        }

        let prg_rom = buffer[prg_start..prg_end].to_vec();
        let chr_rom = if chr_size > 0 {
            buffer[chr_start..chr_end].to_vec()
        } else {
            Vec::new()
        };

        Ok(Rom {
            prg_rom,
            chr_rom,
            mapper,
            submapper,
            prg_size,
            chr_size,
            prg_ram_size,
            chr_ram_size,
            mirroring,
            has_trainer,
            is_nes_2_0,
        })
    }

    pub fn prg_rom(&self) -> &[u8] {
        &self.prg_rom
    }

    pub fn chr_rom(&self) -> &[u8] {
        &self.chr_rom
    }

    pub fn mapper(&self) -> u16 {
        self.mapper
    }

    pub fn submapper(&self) -> u8 {
        self.submapper
    }

    pub fn mirroring(&self) -> Mirroring {
        self.mirroring.clone()
    }

    pub fn prg_ram_size(&self) -> usize {
        self.prg_ram_size
    }

    pub fn chr_ram_size(&self) -> usize {
        self.chr_ram_size
    }

    pub fn chr_size(&self) -> usize {
        self.chr_size
    }

    pub fn prg_size(&self) -> usize {
        self.prg_size
    }
}
