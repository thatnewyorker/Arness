/*!
Controller abstraction for NES-style serial input via $4016/$4017.

Behavior:
- Buttons are represented as a bitmask in the order the CPU reads them:
  A, B, Select, Start, Up, Down, Left, Right  (bit 0 through bit 7)
- Writing to $4016 controls the strobe (bit 0 of the written value):
  * When strobe is set to 1, the controller continually latches the current button
    states and read() will always return the A button state (bit 0 of the latched
    data).
  * When strobe is cleared to 0, a read() returns one bit at a time from the
    latched snapshot, starting with A on the first read, then B, Select, Start,
    Up, Down, Left, Right. After 8 reads, subsequent reads return 1 (matching
    common emulator behavior).
- The latched state is captured on strobe transitions to 1 or when read() is
  called while strobe is high.

Notes:
- This module focuses on controller behavior only. Integration with the CPU Bus
  should forward writes to $4016 to `write_strobe` and reads from $4016/$4017 to
  `read()`.
*/

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Button {
    A,
    B,
    Select,
    Start,
    Up,
    Down,
    Left,
    Right,
}

impl Button {
    #[inline]
    fn mask(self) -> u8 {
        match self {
            Button::A => 1 << 0,
            Button::B => 1 << 1,
            Button::Select => 1 << 2,
            Button::Start => 1 << 3,
            Button::Up => 1 << 4,
            Button::Down => 1 << 5,
            Button::Left => 1 << 6,
            Button::Right => 1 << 7,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Controller {
    // Current live button states. Bit set = pressed.
    buttons: u8,

    // Latched button states captured during strobe or on-demand when strobe is high.
    latched: u8,

    // If true, reads always reflect A button (bit 0 of latched). If false, reads shift serially.
    strobe: bool,

    // Read index [0..=8). 0..7 -> bits A..Right. >=8 -> return 1 on further reads.
    index: u8,
}

impl Default for Controller {
    fn default() -> Self {
        Self::new()
    }
}

impl Controller {
    pub fn new() -> Self {
        Self {
            buttons: 0,
            latched: 0,
            strobe: false,
            index: 0,
        }
    }

    // Set or clear a button.
    pub fn set_button(&mut self, button: Button, pressed: bool) {
        if pressed {
            self.buttons |= button.mask();
        } else {
            self.buttons &= !button.mask();
        }
    }

    // Convenience helpers.
    pub fn press(&mut self, button: Button) {
        self.set_button(button, true);
    }

    pub fn release(&mut self, button: Button) {
        self.set_button(button, false);
    }

    // Replace the entire current button state with the provided mask.
    // Bit set = pressed. Bit layout matches Button::mask ordering.
    pub fn set_state_mask(&mut self, mask: u8) {
        self.buttons = mask;
    }

    // Called when the CPU writes to $4016. Only bit 0 is relevant (strobe).
    // - If strobe becomes 1, we immediately latch and reset the shift index.
    // - If strobe becomes 0, we don't automatically latch; the previously latched
    //   value will be shifted out on subsequent reads.
    pub fn write_strobe(&mut self, value: u8) {
        let new_strobe = (value & 1) != 0;
        self.strobe = new_strobe;
        if self.strobe {
            self.latch();
        }
    }

    // Serial read (emulates CPU read from $4016/$4017 for this controller).
    // Returns only bit 0 meaningful; higher bits are 0 for simplicity.
    // Commonly, after 8 reads, further reads return 1.
    pub fn read(&mut self) -> u8 {
        if self.strobe {
            // While strobe is high, re-latch on each read and always return A state.
            self.latch();
            (self.latched & 1) & 1
        } else {
            // Shifted serially across successive reads.
            let bit = if self.index < 8 {
                let b = (self.latched >> self.index) & 1;
                self.index = self.index.saturating_add(1);
                b
            } else {
                // After 8 reads, the hardware typically returns 1. We emulate that.
                1
            };
            bit & 1
        }
    }

    #[inline]
    fn latch(&mut self) {
        self.latched = self.buttons;
        self.index = 0;
    }

    // Optional helpers to inspect internal state (useful for debugging/integration).
    pub fn strobe(&self) -> bool {
        self.strobe
    }

    pub fn current_mask(&self) -> u8 {
        self.buttons
    }

    pub fn latched_mask(&self) -> u8 {
        self.latched
    }

    pub fn index(&self) -> u8 {
        self.index
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serial_shift_behaviour() {
        let mut c = Controller::new();

        // Press A, Start, Left -> bits 0, 3, 6
        c.set_state_mask((1 << 0) | (1 << 3) | (1 << 6));

        // Strobe high then low to prepare shift.
        c.write_strobe(1);
        c.write_strobe(0);

        // Expect A=1, B=0, Select=0, Start=1, Up=0, Down=0, Left=1, Right=0
        let expected = [1, 0, 0, 1, 0, 0, 1, 0];
        for &e in &expected {
            assert_eq!(c.read(), e);
        }

        // Further reads return 1
        assert_eq!(c.read(), 1);
        assert_eq!(c.read(), 1);
    }

    #[test]
    fn strobe_high_always_returns_a() {
        let mut c = Controller::new();
        c.set_state_mask(0);
        c.press(Button::A);

        c.write_strobe(1);
        for _ in 0..16 {
            assert_eq!(c.read(), 1); // A pressed
        }

        // Release A while strobe is high; read should reflect new A state after latch on read.
        c.release(Button::A);
        assert_eq!(c.read(), 0);
    }
}
