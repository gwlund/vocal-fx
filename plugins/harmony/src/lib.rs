// Vocal FX Harmony Plugin
//
// Generates up to 4 pitch-shifted harmony voices from a single vocal input
// using TD-PSOLA (Time-Domain Pitch-Synchronous Overlap-Add).
// Each voice has configurable interval, fine tune, level, delay, and detune.
// Unvoiced segments (consonants, breaths) pass through unshifted to avoid artifacts.

use nih_plug::prelude::*;
use std::sync::Arc;

mod detector;
mod shifter;

use detector::PitchDetectorWrapper;
use shifter::VoiceShifter;

const NUM_VOICES: usize = 4;
const DEFAULT_WINDOW_MS: f32 = 30.0;
/// Maximum per-voice delay in seconds (50ms)
const MAX_VOICE_DELAY_SEC: f32 = 0.05;

// --- Enums for dropdown parameters ---

#[derive(Enum, Debug, PartialEq, Eq, Clone, Copy)]
enum VoiceEnable {
    #[id = "off"]
    Off,
    #[id = "on"]
    On,
}

#[derive(Enum, Debug, PartialEq, Eq, Clone, Copy)]
enum Interval {
    #[id = "oct-down"]
    #[name = "Octave Down (-12)"]
    OctaveDown,
    #[id = "5th-down"]
    #[name = "5th Down (-7)"]
    FifthDown,
    #[id = "4th-down"]
    #[name = "4th Down (-5)"]
    FourthDown,
    #[id = "maj3-down"]
    #[name = "Major 3rd Down (-4)"]
    MajorThirdDown,
    #[id = "min3-down"]
    #[name = "Minor 3rd Down (-3)"]
    MinorThirdDown,
    #[id = "2nd-down"]
    #[name = "2nd Down (-2)"]
    SecondDown,
    #[id = "unison"]
    #[name = "Unison (0)"]
    Unison,
    #[id = "2nd-up"]
    #[name = "2nd Up (+2)"]
    SecondUp,
    #[id = "min3-up"]
    #[name = "Minor 3rd Up (+3)"]
    MinorThirdUp,
    #[id = "maj3-up"]
    #[name = "Major 3rd Up (+4)"]
    MajorThirdUp,
    #[id = "4th-up"]
    #[name = "4th Up (+5)"]
    FourthUp,
    #[id = "5th-up"]
    #[name = "5th Up (+7)"]
    FifthUp,
    #[id = "oct-up"]
    #[name = "Octave Up (+12)"]
    OctaveUp,
    #[id = "oct-3rd-up"]
    #[name = "Oct+3rd Up (+16)"]
    OctaveThirdUp,
    #[id = "oct-5th-up"]
    #[name = "Oct+5th Up (+19)"]
    OctaveFifthUp,
    #[id = "2oct-up"]
    #[name = "2 Octaves Up (+24)"]
    TwoOctavesUp,
}

impl Interval {
    fn semitones(self) -> f32 {
        match self {
            Interval::OctaveDown => -12.0,
            Interval::FifthDown => -7.0,
            Interval::FourthDown => -5.0,
            Interval::MajorThirdDown => -4.0,
            Interval::MinorThirdDown => -3.0,
            Interval::SecondDown => -2.0,
            Interval::Unison => 0.0,
            Interval::SecondUp => 2.0,
            Interval::MinorThirdUp => 3.0,
            Interval::MajorThirdUp => 4.0,
            Interval::FourthUp => 5.0,
            Interval::FifthUp => 7.0,
            Interval::OctaveUp => 12.0,
            Interval::OctaveThirdUp => 16.0,
            Interval::OctaveFifthUp => 19.0,
            Interval::TwoOctavesUp => 24.0,
        }
    }
}

#[derive(Enum, Debug, PartialEq, Eq, Clone, Copy)]
enum HarmonyPreset {
    #[id = "off"]
    Off,
    #[id = "3rds-5ths"]
    #[name = "3rds + 5ths"]
    ThirdsAndFifths,
    #[id = "oct-double"]
    #[name = "Octave Double"]
    OctaveDouble,
    #[id = "full-chord"]
    #[name = "Full Chord"]
    FullChord,
    #[id = "thickener"]
    Thickener,
}

// --- Plugin struct ---

struct Harmony {
    params: Arc<HarmonyParams>,
    detector: Option<PitchDetectorWrapper>,
    voices: Vec<VoiceShifter>,
    /// Per-voice delay ring buffers (one per voice)
    voice_delay_bufs: Vec<Vec<f32>>,
    /// Write position for voice delay buffers
    voice_delay_write_pos: usize,
    sample_rate: f32,
    /// Crossfade state for voiced/unvoiced transitions
    crossfade_level: f32,
}

// --- Parameter structs ---

#[derive(Params)]
struct HarmonyParams {
    #[id = "preset"]
    pub preset: EnumParam<HarmonyPreset>,

    #[id = "window"]
    pub window_ms: FloatParam,

    #[id = "mix"]
    pub mix: FloatParam,

    #[nested(array, group = "Voice")]
    pub voices: [VoiceParams; NUM_VOICES],
}

#[derive(Params)]
struct VoiceParams {
    #[id = "enable"]
    pub enable: EnumParam<VoiceEnable>,

    #[id = "shift"]
    pub shift: EnumParam<Interval>,

    #[id = "fine"]
    pub fine: FloatParam,

    #[id = "level"]
    pub level: FloatParam,

    #[id = "delay"]
    pub delay: FloatParam,

    #[id = "detune"]
    pub detune: FloatParam,
}

// --- Default impls ---

impl Default for Harmony {
    fn default() -> Self {
        Self {
            params: Arc::new(HarmonyParams::default()),
            detector: None,
            voices: Vec::new(),
            voice_delay_bufs: Vec::new(),
            voice_delay_write_pos: 0,
            sample_rate: 44100.0,
            crossfade_level: 0.0,
        }
    }
}

impl Default for HarmonyParams {
    fn default() -> Self {
        Self {
            preset: EnumParam::new("Preset", HarmonyPreset::Off),

            window_ms: FloatParam::new(
                "Analysis Window",
                DEFAULT_WINDOW_MS,
                FloatRange::Linear {
                    min: 15.0,
                    max: 50.0,
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),

            mix: FloatParam::new(
                "Mix",
                0.5,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_unit("%")
            .with_value_to_string(Arc::new(|v| format!("{:.0}", v * 100.0)))
            .with_string_to_value(Arc::new(|s| {
                s.trim_end_matches('%').parse::<f32>().ok().map(|v| v / 100.0)
            })),

            voices: [
                VoiceParams::new("1", Interval::MajorThirdUp),
                VoiceParams::new("2", Interval::FifthUp),
                VoiceParams::new("3", Interval::Unison),
                VoiceParams::new("4", Interval::Unison),
            ],
        }
    }
}

impl VoiceParams {
    fn new(suffix: &str, default_interval: Interval) -> Self {
        Self {
            enable: EnumParam::new(
                format!("Voice {} Enable", suffix),
                VoiceEnable::Off,
            ),

            shift: EnumParam::new(
                format!("Voice {} Shift", suffix),
                default_interval,
            ),

            fine: FloatParam::new(
                format!("Voice {} Fine", suffix),
                0.0,
                FloatRange::Linear {
                    min: -100.0,
                    max: 100.0,
                },
            )
            .with_unit(" cents")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),

            level: FloatParam::new(
                format!("Voice {} Level", suffix),
                util::db_to_gain(-6.0),
                FloatRange::Skewed {
                    min: util::db_to_gain(-60.0),
                    max: util::db_to_gain(6.0),
                    factor: FloatRange::gain_skew_factor(-60.0, 6.0),
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_gain_to_db(1))
            .with_string_to_value(formatters::s2v_f32_gain_to_db()),

            delay: FloatParam::new(
                format!("Voice {} Delay", suffix),
                0.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 50.0,
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            detune: FloatParam::new(
                format!("Voice {} Detune", suffix),
                0.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 30.0,
                },
            )
            .with_unit(" cents")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),
        }
    }
}

// --- Plugin impl ---

impl Plugin for Harmony {
    const NAME: &'static str = "Vocal FX Harmony v2";
    const VENDOR: &'static str = "Gil Lund";
    const URL: &'static str = "https://github.com/gwlund/vocal-fx";
    const EMAIL: &'static str = "gil.lund@nucleusnw.com";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(1),
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            aux_input_ports: &[],
            aux_output_ports: &[],
            names: PortNames::const_default(),
        },
    ];

    const MIDI_INPUT: MidiConfig = MidiConfig::None;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        context: &mut impl InitContext<Self>,
    ) -> bool {
        self.sample_rate = buffer_config.sample_rate;

        let window_ms = self.params.window_ms.value();
        self.detector = Some(PitchDetectorWrapper::new(self.sample_rate, window_ms));

        self.voices = (0..NUM_VOICES).map(|_| VoiceShifter::new()).collect();

        // Per-voice delay buffers (50ms max each)
        let delay_buf_size = (MAX_VOICE_DELAY_SEC * self.sample_rate) as usize + 1;
        self.voice_delay_bufs = vec![vec![0.0; delay_buf_size]; NUM_VOICES];
        self.voice_delay_write_pos = 0;

        if let Some(ref det) = self.detector {
            context.set_latency_samples(det.latency_samples());
        }

        self.crossfade_level = 0.0;

        true
    }

    fn reset(&mut self) {
        if let Some(ref mut det) = self.detector {
            det.reset();
        }
        for voice in &mut self.voices {
            voice.reset();
        }
        for buf in &mut self.voice_delay_bufs {
            buf.fill(0.0);
        }
        self.voice_delay_write_pos = 0;
        self.crossfade_level = 0.0;
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let detector = match self.detector.as_mut() {
            Some(d) => d,
            None => return ProcessStatus::Normal,
        };

        let mix = self.params.mix.value();

        // Crossfade time constant (~5ms)
        let crossfade_coeff = 1.0 - (-1.0 / (0.005 * self.sample_rate)).exp();

        let delay_buf_len = if self.voice_delay_bufs.is_empty() {
            1
        } else {
            self.voice_delay_bufs[0].len()
        };

        for mut channel_samples in buffer.iter_samples() {
            let input = *channel_samples.get_mut(0).unwrap();

            // Feed pitch detector
            detector.process(input);
            let source_wavelength = detector.wavelength_samples();
            let is_voiced = detector.is_voiced();

            // Smooth crossfade between voiced (shifted) and unvoiced (passthrough)
            let target_cf = if is_voiced { 1.0 } else { 0.0 };
            self.crossfade_level += crossfade_coeff * (target_cf - self.crossfade_level);

            // Sum harmony voices
            let mut harmony_sum = 0.0f32;
            for (i, voice) in self.voices.iter_mut().enumerate() {
                if self.params.voices[i].enable.value() == VoiceEnable::Off {
                    continue;
                }

                let semitones = self.params.voices[i].shift.value().semitones();
                let fine = self.params.voices[i].fine.value();
                let detune = self.params.voices[i].detune.value();
                let level = self.params.voices[i].level.value();
                let delay_ms = self.params.voices[i].delay.value();

                // Total cents = fine tune + detune
                let total_cents = fine + detune;

                // Pitch shift via PSOLA
                let shifted = voice.process(input, source_wavelength, semitones, total_cents);
                let voice_out = shifted * self.crossfade_level
                    + input * (1.0 - self.crossfade_level);

                // Write to per-voice delay buffer
                if i < self.voice_delay_bufs.len() {
                    self.voice_delay_bufs[i][self.voice_delay_write_pos] = voice_out * level;

                    // Read from delay buffer
                    let delay_samples = (delay_ms * 0.001 * self.sample_rate) as usize;
                    let read_pos =
                        (self.voice_delay_write_pos + delay_buf_len - delay_samples) % delay_buf_len;
                    harmony_sum += self.voice_delay_bufs[i][read_pos];
                }
            }

            // Advance shared delay write position
            self.voice_delay_write_pos = (self.voice_delay_write_pos + 1) % delay_buf_len;

            // Mix: 0% = dry only, 100% = harmony only
            let output = input * (1.0 - mix) + harmony_sum * mix;
            for sample in channel_samples.iter_mut() {
                *sample = output;
            }
        }

        ProcessStatus::Normal
    }

    fn deactivate(&mut self) {}
}

impl ClapPlugin for Harmony {
    const CLAP_ID: &'static str = "com.gwlund.vocal-fx.harmony-v2";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("Pitch-shifting harmony generator for vocals");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Mono,
        ClapFeature::Stereo,
        ClapFeature::PitchShifter,
    ];
}

impl Vst3Plugin for Harmony {
    const VST3_CLASS_ID: [u8; 16] = *b"VoxFxHarm2Gwlund";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::PitchShift];
}

nih_export_clap!(Harmony);
nih_export_vst3!(Harmony);
