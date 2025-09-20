/*!
CPU interface dispatcher

Purpose
- Centralize CPU-visible memory mapping and delegate to devices.
- Use PpuRegisters for the 0x2000-0x3FFF register window, preserving PPUDATA semantics.
- Provide a single place to evolve address decoding without touching the Bus façade.

Notes
- This module does not own device state. It operates on a &mut Bus and delegates to devices.
- OAM DMA ($4014) requires starting the DMA controller; to avoid private-field access,
  callers provide a `dma_start` callback that triggers DMA with the value written.

Address map (summary, unchanged):
- $0000-$07FF: 2KB internal RAM
- $0800-$1FFF: Mirrors of $0000-$07FF (mask & 0x07FF)
- $2000-$2007: PPU registers
- $2008-$3FFF: Mirrors of $2000-$2007 (mask with & 0x0007)
- $4000-$4013: APU registers
- $4014: OAM DMA (PPU) - write triggers DMA start
- $4015: APU status (read) / enables (write)
- $4016: Controller 1 strobe (write), Controller 1 serial read (read)
- $4017: APU frame counter (write), Controller 2 serial read (read)
- $4018-$401F: Typically disabled test registers
- $4020-$5FFF: Expansion area (stubbed)
- $6000-$7FFF: Cartridge PRG RAM (if present)
- $8000-$FFFF: Cartridge PRG ROM (mapper-controlled)
*/

use crate::bus::Bus;
use crate::bus::apu_registers::ApuRegisters;
use crate::bus::controller_registers::ControllerRegisters;
use crate::bus::ppu_registers::PpuRegisters;

/// CPU-visible read from the unified address space.
pub fn cpu_read(bus: &mut Bus, addr: u16) -> u8 {
    match addr {
        0x0000..=0x1FFF => {
            // 2KB RAM mirrored every 0x0800
            let _idx = (addr & 0x07FF) as usize;
            // Use dedicated RAM helper to avoid calling back into Bus::read
            ram_read(bus, addr)
        }
        0x2000..=0x3FFF => {
            // Delegate to PPU registers handler (handles mirroring, PPUDATA semantics).
            PpuRegisters::read(bus, addr)
        }
        0x4000..=0x4017 => {
            if let Some(v) = ApuRegisters::read(bus, addr) {
                v
            } else if addr == 0x4014 {
                // OAM DMA register read not meaningful; return 0
                0
            } else if let Some(v) = ControllerRegisters::read(bus, addr) {
                v
            } else {
                0
            }
        }
        0x4018..=0x401F => 0,
        0x4020..=0x5FFF => 0,
        0x6000..=0x7FFF => {
            if let Some(cart) = &bus.cartridge {
                cart.mapper.borrow_mut().cpu_read(addr)
            } else {
                0
            }
        }
        0x8000..=0xFFFF => {
            if let Some(cart) = &bus.cartridge {
                cart.mapper.borrow_mut().cpu_read(addr)
            } else {
                0xFF
            }
        }
    }
}

/// CPU-visible write to the unified address space, with a callback to start OAM DMA ($4014).
///
/// The `dma_start` callback receives the bus and the written byte (source page).
pub fn cpu_write<F>(bus: &mut Bus, addr: u16, value: u8, dma_start: F)
where
    F: FnOnce(&mut Bus, u8),
{
    match addr {
        0x0000..=0x1FFF => {
            // 2KB RAM mirrored every 0x0800
            let _idx = (addr & 0x07FF) as usize;
            // Use dedicated RAM helper to avoid calling back into Bus::write
            ram_write(bus, addr, value);
        }
        0x2000..=0x3FFF => {
            // Delegate to PPU registers handler (mirroring handled inside).
            PpuRegisters::write(bus, addr, value);
        }
        0x4000..=0x4017 => {
            if ApuRegisters::write(bus, addr, value) {
                // handled by APU
            } else if addr == 0x4014 {
                // OAM DMA: invoke the provided callback to start DMA with cycle-accurate semantics.
                dma_start(bus, value);
            } else if ControllerRegisters::write(bus, addr, value) {
                // handled by controllers
            } else {
                // not handled here
            }
        }
        0x4018..=0x401F => {
            // Typically disabled test registers; ignore writes
        }
        0x4020..=0x5FFF => {
            // Expansion area (stub)
        }
        0x6000..=0x7FFF => {
            if let Some(cart) = &bus.cartridge {
                cart.mapper.borrow_mut().cpu_write(addr, value);
            }
        }
        0x8000..=0xFFFF => {
            if let Some(cart) = &bus.cartridge {
                cart.mapper.borrow_mut().cpu_write(addr, value);
            }
        }
    }
}

/// Convenience: little-endian word read used by CPU vectors and indirect addressing.
fn ram_read(bus: &mut Bus, addr: u16) -> u8 {
    bus.ram_read_mirrored(addr)
}

fn ram_write(bus: &mut Bus, addr: u16, value: u8) {
    bus.ram_write_mirrored(addr, value)
}

pub fn cpu_read_word(bus: &mut Bus, addr: u16) -> u16 {
    let lo = cpu_read(bus, addr) as u16;
    let hi = cpu_read(bus, addr.wrapping_add(1)) as u16;
    (hi << 8) | lo
}
