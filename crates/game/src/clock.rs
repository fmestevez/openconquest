//! The original's game clock.
//!
//! Timings in the original count units of the game time `[game+0xde]`,
//! which advances once a tick. A tick's length is the game speed's row of
//! the table at 0x5bed7c (13, 17, 20, 33, 40, 40, 40 ms); a new game starts
//! at speed 5 (0x501710), 40 ms. Measured on the original under Wine: 470
//! units in 20.3 seconds.

/// Seconds a game tick lasts at the default speed.
pub const TICK: f32 = 0.04;

/// The same, in milliseconds.
pub const TICK_MS: u64 = 40;

/// Seconds `ticks` game ticks last.
pub fn ticks(ticks: u32) -> f32 {
    ticks as f32 * TICK
}
