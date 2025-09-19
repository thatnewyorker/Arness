/*!
Clock/timing orchestrator for the Bus.

Purpose
- Centralize the order-of-operations for a single CPU cycle:
  * Increment CPU cycle counter
  * Step PPU three times with a read-only bus view
  * Step one DMA micro-cycle (via a callback provided by the Bus)
  * Latch NMI from PPU
  * Step APU and aggregate IRQ with mapper line

Integration
- The Bus should delegate its `tick(cycles)` implementation to this module by
  calling `clock::tick(self, cycles, |bus| { ...step DMA micro-cycle... })`.
- The DMA micro-step is provided as a callback because the DMA controller is an
  internal Bus field; keeping it in the Bus preserves encapsulation.

Behavioral compatibility
- The sequence matches the legacy Bus::tick: PPU ticks, DMA micro-step, NMI latch,
  APU tick, mapper IRQ, IRQ line aggregation.
*/

use crate::bus::Bus;
use crate::bus::interfaces::BusPpuView;
use crate::ppu::Ppu;

/// Orchestrate `cycles` CPU cycles worth of work across PPU/APU/DMA with exact ordering.
///
/// The `dma_step` callback is invoked at most once per CPU cycle and should perform
/// a single DMA micro-step (alignment/read/write) if DMA is active. The callback
/// receives `&mut Bus` so it can access internal DMA/PPU as needed.
///
/// This function preserves the legacy ordering semantics currently used by Bus::tick.
pub fn tick<F>(bus: &mut Bus, cycles: u32, mut dma_step: F)
where
    F: FnMut(&mut Bus),
{
    for _ in 0..cycles {
        // 1) Advance CPU cycle
        bus.cpu_cycle = bus.cpu_cycle.wrapping_add(1);

        // 2) Step PPU three times using a short-lived read-only view of the Bus.
        //    We move the PPU out briefly to avoid overlapping borrows while still
        //    allowing immutable access to the Bus for PPU memory reads.
        {
            let mut ppu = std::mem::replace(&mut bus.ppu, Ppu::new());
            for _ in 0..3 {
                let view = BusPpuView::new(&*bus);
                ppu.tick(&view);
                bus.ppu_cycle = bus.ppu_cycle.wrapping_add(1);
            }
            bus.ppu = ppu;
        }

        // 3) DMA micro-step (if active). The callback is responsible for exactly one
        //    micro-step per call and for preserving CPU stall semantics internally.
        if bus.dma_is_active() {
            dma_step(bus);
        }

        // 4) Latch NMI request from PPU (if any)
        if bus.ppu.take_nmi_request() {
            bus.nmi_pending = true;
        }

        // 5) Step APU once per CPU cycle and aggregate IRQ (APU OR mapper)
        bus.apu.tick(1);
        let mapper_irq = if let Some(cart) = &bus.cartridge {
            cart.mapper.borrow().irq_pending()
        } else {
            false
        };
        bus.irq_line = bus.apu.irq_asserted() || mapper_irq;
    }
}
