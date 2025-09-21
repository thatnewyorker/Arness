//! PPU memory helpers: Bus-level mapping with mirroring resolution
/*!
PPU memory helpers module

Purpose
- Host the PPU address-space mapping helpers that the Bus uses to service PPU reads/writes.
- Centralize header/dynamic mirroring resolution and palette/nametable access so the logic
  is shared between CPU-initiated PPUDATA accesses and PPU rendering/ticking paths.

Intended responsibilities
- Provide small, focused functions that operate on a `&Bus`/`&mut Bus` to read/write PPU space:
  - `ppu_mem_read(bus: &Bus, addr: u16) -> u8`
  - `ppu_mem_write(bus: &mut Bus, addr: u16, value: u8)`
- These helpers:
  - Resolve header mirroring (Horizontal/Vertical/FourScreen) from the cartridge header.
  - Resolve dynamic (mapper-provided) mirroring if applicable (when not FourScreen).
  - Delegate CHR/pattern table access (0x0000..=0x1FFF) to the cartridge mapper.
  - Access nametable/palette RAM via `PpuAddressSpace` with the resolved mirroring modes.

Notes
- Keep functions in this module `pub(in crate::bus)` to avoid exposing internal mapping logic
  outside the bus subsystem.
- Favor small, inline-friendly helpers to keep hot paths efficient.
- Unit tests that rely on PPU space mapping should continue to go through the Bus façade;
  this module’s helpers are implementation details.

*/

#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(unused_macros)]

#[inline]
pub(in crate::bus) fn ppu_mem_read(bus: &crate::bus::Bus, addr: u16) -> u8 {
    let a = addr & 0x3FFF;
    match a {
        0x0000..=0x1FFF => {
            if let Some(cart) = &bus.cartridge {
                cart.mapper.borrow().ppu_read(a)
            } else {
                0
            }
        }
        0x2000..=0x3FFF => {
            // Resolve header mirroring
            let header_mode = if let Some(cart) = &bus.cartridge {
                match cart.mirroring() {
                    crate::cartridge::Mirroring::Horizontal => {
                        crate::bus::ppu_space::HeaderMirroring::Horizontal
                    }
                    crate::cartridge::Mirroring::Vertical => {
                        crate::bus::ppu_space::HeaderMirroring::Vertical
                    }
                    crate::cartridge::Mirroring::FourScreen => {
                        crate::bus::ppu_space::HeaderMirroring::FourScreen
                    }
                }
            } else {
                crate::bus::ppu_space::HeaderMirroring::Horizontal
            };
            // Resolve dynamic (mapper) mirroring unless header enforces FourScreen
            let dyn_mode = if let Some(cart) = &bus.cartridge {
                if !matches!(cart.mirroring(), crate::cartridge::Mirroring::FourScreen) {
                    cart.mapper.borrow().current_mirroring().map(|m| match m {
                        crate::mapper::MapperMirroring::SingleScreenLower => {
                            crate::bus::ppu_space::DynMirroring::SingleScreenLower
                        }
                        crate::mapper::MapperMirroring::SingleScreenUpper => {
                            crate::bus::ppu_space::DynMirroring::SingleScreenUpper
                        }
                        crate::mapper::MapperMirroring::Vertical => {
                            crate::bus::ppu_space::DynMirroring::Vertical
                        }
                        crate::mapper::MapperMirroring::Horizontal => {
                            crate::bus::ppu_space::DynMirroring::Horizontal
                        }
                    })
                } else {
                    None
                }
            } else {
                None
            };
            bus.ppu_mem().ppu_read(a, header_mode, dyn_mode)
        }
        _ => 0,
    }
}

#[inline]
pub(in crate::bus) fn ppu_mem_write(bus: &mut crate::bus::Bus, addr: u16, value: u8) {
    let a = addr & 0x3FFF;
    match a {
        0x0000..=0x1FFF => {
            if let Some(cart) = &bus.cartridge {
                cart.mapper.borrow_mut().ppu_write(a, value);
            }
        }
        0x2000..=0x3FFF => {
            // Resolve header mirroring
            let header_mode = if let Some(cart) = &bus.cartridge {
                match cart.mirroring() {
                    crate::cartridge::Mirroring::Horizontal => {
                        crate::bus::ppu_space::HeaderMirroring::Horizontal
                    }
                    crate::cartridge::Mirroring::Vertical => {
                        crate::bus::ppu_space::HeaderMirroring::Vertical
                    }
                    crate::cartridge::Mirroring::FourScreen => {
                        crate::bus::ppu_space::HeaderMirroring::FourScreen
                    }
                }
            } else {
                crate::bus::ppu_space::HeaderMirroring::Horizontal
            };
            // Resolve dynamic (mapper) mirroring unless header enforces FourScreen
            let dyn_mode = if let Some(cart) = &bus.cartridge {
                if !matches!(cart.mirroring(), crate::cartridge::Mirroring::FourScreen) {
                    cart.mapper.borrow().current_mirroring().map(|m| match m {
                        crate::mapper::MapperMirroring::SingleScreenLower => {
                            crate::bus::ppu_space::DynMirroring::SingleScreenLower
                        }
                        crate::mapper::MapperMirroring::SingleScreenUpper => {
                            crate::bus::ppu_space::DynMirroring::SingleScreenUpper
                        }
                        crate::mapper::MapperMirroring::Vertical => {
                            crate::bus::ppu_space::DynMirroring::Vertical
                        }
                        crate::mapper::MapperMirroring::Horizontal => {
                            crate::bus::ppu_space::DynMirroring::Horizontal
                        }
                    })
                } else {
                    None
                }
            } else {
                None
            };
            bus.ppu_mem_mut().ppu_write(a, value, header_mode, dyn_mode);
        }
        _ => {}
    }
}
