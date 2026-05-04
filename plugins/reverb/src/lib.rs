// Vocal FX Reverb Plugin
//
// A feedback delay network (FDN) reverb. Uses 4 delay lines with prime-number
// lengths fed back through a mixing matrix (Hadamard). Each delay line has a
// lowpass damping filter so high frequencies decay faster (like a real room).
//
// Architecture:
//   Input → Pre-delay → [4 delay lines with damping + cross-feedback] → Mix with dry

use nih_plug::prelude::*;
use std::sync::Arc;

/// Number of delay lines in the FDN
const NUM_DELAYS: usize = 4;

/// Prime-number delay lengths in samples at 44100 Hz.
/// Primes avoid resonant modes that color the reverb.
const BASE_DELAY_LENGTHS: [usize; NUM_DELAYS] = [1087, 1283, 1511, 1753];

/// Maximum pre-delay in samples (100ms at 48kHz)
const MAX_PREDELAY_SAMPLES: usize = 4800;

struct Reverb {
    params: Arc<ReverbParams>,
    /// FDN delay lines (one set per channel)
    fdn: Vec<FdnState>,
    /// Pre-delay ring buffers
    predelay_buffers: Vec<Vec<f32>>,
    predelay_write_pos: usize,
    sample_rate: f32,
}

/// State for one channel's FDN
struct FdnState {
    /// Delay line buffers
    delays: [Vec<f32>; NUM_DELAYS],
    /// Write positions for each delay line
    write_pos: [usize; NUM_DELAYS],
    /// One-pole lowpass state for damping (one per delay line)
    damp_state: [f32; NUM_DELAYS],
}

impl FdnState {
    fn new(sample_rate: f32) -> Self {
        let scale = sample_rate / 44100.0;
        Self {
            delays: std::array::from_fn(|i| {
                vec![0.0; (BASE_DELAY_LENGTHS[i] as f32 * scale) as usize + 1]
            }),
            write_pos: [0; NUM_DELAYS],
            damp_state: [0.0; NUM_DELAYS],
        }
    }

    fn reset(&mut self) {
        for delay in &mut self.delays {
            delay.fill(0.0);
        }
        self.write_pos = [0; NUM_DELAYS];
        self.damp_state = [0.0; NUM_DELAYS];
    }
}

#[derive(Params)]
struct ReverbParams {
    #[id = "decay"]
    pub decay: FloatParam,

    #[id = "damping"]
    pub damping: FloatParam,

    #[id = "predelay"]
    pub predelay: FloatParam,

    #[id = "size"]
    pub size: FloatParam,

    #[id = "mix"]
    pub mix: FloatParam,
}

impl Default for Reverb {
    fn default() -> Self {
        Self {
            params: Arc::new(ReverbParams::default()),
            fdn: Vec::new(),
            predelay_buffers: Vec::new(),
            predelay_write_pos: 0,
            sample_rate: 44100.0,
        }
    }
}

impl Default for ReverbParams {
    fn default() -> Self {
        Self {
            decay: FloatParam::new(
                "Decay",
                1.5,
                FloatRange::Skewed {
                    min: 0.1,
                    max: 10.0,
                    factor: FloatRange::skew_factor(-1.0),
                },
            )
            .with_unit(" s")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            damping: FloatParam::new(
                "Damping",
                0.5,
                FloatRange::Linear { min: 0.0, max: 1.0 },
            )
            .with_unit("%")
            .with_value_to_string(Arc::new(|v| format!("{:.0}", v * 100.0))),

            predelay: FloatParam::new(
                "Pre-delay",
                20.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 100.0,
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),

            size: FloatParam::new(
                "Size",
                0.7,
                FloatRange::Linear { min: 0.2, max: 1.5 },
            )
            .with_value_to_string(formatters::v2s_f32_rounded(2)),

            mix: FloatParam::new(
                "Mix",
                0.3,
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

/// 4x4 Hadamard matrix multiply (unnormalized).
/// Mixes the 4 delay line outputs so they cross-feed, creating dense reflections.
fn hadamard4(x: [f32; 4]) -> [f32; 4] {
    let scale = 0.5; // 1/sqrt(4) normalization
    [
        scale * (x[0] + x[1] + x[2] + x[3]),
        scale * (x[0] - x[1] + x[2] - x[3]),
        scale * (x[0] + x[1] - x[2] - x[3]),
        scale * (x[0] - x[1] - x[2] + x[3]),
    ]
}

impl Plugin for Reverb {
    const NAME: &'static str = "Vocal FX Reverb";
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
        let num_channels = _audio_io_layout
            .main_output_channels
            .map(|c| c.get() as usize)
            .unwrap_or(2);

        self.fdn = (0..num_channels)
            .map(|_| FdnState::new(self.sample_rate))
            .collect();

        let predelay_size = MAX_PREDELAY_SAMPLES + 1;
        self.predelay_buffers = vec![vec![0.0; predelay_size]; num_channels];
        self.predelay_write_pos = 0;
        true
    }

    fn reset(&mut self) {
        for fdn in &mut self.fdn {
            fdn.reset();
        }
        for buf in &mut self.predelay_buffers {
            buf.fill(0.0);
        }
        self.predelay_write_pos = 0;
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let decay = self.params.decay.value();
        let damping = self.params.damping.value();
        let predelay_ms = self.params.predelay.value();
        let size = self.params.size.value();
        let mix = self.params.mix.value();

        let predelay_samples =
            (predelay_ms * 0.001 * self.sample_rate) as usize;
        let predelay_len = self.predelay_buffers.first().map(|b| b.len()).unwrap_or(1);

        for mut channel_samples in buffer.iter_samples() {
            for (ch, sample) in channel_samples.iter_mut().enumerate() {
                if ch >= self.fdn.len() {
                    continue;
                }

                let dry = *sample;
                let fdn = &mut self.fdn[ch];

                // Pre-delay: write input, read delayed
                self.predelay_buffers[ch][self.predelay_write_pos % predelay_len] = dry;
                let predelay_read = (self.predelay_write_pos + predelay_len
                    - predelay_samples.min(predelay_len - 1))
                    % predelay_len;
                let predelayed = self.predelay_buffers[ch][predelay_read];

                // Read from each delay line
                let mut outputs = [0.0_f32; NUM_DELAYS];
                for i in 0..NUM_DELAYS {
                    let delay_len = fdn.delays[i].len();
                    let read_samples = ((delay_len as f32 - 1.0) * size) as usize;
                    let read_pos =
                        (fdn.write_pos[i] + delay_len - read_samples.min(delay_len - 1))
                            % delay_len;
                    outputs[i] = fdn.delays[i][read_pos];
                }

                // Mix through Hadamard matrix for cross-feedback
                let mixed = hadamard4(outputs);

                // Calculate feedback gain from decay time
                // Longer delay lines need less feedback to achieve the same decay time
                let avg_delay_samples = BASE_DELAY_LENGTHS.iter().sum::<usize>() as f32
                    / NUM_DELAYS as f32
                    * size
                    * (self.sample_rate / 44100.0);
                let feedback = if avg_delay_samples > 0.0 && decay > 0.0 {
                    // How much gain per pass to achieve the target decay time
                    (-3.0 * avg_delay_samples / (decay * self.sample_rate))
                        .exp()
                        .min(0.99)
                } else {
                    0.0
                };

                // Write back into delay lines with feedback and damping
                for i in 0..NUM_DELAYS {
                    // One-pole lowpass for damping (high frequencies decay faster)
                    fdn.damp_state[i] =
                        fdn.damp_state[i] + damping * (mixed[i] - fdn.damp_state[i]);
                    let damped = mixed[i] - damping * fdn.damp_state[i];

                    let delay_len = fdn.delays[i].len();
                    fdn.delays[i][fdn.write_pos[i]] = predelayed + damped * feedback;
                    fdn.write_pos[i] = (fdn.write_pos[i] + 1) % delay_len;
                }

                // Sum the 4 delay outputs for the wet signal
                let wet = outputs.iter().sum::<f32>() * 0.25;

                // Mix dry and wet
                *sample = dry * (1.0 - mix) + wet * mix;
            }

            self.predelay_write_pos = (self.predelay_write_pos + 1) % predelay_len;
        }

        ProcessStatus::Normal
    }

    fn deactivate(&mut self) {}
}

impl ClapPlugin for Reverb {
    const CLAP_ID: &'static str = "com.gwlund.vocal-fx.reverb";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("FDN reverb for vocals");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Stereo,
        ClapFeature::Mono,
        ClapFeature::Reverb,
    ];
}

impl Vst3Plugin for Reverb {
    const VST3_CLASS_ID: [u8; 16] = *b"VocalFxRevbGwlnd";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Reverb];
}

nih_export_clap!(Reverb);
nih_export_vst3!(Reverb);
