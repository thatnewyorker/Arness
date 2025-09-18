/*!
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
use crate::mapper::MapperMirroring;
use crate::ppu::Ppu;

pub struct Bus {
    // 2KB CPU RAM
    ram: [u8; 0x0800],

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
    nt_ram: [u8; 0x0800],  // 2 KiB nametable RAM
    palette_ram: [u8; 32], // 32-byte palette RAM

    // DMA state (cycle-accurate OAM DMA)
    dma_active: bool,
    dma_src_addr: u16,
    dma_index: u16,
    dma_phase: u8, // 1 = read, 2 = write
    dma_latch: u8,
    dma_align_cycles: u8, // initial 1 or 2 cycle alignment (513 or 514 total)

    // Interrupt lines
    pub nmi_pending: bool,
    pub irq_line: bool,
}

impl Bus {
    pub fn new() -> Self {
        Self {
            ram: [0; 0x0800],
            ppu: Ppu::new(),
            apu: Apu::new(),
            controllers: [Controller::new(), Controller::new()],
            cartridge: None,

            cpu_cycle: 0,
            ppu_cycle: 0,

            // PPU memory and register shadows initialized
            nt_ram: [0; 0x0800],
            palette_ram: [0; 32],

            dma_active: false,
            dma_src_addr: 0,
            dma_index: 0,
            dma_phase: 1,
            dma_latch: 0,
            dma_align_cycles: 0,

            nmi_pending: false,
            irq_line: false,
        }
    }

    pub fn reset(&mut self) {
        self.ram.fill(0);
        self.ppu.reset();
        self.apu.reset();

        // Clear PPU Bus-side memory and shadows
        self.nt_ram.fill(0);
        self.palette_ram.fill(0);

        // Controllers: keep state, clear latches/indices
        self.controllers = [Controller::new(), Controller::new()];
        // Cartridge: keep ROM/RAM contents; callers can reload if desired.
    }

    pub fn attach_cartridge(&mut self, cart: Cartridge) {
        self.cartridge = Some(cart);
    }

    // -----------------------------
    // CPU-visible memory interface
    // -----------------------------

    pub fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => {
                let idx = (addr & 0x07FF) as usize;
                self.ram[idx]
            }
            0x2000..=0x3FFF => {
                let reg = 0x2000 | (addr & 0x0007);
                match reg {
                    0x2002 => {
                        // PPUSTATUS: PPU handles vblank clear and write toggle reset
                        self.ppu.read_reg(reg)
                    }
                    0x2007 => {
                        // PPUDATA: Bus-side memory read using PPU latches for buffering and increment
                        let addr = self.ppu.get_vram_addr() & 0x3FFF;
                        let value = self.ppu_mem_read(addr);
                        let ret = if addr < 0x3F00 {
                            let out = self.ppu.get_vram_buffer();
                            self.ppu.set_vram_buffer(value);
                            out
                        } else {
                            value
                        };
                        let inc = self.ppu.vram_increment_step();
                        self.ppu.set_vram_addr(addr.wrapping_add(inc) & 0x3FFF);
                        ret
                    }
                    _ => self.ppu.read_reg(reg),
                }
            }
            0x4000..=0x4013 => self.apu.read_reg(addr),
            0x4014 => {
                // OAM DMA register read is not meaningful; return 0
                0
            }
            0x4015 => self.apu.read_status(),
            0x4016 => self.controllers[0].read(),
            0x4017 => {
                // Controller 2 serial read on read; writes go to APU frame counter.
                self.controllers[1].read()
            }
            0x4018..=0x401F => {
                // Typically disabled test registers; return 0
                0
            }
            0x4020..=0x5FFF => {
                // Expansion area (unused here)
                0
            }
            0x6000..=0x7FFF => {
                if let Some(cart) = &self.cartridge {
                    cart.mapper.borrow_mut().cpu_read(addr)
                } else {
                    0
                }
            }
            0x8000..=0xFFFF => {
                if let Some(cart) = &self.cartridge {
                    cart.mapper.borrow_mut().cpu_read(addr)
                } else {
                    0xFF
                }
            }
        }
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF => {
                let idx = (addr & 0x07FF) as usize;
                self.ram[idx] = value;
            }
            0x2000..=0x3FFF => {
                let reg = 0x2000 | (addr & 0x0007);
                match reg {
                    0x2000 => {
                        // PPUCTRL: forward to PPU; PPU owns ctrl state
                        self.ppu.write_reg(reg, value);
                    }
                    0x2005 => {
                        // PPUSCROLL: PPU manages write toggle internally
                        self.ppu.write_reg(reg, value);
                    }
                    0x2006 => {
                        // PPUADDR: PPU manages VRAM address and write toggle
                        self.ppu.write_reg(reg, value);
                    }
                    0x2007 => {
                        // PPUDATA: write via Bus mapping, then increment VRAM address via PPU
                        let addr = self.ppu.get_vram_addr() & 0x3FFF;
                        self.ppu_mem_write(addr, value);
                        let inc = self.ppu.vram_increment_step();
                        self.ppu.set_vram_addr(addr.wrapping_add(inc) & 0x3FFF);
                    }
                    _ => {
                        self.ppu.write_reg(reg, value);
                    }
                }
            }
            0x4000..=0x4013 => self.apu.write_reg(addr, value),
            0x4014 => {
                // OAM DMA: schedule a cycle-accurate transfer (CPU stalled; PPU/APU continue)
                self.dma_active = true;
                self.dma_src_addr = (value as u16) << 8;
                self.dma_index = 0;
                self.dma_phase = 1; // start with read phase after alignment
                self.dma_latch = 0;
                // 513 cycles if current CPU cycle is even, 514 if odd
                self.dma_align_cycles = 1 + ((self.cpu_cycle & 1) as u8);
            }
            0x4015 => self.apu.write_reg(addr, value),
            0x4016 => {
                // Controller strobe for both controllers (bit 0 relevant)
                self.controllers[0].write_strobe(value);
                self.controllers[1].write_strobe(value);
            }
            0x4017 => self.apu.write_reg(addr, value),
            0x4018..=0x401F => {
                // Typically disabled test registers; ignore writes
            }
            0x4020..=0x5FFF => {
                // Expansion area: ignore
            }
            0x6000..=0x7FFF => {
                if let Some(cart) = &self.cartridge {
                    cart.mapper.borrow_mut().cpu_write(addr, value);
                }
            }
            0x8000..=0xFFFF => {
                if let Some(cart) = &self.cartridge {
                    cart.mapper.borrow_mut().cpu_write(addr, value);
                }
            }
        }
    }

    // -----------------------------
    // PPU memory mapping helpers
    // -----------------------------

    fn ppu_mem_read(&self, addr: u16) -> u8 {
        let a = addr & 0x3FFF;
        match a {
            0x0000..=0x1FFF => {
                if let Some(cart) = &self.cartridge {
                    cart.mapper.borrow().ppu_read(a)
                } else {
                    0
                }
            }
            0x2000..=0x3EFF => {
                // Nametable region with mirroring; $3000-$3EFF mirrors $2000-$2EFF
                let base = 0x2000 | (a & 0x0FFF);
                let idx = self.map_nametable_addr(base);
                self.nt_ram[idx]
            }
            0x3F00..=0x3FFF => {
                let idx = self.map_palette_addr(a);
                self.palette_ram[idx]
            }
            _ => 0,
        }
    }

    fn ppu_mem_write(&mut self, addr: u16, value: u8) {
        let a = addr & 0x3FFF;
        match a {
            0x0000..=0x1FFF => {
                if let Some(cart) = &self.cartridge {
                    cart.mapper.borrow_mut().ppu_write(a, value);
                }
            }
            0x2000..=0x3EFF => {
                let base = 0x2000 | (a & 0x0FFF);
                let idx = self.map_nametable_addr(base);
                self.nt_ram[idx] = value;
            }
            0x3F00..=0x3FFF => {
                let idx = self.map_palette_addr(a);
                self.palette_ram[idx] = value;
            }
            _ => {}
        }
    }

    fn map_nametable_addr(&self, addr: u16) -> usize {
        // Reduce to $2000-$2FFF
        let a = (addr - 0x2000) & 0x0FFF;
        let table = (a / 0x400) as u16; // 0..3
        let offset = (a % 0x400) as usize;

        // Determine header mirroring and (if allowed) dynamic mapper mirroring override.
        let (header_mirr, dyn_mirr) = if let Some(cart) = &self.cartridge {
            let header = cart.mirroring();
            // Do not allow mapper override for four-screen (header enforced).
            let dynamic = if !matches!(header, crate::cartridge::Mirroring::FourScreen) {
                cart.mapper.borrow().current_mirroring()
            } else {
                None
            };
            (header, dynamic)
        } else {
            (crate::cartridge::Mirroring::Horizontal, None)
        };

        let bank = if let Some(mode) = dyn_mirr {
            match mode {
                MapperMirroring::SingleScreenLower => 0,
                MapperMirroring::SingleScreenUpper => 1,
                MapperMirroring::Vertical => {
                    // 0,2 -> bank 0; 1,3 -> bank 1
                    if (table & 1) == 0 { 0 } else { 1 }
                }
                MapperMirroring::Horizontal => {
                    // 0,1 -> bank 0; 2,3 -> bank 1
                    if table < 2 { 0 } else { 1 }
                }
            }
        } else {
            match header_mirr {
                crate::cartridge::Mirroring::Horizontal => {
                    if table < 2 {
                        0
                    } else {
                        1
                    }
                }
                crate::cartridge::Mirroring::Vertical => {
                    if (table & 1) == 0 {
                        0
                    } else {
                        1
                    }
                }
                crate::cartridge::Mirroring::FourScreen => {
                    // Four-screen not fully modeled; approximate as vertical mirroring
                    if (table & 1) == 0 { 0 } else { 1 }
                }
            }
        };

        (bank as usize) * 0x400 + offset
    }

    /// Public wrapper for reading from PPU address space (0x0000-0x3FFF) using the
    /// same mirroring and mapper logic as CPU-driven PPUDATA accesses. Intended
    /// for rendering code (e.g., background/sprite fetching) to obtain pattern,
    /// nametable, attribute and palette bytes without duplicating mapping logic.
    pub fn ppu_read(&self, addr: u16) -> u8 {
        self.ppu_mem_read(addr)
    }

    /// Public wrapper for writing to PPU address space (used mainly in tests or
    /// tools to prime pattern/nametable/palette memory deterministically).
    pub fn ppu_write(&mut self, addr: u16, value: u8) {
        self.ppu_mem_write(addr, value);
    }

    fn map_palette_addr(&self, addr: u16) -> usize {
        // Mirror to 0x3F00-0x3F1F
        let mut idx = (addr - 0x3F00) as usize & 0x1F;
        // $3F10/$3F14/$3F18/$3F1C mirror $3F00/$3F04/$3F08/$3F0C
        if idx >= 16 && (idx & 0x03) == 0 {
            idx -= 16;
        }
        idx
    }

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
        self.dma_active
    }

    #[inline]
    pub fn dma_stall_remaining(&self) -> u32 {
        if !self.dma_active {
            return 0;
        }
        let align = self.dma_align_cycles as u32;
        let bytes_left = 256u32.saturating_sub(self.dma_index as u32);
        // If we're in read phase, each remaining byte costs 2 cycles.
        // If in write phase, 1 cycle to write current latched byte, then 2 cycles per remaining-1 bytes.
        let transfer_cycles = if self.dma_phase == 1 {
            bytes_left.saturating_mul(2)
        } else {
            if bytes_left == 0 {
                0
            } else {
                1 + (bytes_left - 1) * 2
            }
        };
        align + transfer_cycles
    }

    /// Advance bus time by the specified number of CPU cycles.
    /// - Increments CPU cycles; steps PPU 3x per CPU cycle.
    /// - Consumes DMA stall cycles when active.
    /// - Polls PPU NMI latch after each CPU cycle and sets nmi_pending if requested.
    pub fn tick(&mut self, cycles: u32) {
        for _ in 0..cycles {
            // Advance CPU counter
            self.cpu_cycle = self.cpu_cycle.wrapping_add(1);

            // Step PPU three times per CPU cycle
            for _ in 0..3 {
                self.ppu.tick();
                self.ppu_cycle = self.ppu_cycle.wrapping_add(1);
            }

            // Execute one DMA micro-step per CPU cycle if active
            if self.dma_active {
                if self.dma_align_cycles > 0 {
                    // Alignment/dummy cycles before first read
                    self.dma_align_cycles = self.dma_align_cycles.saturating_sub(1);
                } else if self.dma_phase == 1 {
                    // Read phase: fetch from CPU memory
                    let addr = self.dma_src_addr.wrapping_add(self.dma_index);
                    self.dma_latch = self.read(addr);
                    self.dma_phase = 2;
                } else {
                    // Write phase: write to PPU OAM via OAMDATA (increments OAMADDR)
                    self.ppu.write_reg(0x2004, self.dma_latch);
                    self.dma_index = self.dma_index.wrapping_add(1);
                    if self.dma_index >= 256 {
                        self.dma_active = false;
                        self.dma_phase = 1;
                    } else {
                        self.dma_phase = 1;
                    }
                }
            }

            // Latch NMI request from PPU (if any)
            if self.ppu.take_nmi_request() {
                self.nmi_pending = true;
            }

            // Step APU once per CPU cycle and aggregate IRQ (APU OR mapper)
            self.apu.tick(1);
            let mapper_irq = if let Some(cart) = &self.cartridge {
                cart.mapper.borrow().irq_pending()
            } else {
                false
            };
            self.irq_line = self.apu.irq_asserted() || mapper_irq;
        }
    }

    /// Return the total number of CPU cycles elapsed (external accessor for tests).
    pub fn total_ticks(&self) -> u64 {
        self.cpu_cycle as u64
    }

    // -----------------------------
    // Integration helpers
    // -----------------------------

    pub fn ppu_mut(&mut self) -> &mut Ppu {
        &mut self.ppu
    }

    pub fn apu_mut(&mut self) -> &mut Apu {
        &mut self.apu
    }

    /// Immutable reference to the PPU (useful in tests after a scoped mutable borrow).
    pub fn ppu(&self) -> &Ppu {
        &self.ppu
    }

    /// Render a full PPU frame into the PPU's internal framebuffer.
    ///
    /// Safe implementation: move the PPU out, render with only an immutable
    /// borrow of the Bus (Ppu::render_frame now takes &Bus), then move it back.
    /// This avoids overlapping mutable borrows of `self` and `self.ppu` without
    /// requiring any unsafe code.
    pub fn render_ppu_frame(&mut self) {
        // Move the PPU out to eliminate simultaneous &mut borrows the compiler would reject.
        let mut ppu = std::mem::replace(&mut self.ppu, Ppu::new());
        // Bus implements PpuBus; &*self satisfies the generic bound for render_frame.
        ppu.render_frame(&*self);
        // Move updated PPU state back.
        self.ppu = ppu;
    }

    pub fn controller_mut(&mut self, idx: usize) -> Option<&mut Controller> {
        self.controllers.get_mut(idx)
    }

    pub fn cartridge_mut(&mut self) -> Option<&mut Cartridge> {
        self.cartridge.as_mut()
    }

    pub fn cartridge_ref(&self) -> Option<&Cartridge> {
        self.cartridge.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cartridge::Cartridge;
    use crate::test_utils::build_ines;

    #[test]
    fn ram_mirroring() {
        let mut bus = Bus::new();
        bus.write(0x0001, 0xAA);
        assert_eq!(bus.read(0x0001), 0xAA);
        assert_eq!(bus.read(0x0801), 0xAA);
        assert_eq!(bus.read(0x1801), 0xAA);
    }

    #[test]
    fn ppu_reg_mirror() {
        let mut bus = Bus::new();
        // Write to $2000 via mirror $2008
        bus.write(0x2008, 0x80);
        // Read back PPUCTRL via $2000
        assert_eq!(bus.read(0x2000) & 0x80, 0x80);
    }

    #[test]
    fn controller_strobe_and_read() {
        let mut bus = Bus::new();
        // Strobe high then low
        bus.write(0x4016, 1);
        bus.write(0x4016, 0);
        // Default state: all released -> first 8 reads on $4016 should be 0, then 1s
        for _ in 0..8 {
            assert_eq!(bus.read(0x4016), 0);
        }
        assert_eq!(bus.read(0x4016), 1);
    }

    #[test]
    fn prg_ram_basic() {
        let mut bus = Bus::new();
        // Build minimal NROM cart with PRG RAM (using helper that allocates 8KiB by default)
        let data = {
            // iNES header: "NES<1A>", 1x16KB PRG, 1x8KB CHR, flags6=0, flags7=0, prg_ram_8k=1
            let mut v = vec![];
            v.extend_from_slice(b"NES\x1A");
            v.push(1);
            v.push(1);
            v.push(0);
            v.push(0);
            v.push(1);
            v.extend_from_slice(&[0u8; 7]);
            v.extend(std::iter::repeat(0xAA).take(16 * 1024));
            v.extend(std::iter::repeat(0xCC).take(8 * 1024));
            v
        };
        let cart = Cartridge::from_ines_bytes(&data).expect("parse");
        bus.attach_cartridge(cart);

        bus.write(0x6000, 0x42);
        assert_eq!(bus.read(0x6000), 0x42);
    }

    #[test]
    fn oam_dma_copies_256_bytes() {
        let mut bus = Bus::new();
        // Prepare page $0200..$02FF with incrementing values.
        for i in 0..256u16 {
            bus.write(0x0200 + i, (i & 0xFF) as u8);
        }
        // Set OAMADDR to 0xFE
        bus.write(0x2003, 0xFE);
        // Trigger OAM DMA from $0200
        bus.write(0x4014, 0x02);

        // Advance cycles for DMA to complete (CPU even -> 513 cycles)
        bus.tick(513);

        // OAM should now contain 0x00 at 0xFE, 0x01 at 0xFF, 0x02 at 0x00, ...
        assert_eq!(bus.ppu.peek_oam(0xFE), 0x00);
        assert_eq!(bus.ppu.peek_oam(0xFF), 0x01);
        assert_eq!(bus.ppu.peek_oam(0x00), 0x02);
        assert_eq!(bus.ppu.peek_oam(0x01), 0x03);
    }

    // ---------- New tests for PPU memory mapping ----------

    #[test]
    fn nametable_horizontal_mirroring() {
        // Horizontal mirroring: $2000/$2400 share, $2800/$2C00 share
        let flags6 = 0b0000_0000; // horizontal
        let flags7 = 0;
        let rom = build_ines(1, 1, flags6, flags7, 1, None);
        let cart = Cartridge::from_ines_bytes(&rom).expect("parse");
        let mut bus = Bus::new();
        bus.attach_cartridge(cart);

        // Set PPUADDR to $2000 and write via PPUDATA
        bus.write(0x2006, 0x20);
        bus.write(0x2006, 0x00);
        bus.write(0x2007, 0x55);

        // Read back from mirror $2400: reset addr latch then read twice (buffer then true)
        let _ = bus.read(0x2002); // reset write toggle
        bus.write(0x2006, 0x24);
        bus.write(0x2006, 0x00);
        let _ = bus.read(0x2007); // buffered read (ignored)
        let v = bus.read(0x2007);
        assert_eq!(v, 0x55);
    }

    #[test]
    fn nametable_vertical_mirroring() {
        // Vertical mirroring: $2000/$2800 share, $2400/$2C00 share
        let flags6 = 0b0000_0001; // vertical
        let flags7 = 0;
        let rom = build_ines(1, 1, flags6, flags7, 1, None);
        let cart = Cartridge::from_ines_bytes(&rom).expect("parse");
        let mut bus = Bus::new();
        bus.attach_cartridge(cart);

        // Set PPUADDR to $2000 and write via PPUDATA
        bus.write(0x2006, 0x20);
        bus.write(0x2006, 0x00);
        bus.write(0x2007, 0x66);

        // Read back from mirror $2800
        let _ = bus.read(0x2002);
        bus.write(0x2006, 0x28);
        bus.write(0x2006, 0x00);
        let _ = bus.read(0x2007);
        let v = bus.read(0x2007);
        assert_eq!(v, 0x66);
    }

    #[test]
    fn palette_mirroring_3f10_mirrors_3f00() {
        let mut bus = Bus::new();

        // Write $3F00 via PPUDATA
        bus.write(0x2006, 0x3F);
        bus.write(0x2006, 0x00);
        bus.write(0x2007, 0x12);

        // Read $3F10 (mirror of $3F00); palette reads are immediate (not buffered)
        let _ = bus.read(0x2002);
        bus.write(0x2006, 0x3F);
        bus.write(0x2006, 0x10);
        let v = bus.read(0x2007);
        assert_eq!(v, 0x12);
    }

    #[test]
    fn ppudata_buffered_read_and_increment_via_bus() {
        // Use CHR RAM by building a cart with 0 CHR banks (CHR RAM allocated)
        let flags6 = 0b0000_0000; // mirroring doesn't matter here
        let flags7 = 0;
        let rom = build_ines(1, 0, flags6, flags7, 1, None);
        let cart = Cartridge::from_ines_bytes(&rom).expect("parse");

        let mut bus = Bus::new();
        bus.attach_cartridge(cart);

        // Set VRAM increment to 1
        bus.write(0x2000, 0x00);

        // Write to CHR RAM at $0000 and $0001 via PPUDATA
        bus.write(0x2006, 0x00);
        bus.write(0x2006, 0x00);
        bus.write(0x2007, 0x11);
        bus.write(0x2007, 0x22);

        // Reset latch, set address to $0000 and perform buffered reads
        let _ = bus.read(0x2002);
        bus.write(0x2006, 0x00);
        bus.write(0x2006, 0x00);

        // First read returns buffer (initially 0)
        assert_eq!(bus.read(0x2007), 0x00);
        // Second read returns 0x11
        assert_eq!(bus.read(0x2007), 0x11);
        // Third read returns 0x22
        assert_eq!(bus.read(0x2007), 0x22);
    }
    // -------- Dynamic MMC1 mirroring tests --------
    //
    // These tests construct a minimal MMC1 cartridge (mapper 1) and exercise
    // control register (mirroring bits 0-1) to verify Bus nametable mapping
    // responds immediately to dynamic mirroring changes.

    fn build_mmc1_ines(prg_16k: u8, chr_8k: u8, flags6: u8, flags7: u8, prg_ram_8k: u8) -> Vec<u8> {
        // Minimal iNES builder (no trainer, no NES2.0) with mapper id from flags.
        let mut v = Vec::new();
        v.extend_from_slice(b"NES\x1A");
        v.push(prg_16k); // PRG banks (16K units)
        v.push(chr_8k); // CHR banks (8K units; 0 => CHR RAM 8K)
        v.push(flags6); // flags6
        v.push(flags7); // flags7
        v.push(prg_ram_8k); // PRG RAM size in 8K units (0 => custom allocate 8K convention; we pass 1)
        v.extend_from_slice(&[0u8; 7]); // padding
        // PRG ROM
        v.extend(std::iter::repeat(0xEA).take(prg_16k as usize * 16 * 1024));
        // CHR (or zero if CHR RAM requested)
        if chr_8k > 0 {
            v.extend(std::iter::repeat(0x00).take(chr_8k as usize * 8 * 1024));
        }
        v
    }

    fn mmc1_serial_write(bus: &mut Bus, value5: u8) {
        // Write 5 LSB-first bits to $8000
        for i in 0..5 {
            let bit = (value5 >> i) & 1;
            bus.write(0x8000, bit);
        }
    }

    #[test]
    fn mmc1_dynamic_vertical_mirroring() {
        // Start with mapper 1 (MMC1). Set control bits to 0b00010 (vertical)
        // Mapper id 1 => flags6 upper nibble = 0001 => 0x10
        let rom = build_mmc1_ines(2, 1, 0x10, 0x00, 1);
        let cart = Cartridge::from_ines_bytes(&rom).expect("parse");
        let mut bus = Bus::new();
        bus.attach_cartridge(cart);

        // Write control = 0b00010 (vertical)
        mmc1_serial_write(&mut bus, 0b00010);

        // Write to $2000
        bus.write(0x2006, 0x20);
        bus.write(0x2006, 0x00);
        bus.write(0x2007, 0xA1);

        // Mirror in vertical: $2800 mirrors $2000
        let _ = bus.read(0x2002);
        bus.write(0x2006, 0x28);
        bus.write(0x2006, 0x00);
        let _ = bus.read(0x2007);
        let v = bus.read(0x2007);
        assert_eq!(v, 0xA1);
    }

    #[test]
    fn mmc1_dynamic_horizontal_mirroring() {
        // Control bits 0b00011 => horizontal
        let rom = build_mmc1_ines(2, 1, 0x10, 0x00, 1);
        let cart = Cartridge::from_ines_bytes(&rom).expect("parse");
        let mut bus = Bus::new();
        bus.attach_cartridge(cart);

        mmc1_serial_write(&mut bus, 0b00011);

        // Write to $2000
        bus.write(0x2006, 0x20);
        bus.write(0x2006, 0x00);
        bus.write(0x2007, 0xB2);

        // Horizontal: $2400 mirrors $2000
        let _ = bus.read(0x2002);
        bus.write(0x2006, 0x24);
        bus.write(0x2006, 0x00);
        let _ = bus.read(0x2007);
        let v = bus.read(0x2007);
        assert_eq!(v, 0xB2);
    }

    #[test]
    fn mmc1_dynamic_single_screen_lower() {
        // Control bits 0b00000 => single-screen lower ($2000 region everywhere)
        let rom = build_mmc1_ines(1, 1, 0x10, 0x00, 1);
        let cart = Cartridge::from_ines_bytes(&rom).expect("parse");
        let mut bus = Bus::new();
        bus.attach_cartridge(cart);

        mmc1_serial_write(&mut bus, 0b00000);

        // Write to $2000
        bus.write(0x2006, 0x20);
        bus.write(0x2006, 0x10);
        bus.write(0x2007, 0xC3);

        // Read from $2400 (should mirror single-screen lower)
        let _ = bus.read(0x2002);
        bus.write(0x2006, 0x24);
        bus.write(0x2006, 0x10);
        let _ = bus.read(0x2007);
        let v = bus.read(0x2007);
        assert_eq!(v, 0xC3);
        // Also $2C00 should mirror
        let _ = bus.read(0x2002);
        bus.write(0x2006, 0x2C);
        bus.write(0x2006, 0x10);
        let _ = bus.read(0x2007);
        let v2 = bus.read(0x2007);
        assert_eq!(v2, 0xC3);
    }

    #[test]
    fn mmc1_dynamic_single_screen_upper() {
        // Control bits 0b00001 => single-screen upper ($2400 region everywhere)
        let rom = build_mmc1_ines(1, 1, 0x10, 0x00, 1);
        let cart = Cartridge::from_ines_bytes(&rom).expect("parse");
        let mut bus = Bus::new();
        bus.attach_cartridge(cart);

        mmc1_serial_write(&mut bus, 0b00001);

        // Write to $2400
        bus.write(0x2006, 0x24);
        bus.write(0x2006, 0x10);
        bus.write(0x2007, 0xD4);

        // Read from $2000 should mirror $2400 in single-screen upper
        let _ = bus.read(0x2002);
        bus.write(0x2006, 0x20);
        bus.write(0x2006, 0x10);
        let _ = bus.read(0x2007);
        let v = bus.read(0x2007);
        assert_eq!(v, 0xD4);
        // $2C00 also mirrors
        let _ = bus.read(0x2002);
        bus.write(0x2006, 0x2C);
        bus.write(0x2006, 0x10);
        let _ = bus.read(0x2007);
        let v2 = bus.read(0x2007);
        assert_eq!(v2, 0xD4);
    }
    #[test]
    fn mmc3_dynamic_mirroring_vertical_then_horizontal() {
        // Mapper 4 (MMC3). Set mapper id low nibble = 4 (flags6 upper nibble).
        // flags6: 0x40 => mapper low nibble=4, horizontal header mirroring (bit0=0)
        let flags6 = 0x40;
        let flags7 = 0x00;
        let rom = build_ines(2, 1, flags6, flags7, 1, None);
        let cart = Cartridge::from_ines_bytes(&rom).expect("parse");
        let mut bus = Bus::new();
        bus.attach_cartridge(cart);

        // First force Vertical mirroring via $A000 write (bit0=0)
        bus.write(0xA000, 0x00);

        // Write a byte to $2000
        bus.write(0x2006, 0x20);
        bus.write(0x2006, 0x05);
        bus.write(0x2007, 0x9A);

        // In vertical mode $2800 mirrors $2000
        let _ = bus.read(0x2002);
        bus.write(0x2006, 0x28);
        bus.write(0x2006, 0x05);
        let _ = bus.read(0x2007);
        let v_vert = bus.read(0x2007);
        assert_eq!(
            v_vert, 0x9A,
            "Vertical mirroring: $2800 should mirror $2000"
        );

        // Now switch to Horizontal mirroring (bit0=1)
        bus.write(0xA000, 0x01);

        // Overwrite $2000 with a different value
        bus.write(0x2006, 0x20);
        bus.write(0x2006, 0x05);
        bus.write(0x2007, 0x6E);

        // In horizontal mode $2400 mirrors $2000
        let _ = bus.read(0x2002);
        bus.write(0x2006, 0x24);
        bus.write(0x2006, 0x05);
        let _ = bus.read(0x2007);
        let v_horiz = bus.read(0x2007);
        assert_eq!(
            v_horiz, 0x6E,
            "Horizontal mirroring: $2400 should mirror $2000"
        );
    }

    #[test]
    fn mmc3_dynamic_mirroring_switch_back() {
        // Start with vertical, switch to horizontal, then back to vertical again.
        let flags6 = 0x40; // mapper id low nibble = 4
        let flags7 = 0x00;
        let rom = build_ines(2, 1, flags6, flags7, 1, None);
        let cart = Cartridge::from_ines_bytes(&rom).expect("parse");
        let mut bus = Bus::new();
        bus.attach_cartridge(cart);

        // Vertical
        bus.write(0xA000, 0x00);
        bus.write(0x2006, 0x20);
        bus.write(0x2006, 0x33);
        bus.write(0x2007, 0x11);

        // Confirm vertical ($2800 mirror)
        let _ = bus.read(0x2002);
        bus.write(0x2006, 0x28);
        bus.write(0x2006, 0x33);
        let _ = bus.read(0x2007);
        assert_eq!(bus.read(0x2007), 0x11);

        // Horizontal
        bus.write(0xA000, 0x01);
        bus.write(0x2006, 0x20);
        bus.write(0x2006, 0x44);
        bus.write(0x2007, 0x22);

        // Confirm horizontal ($2400 mirror)
        let _ = bus.read(0x2002);
        bus.write(0x2006, 0x24);
        bus.write(0x2006, 0x44);
        let _ = bus.read(0x2007);
        assert_eq!(bus.read(0x2007), 0x22);

        // Back to Vertical
        bus.write(0xA000, 0x00);
        bus.write(0x2006, 0x20);
        bus.write(0x2006, 0x55);
        bus.write(0x2007, 0x33);

        let _ = bus.read(0x2002);
        bus.write(0x2006, 0x28);
        bus.write(0x2006, 0x55);
        let _ = bus.read(0x2007);
        assert_eq!(bus.read(0x2007), 0x33);
    }
}
