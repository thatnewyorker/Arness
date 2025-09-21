//! RAM helpers: mirrored CPU RAM access wrappers
/*!
RAM helper wrappers

Purpose
- Provide thin helper functions for mirrored CPU RAM access that operate on a `&Bus`/`&mut Bus`.
- Centralize RAM read/write wrappers so the `Bus` façade can delegate to these helpers,
  keeping `bus_impl.rs` smaller while preserving hot-path efficiency.

Scope and visibility
- These helpers are internal to the `bus` module (`pub(in crate::bus)`), not part of the public API.
- Callers should prefer the `Bus` façade methods; these functions exist to reduce file size
  and clarify responsibilities inside the `bus` module.

Behavior
- Reads/writes use the NES 2 KiB RAM mirroring semantics ($0000–$1FFF mirrored every 0x0800).
- Delegates to `Ram::read`/`Ram::write`, which apply mirroring.

Notes
- The `Bus` exposes restricted accessors `ram()`/`ram_mut()` for sibling modules in `bus` to
  reach the underlying `Ram` storage without exposing fields publicly.
*/

#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(unused_macros)]

#[inline]
pub(in crate::bus) fn ram_read_mirrored(bus: &crate::bus::bus_impl::Bus, addr: u16) -> u8 {
    bus.ram().read(addr)
}

#[inline]
pub(in crate::bus) fn ram_write_mirrored(
    bus: &mut crate::bus::bus_impl::Bus,
    addr: u16,
    value: u8,
) {
    bus.ram_mut().write(addr, value);
}
