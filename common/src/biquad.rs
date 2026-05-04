// Biquad filter — the workhorse of audio EQ.
// A second-order IIR filter that can be configured as high pass, low pass,
// bell (parametric), or shelf. Used by the EQ plugin and available to others.
//
// Math reference: Robert Bristow-Johnson's Audio EQ Cookbook
// https://www.w3.org/2011/audio/audio-eq-cookbook.html
//
// The filter processes samples using the "direct form 2 transposed" structure,
// which has better numerical stability than direct form 1.

use std::f32::consts::PI;

/// Filter type determines the shape of the frequency response
#[derive(Clone, Copy, PartialEq)]
pub enum FilterType {
    HighPass,
    LowPass,
    Bell,
    HighShelf,
    LowShelf,
}

/// Biquad filter coefficients
struct Coefficients {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

/// A single biquad filter stage
pub struct BiquadFilter {
    coeffs: Coefficients,
    /// State variables for direct form 2 transposed
    s1: f32,
    s2: f32,
    /// Current settings (stored so we can recalculate coefficients)
    filter_type: FilterType,
    frequency: f32,
    gain_db: f32,
    bandwidth_oct: f32,
    sample_rate: f32,
}

impl BiquadFilter {
    pub fn new(sample_rate: f32) -> Self {
        let mut filter = Self {
            coeffs: Coefficients {
                b0: 1.0,
                b1: 0.0,
                b2: 0.0,
                a1: 0.0,
                a2: 0.0,
            },
            s1: 0.0,
            s2: 0.0,
            filter_type: FilterType::Bell,
            frequency: 1000.0,
            gain_db: 0.0,
            bandwidth_oct: 1.0,
            sample_rate,
        };
        filter.calculate_coefficients();
        filter
    }

    /// Configure the filter. Call this when any parameter changes.
    pub fn set_params(
        &mut self,
        filter_type: FilterType,
        frequency: f32,
        gain_db: f32,
        bandwidth_oct: f32,
    ) {
        self.filter_type = filter_type;
        self.frequency = frequency.clamp(20.0, self.sample_rate * 0.49);
        self.gain_db = gain_db;
        self.bandwidth_oct = bandwidth_oct.max(0.05);
        self.calculate_coefficients();
    }

    pub fn set_sample_rate(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.calculate_coefficients();
    }

    /// Process one sample through the filter
    pub fn process(&mut self, input: f32) -> f32 {
        // Direct form 2 transposed
        let output = self.coeffs.b0 * input + self.s1;
        self.s1 = self.coeffs.b1 * input - self.coeffs.a1 * output + self.s2;
        self.s2 = self.coeffs.b2 * input - self.coeffs.a2 * output;
        output
    }

    /// Reset filter state (prevents clicks when switching presets)
    pub fn reset(&mut self) {
        self.s1 = 0.0;
        self.s2 = 0.0;
    }

    /// Recalculate coefficients from current parameters.
    /// Uses the Audio EQ Cookbook formulas.
    fn calculate_coefficients(&mut self) {
        let w0 = 2.0 * PI * self.frequency / self.sample_rate;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();

        // Q from bandwidth in octaves
        let alpha =
            sin_w0 * (2.0_f32.ln() / 2.0 * self.bandwidth_oct * w0 / sin_w0).sinh();

        let a = 10.0_f32.powf(self.gain_db / 40.0); // sqrt of linear gain

        let (b0, b1, b2, a0, a1, a2) = match self.filter_type {
            FilterType::HighPass => (
                (1.0 + cos_w0) / 2.0,
                -(1.0 + cos_w0),
                (1.0 + cos_w0) / 2.0,
                1.0 + alpha,
                -2.0 * cos_w0,
                1.0 - alpha,
            ),
            FilterType::LowPass => (
                (1.0 - cos_w0) / 2.0,
                1.0 - cos_w0,
                (1.0 - cos_w0) / 2.0,
                1.0 + alpha,
                -2.0 * cos_w0,
                1.0 - alpha,
            ),
            FilterType::Bell => (
                1.0 + alpha * a,
                -2.0 * cos_w0,
                1.0 - alpha * a,
                1.0 + alpha / a,
                -2.0 * cos_w0,
                1.0 - alpha / a,
            ),
            FilterType::HighShelf => {
                let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) + (a - 1.0) * cos_w0 + two_sqrt_a_alpha),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w0),
                    a * ((a + 1.0) + (a - 1.0) * cos_w0 - two_sqrt_a_alpha),
                    (a + 1.0) - (a - 1.0) * cos_w0 + two_sqrt_a_alpha,
                    2.0 * ((a - 1.0) - (a + 1.0) * cos_w0),
                    (a + 1.0) - (a - 1.0) * cos_w0 - two_sqrt_a_alpha,
                )
            }
            FilterType::LowShelf => {
                let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) - (a - 1.0) * cos_w0 + two_sqrt_a_alpha),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w0),
                    a * ((a + 1.0) - (a - 1.0) * cos_w0 - two_sqrt_a_alpha),
                    (a + 1.0) + (a - 1.0) * cos_w0 + two_sqrt_a_alpha,
                    -2.0 * ((a - 1.0) + (a + 1.0) * cos_w0),
                    (a + 1.0) + (a - 1.0) * cos_w0 - two_sqrt_a_alpha,
                )
            }
        };

        // Normalize by a0
        self.coeffs.b0 = b0 / a0;
        self.coeffs.b1 = b1 / a0;
        self.coeffs.b2 = b2 / a0;
        self.coeffs.a1 = a1 / a0;
        self.coeffs.a2 = a2 / a0;
    }
}
