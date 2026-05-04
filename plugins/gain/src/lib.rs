// Vocal FX Gain Plugin
//
// A simple gain plugin — the "hello world" of audio plugins.
// Multiplies every audio sample by a gain value controlled by a single knob.
//
// Rust concepts introduced:
// - `struct`: groups related data (like a C struct)
// - `impl`: adds methods to a struct (like C++ class methods)
// - `Arc`: reference-counted pointer for safe shared ownership
// - `trait`: defines an interface a struct must implement (like a C++ abstract class)
// - `derive`: auto-generates trait implementations (like C++ template magic)
// - `&mut self`: mutable reference to self (like C++ non-const method)

use nih_plug::prelude::*;
use std::sync::Arc;

/// The main plugin struct. Holds all state that persists across audio callbacks.
/// In nih-plug, this is the "brain" of your plugin.
struct Gain {
    params: Arc<GainParams>,
}

/// Parameters exposed to the host (Reaper, Carla, etc.).
/// The `#[derive(Params)]` macro generates the boilerplate that tells nih-plug
/// how to discover and serialize these parameters.
#[derive(Params)]
struct GainParams {
    /// The `#[id = "gain"]` attribute gives this parameter a stable identifier.
    /// The host uses this to save/restore your plugin's state.
    /// Once set, never change the id string — it would break saved presets.
    #[id = "gain"]
    pub gain: FloatParam,
}

/// `Default` trait — tells Rust how to create a new instance with default values.
/// Called when the host first loads the plugin.
impl Default for Gain {
    fn default() -> Self {
        Self {
            params: Arc::new(GainParams::default()),
        }
    }
}

impl Default for GainParams {
    fn default() -> Self {
        Self {
            gain: FloatParam::new(
                // Display name shown in the host
                "Gain",
                // Default value: 0 dB (gain factor of 1.0 = no change)
                util::db_to_gain(0.0),
                // Range: -60 dB to +12 dB with a skew so the knob feels natural
                // (more resolution near 0 dB where small changes matter most)
                FloatRange::Skewed {
                    min: util::db_to_gain(-60.0),
                    max: util::db_to_gain(12.0),
                    factor: FloatRange::gain_skew_factor(-60.0, 12.0),
                },
            )
            // Smoother prevents clicks when the knob moves — interpolates over 50ms
            .with_smoother(SmoothingStyle::Logarithmic(50.0))
            // Display " dB" after the value in the host UI
            .with_unit(" dB")
            // Convert internal gain factor to dB for display (2 decimal places)
            .with_value_to_string(formatters::v2s_f32_gain_to_db(2))
            // Parse dB string back to gain factor (for typed input)
            .with_string_to_value(formatters::s2v_f32_gain_to_db()),
        }
    }
}

/// The Plugin trait — this is the core interface every nih-plug plugin must implement.
/// It defines your plugin's identity, I/O layout, and audio processing.
impl Plugin for Gain {
    const NAME: &'static str = "Vocal FX Gain";
    const VENDOR: &'static str = "Gil Lund";
    const URL: &'static str = "https://github.com/gwlund/vocal-fx";
    const EMAIL: &'static str = "gil.lund@nucleusnw.com";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    // Accept both stereo and mono input/output configurations.
    // The host picks whichever matches the track's channel count.
    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        // Stereo
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            aux_input_ports: &[],
            aux_output_ports: &[],
            names: PortNames::const_default(),
        },
        // Mono (for your Scarlett single-mic input)
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

    /// Called for every audio buffer the host sends us.
    /// This is the real-time audio thread — no allocations, no locks, no I/O.
    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        // Iterate over each sample position across all channels.
        // `iter_samples()` yields one "column" at a time (one sample per channel).
        for channel_samples in buffer.iter_samples() {
            // Get the next smoothed gain value (prevents clicks on parameter changes)
            let gain = self.params.gain.smoothed.next();

            // Apply gain to every channel at this sample position
            for sample in channel_samples {
                *sample *= gain;
            }
        }

        ProcessStatus::Normal
    }

    fn deactivate(&mut self) {}
}

/// CLAP plugin metadata — used when loading as a .clap plugin
impl ClapPlugin for Gain {
    const CLAP_ID: &'static str = "com.gwlund.vocal-fx.gain";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Simple gain control");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Stereo,
        ClapFeature::Mono,
        ClapFeature::Utility,
    ];
}

/// VST3 plugin metadata — used when loading as a .vst3 plugin.
/// The class ID must be exactly 16 bytes and unique across all plugins.
impl Vst3Plugin for Gain {
    const VST3_CLASS_ID: [u8; 16] = *b"VocalFxGainGwlnd";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Tools];
}

// These macros generate the C-compatible entry points that hosts use to load the plugin.
nih_export_clap!(Gain);
nih_export_vst3!(Gain);
