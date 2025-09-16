/*!
Stub APU for NES register read/write and status behavior.

Scope:
- Implements a minimal APU interface for CPU bus integration.
- Handles register writes to $4000..$4017 (APU+I/O range) that are APU-relevant:
  * $4000..$4013: Channel registers (square1, square2, triangle, noise, DMC).
  * $4015: Status register (write: channel enables; read: channel status + IRQ flags).
  * $4017: Frame counter (mode + IRQ inhibit).
- $4014 (OAM DMA) is not handled here; it should be intercepted by the Bus/PPU.

Status semantics (stubbed):
- Reading $4015 returns:
    bit 0: Square 1 active (stubbed as "enabled" from last write to $4015)
    bit 1: Square 2 active (stubbed as "enabled" from last write to $4015)
    bit 2: Triangle active (stubbed as "enabled" from last write to $4015)
    bit 3: Noise active (stubbed as "enabled" from last write to $4015)
    bit 4: DMC active (stubbed as "enabled" from last write to $4015)
    bit 5: unused (0)
    bit 6: Frame interrupt flag (frame IRQ)
    bit 7: DMC interrupt flag
  Reading $4015 clears the frame interrupt flag (bit 6), as on real hardware.

Write semantics:
- $4015 write (APUSTATUS):
    - Updates "enabled" mask for channels (bits 0..4).
    - Stub does not model length counters; it uses the enable mask as status source.
- $4017 write (FRAME_COUNTER):
    - bit 7: mode (0 = 4-step, 1 = 5-step). Stub stores but does not generate clocks.
    - bit 6: IRQ inhibit (1 = inhibit). If set, clear frame IRQ flag immediately.
    - bits 0..5 ignored in this stub.

This APU does NOT generate audio or emulate timing. It only provides a stable interface
for the CPU bus and a place to evolve behavior later.
*/

#[derive(Clone, Debug)]
pub struct Apu {
    // Raw register mirror for $4000..=$4017 (24 bytes)
    // Note: $4014 (OAM DMA) belongs to PPU/Bus; we still keep a slot to avoid gaps.
    regs: [u8; 0x18],

    // Channel enabled mask from last write to $4015 (bits 0..4).
    enabled_mask: u8,

    // Frame counter control from $4017
    frame_counter_mode_5step: bool, // bit 7 of $4017
    frame_irq_inhibit: bool,        // bit 6 of $4017

    // Interrupt flags exposed via $4015 read
    frame_irq_flag: bool, // bit 6
    dmc_irq_flag: bool,   // bit 7

    // Timing and simple frame sequencer state
    cpu_cycle_counter: u32, // increments once per CPU cycle (driven by Bus)
    frame_cycle: u32,       // accumulates cycles within current frame sequence
}

impl Default for Apu {
    fn default() -> Self {
        Self::new()
    }
}

impl Apu {
    pub fn new() -> Self {
        Self {
            regs: [0; 0x18],
            enabled_mask: 0,
            frame_counter_mode_5step: false,
            frame_irq_inhibit: false,
            frame_irq_flag: false,
            dmc_irq_flag: false,
            cpu_cycle_counter: 0,
            frame_cycle: 0,
        }
    }

    pub fn reset(&mut self) {
        self.regs = [0; 0x18];
        self.enabled_mask = 0;
        self.frame_counter_mode_5step = false;
        self.frame_irq_inhibit = false;
        self.frame_irq_flag = false;
        self.dmc_irq_flag = false;
        self.cpu_cycle_counter = 0;
        self.frame_cycle = 0;
    }

    // Write to an APU register within $4000..=$4017.
    // The Bus should call this for APU-relevant addresses only (excluding $4014 DMA which is PPU/Bus).
    pub fn write_reg(&mut self, addr: u16, value: u8) {
        if !(0x4000..=0x4017).contains(&addr) {
            return;
        }
        let idx = (addr - 0x4000) as usize;
        self.regs[idx] = value;

        match addr {
            0x4015 => {
                // Channel enables. Stub uses this as "active" bits for $4015 reads.
                self.enabled_mask = value & 0b0001_1111; // DMC=bit4, Noise=3, Triangle=2, Square2=1, Square1=0
                // Real hardware would clear length counters for disabled channels here.
            }
            0x4017 => {
                // Frame counter control
                self.frame_counter_mode_5step = (value & 0x80) != 0;
                self.frame_irq_inhibit = (value & 0x40) != 0;

                // If IRQs are inhibited, clear the frame IRQ flag.
                if self.frame_irq_inhibit {
                    self.frame_irq_flag = false;
                }
                // Real hardware also immediately clocks sequencer depending on mode; omitted here.
            }
            _ => {
                // Other channel registers are stored but not actively simulated.
            }
        }
    }

    // Read an APU register. Only $4015 has meaningful read-back per this stub.
    // Other reads return the mirrored register value (or 0 if you prefer strictness).
    pub fn read_reg(&mut self, addr: u16) -> u8 {
        if !(0x4000..=0x4017).contains(&addr) {
            return 0;
        }
        match addr {
            0x4015 => self.read_status(),
            _ => {
                // Return register mirror for visibility in tests/integration.
                let idx = (addr - 0x4000) as usize;
                self.regs[idx]
            }
        }
    }

    // Status read from $4015. Reading clears frame IRQ flag (bit 6).
    pub fn read_status(&mut self) -> u8 {
        let mut status = 0u8;

        // Bits 0..4: channel status (stubbed as "enabled" mask).
        status |= self.enabled_mask & 0b0001_1111;

        // Bit 6: frame IRQ flag
        if self.frame_irq_flag {
            status |= 1 << 6;
        }

        // Bit 7: DMC IRQ flag
        if self.dmc_irq_flag {
            status |= 1 << 7;
        }

        // Reading $4015 clears the frame IRQ flag.
        self.frame_irq_flag = false;

        status
    }

    // External hooks for triggering/clearing IRQ flags.
    pub fn set_frame_interrupt(&mut self, active: bool) {
        if !self.frame_irq_inhibit {
            self.frame_irq_flag = active;
        } else if active {
            // If inhibited, ignore setting; ensure cleared.
            self.frame_irq_flag = false;
        }
    }

    pub fn set_dmc_interrupt(&mut self, active: bool) {
        self.dmc_irq_flag = active;
    }

    // Accessors for Bus/integration
    pub fn enabled_mask(&self) -> u8 {
        self.enabled_mask
    }

    pub fn frame_counter_mode_5step(&self) -> bool {
        self.frame_counter_mode_5step
    }

    pub fn frame_irq_inhibit(&self) -> bool {
        self.frame_irq_inhibit
    }

    pub fn frame_irq_flag(&self) -> bool {
        self.frame_irq_flag
    }

    pub fn dmc_irq_flag(&self) -> bool {
        self.dmc_irq_flag
    }

    // Returns whether the APU is asserting an IRQ line (frame IRQ if not inhibited, or DMC IRQ).
    // Bus should poll this once per CPU cycle and OR it into its irq_line aggregation.
    pub fn irq_asserted(&self) -> bool {
        (!self.frame_irq_inhibit && self.frame_irq_flag) || self.dmc_irq_flag
    }

    // Advance APU by a given number of CPU cycles.
    // Implements a simple frame sequencer that asserts the frame IRQ periodically in 4-step mode.
    // This is an approximation sufficient to integrate with Bus timing and IRQ flow.
    pub fn tick(&mut self, cpu_cycles: u32) {
        // Accumulate cycles
        self.cpu_cycle_counter = self.cpu_cycle_counter.wrapping_add(cpu_cycles);
        self.frame_cycle = self.frame_cycle.wrapping_add(cpu_cycles);

        // Very rough NTSC 4-step approximation: trigger frame IRQ about every 14916 CPU cycles.
        // In 5-step mode, suppress IRQs (as on hardware).
        const FOUR_STEP_PERIOD: u32 = 14916;

        if !self.frame_counter_mode_5step {
            if self.frame_cycle >= FOUR_STEP_PERIOD {
                self.frame_cycle -= FOUR_STEP_PERIOD;
                // Assert frame IRQ if not inhibited.
                if !self.frame_irq_inhibit {
                    self.frame_irq_flag = true;
                }
            }
        } else {
            // 5-step mode: no IRQ; keep frame cycle bounded to avoid overflow.
            if self.frame_cycle >= FOUR_STEP_PERIOD {
                self.frame_cycle -= FOUR_STEP_PERIOD;
            }
            // Ensure frame IRQ is not latched in 5-step mode
            self.frame_irq_flag = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_and_read_status() {
        let mut apu = Apu::new();

        // Enable square1 and DMC via $4015 write
        apu.write_reg(0x4015, 0b0001_0001);
        assert_eq!(apu.enabled_mask(), 0b0001_0001);

        // Set both IRQ flags
        apu.set_frame_interrupt(true);
        apu.set_dmc_interrupt(true);

        // Read $4015 -> expect enabled bits + both IRQ flags
        let s = apu.read_reg(0x4015);
        assert_eq!(s & 0b0001_1111, 0b0001_0001); // channels
        assert_ne!(s & (1 << 6), 0); // frame IRQ set
        assert_ne!(s & (1 << 7), 0); // DMC IRQ set

        // Reading clears frame IRQ
        let s2 = apu.read_reg(0x4015);
        assert_eq!(s2 & (1 << 6), 0);
        // DMC IRQ stays set until cleared explicitly
        assert_ne!(s2 & (1 << 7), 0);
    }

    #[test]
    fn frame_irq_inhibit_clears_flag() {
        let mut apu = Apu::new();

        apu.set_frame_interrupt(true);
        assert!(apu.frame_irq_flag());

        // Write $4017 with IRQ inhibit flag -> should clear frame IRQ
        apu.write_reg(0x4017, 0b0100_0000);
        assert!(apu.frame_irq_inhibit());
        assert!(!apu.frame_irq_flag());

        // Trying to set frame IRQ while inhibited should have no effect
        apu.set_frame_interrupt(true);
        assert!(!apu.frame_irq_flag());
    }
}
