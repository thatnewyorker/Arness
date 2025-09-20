/*!
APU registers handler

Purpose
- Provide a focused entry point for CPU-visible APU register access (0x4000–0x4017).
- Centralize read/write semantics for APU registers and keep Bus/cpu_interface smaller.

Addressing overview (CPU):
- 0x4000–0x4013: APU channel registers (read/write)
- 0x4014: OAM DMA (not APU; handled elsewhere)
- 0x4015: APU status (read) / APU enable flags (write)
- 0x4016: Controller 1 strobe/read (not APU; handled by controller module)
- 0x4017: APU frame counter (write); Controller 2 read (on read path, not APU)

Usage
- Call `ApuRegisters::read(bus, addr)` to attempt an APU register read; `None` means
  the address is not handled by the APU (e.g., controllers).
- Call `ApuRegisters::write(bus, addr, value)` to attempt an APU register write; `false`
  indicates the address is not an APU register and should be routed elsewhere.

Notes
- This module intentionally does NOT handle controller reads (0x4016/0x4017) or OAM DMA (0x4014).
  Those are expected to be routed by the CPU dispatcher to the appropriate modules.
*/

use crate::bus::Bus;

pub struct ApuRegisters;

impl ApuRegisters {
    /// Attempt to read an APU register at the given CPU address.
    ///
    /// Returns:
    /// - `Some(value)` if the address is an APU register (0x4000–0x4013 or 0x4015).
    /// - `None` if the address is not an APU read (e.g., controllers/OAM DMA).
    #[inline]
    pub fn read(bus: &mut Bus, addr: u16) -> Option<u8> {
        match addr {
            0x4000..=0x4013 => Some(bus.apu.read_reg(addr)),
            0x4015 => Some(bus.apu.read_status()),
            // 0x4016/0x4017 reads are controllers; 0x4014 is OAM DMA.
            _ => None,
        }
    }

    /// Attempt to write an APU register at the given CPU address.
    ///
    /// Returns:
    /// - `true` if the address is an APU register (0x4000–0x4013, 0x4015, or 0x4017) and was handled.
    /// - `false` if the address is not an APU write (e.g., controllers/OAM DMA).
    ///
    /// Notes:
    /// - 0x4017 write configures the APU frame counter; reads at 0x4017 are for Controller 2.
    #[inline]
    pub fn write(bus: &mut Bus, addr: u16, value: u8) -> bool {
        match addr {
            0x4000..=0x4013 => {
                bus.apu.write_reg(addr, value);
                true
            }
            0x4015 => {
                bus.apu.write_reg(addr, value);
                true
            }
            0x4017 => {
                // APU frame counter write
                bus.apu.write_reg(addr, value);
                true
            }
            // 0x4014 is OAM DMA (handled by DMA), 0x4016 is controller strobe
            _ => false,
        }
    }
}
