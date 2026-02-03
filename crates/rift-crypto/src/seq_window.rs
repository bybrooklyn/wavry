//! Sequence window for replay protection.
//!
//! This implements a sliding window to detect and reject replayed packets.
//! The window tracks the highest seen sequence number and maintains a
//! bitmap of recently seen packets within the window.
//!
//! # Design
//!
//! - Window size: 128 packets (configurable)
//! - Packets older than `highest - window_size` are rejected
//! - Packets already seen within the window are rejected
//! - New packets update the bitmap
//!
//! # Thread Safety
//!
//! This implementation is NOT thread-safe. Wrap in a Mutex if needed.

/// Sliding window for replay protection.
///
/// Tracks sequence numbers to detect and reject replayed packets.
#[derive(Debug, Clone)]
pub struct SequenceWindow {
    /// Highest sequence number seen
    highest: u64,
    /// Bitmap of received packets within window
    /// Bit 0 = highest, bit 1 = highest-1, etc.
    bitmap: u128,
    /// Window size (number of packets to track)
    window_size: u64,
}

impl Default for SequenceWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl SequenceWindow {
    /// Default window size: 128 packets
    pub const DEFAULT_WINDOW_SIZE: u64 = 128;

    /// Create a new sequence window with default size (128).
    pub fn new() -> Self {
        Self::with_size(Self::DEFAULT_WINDOW_SIZE)
    }

    /// Create a new sequence window with custom size.
    ///
    /// # Panics
    /// Panics if size is 0 or greater than 128.
    pub fn with_size(size: u64) -> Self {
        assert!(size > 0 && size <= 128, "window size must be 1-128");
        Self {
            highest: 0,
            bitmap: 0,
            window_size: size,
        }
    }

    /// Check if a sequence number is valid (not replayed).
    ///
    /// Does NOT update internal state. Use `check_and_update` for that.
    pub fn check(&self, seq: u64) -> bool {
        // First packet always valid
        if self.highest == 0 && self.bitmap == 0 {
            return true;
        }

        // Too old (before window)
        if seq + self.window_size <= self.highest {
            return false;
        }

        // Ahead of window (always valid)
        if seq > self.highest {
            return true;
        }

        // Within window - check bitmap
        let offset = self.highest - seq;
        let mask = 1u128 << offset;
        (self.bitmap & mask) == 0
    }

    /// Check and update: returns true if valid, false if replay.
    ///
    /// If valid, marks the sequence number as seen.
    pub fn check_and_update(&mut self, seq: u64) -> bool {
        // First packet
        if self.highest == 0 && self.bitmap == 0 {
            self.highest = seq;
            self.bitmap = 1; // Mark position 0 (current highest)
            return true;
        }

        // Too old (before window)
        if seq + self.window_size <= self.highest {
            return false;
        }

        // Ahead of window - advance
        if seq > self.highest {
            let shift = seq - self.highest;
            if shift >= 128 {
                // Too far ahead, reset window
                self.bitmap = 1;
            } else {
                self.bitmap <<= shift;
                self.bitmap |= 1; // Mark new highest
            }
            self.highest = seq;
            return true;
        }

        // Within window - check and update bitmap
        let offset = self.highest - seq;
        let mask = 1u128 << offset;

        if self.bitmap & mask != 0 {
            // Already seen
            return false;
        }

        // Mark as seen
        self.bitmap |= mask;
        true
    }

    /// Get the highest sequence number seen.
    pub fn highest(&self) -> u64 {
        self.highest
    }

    /// Reset the window to initial state.
    pub fn reset(&mut self) {
        self.highest = 0;
        self.bitmap = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sequential_packets() {
        let mut window = SequenceWindow::new();

        for i in 1..=100 {
            assert!(window.check_and_update(i), "packet {} should be valid", i);
        }
    }

    #[test]
    fn test_replay_detection() {
        let mut window = SequenceWindow::new();

        assert!(window.check_and_update(1));
        assert!(window.check_and_update(2));
        assert!(window.check_and_update(3));

        // Replay should fail
        assert!(!window.check_and_update(1));
        assert!(!window.check_and_update(2));
        assert!(!window.check_and_update(3));
    }

    #[test]
    fn test_out_of_order() {
        let mut window = SequenceWindow::new();

        assert!(window.check_and_update(5));
        assert!(window.check_and_update(3)); // Out of order but within window
        assert!(window.check_and_update(4));
        assert!(window.check_and_update(1)); // Still within window
        assert!(window.check_and_update(2));

        // All should now be seen
        assert!(!window.check_and_update(1));
        assert!(!window.check_and_update(2));
        assert!(!window.check_and_update(3));
        assert!(!window.check_and_update(4));
        assert!(!window.check_and_update(5));
    }

    #[test]
    fn test_old_packet_rejected() {
        let mut window = SequenceWindow::new();

        // Fill window past 128
        for i in 1..200 {
            assert!(window.check_and_update(i));
        }

        // Old packets should be rejected
        assert!(!window.check_and_update(1));
        assert!(!window.check_and_update(50));
    }

    #[test]
    fn test_window_slides() {
        let mut window = SequenceWindow::with_size(10);

        for i in 1..=10 {
            assert!(window.check_and_update(i));
        }

        // Packet 1 is still in window
        assert!(!window.check_and_update(1));

        // Advance window
        assert!(window.check_and_update(11));

        // Packet 1 is now outside window
        assert!(!window.check_and_update(1));
    }

    #[test]
    fn test_large_jump() {
        let mut window = SequenceWindow::new();

        assert!(window.check_and_update(1));
        assert!(window.check_and_update(1000)); // Big jump

        // Old packet way outside window
        assert!(!window.check_and_update(1));
    }

    #[test]
    fn test_check_without_update() {
        let mut window = SequenceWindow::new();

        assert!(window.check_and_update(1));
        assert!(window.check_and_update(2));

        // Check doesn't update
        assert!(window.check(3));
        assert!(window.check(3)); // Still valid because we didn't update

        // Now update
        assert!(window.check_and_update(3));
        assert!(!window.check(3)); // Now seen
    }
}
