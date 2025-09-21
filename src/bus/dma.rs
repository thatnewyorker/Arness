/*!
DmaController: cycle-accurate OAM DMA state machine.

Purpose
- Encapsulate the OAM DMA lifecycle and per-cycle micro-steps.
- Provide a minimal interface that reads from CPU memory and writes to PPU OAM.
- Make CPU stall semantics and remaining stall cycles queryable.

Behavioral model (matching current Bus DMA semantics)
- When started (write to $4014), DMA performs:
  - 1 or 2 initial alignment cycles (513 total cycles if on even CPU cycle; 514 if on odd).
  - Then 256 alternating read/write micro-steps:
    - Read one byte from CPU space at ($XX00 + index).
    - Write the latched byte to PPU OAMDATA ($2004), which increments OAMADDR internally.
- While active, CPU is considered "stalled" each cycle, even though other devices continue (PPU/APU).
- Reads from CPU address space must preserve side-effects exactly as a CPU read would.

Public API
- DmaController::start(src_page, cpu_cycle): begin a DMA transfer from `src_page << 8` source.
- DmaController::step_one_cycle(mem, oam): perform a single micro-step; returns whether CPU is stalled this cycle.
- DmaController::is_active(): whether a transfer is ongoing.
- DmaController::stall_remaining(): number of CPU stall cycles remaining (alignment + transfer cycles).

Integration notes
- Call `start` when CPU writes to $4014 (OAM DMA register).
- Call `step_one_cycle` once per CPU cycle while `is_active()` returns true.
- PPU writes use a minimal `OamWriter` trait to avoid borrowing the full PPU.
*/

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DmaPhase {
    Read,
    Write,
}

/// Minimal CPU-memory interface used by DMA to fetch source bytes.
/// This must behave exactly like CPU-visible reads (including any side-effects).
pub trait CpuMemory {
    fn cpu_read(&mut self, addr: u16) -> u8;
}

/// Minimal OAM write interface used by DMA to push a byte into PPU OAM.
/// Equivalent to writing to $2004 (OAMDATA), which increments OAMADDR internally.
pub trait OamWriter {
    fn write_oam_data(&mut self, value: u8);
}

/// CpuMemoryView: a lightweight adapter that borrows Bus subfields used by DMA.
/// Implements CpuMemory without borrowing the full Bus, enabling non-overlapping borrows.
pub(in crate::bus) struct CpuMemoryView<'a> {
    ram: &'a mut crate::bus::ram::Ram,
    cartridge: Option<&'a mut crate::cartridge::Cartridge>,
    controllers: &'a mut [crate::controller::Controller; 2],
}

impl<'a> CpuMemoryView<'a> {
    #[inline]
    pub(in crate::bus) fn from_parts(
        ram: &'a mut crate::bus::ram::Ram,
        cartridge: Option<&'a mut crate::cartridge::Cartridge>,
        controllers: &'a mut [crate::controller::Controller; 2],
    ) -> Self {
        Self {
            ram,
            cartridge,
            controllers,
        }
    }
}

impl<'a> CpuMemory for CpuMemoryView<'a> {
    #[inline]
    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => {
                // 2KB RAM mirrored every 0x0800
                self.ram.read(addr)
            }
            0x2000..=0x3FFF => {
                // DMA reads from PPU register window are treated as open bus/no side-effects here.
                // Matching historical behavior where DMA used a placeholder PPU instance.
                0
            }
            0x4000..=0x4017 => {
                // APU and controller registers are not sourced during DMA in our model.
                // Returning 0 avoids side effects during DMA source reads.
                0
            }
            0x4018..=0x5FFF => 0,
            0x6000..=0x7FFF => {
                if let Some(cart) = self.cartridge.as_mut() {
                    cart.mapper.borrow_mut().cpu_read(addr)
                } else {
                    0
                }
            }
            0x8000..=0xFFFF => {
                if let Some(cart) = self.cartridge.as_mut() {
                    cart.mapper.borrow_mut().cpu_read(addr)
                } else {
                    0xFF
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct DmaController {
    active: bool,
    src_addr: u16,    // high page << 8
    index: u16,       // 0..=255
    phase: DmaPhase,  // Read -> Write alternating
    latch: u8,        // latched byte between read and write
    align_cycles: u8, // 1 if even start cycle, 2 if odd
}

impl Default for DmaController {
    fn default() -> Self {
        Self::new()
    }
}

impl DmaController {
    pub fn new() -> Self {
        Self {
            active: false,
            src_addr: 0,
            index: 0,
            phase: DmaPhase::Read,
            latch: 0,
            align_cycles: 0,
        }
    }

    /// Reset to idle state with no active DMA.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// Start an OAM DMA transfer from the given source page ($XX00-$XXFF).
    ///
    /// `cpu_cycle` is the current CPU cycle counter; its parity determines alignment:
    /// - even -> 1 alignment cycle (513 total)
    /// - odd  -> 2 alignment cycles (514 total)
    pub fn start(&mut self, src_page: u8, cpu_cycle: u64) {
        self.active = true;
        self.src_addr = (src_page as u16) << 8;
        self.index = 0;
        self.phase = DmaPhase::Read;
        self.latch = 0;
        self.align_cycles = 1 + ((cpu_cycle & 1) as u8);
    }

    /// Returns true if a DMA transfer is currently in progress.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Number of CPU stall cycles remaining for the current transfer (including alignment).
    /// Returns 0 when idle.
    pub fn stall_remaining(&self) -> u32 {
        if !self.active {
            return 0;
        }
        let align = self.align_cycles as u32;
        let bytes_left = 256u32.saturating_sub(self.index as u32);

        // If currently in read phase: each remaining byte costs 2 cycles (R/W).
        // If currently in write phase: 1 cycle to finish current write, then 2 per remaining-1 bytes.
        let transfer_cycles = match self.phase {
            DmaPhase::Read => bytes_left.saturating_mul(2),
            DmaPhase::Write => {
                if bytes_left == 0 {
                    0
                } else {
                    1 + (bytes_left - 1) * 2
                }
            }
        };
        align + transfer_cycles
    }

    /// Perform a single DMA micro-step (one CPU cycle).
    ///
    /// Returns:
    /// - true if the CPU is stalled this cycle (always true while DMA is active)
    /// - false if no DMA is active (no stall)
    ///
    /// Contract:
    /// - Must be called once per CPU cycle while `is_active()` is true.
    /// - Reads are performed via `CpuMemory::cpu_read` to preserve side-effects.
    /// - Writes are performed via `OamWriter::write_oam_data`, equivalent to writing $2004.
    pub fn step_one_cycle<M: CpuMemory, O: OamWriter>(&mut self, mem: &mut M, oam: &mut O) -> bool {
        if !self.active {
            return false;
        }

        // Alignment cycles before first read
        if self.align_cycles > 0 {
            self.align_cycles = self.align_cycles.saturating_sub(1);
            return true;
        }

        match self.phase {
            DmaPhase::Read => {
                let addr = self.src_addr.wrapping_add(self.index);
                self.latch = mem.cpu_read(addr);
                self.phase = DmaPhase::Write;
                true
            }
            DmaPhase::Write => {
                oam.write_oam_data(self.latch);
                self.index = self.index.wrapping_add(1);
                if self.index >= 256 {
                    // Done
                    self.active = false;
                    self.phase = DmaPhase::Read;
                } else {
                    self.phase = DmaPhase::Read;
                }
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::ram::Ram;
    use crate::controller::Controller;

    struct MockMem {
        data: [u8; 0x10000],
    }

    impl MockMem {
        fn new_fill_pattern() -> Self {
            let mut m = MockMem { data: [0; 0x10000] };
            for i in 0..0x10000usize {
                m.data[i] = (i & 0xFF) as u8;
            }
            m
        }
    }

    impl CpuMemory for MockMem {
        fn cpu_read(&mut self, addr: u16) -> u8 {
            self.data[addr as usize]
        }
    }

    struct SinkOam {
        writes: Vec<u8>,
    }

    impl SinkOam {
        fn new() -> Self {
            SinkOam { writes: Vec::new() }
        }
    }

    impl OamWriter for SinkOam {
        fn write_oam_data(&mut self, value: u8) {
            self.writes.push(value);
        }
    }

    #[test]
    fn cpu_memory_view_ram_mirroring() {
        let mut ram = Ram::new();
        let mut controllers = [Controller::new(), Controller::new()];

        // Write a value and verify mirrors read the same
        ram.write(0x0001, 0xAA);

        {
            let mut view = CpuMemoryView::from_parts(&mut ram, None, &mut controllers);
            assert_eq!(view.cpu_read(0x0001), 0xAA);
            assert_eq!(view.cpu_read(0x0801), 0xAA);
            assert_eq!(view.cpu_read(0x1801), 0xAA);
        }

        // Overwrite via a mirrored address and re-verify
        ram.write(0x1801, 0x55);
        {
            let mut view = CpuMemoryView::from_parts(&mut ram, None, &mut controllers);
            assert_eq!(view.cpu_read(0x0001), 0x55);
            assert_eq!(view.cpu_read(0x0801), 0x55);
            assert_eq!(view.cpu_read(0x1801), 0x55);
        }
    }

    #[test]
    fn dma_align_cycles_behavior_even_cycle() {
        let mut dma = DmaController::new();
        let mut mem = MockMem::new_fill_pattern();
        let mut oam = SinkOam::new();

        // Start on even CPU cycle: expect 1 align cycle (513 total)
        dma.start(0x02, 0);
        assert_eq!(dma.stall_remaining(), 513);

        let mut cycles = 0u32;
        while dma.is_active() {
            let _stalled = dma.step_one_cycle(&mut mem, &mut oam);
            cycles += 1;
        }

        assert_eq!(cycles, 513);
        assert_eq!(oam.writes.len(), 256);

        // Expected sequence: bytes from page 0x02 (0x0200..=0x02FF)
        for (i, &b) in oam.writes.iter().enumerate() {
            assert_eq!(b, ((0x0200 + i as u16) & 0xFF) as u8);
        }

        assert_eq!(dma.stall_remaining(), 0);
    }

    #[test]
    fn dma_align_cycles_behavior_odd_cycle() {
        let mut dma = DmaController::new();
        let mut mem = MockMem::new_fill_pattern();
        let mut oam = SinkOam::new();

        // Start on odd CPU cycle: expect 2 align cycles (514 total)
        dma.start(0x03, 1);
        assert_eq!(dma.stall_remaining(), 514);

        let mut cycles = 0u32;
        while dma.is_active() {
            let _stalled = dma.step_one_cycle(&mut mem, &mut oam);
            cycles += 1;
        }

        assert_eq!(cycles, 514);
        assert_eq!(oam.writes.len(), 256);

        // Expected sequence: bytes from page 0x03 (0x0300..=0x03FF)
        for (i, &b) in oam.writes.iter().enumerate() {
            assert_eq!(b, ((0x0300 + i as u16) & 0xFF) as u8);
        }

        assert_eq!(dma.stall_remaining(), 0);
    }

    #[test]
    fn dma_read_write_alternation_and_completion() {
        let mut dma = DmaController::new();
        let mut mem = MockMem::new_fill_pattern();
        let mut oam = SinkOam::new();

        // Even cycle -> 1 align cycle, then R/W alternation
        dma.start(0x10, 0);
        assert!(dma.is_active());

        // Consume alignment
        let _ = dma.step_one_cycle(&mut mem, &mut oam);
        assert!(dma.is_active());
        assert_eq!(oam.writes.len(), 0);

        // First read (no write yet)
        let _ = dma.step_one_cycle(&mut mem, &mut oam);
        assert!(dma.is_active());
        assert_eq!(oam.writes.len(), 0);

        // Then write (one byte to OAM)
        let _ = dma.step_one_cycle(&mut mem, &mut oam);
        assert_eq!(oam.writes.len(), 1);
        assert_eq!(oam.writes[0], 0x00); // 0x1000 & 0xFF == 0x00

        // Finish the transfer
        let mut total_cycles = 3u32;
        while dma.is_active() {
            let _ = dma.step_one_cycle(&mut mem, &mut oam);
            total_cycles += 1;
        }

        // Total cycles = 513; total writes = 256
        assert_eq!(total_cycles, 513);
        assert_eq!(oam.writes.len(), 256);
        assert_eq!(dma.stall_remaining(), 0);
        assert!(!dma.is_active());
    }
}
