#![doc = r#"
bus_helpers: internal, non-hot-path helpers for the Bus façade

What belongs here
- Integration/debug helpers and longer convenience routines that would otherwise add noise to the Bus façade.
- Orchestration utilities that are not on a performance-critical inner path (e.g., frame-level helpers).
- Small glue helpers that keep the Bus façade thin by delegating to focused functions.

Visibility and access
- This module is internal to `crate::bus` (not publicly re-exported).
- Functions should prefer `pub(in crate::bus)` visibility and operate via `Bus`'s public or crate-scoped accessors.
- Do not access private fields directly; use accessors or add small crate-visible split-borrow helpers on `Bus` when needed.
- Test-only helpers should be annotated with `#[cfg(test)]`.

Performance guidance
- Avoid moving inner-loop or hot-path code here. If a moved helper is tiny and called infrequently, consider `#[inline]`.
- The public Bus API exposes thin delegators that call helpers here to preserve compatibility and discoverability.
"#]

use crate::bus::Bus;

/// Render a full PPU frame using the Bus' PPU address space and optional cartridge mapping.
///
/// This is an integration/helper function extracted from `Bus::render_ppu_frame` to reduce
/// noise in the Bus façade. The public Bus API continues to expose `render_ppu_frame` as a
/// thin delegator to this helper for compatibility.
///
/// Visibility is restricted to the `bus` module to avoid leaking internal structure.
#[inline]
pub(in crate::bus) fn render_ppu_frame(bus: &mut Bus) {
    // Legacy frame-level renderer retained for tests and debug flows.
    // The cycle-accurate pipeline should be driven by repeated calls to tick().
    let (ppu, ppu_mem, cart) = bus.split_ppu_mem_and_cart();
    let view = crate::bus::interfaces::BusPpuView::from_parts(ppu_mem, cart);
    ppu.render_frame(&view);
}

/// Perform one DMA micro-step (alignment/read/write) using the controller.
/// Helper used by the scheduler tick to keep the Bus façade focused on orchestration.
#[inline]
pub(in crate::bus) fn dma_step_one(
    dma: &mut crate::bus::dma::DmaController,
    mem_view: &mut crate::bus::dma::CpuMemoryView<'_>,
    ppu: &mut crate::ppu::Ppu,
) {
    dma.step_one_cycle(mem_view, ppu);
}

#[inline]
pub(in crate::bus) fn step_ppu_three(bus: &mut Bus) {
    for _ in 0..3 {
        {
            let (ppu, ppu_mem, cart) = bus.split_ppu_mem_and_cart();
            let view = crate::bus::interfaces::BusPpuView::from_parts(ppu_mem, cart);
            ppu.tick(&view);
        }
        bus.ppu_cycle = bus.ppu_cycle.wrapping_add(1);
    }
}
