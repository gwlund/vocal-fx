// Per-voice PSOLA pitch shifter using tdpsola.
// Each voice has its own TdpsolaAnalysis + TdpsolaSynthesis pair.
// The source wavelength comes from the pitch detector; the target wavelength
// is computed from the semitone/cent shift parameters.

use tdpsola::{AlternatingHann, TdpsolaAnalysis, TdpsolaSynthesis, Speed};

/// Default wavelength in samples (corresponds to ~200 Hz at 44100 Hz).
/// Used as initial value before pitch detection kicks in.
const DEFAULT_WAVELENGTH: f32 = 220.0;

pub struct VoiceShifter {
    analysis: TdpsolaAnalysis,
    synthesis: TdpsolaSynthesis,
    window: AlternatingHann,
    /// Current source wavelength (from pitch detector)
    source_wavelength: f32,
    /// Current target wavelength (shifted)
    target_wavelength: f32,
    /// Pre-padding length for initial samples
    padding_length: usize,
    /// Number of samples pushed (to track when past padding)
    samples_pushed: u64,
}

impl VoiceShifter {
    pub fn new() -> Self {
        let source_wavelength = DEFAULT_WAVELENGTH;
        let window = AlternatingHann::new(source_wavelength);
        let analysis = TdpsolaAnalysis::new(&window);
        let synthesis = TdpsolaSynthesis::new(Speed::from_f32(1.0), source_wavelength);
        let padding_length = source_wavelength as usize + 1;

        let mut shifter = Self {
            analysis,
            synthesis,
            window,
            source_wavelength,
            target_wavelength: source_wavelength,
            padding_length,
            samples_pushed: 0,
        };

        // Pre-pad with silence to avoid fade-in artifacts
        for _ in 0..padding_length {
            shifter.analysis.push_sample(0.0, &mut shifter.window);
            shifter.samples_pushed += 1;
        }

        shifter
    }

    /// Push one input sample and get one output sample.
    /// Returns the pitch-shifted sample.
    ///
    /// `source_wavelength`: current pitch period in samples from detector (None if unvoiced).
    /// `semitones`: pitch shift in semitones (e.g., +4 for major 3rd).
    /// `cents`: fine tune in cents (e.g., +10 for 10 cents sharp).
    pub fn process(
        &mut self,
        sample: f32,
        source_wavelength: Option<f32>,
        semitones: f32,
        cents: f32,
    ) -> f32 {
        // Update source wavelength if pitch detected
        if let Some(wl) = source_wavelength {
            if (wl - self.source_wavelength).abs() > 0.5 {
                self.source_wavelength = wl;
                self.window = AlternatingHann::new(wl);
            }
        }

        // Compute target wavelength from shift
        let shift_ratio = 2.0f32.powf(-(semitones + cents / 100.0) / 12.0);
        let new_target = self.source_wavelength * shift_ratio;
        if (new_target - self.target_wavelength).abs() > 0.5 {
            self.target_wavelength = new_target;
            self.synthesis.set_wavelength(new_target);
        }

        // Feed sample into analysis
        self.analysis.push_sample(sample, &mut self.window);
        self.samples_pushed += 1;

        // Get output sample from synthesis
        match self.synthesis.try_get_sample(&self.analysis) {
            Ok(output) => {
                self.synthesis.step(&self.analysis);
                output
            }
            Err(_) => {
                // Not enough samples yet — pass through dry
                sample
            }
        }
    }

    /// Reset the shifter to initial state.
    pub fn reset(&mut self) {
        let source_wavelength = DEFAULT_WAVELENGTH;
        self.window = AlternatingHann::new(source_wavelength);
        self.analysis = TdpsolaAnalysis::new(&self.window);
        self.synthesis = TdpsolaSynthesis::new(Speed::from_f32(1.0), source_wavelength);
        self.source_wavelength = source_wavelength;
        self.target_wavelength = source_wavelength;
        self.padding_length = source_wavelength as usize + 1;
        self.samples_pushed = 0;

        // Pre-pad again
        for _ in 0..self.padding_length {
            self.analysis.push_sample(0.0, &mut self.window);
            self.samples_pushed += 1;
        }
    }
}
