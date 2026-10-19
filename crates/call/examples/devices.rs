#![allow(clippy::expect_used)]
//! Lists the microphones and speakers WebRTC finds: `cargo run -p morpheus-call --example devices`.
fn main() {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    rt.block_on(async {
        match livekit::prelude::PlatformAudio::new() {
            Ok(audio) => {
                for d in audio.recording_devices() {
                    println!("mic     [{}] {}", d.index, d.name);
                }
                for d in audio.playout_devices() {
                    println!("speaker [{}] {}", d.index, d.name);
                }
            }
            Err(err) => println!("no platform audio: {err}"),
        }
    });
}
