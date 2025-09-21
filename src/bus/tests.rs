use crate::bus::Bus;
use crate::cartridge::Cartridge;

/// Build a minimal iNES image with the specified counts and flags.
/// - prg_16k: number of 16 KiB PRG ROM banks
/// - chr_8k: number of 8 KiB CHR ROM banks (0 => CHR RAM)
/// - flags6/flags7: iNES header flags (mapper low/high nibbles, mirroring, etc.)
/// - prg_ram_8k: PRG RAM size in 8 KiB units (0 => often treated as 8 KiB conventionally)
/// - trainer: optional 512-byte trainer
fn build_ines(
    prg_16k: u8,
    chr_8k: u8,
    flags6: u8,
    flags7: u8,
    prg_ram_8k: u8,
    trainer: Option<Vec<u8>>,
) -> Vec<u8> {
    let mut v = Vec::new();
    // Header
    v.extend_from_slice(b"NES\x1A");
    v.push(prg_16k);
    v.push(chr_8k);
    v.push(flags6);
    v.push(flags7);
    v.push(prg_ram_8k);
    v.extend_from_slice(&[0u8; 7]); // padding

    // Optional trainer
    if let Some(t) = trainer {
        v.extend_from_slice(&t);
    }

    // PRG ROM (fill with 0xEA NOPs by default)
    let prg_len = prg_16k as usize * 16 * 1024;
    if prg_len > 0 {
        v.extend(std::iter::repeat_n(0xEA, prg_len));
    }

    // CHR ROM (fill with 0x00 if present; if 0, mapper will allocate CHR RAM)
    let chr_len = chr_8k as usize * 8 * 1024;
    if chr_len > 0 {
        v.extend(std::iter::repeat_n(0x00, chr_len));
    }

    v
}

/// Build a minimal iNES image for MMC1 tests with mapper id embedded in flags6/flags7.
/// - For these tests, we keep PRG/CHR sizes parametric and let the mapper handle CHR RAM if CHR size is 0.
fn build_mmc1_ines(prg_16k: u8, chr_8k: u8, flags6: u8, flags7: u8, prg_ram_8k: u8) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(b"NES\x1A");
    v.push(prg_16k);
    v.push(chr_8k);
    v.push(flags6);
    v.push(flags7);
    v.push(prg_ram_8k);
    v.extend_from_slice(&[0u8; 7]);
    // PRG ROM
    v.extend(std::iter::repeat_n(0xEA, prg_16k as usize * 16 * 1024));
    // CHR ROM (present only if chr_8k > 0)
    if chr_8k > 0 {
        v.extend(std::iter::repeat_n(0x00, chr_8k as usize * 8 * 1024));
    }
    v
}

/// Helper for MMC1 serial writes (5 LSB-first writes to $8000).
fn mmc1_serial_write(bus: &mut Bus, value5: u8) {
    for i in 0..5 {
        let bit = (value5 >> i) & 1;
        bus.write(0x8000, bit);
    }
}

#[test]
fn ram_mirroring() {
    let mut bus = Bus::new();

    // Write to $0001
    bus.write(0x0001, 0xAA);

    // Read at mirrors: $0001, $0801, $1801 should all see the same byte.
    assert_eq!(bus.read(0x0001), 0xAA);
    assert_eq!(bus.read(0x0801), 0xAA);
    assert_eq!(bus.read(0x1801), 0xAA);

    // Overwrite via a mirror address and verify all mirrors reflect it.
    bus.write(0x1801, 0x55);
    assert_eq!(bus.read(0x0001), 0x55);
    assert_eq!(bus.read(0x0801), 0x55);
    assert_eq!(bus.read(0x1801), 0x55);
}

#[test]
fn ppu_reg_mirror() {
    let mut bus = Bus::new();
    // Write to $2000 via mirror $2008
    bus.write(0x2008, 0x80);
    // Read back PPUCTRL via $2000
    assert_eq!(bus.read(0x2000) & 0x80, 0x80);
}

#[test]
fn controller_strobe_and_read() {
    let mut bus = Bus::new();
    // Strobe high then low
    bus.write(0x4016, 1);
    bus.write(0x4016, 0);
    // Default state: all released -> first 8 reads on $4016 should be 0, then 1s
    for _ in 0..8 {
        assert_eq!(bus.read(0x4016), 0);
    }
    assert_eq!(bus.read(0x4016), 1);
}

#[test]
fn prg_ram_basic() {
    let mut bus = Bus::new();
    // Build minimal NROM cart with PRG RAM (8 KiB by default via header)
    let data = {
        // iNES header: "NES<1A>", 1x16KB PRG, 1x8KB CHR, flags6=0, flags7=0, prg_ram_8k=1
        let mut v = vec![];
        v.extend_from_slice(b"NES\x1A");
        v.push(1); // PRG
        v.push(1); // CHR
        v.push(0); // flags6
        v.push(0); // flags7
        v.push(1); // PRG RAM size (8 KiB)
        v.extend_from_slice(&[0u8; 7]); // padding
        v.extend(std::iter::repeat_n(0xAA, 16 * 1024)); // PRG ROM
        v.extend(std::iter::repeat_n(0xCC, 8 * 1024)); // CHR ROM
        v
    };
    let cart = Cartridge::from_ines_bytes(&data).expect("parse");
    bus.attach_cartridge(cart);

    bus.write(0x6000, 0x42);
    assert_eq!(bus.read(0x6000), 0x42);
}

#[test]
fn oam_dma_copies_256_bytes() {
    let mut bus = Bus::new();
    // Prepare page $0200..$02FF with incrementing values.
    for i in 0..256u16 {
        bus.write(0x0200 + i, (i & 0xFF) as u8);
    }
    // Set OAMADDR to 0xFE
    bus.write(0x2003, 0xFE);
    // Trigger OAM DMA from $0200
    bus.write(0x4014, 0x02);

    // Advance cycles for DMA to complete (CPU even -> 513 cycles)
    bus.tick(513);

    // OAM should now contain 0x00 at 0xFE, 0x01 at 0xFF, 0x02 at 0x00, ...
    assert_eq!(bus.ppu.peek_oam(0xFE), 0x00);
    assert_eq!(bus.ppu.peek_oam(0xFF), 0x01);
    assert_eq!(bus.ppu.peek_oam(0x00), 0x02);
    assert_eq!(bus.ppu.peek_oam(0x01), 0x03);
}

// ---------- PPU memory mapping tests ----------

#[test]
fn nametable_horizontal_mirroring() {
    // Horizontal mirroring: $2000/$2400 share, $2800/$2C00 share
    let flags6 = 0b0000_0000; // header mirroring: horizontal
    let flags7 = 0;
    let rom = build_ines(1, 1, flags6, flags7, 1, None);
    let cart = Cartridge::from_ines_bytes(&rom).expect("parse");
    let mut bus = Bus::new();
    bus.attach_cartridge(cart);

    // Set PPUADDR to $2000 and write via PPUDATA
    bus.write(0x2006, 0x20);
    bus.write(0x2006, 0x00);
    bus.write(0x2007, 0x55);

    // Read back from mirror $2400: reset addr latch then read twice (buffer then true)
    let _ = bus.read(0x2002); // reset write toggle
    bus.write(0x2006, 0x24);
    bus.write(0x2006, 0x00);
    let _ = bus.read(0x2007); // buffered read (ignored)
    let v = bus.read(0x2007);
    assert_eq!(v, 0x55);
}

#[test]
fn nametable_vertical_mirroring() {
    // Vertical mirroring: $2000/$2800 share, $2400/$2C00 share
    let flags6 = 0b0000_0001; // header mirroring: vertical
    let flags7 = 0;
    let rom = build_ines(1, 1, flags6, flags7, 1, None);
    let cart = Cartridge::from_ines_bytes(&rom).expect("parse");
    let mut bus = Bus::new();
    bus.attach_cartridge(cart);

    // Set PPUADDR to $2000 and write via PPUDATA
    bus.write(0x2006, 0x20);
    bus.write(0x2006, 0x00);
    bus.write(0x2007, 0x66);

    // Read back from mirror $2800
    let _ = bus.read(0x2002);
    bus.write(0x2006, 0x28);
    bus.write(0x2006, 0x00);
    let _ = bus.read(0x2007);
    let v = bus.read(0x2007);
    assert_eq!(v, 0x66);
}

#[test]
fn palette_mirroring_3f10_mirrors_3f00() {
    let mut bus = Bus::new();

    // Write $3F00 via PPUDATA
    bus.write(0x2006, 0x3F);
    bus.write(0x2006, 0x00);
    bus.write(0x2007, 0x12);

    // Read $3F10 (mirror of $3F00); palette reads are immediate (not buffered)
    let _ = bus.read(0x2002);
    bus.write(0x2006, 0x3F);
    bus.write(0x2006, 0x10);
    let v = bus.read(0x2007);
    assert_eq!(v, 0x12);
}

#[test]
fn ppudata_buffered_read_and_increment_via_bus() {
    // Use CHR RAM by building a cart with 0 CHR banks (CHR RAM allocated)
    let flags6 = 0b0000_0000; // mirroring doesn't matter here
    let flags7 = 0;
    let rom = build_ines(1, 0, flags6, flags7, 1, None);
    let cart = Cartridge::from_ines_bytes(&rom).expect("parse");

    let mut bus = Bus::new();
    bus.attach_cartridge(cart);

    // Set VRAM increment to 1
    bus.write(0x2000, 0x00);

    // Write to CHR RAM at $0000 and $0001 via PPUDATA
    bus.write(0x2006, 0x00);
    bus.write(0x2006, 0x00);
    bus.write(0x2007, 0x11);
    bus.write(0x2007, 0x22);

    // Reset latch, set address to $0000 and perform buffered reads
    let _ = bus.read(0x2002);
    bus.write(0x2006, 0x00);
    bus.write(0x2006, 0x00);

    // First read returns buffer (initially 0)
    assert_eq!(bus.read(0x2007), 0x00);
    // Second read returns 0x11
    assert_eq!(bus.read(0x2007), 0x11);
    // Third read returns 0x22
    assert_eq!(bus.read(0x2007), 0x22);
}

// -------- Dynamic MMC1 mirroring tests --------

#[test]
fn mmc1_dynamic_vertical_mirroring() {
    // Mapper 1 (MMC1). Set control bits to 0b00010 (vertical)
    // Mapper id 1 => flags6 upper nibble = 0001 => 0x10
    let rom = build_mmc1_ines(2, 1, 0x10, 0x00, 1);
    let cart = Cartridge::from_ines_bytes(&rom).expect("parse");
    let mut bus = Bus::new();
    bus.attach_cartridge(cart);

    // Write control = 0b00010 (vertical)
    mmc1_serial_write(&mut bus, 0b00010);

    // Write to $2000
    bus.write(0x2006, 0x20);
    bus.write(0x2006, 0x00);
    bus.write(0x2007, 0xA1);

    // Mirror in vertical: $2800 mirrors $2000
    let _ = bus.read(0x2002);
    bus.write(0x2006, 0x28);
    bus.write(0x2006, 0x00);
    let _ = bus.read(0x2007);
    let v = bus.read(0x2007);
    assert_eq!(v, 0xA1);
}

#[test]
fn mmc1_dynamic_horizontal_mirroring() {
    // Control bits 0b00011 => horizontal
    let rom = build_mmc1_ines(2, 1, 0x10, 0x00, 1);
    let cart = Cartridge::from_ines_bytes(&rom).expect("parse");
    let mut bus = Bus::new();
    bus.attach_cartridge(cart);

    mmc1_serial_write(&mut bus, 0b00011);

    // Write to $2000
    bus.write(0x2006, 0x20);
    bus.write(0x2006, 0x00);
    bus.write(0x2007, 0xB2);

    // Horizontal: $2400 mirrors $2000
    let _ = bus.read(0x2002);
    bus.write(0x2006, 0x24);
    bus.write(0x2006, 0x00);
    let _ = bus.read(0x2007);
    let v = bus.read(0x2007);
    assert_eq!(v, 0xB2);
}

#[test]
fn mmc1_dynamic_single_screen_lower() {
    // Control bits 0b00000 => single-screen lower ($2000 region everywhere)
    let rom = build_mmc1_ines(1, 1, 0x10, 0x00, 1);
    let cart = Cartridge::from_ines_bytes(&rom).expect("parse");
    let mut bus = Bus::new();
    bus.attach_cartridge(cart);

    mmc1_serial_write(&mut bus, 0b00000);

    // Write to $2000
    bus.write(0x2006, 0x20);
    bus.write(0x2006, 0x10);
    bus.write(0x2007, 0xC3);

    // Read from $2400 (should mirror single-screen lower)
    let _ = bus.read(0x2002);
    bus.write(0x2006, 0x24);
    bus.write(0x2006, 0x10);
    let _ = bus.read(0x2007);
    let v = bus.read(0x2007);
    assert_eq!(v, 0xC3);
    // Also $2C00 should mirror
    let _ = bus.read(0x2002);
    bus.write(0x2006, 0x2C);
    bus.write(0x2006, 0x10);
    let _ = bus.read(0x2007);
    let v2 = bus.read(0x2007);
    assert_eq!(v2, 0xC3);
}

#[test]
fn mmc1_dynamic_single_screen_upper() {
    // Control bits 0b00001 => single-screen upper ($2400 region everywhere)
    let rom = build_mmc1_ines(1, 1, 0x10, 0x00, 1);
    let cart = Cartridge::from_ines_bytes(&rom).expect("parse");
    let mut bus = Bus::new();
    bus.attach_cartridge(cart);

    mmc1_serial_write(&mut bus, 0b00001);

    // Write to $2400
    bus.write(0x2006, 0x24);
    bus.write(0x2006, 0x10);
    bus.write(0x2007, 0xD4);

    // Read from $2000 should mirror $2400 in single-screen upper
    let _ = bus.read(0x2002);
    bus.write(0x2006, 0x20);
    bus.write(0x2006, 0x10);
    let _ = bus.read(0x2007);
    let v = bus.read(0x2007);
    assert_eq!(v, 0xD4);
    // $2C00 also mirrors
    let _ = bus.read(0x2002);
    bus.write(0x2006, 0x2C);
    bus.write(0x2006, 0x10);
    let _ = bus.read(0x2007);
    let v2 = bus.read(0x2007);
    assert_eq!(v2, 0xD4);
}

#[test]
fn mmc3_dynamic_mirroring_vertical_then_horizontal() {
    // Mapper 4 (MMC3). Set mapper id low nibble = 4 (flags6 upper nibble).
    // flags6: 0x40 => mapper low nibble=4, horizontal header mirroring (bit0=0)
    let flags6 = 0x40;
    let flags7 = 0x00;
    let rom = build_ines(2, 1, flags6, flags7, 1, None);
    let cart = Cartridge::from_ines_bytes(&rom).expect("parse");
    let mut bus = Bus::new();
    bus.attach_cartridge(cart);

    // First force Vertical mirroring via $A000 write (bit0=0)
    bus.write(0xA000, 0x00);

    // Write a byte to $2000
    bus.write(0x2006, 0x20);
    bus.write(0x2006, 0x05);
    bus.write(0x2007, 0x9A);

    // In vertical mode $2800 mirrors $2000
    let _ = bus.read(0x2002);
    bus.write(0x2006, 0x28);
    bus.write(0x2006, 0x05);
    let _ = bus.read(0x2007);
    let v_vert = bus.read(0x2007);
    assert_eq!(
        v_vert, 0x9A,
        "Vertical mirroring: $2800 should mirror $2000"
    );

    // Now switch to Horizontal mirroring (bit0=1)
    bus.write(0xA000, 0x01);

    // Overwrite $2000 with a different value
    bus.write(0x2006, 0x20);
    bus.write(0x2006, 0x05);
    bus.write(0x2007, 0x6E);

    // In horizontal mode $2400 mirrors $2000
    let _ = bus.read(0x2002);
    bus.write(0x2006, 0x24);
    bus.write(0x2006, 0x05);
    let _ = bus.read(0x2007);
    let v_horiz = bus.read(0x2007);
    assert_eq!(
        v_horiz, 0x6E,
        "Horizontal mirroring: $2400 should mirror $2000"
    );
}

#[test]
fn mmc3_dynamic_mirroring_switch_back() {
    // Start with vertical, switch to horizontal, then back to vertical again.
    let flags6 = 0x40; // mapper id low nibble = 4
    let flags7 = 0x00;
    let rom = build_ines(2, 1, flags6, flags7, 1, None);
    let cart = Cartridge::from_ines_bytes(&rom).expect("parse");
    let mut bus = Bus::new();
    bus.attach_cartridge(cart);

    // Vertical
    bus.write(0xA000, 0x00);
    bus.write(0x2006, 0x20);
    bus.write(0x2006, 0x33);
    bus.write(0x2007, 0x11);

    // Confirm vertical ($2800 mirror)
    let _ = bus.read(0x2002);
    bus.write(0x2006, 0x28);
    bus.write(0x2006, 0x33);
    let _ = bus.read(0x2007);
    assert_eq!(bus.read(0x2007), 0x11);

    // Horizontal
    bus.write(0xA000, 0x01);
    bus.write(0x2006, 0x20);
    bus.write(0x2006, 0x44);
    bus.write(0x2007, 0x22);

    // Confirm horizontal ($2400 mirror)
    let _ = bus.read(0x2002);
    bus.write(0x2006, 0x24);
    bus.write(0x2006, 0x44);
    let _ = bus.read(0x2007);
    assert_eq!(bus.read(0x2007), 0x22);

    // Back to Vertical
    bus.write(0xA000, 0x00);
    bus.write(0x2006, 0x20);
    bus.write(0x2006, 0x55);
    bus.write(0x2007, 0x33);

    let _ = bus.read(0x2002);
    bus.write(0x2006, 0x28);
    bus.write(0x2006, 0x55);
    let _ = bus.read(0x2007);
    assert_eq!(bus.read(0x2007), 0x33);
}
