// Envelope follower — tracks the amplitude of an audio signal over time.
// Used by the gate (to decide when to open/close) and compressor (to calculate gain reduction).
//
// How it works: takes the absolute value of each sample, then smooths it with
// separate attack and release time constants. Attack = how fast the envelope rises
// when signal gets louder. Release = how fast it falls when signal gets quieter.

use std::f32::consts::PI;

pub struct EnvelopeFollower {
    /// Current envelope level (0.0 to ~1.0+)
    level: f32,
    /// Attack coefficient (0.0 to 1.0) — higher = faster attack
    attack_coeff: f32,
    /// Release coefficient (0.0 to 1.0) — higher = faster release
    release_coeff: f32,
    /// Sample rate, needed to recalculate coefficients
    sample_rate: f32,
}

impl EnvelopeFollower {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            level: 0.0,
            attack_coeff: time_to_coeff(10.0, sample_rate),
            release_coeff: time_to_coeff(100.0, sample_rate),
            sample_rate,
        }
    }

    /// Set attack time in milliseconds
    pub fn set_attack_ms(&mut self, ms: f32) {
        self.attack_coeff = time_to_coeff(ms, self.sample_rate);
    }

    /// Set release time in milliseconds
    pub fn set_release_ms(&mut self, ms: f32) {
        self.release_coeff = time_to_coeff(ms, self.sample_rate);
    }

    /// Update sample rate (call from Plugin::initialize)
    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }

    /// Process one sample, returns the current envelope level
    pub fn process(&mut self, sample: f32) -> f32 {
        let input_level = sample.abs();

        // Use attack coefficient when signal is rising, release when falling
        let coeff = if input_level > self.level {
            self.attack_coeff
        } else {
            self.release_coeff
        };

        // One-pole lowpass filter: level smoothly follows the input amplitude
        self.level += coeff * (input_level - self.level);
        self.level
    }

    /// Reset envelope to zero (call when plugin is reset)
    pub fn reset(&mut self) {
        self.level = 0.0;
    }
}

/// Convert milliseconds to a one-pole filter coefficient.
/// The coefficient determines how quickly the envelope responds.
/// Smaller ms = larger coefficient = faster response.
fn time_to_coeff(ms: f32, sample_rate: f32) -> f32 {
    if ms <= 0.0 {
        return 1.0; // Instant response
    }
    let samples = ms * 0.001 * sample_rate;
    // One-pole coefficient: reaches ~63% of target in the given time
    1.0 - (-2.0 * PI / samples).exp()
}
