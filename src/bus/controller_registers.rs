/*!
Controller registers handler

Purpose
- Provide a focused entry point for CPU-visible controller register access.
- Centralize read/write semantics for $4016/$4017 and keep the CPU dispatcher smaller.

Addressing overview (CPU):
- $4016 (write): Controller strobe (bit 0) — applies to both controllers.
- $4016 (read) : Controller 1 serial read.
- $4017 (read) : Controller 2 serial read.
- $4017 (write): APU frame counter (not handled here; see APU registers handler).

Usage
- Call `ControllerRegisters::read(bus, addr)` to attempt a controller read; `None` if not a controller address.
- Call `ControllerRegisters::write(bus, addr, value)` to attempt a controller write; `false` if not a controller address.
*/

use crate::bus::Bus;

pub struct ControllerRegisters;

impl ControllerRegisters {
    /// Attempt to read from a controller register.
    ///
    /// Returns:
    /// - `Some(value)` for $4016 (Controller 1) and $4017 (Controller 2).
    /// - `None` for non-controller addresses.
    #[inline]
    pub fn read(bus: &mut Bus, addr: u16) -> Option<u8> {
        match addr {
            0x4016 => Some(bus.controllers[0].read()),
            0x4017 => Some(bus.controllers[1].read()),
            _ => None,
        }
    }

    /// Attempt to write to a controller register.
    ///
    /// Returns:
    /// - `true` for $4016 (controller strobe applied to both controllers).
    /// - `false` for non-controller addresses (e.g., $4017 is APU frame counter).
    #[inline]
    pub fn write(bus: &mut Bus, addr: u16, value: u8) -> bool {
        match addr {
            0x4016 => {
                // Controller strobe for both controllers (bit 0 relevant)
                bus.controllers[0].write_strobe(value);
                bus.controllers[1].write_strobe(value);
                true
            }
            _ => false,
        }
    }
}
