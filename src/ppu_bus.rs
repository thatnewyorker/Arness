#![doc = r#"
Compatibility shim for the PPU bus interface.

This module re-exports the `PpuBus` trait centralized in `bus::interfaces`.
It also re-exports the `MockPpuBus` used by tests, keeping existing import
paths (`crate::ppu_bus::...`) working during the migration window.

Preferred import going forward:
- use `crate::bus::interfaces::PpuBus`;

Temporary compatibility (tests only):
- `MockPpuBus` continues to be available here under `#[cfg(test)]`.
"#]

pub use crate::bus::interfaces::PpuBus;

#[cfg(test)]
pub use crate::bus::interfaces::MockPpuBus;
