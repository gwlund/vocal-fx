// Vocal FX EQ Plugin
//
// 4-band parametric equalizer. Each band has its own filter type, frequency,
// gain, and bandwidth. Bands are processed in series (cascaded).

use nih_plug::prelude::*;
use std::sync::Arc;
use vocal_fx_common::biquad::{BiquadFilter, FilterType};

const NUM_BANDS: usize = 4;

struct Eq {
    params: Arc<EqParams>,
    /// One set of filters per channel, each channel has NUM_BANDS filters
    filters: Vec<[BiquadFilter; NUM_BANDS]>,
}

#[derive(Params)]
struct EqParams {
    #[nested(array, group = "Band")]
    pub bands: [BandParams; NUM_BANDS],
}

#[derive(Params)]
struct BandParams {
    #[id = "freq"]
    pub frequency: FloatParam,

    #[id = "gain"]
    pub gain: FloatParam,

    #[id = "bw"]
    pub bandwidth: FloatParam,

    #[id = "type"]
    pub filter_type: IntParam,
}

impl Default for Eq {
    fn default() -> Self {
        Self {
            params: Arc::new(EqParams::default()),
            filters: Vec::new(),
        }
    }
}

impl Default for EqParams {
    fn default() -> Self {
        // Default frequencies spread across the vocal range
        let default_freqs = [100.0, 500.0, 2000.0, 8000.0];
        Self {
            bands: std::array::from_fn(|i| BandParams::new(i, default_freqs[i])),
        }
    }
}

impl BandParams {
    fn new(index: usize, default_freq: f32) -> Self {
        let band_num = index + 1;
        Self {
            frequency: FloatParam::new(
                format!("Band {band_num} Freq"),
                default_freq,
                FloatRange::Skewed {
                    min: 20.0,
                    max: 20000.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" Hz")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),

            gain: FloatParam::new(
                format!("Band {band_num} Gain"),
                0.0,
                FloatRange::Linear {
                    min: -24.0,
                    max: 24.0,
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            bandwidth: FloatParam::new(
                format!("Band {band_num} BW"),
                1.0,
                FloatRange::Skewed {
                    min: 0.1,
                    max: 4.0,
                    factor: FloatRange::skew_factor(-1.0),
                },
            )
            .with_unit(" oct")
            .with_value_to_string(formatters::v2s_f32_rounded(2)),

            // 0=HighPass, 1=LowPass, 2=Bell, 3=HighShelf, 4=LowShelf
            filter_type: IntParam::new(
                format!("Band {band_num} Type"),
                2, // Default to Bell
                IntRange::Linear { min: 0, max: 4 },
            )
            .with_value_to_string(Arc::new(|v| {
                match v {
                    0 => "High Pass",
                    1 => "Low Pass",
                    2 => "Bell",
                    3 => "High Shelf",
                    4 => "Low Shelf",
                    _ => "Bell",
                }
                .to_string()
            })),
        }
    }
}

fn int_to_filter_type(v: i32) -> FilterType {
    match v {
        0 => FilterType::HighPass,
        1 => FilterType::LowPass,
        2 => FilterType::Bell,
        3 => FilterType::HighShelf,
        4 => FilterType::LowShelf,
        _ => FilterType::Bell,
    }
}

impl Plugin for Eq {
    const NAME: &'static str = "Vocal FX EQ";
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

        self.filters = (0..num_channels)
            .map(|_| std::array::from_fn(|_| BiquadFilter::new(buffer_config.sample_rate)))
            .collect();
        true
    }

    fn reset(&mut self) {
        for channel_filters in &mut self.filters {
            for filter in channel_filters {
                filter.reset();
            }
        }
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        // Update filter coefficients from current parameter values
        for channel_filters in &mut self.filters {
            for (band_idx, filter) in channel_filters.iter_mut().enumerate() {
                let band = &self.params.bands[band_idx];
                filter.set_params(
                    int_to_filter_type(band.filter_type.value()),
                    band.frequency.value(),
                    band.gain.value(),
                    band.bandwidth.value(),
                );
            }
        }

        // Process audio: each sample passes through all 4 bands in series
        for mut channel_samples in buffer.iter_samples() {
            for (ch, sample) in channel_samples.iter_mut().enumerate() {
                if ch >= self.filters.len() {
                    continue;
                }
                for filter in &mut self.filters[ch] {
                    *sample = filter.process(*sample);
                }
            }
        }

        ProcessStatus::Normal
    }

    fn deactivate(&mut self) {}
}

impl ClapPlugin for Eq {
    const CLAP_ID: &'static str = "com.gwlund.vocal-fx.eq";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("4-band parametric EQ for vocals");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Stereo,
        ClapFeature::Mono,
        ClapFeature::Equalizer,
    ];
}

impl Vst3Plugin for Eq {
    const VST3_CLASS_ID: [u8; 16] = *b"VocalFxEqByGwlnd";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Eq];
}

nih_export_clap!(Eq);
nih_export_vst3!(Eq);
