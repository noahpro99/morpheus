//! The call chimes and ringtones (`crates/ui/src/sounds.rs`), played by the
//! app itself through rodio on a thread of their own that opens the output
//! device on the first sound. Test runs (`MORPHEUS_CALL_AUDIO=tone`) make no
//! sound; every sound is noted in the shell (`.sound-log`) and, in a test
//! build, in `played-sounds.log` of the data directory.

use std::{
    path::PathBuf,
    sync::{OnceLock, mpsc},
};

use crate::view_model::calls::{SOUND_RATE, Sound};

/// Whether sounds come out of the speakers (not in test runs).
fn audible() -> bool {
    morpheus_call::AudioMode::from_env() == morpheus_call::AudioMode::Platform
}

fn player() -> Option<&'static mpsc::Sender<Vec<f32>>> {
    static PLAYER: OnceLock<Option<mpsc::Sender<Vec<f32>>>> = OnceLock::new();
    PLAYER
        .get_or_init(|| {
            let (tx, rx) = mpsc::channel::<Vec<f32>>();
            let spawned = std::thread::Builder::new()
                .name("morpheus-sounds".into())
                .spawn(move || {
                    let mut sink: Option<rodio::MixerDeviceSink> = None;
                    for samples in rx {
                        if sink.is_none() {
                            match rodio::DeviceSinkBuilder::open_default_sink() {
                                Ok(opened) => sink = Some(opened),
                                Err(err) => {
                                    eprintln!("morpheus-gpui: no sound output: {err}");
                                    continue;
                                }
                            }
                        }
                        let (Some(sink), Some(rate), Some(mono)) = (
                            sink.as_ref(),
                            std::num::NonZero::new(SOUND_RATE),
                            std::num::NonZero::new(1u16),
                        ) else {
                            continue;
                        };
                        sink.mixer()
                            .add(rodio::buffer::SamplesBuffer::new(mono, rate, samples));
                    }
                });
            spawned.ok().map(|_| tx)
        })
        .as_ref()
}

/// Play `sound` (test runs: only note it in the file, if there is one).
pub fn play(sound: Sound, log: Option<&PathBuf>) {
    if let Some(log) = log {
        let written = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
            .and_then(|mut file| {
                std::io::Write::write_all(&mut file, format!("{}\n", sound.key()).as_bytes())
            });
        if let Err(err) = written {
            eprintln!("morpheus-gpui: test sound log not written: {err}");
        }
    }
    if !audible() {
        return;
    }
    if let Some(player) = player() {
        let samples = sound
            .samples()
            .iter()
            .map(|s| f32::from(*s) / f32::from(i16::MAX))
            .collect();
        let _ = player.send(samples);
    }
}
