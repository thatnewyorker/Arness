/*!
RAM module: encapsulates the 2 KiB CPU RAM with mirrored access.

CPU address map for internal RAM:
- $0000-$07FF: 2 KiB internal RAM
- $0800-$1FFF: Mirrors of $0000-$07FF (mask with & 0x07FF)

This module provides a small, hot-path-friendly API for reading and writing
bytes in the CPU RAM using the NES mirroring semantics. It is intended to be
owned by the Bus and accessed by the CPU-visible address dispatcher.
*/

/// Size of CPU internal RAM (in bytes).
pub const CPU_RAM_SIZE: usize = 0x0800;

/// CPU internal RAM with mirrored access helpers.
///
/// Addresses in the range $0000-$1FFF are mirrored every 2 KiB.
/// Users should call `read`/`write` with CPU addresses, and this type
/// will mask them down to the physical RAM range automatically.
pub struct Ram {
    data: [u8; CPU_RAM_SIZE],
}

impl Default for Ram {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl Ram {
    /// Create a new RAM instance initialized to 0.
    #[inline]
    pub fn new() -> Self {
        Self {
            data: [0; CPU_RAM_SIZE],
        }
    }

    /// Clear RAM contents to 0.
    #[inline]
    pub fn reset(&mut self) {
        self.data.fill(0);
    }

    /// Read a byte from CPU-visible RAM space ($0000-$1FFF), applying 2 KiB mirroring.
    #[inline]
    pub fn read(&self, addr: u16) -> u8 {
        let idx = Self::mirror_index(addr);
        // SAFETY: index is guaranteed within bounds by mirroring mask.
        unsafe { *self.data.get_unchecked(idx) }
    }

    /// Write a byte to CPU-visible RAM space ($0000-$1FFF), applying 2 KiB mirroring.
    #[inline]
    pub fn write(&mut self, addr: u16, value: u8) {
        let idx = Self::mirror_index(addr);
        // SAFETY: index is guaranteed within bounds by mirroring mask.
        unsafe {
            *self.data.get_unchecked_mut(idx) = value;
        }
    }

    /// Directly read a byte by physical index (0..CPU_RAM_SIZE).
    /// This does not perform address mirroring; intended for tests/tools.
    #[inline]
    pub fn get(&self, index: usize) -> u8 {
        self.data[index]
    }

    /// Directly write a byte by physical index (0..CPU_RAM_SIZE).
    /// This does not perform address mirroring; intended for tests/tools.
    #[inline]
    pub fn set(&mut self, index: usize, value: u8) {
        self.data[index] = value;
    }

    /// Expose the internal slice (read-only). Useful for diagnostics or hashing.
    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }

    /// Compute the physical RAM index for a CPU address using 2 KiB mirroring.
    #[inline]
    pub fn mirror_index(addr: u16) -> usize {
        (addr as usize) & (CPU_RAM_SIZE - 1) // mask with 0x07FF
    }
}

#[cfg(test)]
mod tests {
    use super::{CPU_RAM_SIZE, Ram};

    #[test]
    fn size_and_init() {
        let r = Ram::new();
        assert_eq!(r.as_slice().len(), CPU_RAM_SIZE);
        assert!(r.as_slice().iter().all(|&b| b == 0));
    }

    #[test]
    fn mirrored_reads_and_writes() {
        let mut r = Ram::new();

        // Write to $0001
        r.write(0x0001, 0xAA);

        // Read at mirrors: $0001, $0801, $1801 should all see the same byte.
        assert_eq!(r.read(0x0001), 0xAA);
        assert_eq!(r.read(0x0801), 0xAA);
        assert_eq!(r.read(0x1801), 0xAA);

        // Overwrite via a mirror address and verify all mirrors reflect it.
        r.write(0x1801, 0x55);
        assert_eq!(r.read(0x0001), 0x55);
        assert_eq!(r.read(0x0801), 0x55);
        assert_eq!(r.read(0x1801), 0x55);
    }

    #[test]
    fn direct_index_access() {
        let mut r = Ram::new();
        r.set(0x007F, 0xCC);
        assert_eq!(r.get(0x007F), 0xCC);

        // Mirrored address should see the same value
        assert_eq!(r.read(0x087F), 0xCC);
    }
}
