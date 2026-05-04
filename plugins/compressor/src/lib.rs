// Vocal FX Compressor Plugin
//
// Reduces dynamic range by attenuating signals above the threshold.
// Ratio controls how much reduction (2:1 = half as loud above threshold).
// Soft knee provides a gradual transition around the threshold.
// Makeup gain compensates for the overall volume reduction.

use nih_plug::prelude::*;
use std::sync::Arc;
use vocal_fx_common::envelope::EnvelopeFollower;

struct Compressor {
    params: Arc<CompressorParams>,
    envelope: Vec<EnvelopeFollower>,
}

#[derive(Params)]
struct CompressorParams {
    #[id = "threshold"]
    pub threshold: FloatParam,

    #[id = "ratio"]
    pub ratio: FloatParam,

    #[id = "attack"]
    pub attack: FloatParam,

    #[id = "release"]
    pub release: FloatParam,

    #[id = "makeup"]
    pub makeup: FloatParam,

    #[id = "knee"]
    pub knee: FloatParam,
}

impl Default for Compressor {
    fn default() -> Self {
        Self {
            params: Arc::new(CompressorParams::default()),
            envelope: Vec::new(),
        }
    }
}

impl Default for CompressorParams {
    fn default() -> Self {
        Self {
            threshold: FloatParam::new(
                "Threshold",
                util::db_to_gain(-20.0),
                FloatRange::Skewed {
                    min: util::db_to_gain(-60.0),
                    max: util::db_to_gain(0.0),
                    factor: FloatRange::gain_skew_factor(-60.0, 0.0),
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_gain_to_db(1))
            .with_string_to_value(formatters::s2v_f32_gain_to_db()),

            ratio: FloatParam::new(
                "Ratio",
                4.0,
                FloatRange::Skewed {
                    min: 1.0,
                    max: 20.0,
                    factor: FloatRange::skew_factor(-1.0),
                },
            )
            .with_unit(":1")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            attack: FloatParam::new(
                "Attack",
                5.0,
                FloatRange::Skewed {
                    min: 0.1,
                    max: 100.0,
                    factor: FloatRange::skew_factor(-1.0),
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            release: FloatParam::new(
                "Release",
                100.0,
                FloatRange::Skewed {
                    min: 10.0,
                    max: 1000.0,
                    factor: FloatRange::skew_factor(-1.0),
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),

            makeup: FloatParam::new(
                "Makeup Gain",
                util::db_to_gain(0.0),
                FloatRange::Skewed {
                    min: util::db_to_gain(0.0),
                    max: util::db_to_gain(30.0),
                    factor: FloatRange::gain_skew_factor(0.0, 30.0),
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_gain_to_db(1))
            .with_string_to_value(formatters::s2v_f32_gain_to_db()),

            knee: FloatParam::new(
                "Knee",
                6.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 20.0,
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),
        }
    }
}

impl Plugin for Compressor {
    const NAME: &'static str = "Vocal FX Compressor";
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
        true
    }

    fn reset(&mut self) {
        for env in &mut self.envelope {
            env.reset();
        }
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let threshold_db = util::gain_to_db(self.params.threshold.value());
        let ratio = self.params.ratio.value();
        let attack_ms = self.params.attack.value();
        let release_ms = self.params.release.value();
        let makeup = self.params.makeup.value();
        let knee_db = self.params.knee.value();
        let half_knee = knee_db / 2.0;

        for env in &mut self.envelope {
            env.set_attack_ms(attack_ms);
            env.set_release_ms(release_ms);
        }

        for mut channel_samples in buffer.iter_samples() {
            for (ch, sample) in channel_samples.iter_mut().enumerate() {
                if ch >= self.envelope.len() {
                    continue;
                }

                let level = self.envelope[ch].process(*sample);
                let level_db = util::gain_to_db(level);

                // Calculate gain reduction with soft knee
                let gain_reduction_db = if level_db < threshold_db - half_knee {
                    // Below knee — no compression
                    0.0
                } else if level_db > threshold_db + half_knee {
                    // Above knee — full compression
                    (threshold_db - level_db) * (1.0 - 1.0 / ratio)
                } else {
                    // In the knee — gradual transition
                    let x = level_db - threshold_db + half_knee;
                    (1.0 / ratio - 1.0) * x * x / (2.0 * knee_db)
                };

                let gain = util::db_to_gain(gain_reduction_db) * makeup;
                *sample *= gain;
            }
        }

        ProcessStatus::Normal
    }

    fn deactivate(&mut self) {}
}

impl ClapPlugin for Compressor {
    const CLAP_ID: &'static str = "com.gwlund.vocal-fx.compressor";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Vocal compressor with soft knee");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Stereo,
        ClapFeature::Mono,
        ClapFeature::Compressor,
    ];
}

impl Vst3Plugin for Compressor {
    const VST3_CLASS_ID: [u8; 16] = *b"VocalFxCompGwlnd";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Dynamics];
}

nih_export_clap!(Compressor);
nih_export_vst3!(Compressor);
