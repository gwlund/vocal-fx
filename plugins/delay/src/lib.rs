// Vocal FX Delay Plugin
//
// A simple delay effect using a ring buffer (circular buffer).
// Audio is written into a buffer and read back after a configurable time.
// Feedback feeds the delayed signal back into the buffer for repeating echoes.
// Mix blends between dry (original) and wet (delayed) signal.

use nih_plug::prelude::*;
use std::sync::Arc;

/// Maximum delay time in seconds (determines buffer size)
const MAX_DELAY_SEC: f32 = 2.0;

struct Delay {
    params: Arc<DelayParams>,
    /// Ring buffer for each channel
    buffers: Vec<Vec<f32>>,
    /// Current write position in the ring buffer
    write_pos: usize,
    sample_rate: f32,
}

#[derive(Params)]
struct DelayParams {
    #[id = "time"]
    pub time: FloatParam,

    #[id = "feedback"]
    pub feedback: FloatParam,

    #[id = "mix"]
    pub mix: FloatParam,
}

impl Default for Delay {
    fn default() -> Self {
        Self {
            params: Arc::new(DelayParams::default()),
            buffers: Vec::new(),
            write_pos: 0,
            sample_rate: 44100.0,
        }
    }
}

impl Default for DelayParams {
    fn default() -> Self {
        Self {
            time: FloatParam::new(
                "Time",
                250.0,
                FloatRange::Skewed {
                    min: 1.0,
                    max: 1000.0,
                    factor: FloatRange::skew_factor(-1.0),
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            feedback: FloatParam::new(
                "Feedback",
                util::db_to_gain(-6.0),
                FloatRange::Skewed {
                    min: 0.0,
                    max: util::db_to_gain(-1.0),
                    factor: FloatRange::gain_skew_factor(-60.0, -1.0),
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_gain_to_db(1))
            .with_string_to_value(formatters::s2v_f32_gain_to_db()),

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
        }
    }
}

impl Plugin for Delay {
    const NAME: &'static str = "Vocal FX Delay";
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
        self.sample_rate = buffer_config.sample_rate;
        let buffer_size = (MAX_DELAY_SEC * self.sample_rate) as usize + 1;
        let num_channels = _audio_io_layout
            .main_output_channels
            .map(|c| c.get() as usize)
            .unwrap_or(2);

        self.buffers = vec![vec![0.0; buffer_size]; num_channels];
        self.write_pos = 0;
        true
    }

    fn reset(&mut self) {
        for buf in &mut self.buffers {
            buf.fill(0.0);
        }
        self.write_pos = 0;
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let delay_samples =
            (self.params.time.value() * 0.001 * self.sample_rate) as usize;
        let feedback = self.params.feedback.value();
        let mix = self.params.mix.value();

        if self.buffers.is_empty() {
            return ProcessStatus::Normal;
        }
        let buf_len = self.buffers[0].len();

        for mut channel_samples in buffer.iter_samples() {
            for (ch, sample) in channel_samples.iter_mut().enumerate() {
                if ch >= self.buffers.len() {
                    continue;
                }

                let dry = *sample;

                // Read from the ring buffer at the delayed position
                let read_pos = (self.write_pos + buf_len - delay_samples) % buf_len;
                let delayed = self.buffers[ch][read_pos];

                // Write input + feedback into the ring buffer
                self.buffers[ch][self.write_pos] = dry + delayed * feedback;

                // Mix dry and wet
                *sample = dry * (1.0 - mix) + delayed * mix;
            }

            // Advance write position (shared across channels)
            self.write_pos = (self.write_pos + 1) % buf_len;
        }

        ProcessStatus::Normal
    }

    fn deactivate(&mut self) {}
}

impl ClapPlugin for Delay {
    const CLAP_ID: &'static str = "com.gwlund.vocal-fx.delay";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Delay with feedback for vocals");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Stereo,
        ClapFeature::Mono,
        ClapFeature::Delay,
    ];
}

impl Vst3Plugin for Delay {
    const VST3_CLASS_ID: [u8; 16] = *b"VocalFxDelyGwlnd";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Delay];
}

nih_export_clap!(Delay);
nih_export_vst3!(Delay);
