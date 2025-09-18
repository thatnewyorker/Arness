/*
Module: mappers

Slimmed dispatcher module: declares mapper submodules and re-exports their
public types. Concrete implementations live in their own files for clarity.

Implemented:
- CNROM (Mapper 3)
- MMC1 (Mapper 1)
- MMC3 (Mapper 4) – Phase 1 (PRG/CHR banking; IRQ logic pending)
*/

pub mod cnrom;
pub mod mmc1;
pub mod mmc3; // MMC3 phase 1 implemented (IRQ to be added)

pub use cnrom::Cnrom;
pub use mmc1::Mmc1;
pub use mmc3::Mmc3;
