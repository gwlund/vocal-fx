// Pitch detector wrapper — runs McLeod pitch detection on a hop-based schedule.
// Buffers incoming audio in a ring buffer, runs detection every hop_size samples,
// and reports the current pitch frequency and whether the signal is voiced.

use pitch_detection::detector::mcleod::McLeodDetector;
use pitch_detection::detector::PitchDetector;
use vocal_fx_common::ring_buffer::RingBuffer;

/// Minimum vocal pitch we expect (Hz). Sets the analysis window size.
const MIN_PITCH_HZ: f32 = 60.0;

/// Power threshold for pitch detection — below this, signal is too quiet to analyze.
const POWER_THRESHOLD: f64 = 5.0;

/// Clarity threshold — below this, the signal is considered unvoiced.
const CLARITY_THRESHOLD: f64 = 0.7;

// McLeodDetector uses Rc internally (BufferPool) which is !Send.
// Audio plugins process on a single thread, so this is safe.
// We wrap it to satisfy nih-plug's Send requirement on Plugin.
unsafe impl Send for PitchDetectorWrapper {}

pub struct PitchDetectorWrapper {
    ring: RingBuffer,
    detector: McLeodDetector<f64>,
    /// Analysis window size in samples
    window_size: usize,
    /// Hop size in samples (how often we run detection)
    hop_size: usize,
    /// Counter until next detection run
    hop_counter: usize,
    /// Last detected pitch in Hz (0.0 if unvoiced)
    current_pitch_hz: f32,
    /// Whether the current segment is voiced
    is_voiced: bool,
    /// Sample rate from host
    sample_rate: f32,
    /// Temporary buffer for f64 conversion (pre-allocated)
    analysis_buf: Vec<f64>,
}

impl PitchDetectorWrapper {
    pub fn new(sample_rate: f32, window_ms: f32) -> Self {
        // Window must be at least 2 periods of the lowest expected pitch
        let min_window = (2.0 * sample_rate / MIN_PITCH_HZ) as usize;
        let requested_window = (window_ms * 0.001 * sample_rate) as usize;
        let window_size = requested_window.max(min_window).next_power_of_two();
        let padding = window_size / 2;

        // Hop = ~10ms
        let hop_size = (0.01 * sample_rate) as usize;

        // Ring buffer needs to hold at least one full analysis window
        let ring_capacity = window_size * 2;

        Self {
            ring: RingBuffer::new(ring_capacity),
            detector: McLeodDetector::new(window_size, padding),
            window_size,
            hop_size,
            hop_counter: 0,
            current_pitch_hz: 0.0,
            is_voiced: false,
            sample_rate,
            analysis_buf: vec![0.0f64; window_size],
        }
    }

    /// Feed one sample into the detector. Call this for every input sample.
    /// Detection runs automatically every hop_size samples.
    pub fn process(&mut self, sample: f32) {
        self.ring.push(sample);
        self.hop_counter += 1;

        if self.hop_counter >= self.hop_size {
            self.hop_counter = 0;
            self.run_detection();
        }
    }

    /// Whether the signal is currently voiced (has a detectable pitch).
    pub fn is_voiced(&self) -> bool {
        self.is_voiced
    }

    /// Get the source wavelength in samples (sample_rate / pitch_hz).
    /// Returns None if unvoiced.
    pub fn wavelength_samples(&self) -> Option<f32> {
        if self.is_voiced && self.current_pitch_hz > 0.0 {
            Some(self.sample_rate / self.current_pitch_hz)
        } else {
            None
        }
    }

    /// Reset state (call when plugin is reset).
    pub fn reset(&mut self) {
        self.ring.clear();
        self.hop_counter = 0;
        self.current_pitch_hz = 0.0;
        self.is_voiced = false;
    }

    /// Returns the latency in samples (the analysis window size).
    pub fn latency_samples(&self) -> u32 {
        self.window_size as u32
    }

    fn run_detection(&mut self) {
        // Copy the most recent window_size samples from ring buffer into analysis_buf
        for i in 0..self.window_size {
            let delay = self.window_size - 1 - i;
            self.analysis_buf[i] = self.ring.read_behind(delay) as f64;
        }

        match self.detector.get_pitch(
            &self.analysis_buf,
            self.sample_rate as usize,
            POWER_THRESHOLD,
            CLARITY_THRESHOLD,
        ) {
            Some(pitch) => {
                self.current_pitch_hz = pitch.frequency as f32;
                self.is_voiced = true;
            }
            None => {
                self.current_pitch_hz = 0.0;
                self.is_voiced = false;
            }
        }
    }
}
