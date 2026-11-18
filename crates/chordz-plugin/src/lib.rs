//! ChordzMac: a macOS VST3/CLAP chord generator modelled on CodeFN42's Chordz.
//!
//! The audio path is a plain pass-through (the plugin only transforms MIDI), but
//! it still exposes a stereo input/output pair because several hosts refuse to
//! load plugins with no audio buses at all.
//!
//! How it works: incoming note-ons are resolved against the selected scale and
//! chord template, expanded into up to 16 chord notes, delayed by the per-slot
//! start/end timing, scaled in velocity, and sent back out of the MIDI output.
//! Notes outside the configured octave zones are passed through (optionally
//! transposed and snapped to the scale).

use std::sync::Arc;

use chordz_core::engine::{Engine, OutputEvent, OutputKind};
use chordz_core::model::NUM_NOTE_SLOTS;
use nih_plug::prelude::*;

mod editor;
mod params;

use params::ChordzParams;

pub struct ChordzMac {
    params: Arc<ChordzParams>,

    engine: Engine,
    /// Absolute sample position, used to timestamp the MIDI engine.
    processed_samples: u64,
}

impl Default for ChordzMac {
    fn default() -> Self {
        Self {
            params: Arc::new(ChordzParams::new()),
            engine: Engine::new(48_000.0),
            processed_samples: 0,
        }
    }
}

impl ChordzMac {
    fn velocity_to_midi(velocity: f32) -> u8 {
        (velocity.clamp(0.0, 1.0) * 127.0).round().clamp(1.0, 127.0) as u8
    }
}

/// Forward one engine event to the host at a buffer-relative sample offset.
fn send_output(
    context: &mut impl ProcessContext<ChordzMac>,
    block_start: u64,
    block_len: u64,
    event: OutputEvent,
) {
    // Events due exactly at the end of the block are delivered on the last
    // sample instead of the first sample of the next block; some hosts reject
    // event offsets equal to the buffer length.
    let timing = event
        .time
        .saturating_sub(block_start)
        .min(block_len.saturating_sub(1)) as u32;
    match event.kind {
        OutputKind::NoteOn { note, velocity } => {
            context.send_event(NoteEvent::NoteOn {
                timing,
                voice_id: None,
                channel: event.channel,
                note,
                velocity: velocity as f32 / 127.0,
            });
        }
        OutputKind::NoteOff { note } => {
            context.send_event(NoteEvent::NoteOff {
                timing,
                voice_id: None,
                channel: event.channel,
                note,
                velocity: 0.0,
            });
        }
    }
}

impl Plugin for ChordzMac {
    const NAME: &'static str = "ChordzMac";
    const VENDOR: &'static str = "ChordzMac";
    const URL: &'static str = "https://example.invalid/chordzmac";
    const EMAIL: &'static str = "noreply@example.invalid";

    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: NonZeroU32::new(2),
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    const MIDI_INPUT: MidiConfig = MidiConfig::Basic;
    const MIDI_OUTPUT: MidiConfig = MidiConfig::Basic;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        editor::create(self.params.clone())
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        self.engine.set_sample_rate(buffer_config.sample_rate);
        true
    }

    fn reset(&mut self) {
        self.engine.reset();
        self.processed_samples = 0;
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let block_start = self.processed_samples;
        let block_len = buffer.samples() as u64;
        let block_end = block_start + block_len;

        // The state is persisted and edited from the GUI. A read lock is cheap
        // and uncontended in the common case, and the audio thread never writes.
        let state = self.params.chordz_state.read();
        let setups: [chordz_core::NoteSetup; NUM_NOTE_SLOTS] = self.params.setups();

        // Clicking a key in the editor's chord list previews that chord.
        if let Some((variation, index)) = self.params.take_preview() {
            self.engine
                .preview(&state, &setups, variation, index, 0, block_start);
        }
        if let Some(note) = self.params.take_note_preview() {
            self.engine.preview_note(note, 0, block_start);
        }

        while let Some(event) = context.next_event() {
            let time = block_start + event.timing() as u64;

            // Emit everything that is due before this event first so ordering
            // is preserved even when a delayed note-off lands in this block.
            self.engine.flush_until(time, |out| {
                send_output(context, block_start, block_len, out)
            });

            match event {
                NoteEvent::NoteOn {
                    channel,
                    note,
                    velocity,
                    ..
                } => {
                    if note < 128 {
                        self.engine.note_on(
                            &state,
                            &setups,
                            channel & 0x0F,
                            note,
                            Self::velocity_to_midi(velocity),
                            time,
                        );
                    }
                }
                NoteEvent::NoteOff { channel, note, .. }
                | NoteEvent::Choke { channel, note, .. } => {
                    if note < 128 {
                        self.engine
                            .note_off(&state, &setups, channel & 0x0F, note, time);
                    }
                }
                _ => (),
            }
        }

        // Flush the rest of the block, including any delays that extend past
        // the block boundary (they stay queued for the next call).
        self.engine.flush_until(block_end, |out| {
            send_output(context, block_start, block_len, out)
        });

        self.processed_samples = block_end;

        // The audio buffers are left untouched: Chordz is a MIDI processor.
        ProcessStatus::Normal
    }
}

impl ClapPlugin for ChordzMac {
    const CLAP_ID: &'static str = "com.chordzmac.chordz";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("MIDI chord generator with scale templates, easy mode and note timing");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[ClapFeature::NoteEffect, ClapFeature::Utility];
}

impl Vst3Plugin for ChordzMac {
    const VST3_CLASS_ID: [u8; 16] = *b"ChrdzMac00000001";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Tools];
}

nih_export_clap!(ChordzMac);
nih_export_vst3!(ChordzMac);

#[cfg(test)]
mod tests {
    use super::*;
    use nih_plug::prelude::{
        AuxiliaryBuffers, Buffer, NoteEvent, Params, PluginApi, ProcessContext, Transport,
    };

    /// A minimal host: it feeds a fixed event list and records everything the
    /// plugin sends back.
    struct MockContext {
        events: std::vec::IntoIter<NoteEvent<()>>,
        output: Vec<NoteEvent<()>>,
    }

    impl ProcessContext<ChordzMac> for MockContext {
        fn plugin_api(&self) -> PluginApi {
            PluginApi::Vst3
        }

        fn execute_background(&self, _task: ()) {}

        fn execute_gui(&self, _task: ()) {}

        fn transport(&self) -> &Transport {
            unimplemented!("ChordzMac does not read the transport")
        }

        fn next_event(&mut self) -> Option<NoteEvent<()>> {
            self.events.next()
        }

        fn send_event(&mut self, event: NoteEvent<()>) {
            self.output.push(event);
        }

        fn set_latency_samples(&self, _samples: u32) {}

        fn set_current_voice_capacity(&self, _capacity: u32) {}
    }

    fn run_with(plugin: &mut ChordzMac, events: Vec<NoteEvent<()>>) -> Vec<NoteEvent<()>> {
        let mut buffer = Buffer::default();
        let mut aux = AuxiliaryBuffers {
            inputs: &mut [],
            outputs: &mut [],
        };
        let mut context = MockContext {
            events: events.into_iter(),
            output: Vec::new(),
        };
        plugin.process(&mut buffer, &mut aux, &mut context);
        context.output
    }

    fn run(events: Vec<NoteEvent<()>>) -> Vec<NoteEvent<()>> {
        run_with(&mut ChordzMac::default(), events)
    }

    fn note_on(note: u8) -> NoteEvent<()> {
        NoteEvent::NoteOn {
            timing: 0,
            voice_id: None,
            channel: 0,
            note,
            velocity: 100.0 / 127.0,
        }
    }

    fn note_off(note: u8) -> NoteEvent<()> {
        NoteEvent::NoteOff {
            timing: 0,
            voice_id: None,
            channel: 0,
            note,
            velocity: 0.0,
        }
    }

    fn note_ons(events: &[NoteEvent<()>]) -> Vec<u8> {
        events
            .iter()
            .filter_map(|event| match event {
                NoteEvent::NoteOn { note, .. } => Some(*note),
                _ => None,
            })
            .collect()
    }

    fn note_offs(events: &[NoteEvent<()>]) -> Vec<u8> {
        events
            .iter()
            .filter_map(|event| match event {
                NoteEvent::NoteOff { note, .. } => Some(*note),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn exposes_the_thirty_original_parameters() {
        let plugin = ChordzMac::default();
        assert_eq!(plugin.params().param_map().len(), 30);
    }

    #[test]
    fn plays_the_scale_degree_chord() {
        // The default state is C major, so C4 produces a C major triad.
        let output = run(vec![note_on(60), note_off(60)]);
        assert_eq!(note_ons(&output), vec![60, 64, 67]);
        assert_eq!(note_offs(&output), vec![60, 64, 67]);
    }

    #[test]
    fn easy_mode_maps_white_keys_to_degrees() {
        let mut plugin = ChordzMac::default();
        plugin.params.chordz_state.write().easy_mode = true;

        // D4 is the second white key, i.e. the second degree of C major.
        let output = run_with(&mut plugin, vec![note_on(62), note_off(62)]);
        assert_eq!(note_ons(&output), vec![62, 65, 69]);
    }

    #[test]
    fn notes_outside_the_zone_pass_through() {
        let mut plugin = ChordzMac::default();
        {
            let mut state = plugin.params.chordz_state.write();
            state.octave_to = 5;
            state.transpose_above = 1;
            state.follow_trigger_notes = false;
        }

        // C7 is above the zone, so it is transposed by one octave and played
        // as a single note instead of being expanded into a chord.
        let output = run_with(&mut plugin, vec![note_on(96), note_off(96)]);
        assert_eq!(note_ons(&output), vec![108]);
        assert_eq!(note_offs(&output), vec![108]);
    }

    #[test]
    fn editor_preview_plays_the_first_trigger() {
        let mut plugin = ChordzMac::default();
        plugin.params.request_preview(false, 0);
        let output = run_with(&mut plugin, vec![]);
        assert_eq!(note_ons(&output), vec![60, 64, 67]);
    }

    #[test]
    fn chord_setup_survives_host_state_round_trip() {
        let params = ChordzParams::default();
        {
            let mut state = params.chordz_state.write();
            state.easy_mode = true;
            state.tonic = 7;
            state.transpose_octave = 2;
            state.apply_template_by_name("Minor");
        }

        let serialized = params.serialize_fields();
        assert!(serialized.contains_key("chordz-state"));

        let restored = ChordzParams::default();
        restored.deserialize_fields(&serialized);

        let expected = params.chordz_state.read().clone();
        let loaded = restored.chordz_state.read().clone();
        assert_eq!(loaded, expected);
        assert!(loaded.easy_mode);
        assert_eq!(loaded.tonic, 7);
        assert_eq!(loaded.transpose_octave, 2);
    }
}
