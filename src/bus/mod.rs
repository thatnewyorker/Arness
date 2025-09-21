#![doc = r#"
Bus module: modular façade and submodules.

Overview
- This directory contains the modularized Bus façade and focused submodules. It replaces
  the previous single-file `src/bus.rs` incrementally while preserving behavior.

Modules and responsibilities
- Bus: public façade implemented directly in this module (migrated from legacy).
- cpu_interface: CPU-visible address decoder and helpers (read/write, read_word); delegates to devices.
- ppu_registers: CPU-visible PPU register window (0x2000-0x3FFF, with mirroring and PPUDATA semantics).
- ppu_space: PPU address-space mapping (nametables, palette, mirroring) via `PpuAddressSpace` and pure helpers.
- ppu_memory: Bus-level PPU memory helpers that resolve header/dynamic mirroring and delegate to mapper/PPU RAM.
- dma: OAM DMA controller (`DmaController`) and minimal traits (`CpuMemory`, `OamWriter`) for decoupling.
- dma_glue: DMA glue impls (e.g., `CpuMemory` for `Bus`, `OamWriter` for `Ppu`) kept small and internal.
- clock: tick/scheduler orchestration (advance CPU, step PPU 3x, DMA micro-step, latch NMI, step APU, aggregate IRQ).
- interfaces: lightweight views/traits to reduce borrowing friction (e.g., `BusPpuView` for read-only PPU access).
- ram_helpers: CPU RAM mirrored access wrappers (internal helpers used by the Bus façade).
- integration_helpers: convenience wrappers and compatibility accessors used by tests and integration code.
- nametable_mapper: optional pure mapping logic for advanced mirroring strategies.

Migration notes
- The public Bus façade remains stable; internal responsibilities are delegated to these submodules.
- Submodules can evolve independently and are designed for isolated testing.
"#]
#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(unused_macros)]
#![allow(unused_mut)]

// Bus implementation (flattened at top level)
/**
Bus abstraction mapping CPU address space to RAM, PPU/APU, controllers, and cartridge.

Address map (CPU):
- $0000-$07FF: 2KB internal RAM
- $0800-$1FFF: Mirrors of $0000-$07FF (mask with & 0x07FF)
- $2000-$2007: PPU registers
- $2008-$3FFF: Mirrors of $2000-$2007 (mask with & 0x2007)
- $4000-$4013: APU registers
- $4014: OAM DMA (PPU) - cycle-accurate 256-byte transfer from CPU page ($XX00-$XXFF) to PPU OAM; CPU is stalled while PPU/APU continue
- $4015: APU status (read) / enables (write)
- $4016: Controller 1 strobe (write), Controller 1 serial read (read)
- $4017: APU frame counter (write), Controller 2 serial read (read)
- $4020-$5FFF: Expansion area (stubbed: read 0, ignore writes)
- $6000-$7FFF: Cartridge PRG RAM (if present)
- $8000-$FFFF: Cartridge PRG ROM (mapper-controlled; NROM initially)

Notes:
- Bus advances time via tick(), stepping PPU 3x per CPU cycle and APU 1x per CPU cycle.
- OAM DMA is modeled as a per-byte pipeline inside tick(): optional 1-cycle alignment, then alternating read/write cycles (513 cycles if started on even CPU cycle, 514 if odd).
- TODO: DMA source reads from I/O ($2000-$401F) can trigger side effects; consider masking to open bus or restricting DMA source to RAM/PRG to avoid surprising behavior.
- Open bus behavior is not modeled precisely yet; some registers currently return mirrors for development visibility.
*/
use crate::apu::Apu;
use crate::cartridge::Cartridge;
use crate::controller::Controller;

use crate::ppu::Ppu;

// DMA glue trait impls moved to module scope

impl Ppu {
    #[inline]
    fn write_oam_via_dma(&mut self, value: u8) {
        self.write_reg(0x2004, value);
    }
}

pub struct Bus {
    // 2KB CPU RAM
    ram: crate::bus::ram::Ram,

    // Devices
    pub ppu: Ppu,
    pub apu: Apu,
    pub controllers: [Controller; 2],

    // Cartridge (PRG ROM/RAM and mapper)
    pub cartridge: Option<Cartridge>,

    // Timing/cycle tracking
    pub cpu_cycle: u64,
    pub ppu_cycle: u64,

    // PPU memory and register shadows for Bus-side PPUDATA path
    ppu_mem: crate::bus::ppu_space::PpuAddressSpace,

    // DMA controller (cycle-accurate OAM DMA)
    dma: crate::bus::dma::DmaController,

    // Interrupt lines
    pub nmi_pending: bool,
    pub irq_line: bool,
}
impl Default for Bus {
    fn default() -> Self {
        Self::new()
    }
}

impl Bus {
    pub fn new() -> Self {
        Self {
            ram: crate::bus::ram::Ram::new(),
            ppu: Ppu::new(),
            apu: Apu::new(),
            controllers: [Controller::new(), Controller::new()],
            cartridge: None,

            cpu_cycle: 0,
            ppu_cycle: 0,

            // PPU address space
            ppu_mem: crate::bus::ppu_space::PpuAddressSpace::new(),

            dma: crate::bus::dma::DmaController::new(),

            nmi_pending: false,
            irq_line: false,
        }
    }

    pub fn reset(&mut self) {
        self.ram.reset();
        self.ppu.reset();
        self.apu.reset();

        // Clear PPU address space RAM
        self.ppu_mem.nt_ram.fill(0);
        self.ppu_mem.palette_ram.fill(0);

        // Controllers: keep state, clear latches/indices
        self.controllers = [Controller::new(), Controller::new()];
        // Cartridge: keep ROM/RAM contents; callers can reload if desired.
    }

    #[inline]
    pub fn attach_cartridge(&mut self, cart: Cartridge) {
        self.cartridge = Some(cart);
    }

    // -----------------------------
    // CPU-visible memory interface
    // -----------------------------

    #[inline]
    pub fn read(&mut self, addr: u16) -> u8 {
        crate::bus::cpu_interface::cpu_read(self, addr)
    }

    #[inline]
    pub fn write(&mut self, addr: u16, value: u8) {
        crate::bus::cpu_interface::cpu_write(self, addr, value, |bus, page| {
            bus.dma.start(page, bus.cpu_cycle);
        });
    }

    // -----------------------------
    // PPU memory mapping helpers
    // -----------------------------

    #[inline]
    fn ppu_mem_read(&self, addr: u16) -> u8 {
        crate::bus::ppu_memory::ppu_mem_read(self, addr)
    }

    #[inline]
    fn ppu_mem_write(&mut self, addr: u16, value: u8) {
        crate::bus::ppu_memory::ppu_mem_write(self, addr, value)
    }

    // removed: map_nametable_addr moved to bus/ppu_space.rs

    /// Public wrapper for reading from PPU address space (0x0000-0x3FFF) using the
    /// same mirroring and mapper logic as CPU-driven PPUDATA accesses. Intended
    /// for rendering code (e.g., background/sprite fetching) to obtain pattern,
    /// nametable, attribute and palette bytes without duplicating mapping logic.
    #[inline]
    pub fn ppu_read(&self, addr: u16) -> u8 {
        self.ppu_mem_read(addr)
    }

    /// Public wrapper for writing to PPU address space (used mainly in tests or
    /// tools to prime pattern/nametable/palette memory deterministically).
    #[inline]
    pub fn ppu_write(&mut self, addr: u16, value: u8) {
        self.ppu_mem_write(addr, value);
    }

    // removed: map_palette_addr moved to bus/ppu_space.rs

    // Convenience for little-endian word reads via Bus (used by CPU vectors).
    #[inline]
    pub fn read_word(&mut self, addr: u16) -> u16 {
        let lo = self.read(addr) as u16;
        let hi = self.read(addr.wrapping_add(1)) as u16;
        (hi << 8) | lo
    }

    // DMA state accessors for external users (CPU)
    #[inline]
    pub fn dma_is_active(&self) -> bool {
        self.dma.is_active()
    }

    #[inline]
    pub fn dma_stall_remaining(&self) -> u32 {
        self.dma.stall_remaining()
    }

    /// Advance bus time by the specified number of CPU cycles.
    /// - Increments CPU cycles; steps PPU 3x per CPU cycle.
    /// - Consumes DMA stall cycles when active.
    /// - Polls PPU NMI latch after each CPU cycle and sets nmi_pending if requested.
    pub fn tick(&mut self, cycles: u32) {
        crate::bus::clock::tick(self, cycles, |bus| {
            // Perform exactly one DMA micro-step (alignment/read/write) using the controller,
            // borrowing only the necessary Bus subfields to avoid overlapping borrows.
            let mut mem_view = crate::bus::dma::CpuMemoryView::from_parts(
                &mut bus.ram,
                bus.cartridge.as_mut(),
                &mut bus.controllers,
            );
            let ppu = &mut bus.ppu;
            let dma = &mut bus.dma;
            crate::bus::bus_helpers::dma_step_one(dma, &mut mem_view, ppu);
        });
    }

    /// Step the PPU three times (one CPU cycle worth).
    /// Thin delegator to `bus_helpers::step_ppu_three` to keep the Bus façade focused.
    #[inline]
    pub fn step_ppu_three(&mut self) {
        crate::bus::bus_helpers::step_ppu_three(self);
    }

    /// Return the total number of CPU cycles elapsed (external accessor for tests).
    #[inline]
    pub fn total_ticks(&self) -> u64 {
        self.cpu_cycle
    }

    // Direct CPU RAM mirrored accessors (hot path)
    #[inline]
    pub fn ram_read_mirrored(&self, addr: u16) -> u8 {
        crate::bus::ram_helpers::ram_read_mirrored(self, addr)
    }

    #[inline]
    pub fn ram_write_mirrored(&mut self, addr: u16, value: u8) {
        crate::bus::ram_helpers::ram_write_mirrored(self, addr, value);
    }

    // Minimal DMA glue implementations moved to module scope

    // -----------------------------
    // Integration helpers
    // -----------------------------

    #[inline]
    pub fn ppu_mut(&mut self) -> &mut Ppu {
        &mut self.ppu
    }

    #[inline]
    pub fn apu_mut(&mut self) -> &mut Apu {
        &mut self.apu
    }

    /// Immutable reference to the PPU (useful in tests after a scoped mutable borrow).
    #[inline]
    pub fn ppu(&self) -> &Ppu {
        &self.ppu
    }

    /// Immutable reference to the PPU address space (for constructing PPU views).
    #[inline]
    pub fn ppu_mem(&self) -> &crate::bus::ppu_space::PpuAddressSpace {
        &self.ppu_mem
    }

    #[inline]
    pub(in crate::bus) fn ppu_mem_mut(&mut self) -> &mut crate::bus::ppu_space::PpuAddressSpace {
        &mut self.ppu_mem
    }

    /// Split borrow for render/debug helpers: returns a mutable PPU reference together
    /// with immutable references to the PPU address space and inserted cartridge.
    #[inline]
    pub(in crate::bus) fn split_ppu_mem_and_cart(
        &mut self,
    ) -> (
        &mut Ppu,
        &crate::bus::ppu_space::PpuAddressSpace,
        Option<&Cartridge>,
    ) {
        (&mut self.ppu, &self.ppu_mem, self.cartridge.as_ref())
    }

    #[inline]
    pub(in crate::bus) fn ram(&self) -> &crate::bus::ram::Ram {
        &self.ram
    }

    #[inline]
    pub(in crate::bus) fn ram_mut(&mut self) -> &mut crate::bus::ram::Ram {
        &mut self.ram
    }

    /// Render a full PPU frame into the PPU's internal framebuffer.
    ///
    /// Thin delegator to `bus_helpers::render_ppu_frame` to keep the Bus façade focused.
    #[inline]
    pub fn render_ppu_frame(&mut self) {
        crate::bus::bus_helpers::render_ppu_frame(self);
    }

    #[inline]
    pub fn controller_mut(&mut self, idx: usize) -> Option<&mut Controller> {
        self.controllers.get_mut(idx)
    }

    #[inline]
    pub fn cartridge_mut(&mut self) -> Option<&mut Cartridge> {
        self.cartridge.as_mut()
    }

    #[inline]
    pub fn cartridge_ref(&self) -> Option<&Cartridge> {
        self.cartridge.as_ref()
    }
}

/// CPU-visible memory map and helpers (dispatcher for address ranges).
pub mod cpu_interface;

pub mod ppu_memory;
/// PPU registers handler (CPU-visible 0x2000-0x3FFF).
pub mod ppu_registers;
/// PPU address-space mapping: nametables, palette, mirroring rules.
pub mod ppu_space;

pub mod nametable_mapper {
    //! Optional pure component for nametable address mapping.
    //!
    //! Responsibilities (optional):
    //! - Resolve logical nametable address to physical index based on mirroring mode.
}

/// APU register window handler (0x4000–0x4017 subset).
pub mod apu_registers;
mod bus_helpers;
/// Controller ($4016/$4017) handler.
pub mod controller_registers;
pub mod dma;
pub mod dma_glue;
/// Cycle-accurate OAM DMA controller.
pub mod ram;
pub mod ram_helpers;

/// Tick/scheduler orchestration for CPU/PPU/APU/DMA and interrupts.
pub mod clock;

/// Small traits/interfaces to decouple modules.
pub mod interfaces;

pub mod integration_helpers {
    //! Backwards-compatibility and test helpers.
    //!
    //! Responsibilities (to be migrated here):
    //! - `ppu_mut`, `apu_mut`, `cartridge_mut`, etc.
    //! - Frame rendering helpers if needed during migration.
}

// Public re-exports for consumers. As functionality migrates into the submodules,
// these `pub use` lines ensure the public surface remains discoverable from `bus`.
pub use apu_registers::*;
pub use clock::*;
pub use controller_registers::*;
pub use cpu_interface::*;
pub use dma::*;
pub use integration_helpers::*;
pub use nametable_mapper::*;
pub use ppu_registers::*;
pub use ppu_space::*;
pub use ram::*;

#[cfg(test)]
mod tests;
