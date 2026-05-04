// Vocal FX Gate Plugin
//
// A noise gate that silences audio below a threshold.
// Opens when signal exceeds threshold, closes when it drops below.
// Attack/release control how fast the gate opens/closes.
// Hold keeps the gate open for a minimum time to avoid chattering.

use nih_plug::prelude::*;
use std::sync::Arc;
use vocal_fx_common::envelope::EnvelopeFollower;

struct Gate {
    params: Arc<GateParams>,
    /// One envelope follower per channel (mono or stereo)
    envelope: Vec<EnvelopeFollower>,
    /// Hold counter per channel — keeps gate open for minimum duration
    hold_counter: Vec<u32>,
    /// Current gate state per channel (0.0 = closed, 1.0 = open)
    gate_level: Vec<f32>,
}

#[derive(Params)]
struct GateParams {
    #[id = "threshold"]
    pub threshold: FloatParam,

    #[id = "attack"]
    pub attack: FloatParam,

    #[id = "release"]
    pub release: FloatParam,

    #[id = "hold"]
    pub hold: FloatParam,
}

impl Default for Gate {
    fn default() -> Self {
        Self {
            params: Arc::new(GateParams::default()),
            envelope: Vec::new(),
            hold_counter: Vec::new(),
            gate_level: Vec::new(),
        }
    }
}

impl Default for GateParams {
    fn default() -> Self {
        Self {
            threshold: FloatParam::new(
                "Threshold",
                util::db_to_gain(-40.0),
                FloatRange::Skewed {
                    min: util::db_to_gain(-80.0),
                    max: util::db_to_gain(0.0),
                    factor: FloatRange::gain_skew_factor(-80.0, 0.0),
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_gain_to_db(1))
            .with_string_to_value(formatters::s2v_f32_gain_to_db()),

            attack: FloatParam::new(
                "Attack",
                0.5,
                FloatRange::Skewed {
                    min: 0.1,
                    max: 50.0,
                    factor: FloatRange::skew_factor(-1.0),
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            release: FloatParam::new(
                "Release",
                50.0,
                FloatRange::Skewed {
                    min: 10.0,
                    max: 500.0,
                    factor: FloatRange::skew_factor(-1.0),
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            hold: FloatParam::new(
                "Hold",
                50.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 500.0,
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),
        }
    }
}

impl Plugin for Gate {
    const NAME: &'static str = "Vocal FX Gate";
    const VENDOR: &'static str = "Gil Lund";
    const URL: &'static str = "https://github.com/gwlund/vocal-fx";
    const EMAIL: &'static str = "gil.lund@nucleusnw.com";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            aux_input_ports: &[],
            aux_output_ports: &[],
            names: PortNames::const_default(),
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(1),
            ..AudioIOLayout::const_default()
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
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        let num_channels = _audio_io_layout
            .main_output_channels
            .map(|c| c.get() as usize)
            .unwrap_or(2);

        self.envelope = (0..num_channels)
            .map(|_| EnvelopeFollower::new(buffer_config.sample_rate))
            .collect();
        self.hold_counter = vec![0; num_channels];
        self.gate_level = vec![0.0; num_channels];
        true
    }

    fn reset(&mut self) {
        for env in &mut self.envelope {
            env.reset();
        }
        self.hold_counter.fill(0);
        self.gate_level.fill(0.0);
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let threshold = self.params.threshold.value();
        let attack_ms = self.params.attack.value();
        let release_ms = self.params.release.value();
        let hold_ms = self.params.hold.value();

        // Update envelope follower timing
        for env in &mut self.envelope {
            env.set_attack_ms(attack_ms);
            env.set_release_ms(release_ms);
        }

        // Hold time in samples
        let sample_rate = self.envelope.first().map(|_| 44100.0).unwrap_or(44100.0);
        let hold_samples = (hold_ms * 0.001 * sample_rate) as u32;

        for mut channel_samples in buffer.iter_samples() {
            for (ch, sample) in channel_samples.iter_mut().enumerate() {
                if ch >= self.envelope.len() {
                    continue;
                }

                let level = self.envelope[ch].process(*sample);

                // Gate logic: open if above threshold, close after hold expires
                if level > threshold {
                    self.gate_level[ch] = 1.0;
                    self.hold_counter[ch] = hold_samples;
                } else if self.hold_counter[ch] > 0 {
                    self.hold_counter[ch] -= 1;
                    // Gate stays open during hold
                } else {
                    // Smooth close to avoid clicks
                    self.gate_level[ch] *= 0.999;
                    if self.gate_level[ch] < 0.001 {
                        self.gate_level[ch] = 0.0;
                    }
                }

                *sample *= self.gate_level[ch];
            }
        }

        ProcessStatus::Normal
    }

    fn deactivate(&mut self) {}
}

impl ClapPlugin for Gate {
    const CLAP_ID: &'static str = "com.gwlund.vocal-fx.gate";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Noise gate for vocals");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Stereo,
        ClapFeature::Mono,
        ClapFeature::Utility,
    ];
}

impl Vst3Plugin for Gate {
    const VST3_CLASS_ID: [u8; 16] = *b"VocalFxGateGwlnd";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Dynamics];
}

nih_export_clap!(Gate);
nih_export_vst3!(Gate);
