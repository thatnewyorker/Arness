//! Shared test utilities for building minimal iNES (v1) ROM images.
//!
//! These helpers de-duplicate iNES construction logic across tests in the
//! CPU, Bus, and Cartridge modules. They intentionally support just what
//! the test suite needs (NROM mapper, simple flags).
//!
//! Notes on iNES header fields used here:
//! - bytes[0..4] = b"NES\x1A"
//! - byte 4 = PRG ROM size in 16 KiB units
//! - byte 5 = CHR ROM size in 8 KiB units (0 => no CHR ROM; many emulators allocate 8 KiB CHR RAM)
//! - byte 6 = Flags 6 (mirroring, battery, trainer, mapper low nibble)
//! - byte 7 = Flags 7 (PlayChoice/NES 2.0 indicator, mapper high nibble)
//! - byte 8 = PRG RAM size in 8 KiB units (0 => commonly interpreted as 8 KiB by convention)
//! - bytes 9..15 = padding/reserved
//!
//! Vectors:
//! - For 16 KiB PRG (NROM-128): vectors are at PRG offset 0x3FFA..=0x3FFF
//! - For 32 KiB PRG (NROM-256): vectors are at PRG offset 0x7FFA..=0x7FFF
//!
//! These builders do minimal validation (sufficient for unit tests).

#![allow(dead_code)]

/// Build a minimal iNES (v1) image with configurable PRG/CHR sizes and flags.
///
/// - `prg_16k`: number of 16 KiB PRG units (1 => 16 KiB, 2 => 32 KiB)
/// - `chr_8k`: number of 8 KiB CHR units (0 => no CHR in file; many emulators allocate CHR RAM)
/// - `flags6`: iNES Flags 6 (mirroring, battery, trainer, mapper low nibble)
/// - `flags7`: iNES Flags 7 (mapper high nibble and NES 2.0 detection)
/// - `prg_ram_8k`: PRG RAM size in 8 KiB units (0 => allocate-by-convention behavior in our loader)
/// - `trainer`: optional 512-byte trainer to insert after header
pub fn build_ines(
    prg_16k: usize,
    chr_8k: usize,
    flags6: u8,
    flags7: u8,
    prg_ram_8k: u8,
    trainer: Option<&[u8; 512]>,
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(
        16 + trainer.map(|_| 512).unwrap_or(0) + prg_16k * 16 * 1024 + chr_8k * 8 * 1024,
    );

    // Header
    bytes.extend_from_slice(b"NES\x1A");
    bytes.push(prg_16k as u8);
    bytes.push(chr_8k as u8);
    bytes.push(flags6);
    bytes.push(flags7);
    bytes.push(prg_ram_8k);
    bytes.extend_from_slice(&[0u8; 7]);

    // Optional trainer
    if let Some(t) = trainer {
        bytes.extend_from_slice(t);
    }

    // PRG ROM payload (pattern-filled for tests)
    if prg_16k > 0 {
        bytes.extend(std::iter::repeat(0xAA).take(prg_16k * 16 * 1024));
    }

    // CHR ROM payload (if present)
    if chr_8k > 0 {
        bytes.extend(std::iter::repeat(0xCC).take(chr_8k * 8 * 1024));
    }

    bytes
}

/// Build a simple NROM iNES (v1) image that injects a caller-provided PRG program
/// (up to 16 KiB) into a single 16 KiB PRG bank and sets vectors to the provided or
/// default addresses (RESET/NMI/IRQ point to 0x8000 by default).
///
/// - `prg`: program bytes to place at PRG start (must be <= 16 KiB)
/// - `chr_8k`: number of 8 KiB CHR units (0 => CHR RAM allocated by loader; 1 => 8 KiB CHR ROM in file)
/// - `prg_ram_8k`: PRG RAM size in 8 KiB units (tests commonly use 1)
/// - `vectors`: optional (reset, nmi, irq) tuple. Defaults to (0x8000, 0x8000, 0x8000)
///
/// Flags used:
/// - flags6: default 0 (horizontal mirroring, no trainer, no battery, mapper low nibble 0)
/// - flags7: default 0 (mapper high nibble 0, not NES 2.0)
pub fn build_nrom_with_prg(
    prg: &[u8],
    chr_8k: usize,
    prg_ram_8k: u8,
    vectors: Option<(u16, u16, u16)>,
) -> Vec<u8> {
    assert!(
        prg.len() <= 16 * 1024,
        "Program must fit within a 16 KiB PRG bank"
    );

    // Base image with 1x16 KiB PRG, configurable CHR, flags6/flags7 = 0
    let mut rom = build_ines(1, chr_8k, 0, 0, prg_ram_8k, None);

    // Copy program into PRG area
    let header_and_optional_trainer = 16;
    let prg_start = header_and_optional_trainer;
    let prg_end = prg_start + 16 * 1024;
    rom[prg_start..(prg_start + prg.len())].copy_from_slice(prg);

    // Set vectors at end of the single PRG bank (NROM-128 layout)
    let (reset, nmi, irq) = vectors.unwrap_or((0x8000, 0x8000, 0x8000));
    {
        let prg_slice = &mut rom[prg_start..prg_end];
        set_vectors_in_prg(prg_slice, reset, nmi, irq);
    }

    rom
}

/// Write CPU vectors (NMI, RESET, IRQ/BRK) into a PRG slice that is either
/// 16 KiB (NROM-128) or 32 KiB (NROM-256). Panics if PRG length is something else.
///
/// For 16 KiB PRG, vectors are placed at offsets 0x3FFA..=0x3FFF.
/// For 32 KiB PRG, vectors are placed at offsets 0x7FFA..=0x7FFF.
pub fn set_vectors_in_prg(prg: &mut [u8], reset: u16, nmi: u16, irq: u16) {
    match prg.len() {
        16384 => {
            let base = 0x3FFA;
            write_le_u16(prg, base + 0, nmi);
            write_le_u16(prg, base + 2, reset);
            write_le_u16(prg, base + 4, irq);
        }
        32768 => {
            let base = 0x7FFA;
            write_le_u16(prg, base + 0, nmi);
            write_le_u16(prg, base + 2, reset);
            write_le_u16(prg, base + 4, irq);
        }
        other => panic!(
            "Unsupported PRG length for vector placement: {} bytes (expected 16 KiB or 32 KiB)",
            other
        ),
    }
}

#[inline]
fn write_le_u16(buf: &mut [u8], offset: usize, value: u16) {
    buf[offset] = (value & 0x00FF) as u8;
    buf[offset + 1] = (value >> 8) as u8;
}

/// Convenience wrapper for cases where only the RESET vector needs to be overridden.
/// - `reset`: optional RESET vector address. If `None`, defaults to 0x8000 (same as other vectors).
/// - NMI and IRQ vectors remain at 0x8000 to match the existing default behavior.
/// This keeps test call sites concise when they only care about the program start address.
pub fn build_nrom_with_prg_reset_only(
    prg: &[u8],
    chr_8k: usize,
    prg_ram_8k: u8,
    reset: Option<u16>,
) -> Vec<u8> {
    let vectors = reset.map(|r| (r, 0x8000, 0x8000));
    build_nrom_with_prg(prg, chr_8k, prg_ram_8k, vectors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_basic_ines() {
        let rom = build_ines(2, 1, 0x01, 0x00, 1, None);
        assert_eq!(&rom[0..4], b"NES\x1A");
        assert_eq!(rom[4], 2);
        assert_eq!(rom[5], 1);
        assert_eq!(rom[6], 0x01);
        assert_eq!(rom[7], 0x00);
        assert_eq!(rom[8], 1);
        // Basic size sanity
        assert_eq!(rom.len(), 16 + 2 * 16 * 1024 + 1 * 8 * 1024);
    }

    #[test]
    fn writes_vectors_for_16k_prg() {
        let mut prg = vec![0u8; 16 * 1024];
        set_vectors_in_prg(&mut prg, 0x8123, 0x8456, 0x8ABC);
        assert_eq!(prg[0x3FFA], 0x56);
        assert_eq!(prg[0x3FFB], 0x84);
        assert_eq!(prg[0x3FFC], 0x23);
        assert_eq!(prg[0x3FFD], 0x81);
        assert_eq!(prg[0x3FFE], 0xBC);
        assert_eq!(prg[0x3FFF], 0x8A);
    }

    #[test]
    fn writes_vectors_for_32k_prg() {
        let mut prg = vec![0u8; 32 * 1024];
        set_vectors_in_prg(&mut prg, 0x8123, 0x8456, 0x8ABC);
        assert_eq!(prg[0x7FFA], 0x56);
        assert_eq!(prg[0x7FFB], 0x84);
        assert_eq!(prg[0x7FFC], 0x23);
        assert_eq!(prg[0x7FFD], 0x81);
        assert_eq!(prg[0x7FFE], 0xBC);
        assert_eq!(prg[0x7FFF], 0x8A);
    }

    #[test]
    fn builds_nrom_with_prg_and_vectors() {
        let prg = [0xA9, 0x01, 0x00]; // LDA #$01; BRK
        let rom = build_nrom_with_prg(&prg, 1, 1, None);
        // Header magic
        assert_eq!(&rom[0..4], b"NES\x1A");
        // PRG size units
        assert_eq!(rom[4], 1);
        // CHR size units
        assert_eq!(rom[5], 1);
        // Vectors present (RESET low byte) at 0x3FFC within PRG
        let prg_start = 16;
        assert_ne!(rom[prg_start + 0x3FFC], 0x00);
    }
}
