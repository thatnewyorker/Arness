#![doc = r#"
PPU address-space module: pure helper declarations and documentation.

Purpose
- Provide pure helper function declarations for PPU address-space mapping tasks.
- These helpers will be used by a future `PpuAddressSpace` component to centralize
  nametable and palette mapping semantics, decoupled from the `Bus` and `Cartridge`.

Scope (in this step)
- Only declare the helpers and related enums with documentation.
- Implementations will be introduced in a subsequent step; current bodies are placeholders.

Concepts
- Nametable mirroring can be driven by iNES header mirroring as well as mapper-controlled
  dynamic mirroring modes. When header mirroring is FourScreen, mapper overrides should not apply.
- Palette addressing has special mirroring semantics (e.g., $3F10 mirrors $3F00).

Design goals
- Keep mapping logic pure and side-effect free.
- Avoid `Bus` borrowing entanglement by passing all required parameters explicitly.
- Enable unit testing of these helpers in isolation.
"#]

/// Header-level mirroring modes (from iNES header).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum HeaderMirroring {
    Horizontal,
    Vertical,
    FourScreen,
}

/// Dynamic, mapper-controlled mirroring modes.
///
/// Note:
/// - When header mirroring is FourScreen, dynamic overrides must NOT apply.
/// - Single-screen modes map all nametable tables to a single 1KB bank (lower or upper).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum DynMirroring {
    SingleScreenLower,
    SingleScreenUpper,
    Vertical,
    Horizontal,
}

/// Normalize a raw PPU address into the canonical 0x0000-0x3FFF range.
///
/// Invariants:
/// - Always returns `addr & 0x3FFF`.
pub fn normalize_ppu_addr(_addr: u16) -> u16 {
    unimplemented!("normalize_ppu_addr: migration stub (pure helper)")
}

/// Compute the palette RAM byte index (0..=31) for a PPU address in 0x3F00-0x3FFF.
///
/// Behavior (to be implemented later, documented for reference):
/// - Mirror to 0x3F00-0x3F1F (mask lower 5 bits).
/// - $3F10/$3F14/$3F18/$3F1C mirror $3F00/$3F04/$3F08/$3F0C respectively (universal bg color mirroring).
///
/// Returns:
/// - An index in the inclusive range [0, 31] representing the palette RAM location.
pub fn map_palette_addr(addr: u16) -> usize {
    // Mirror to 0x3F00-0x3F1F
    let mut idx = (addr.wrapping_sub(0x3F00)) as usize & 0x1F;
    // $3F10/$3F14/$3F18/$3F1C mirror $3F00/$3F04/$3F08/$3F0C
    if idx >= 16 && (idx & 0x03) == 0 {
        idx -= 16;
    }
    idx
}

/// Compute the effective nametable RAM byte index (0..=0x7FF) for a PPU address in 0x2000-0x2FFF
/// (or its mirror 0x3000-0x3EFF), given header mirroring and optional mapper-controlled override.
///
/// Inputs:
/// - `addr`: The PPU address; function will internally reduce to 0x2000-0x2FFF.
/// - `header`: Header mirroring mode (Horizontal, Vertical, FourScreen).
/// - `dynamic`: Optional mapper-provided mirroring mode. Ignored if header is FourScreen.
///
/// Invariants:
/// - Returns an index in [0, 0x7FF], suitable for a 2 KiB nametable RAM backing store.
///
/// Notes:
/// - Table selection logic must mirror 0x3000-0x3EFF down to 0x2000-0x2FFF.
/// - Horizontal mirroring: (0,1)->bank0, (2,3)->bank1
/// - Vertical mirroring: (0,2)->bank0, (1,3)->bank1
/// - FourScreen: header-enforced; dynamic mirroring is not consulted here (handled by larger system).
/// - SingleScreenLower/Upper: all tables map to the chosen bank.
pub fn map_nametable_addr(
    addr: u16,
    header: HeaderMirroring,
    dynamic: Option<DynMirroring>,
) -> usize {
    // Reduce to $2000-$2FFF (mirror $3000-$3EFF down)
    let a = (addr.wrapping_sub(0x2000)) & 0x0FFF;
    let table = a / 0x0400; // 0..3
    let offset = (a % 0x0400) as usize;

    // Select bank based on dynamic override if present (unless header forces FourScreen),
    // otherwise use header mirroring rules. FourScreen is approximated as vertical here
    // to preserve existing behavior.
    let bank = match dynamic {
        Some(DynMirroring::SingleScreenLower) => 0,
        Some(DynMirroring::SingleScreenUpper) => 1,
        Some(DynMirroring::Vertical) => {
            if (table & 1) == 0 {
                0
            } else {
                1
            }
        }
        Some(DynMirroring::Horizontal) => {
            if table < 2 {
                0
            } else {
                1
            }
        }
        None => match header {
            HeaderMirroring::Horizontal => {
                if table < 2 {
                    0
                } else {
                    1
                }
            }
            HeaderMirroring::Vertical => {
                if (table & 1) == 0 {
                    0
                } else {
                    1
                }
            }
            HeaderMirroring::FourScreen => {
                if (table & 1) == 0 {
                    0
                } else {
                    1
                }
            }
        },
    };

    (bank as usize) * 0x0400 + offset
}

// ----------------------------------------------------------------------------
// PPU address space container
// ----------------------------------------------------------------------------

/// Owns nametable and palette RAM and provides PPU-visible read/write helpers with
/// correct mirroring semantics. Pattern table (0x0000-0x1FFF) access is intentionally
/// left to the cartridge/mapper; reads return 0 and writes are ignored here.
pub struct PpuAddressSpace {
    pub nt_ram: [u8; 0x0800],  // 2 KiB nametable RAM
    pub palette_ram: [u8; 32], // 32-byte palette RAM
}

impl Default for PpuAddressSpace {
    fn default() -> Self {
        Self::new()
    }
}

impl PpuAddressSpace {
    pub fn new() -> Self {
        Self {
            nt_ram: [0; 0x0800],
            palette_ram: [0; 32],
        }
    }

    /// Read from PPU address space (0x0000-0x3FFF) using header/dynamic mirroring.
    /// Pattern table region (0x0000-0x1FFF) returns 0; callers should delegate to mapper.
    pub fn ppu_read(
        &self,
        addr: u16,
        header: HeaderMirroring,
        dynamic: Option<DynMirroring>,
    ) -> u8 {
        let a = addr & 0x3FFF;
        match a {
            0x0000..=0x1FFF => 0, // delegated to mapper externally
            0x2000..=0x3EFF => {
                let base = 0x2000 | (a & 0x0FFF);
                let idx = map_nametable_addr(base, header, dynamic);
                self.nt_ram[idx]
            }
            0x3F00..=0x3FFF => {
                let idx = map_palette_addr(a);
                self.palette_ram[idx]
            }
            _ => 0,
        }
    }

    /// Write to PPU address space (0x0000-0x3FFF) using header/dynamic mirroring.
    /// Pattern table region (0x0000-0x1FFF) is ignored; callers should delegate to mapper.
    pub fn ppu_write(
        &mut self,
        addr: u16,
        value: u8,
        header: HeaderMirroring,
        dynamic: Option<DynMirroring>,
    ) {
        let a = addr & 0x3FFF;
        match a {
            0x0000..=0x1FFF => {
                // delegated to mapper externally
            }
            0x2000..=0x3EFF => {
                let base = 0x2000 | (a & 0x0FFF);
                let idx = map_nametable_addr(base, header, dynamic);
                self.nt_ram[idx] = value;
            }
            0x3F00..=0x3FFF => {
                let idx = map_palette_addr(a);
                self.palette_ram[idx] = value;
            }
            _ => {}
        }
    }
}

/// Compute the 1KB nametable bank index (0 or 1) for a given logical table number (0..=3),
/// factoring in header mirroring and optional dynamic mirroring.
///
/// This helper isolates the bank-selection decision from the final byte offset calculation.
///
/// Inputs:
/// - `table`: Logical table number derived from (addr - 0x2000) / 0x400.
/// - `header`: Header mirroring mode.
/// - `dynamic`: Optional mapper-provided mirroring mode (ignored for FourScreen header).
///
/// Returns:
/// - `0` or `1`, indicating the 1KB bank to use within the 2KB nametable RAM backing store.
pub fn select_nametable_bank(
    _table: u16,
    _header: HeaderMirroring,
    _dynamic: Option<DynMirroring>,
) -> usize {
    unimplemented!("select_nametable_bank: migration stub (pure helper)")
}
