//! Linux: what other apps play, through the PulseAudio protocol (PipeWire's
//! own PulseAudio server speaks it too). See `system_audio.rs`.

use std::{
    collections::HashMap,
    ffi::{CStr, CString},
    io::{BufReader, Read as _},
    os::unix::net::UnixStream,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use pulseaudio::protocol;
use tracing::{debug, info, warn};

use super::system_audio::{CHANNELS, FRAME, Mixer, Out, RATE};

/// Whose playback is captured.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Filter {
    /// Streams of this process are left out (this app's own sound).
    pub exclude_pid: u32,
    /// Only streams playing to this sink (tests: their own null sink).
    pub only_sink: Option<u32>,
    /// Whether Discord's streams are left out too (`None`: as Settings
    /// says, followed while capturing).
    pub leave_out_discord: Option<bool>,
}

impl Filter {
    /// Every app but this one, and Discord unless Settings lets it in.
    pub fn others() -> Self {
        Self {
            exclude_pid: std::process::id(),
            only_sink: None,
            leave_out_discord: None,
        }
    }

    fn discord_left_out(&self) -> bool {
        self.leave_out_discord
            .unwrap_or_else(crate::screen_audio_leaves_out_discord)
    }
}

/// The capture thread, stopped when dropped.
pub(crate) struct Running {
    /// The connection, shut down to end the thread's blocking read.
    sock: UnixStream,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.sock.shutdown(std::net::Shutdown::Both);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Start capturing: a record stream on each playback stream `filter` lets
/// through, now and as they come and go, mixed into `out`.
pub(crate) fn start(filter: Filter, out: Out) -> Result<Running, String> {
    let mut conn = Conn::open(c"Morpheus")?;
    let seq = conn.send(protocol::Command::Subscribe(
        protocol::SubscriptionMask::SINK_INPUT,
    ))?;
    conn.reply(seq)?;
    let sock = conn
        .sock
        .get_ref()
        .try_clone()
        .map_err(|err| err.to_string())?;
    let stop = Arc::new(AtomicBool::new(false));
    let running = stop.clone();
    let thread = std::thread::Builder::new()
        .name("morpheus-screen-audio".into())
        .spawn(move || {
            let mut capture = Capturing {
                conn,
                left_out_discord: filter.discord_left_out(),
                filter,
                out,
                inputs: HashMap::new(),
                channels: HashMap::new(),
                mixer: Mixer::new(),
                pending: HashMap::new(),
                listing: false,
                list_again: false,
            };
            capture.run(&running);
        })
        .map_err(|err| err.to_string())?;
    Ok(Running {
        sock,
        stop,
        thread: Some(thread),
    })
}

/// Samples in a 10 ms stereo frame.
const SAMPLES: usize = FRAME * CHANNELS;
/// How long a read waits before the setting is looked at again (it may have
/// changed while nothing else did).
const IDLE: Duration = Duration::from_millis(500);

/// What a reply answers.
enum Pending {
    List,
    Create(u32),
}

struct Capturing {
    conn: Conn,
    filter: Filter,
    /// Whether Discord was left out when the streams were last chosen.
    left_out_discord: bool,
    out: Out,
    /// Playback streams recorded: their record stream's channel once made.
    inputs: HashMap<u32, Option<u32>>,
    /// The playback stream each record stream (by channel) records.
    channels: HashMap<u32, u32>,
    mixer: Mixer,
    pending: HashMap<u32, Pending>,
    /// A list of the playback streams has been asked for.
    listing: bool,
    /// And something changed since it was asked for.
    list_again: bool,
}

impl Capturing {
    fn run(&mut self, stop: &AtomicBool) {
        info!("screen audio: capturing other apps' sound");
        if let Err(err) = self.ask_list() {
            warn!(%err, "screen audio: listing playback");
            return;
        }
        let mut looked = Instant::now();
        loop {
            let message = match self.conn.next(Some(IDLE)) {
                Ok(message) => message,
                Err(err) => {
                    if !stop.load(Ordering::Relaxed) {
                        warn!(%err, "screen audio: the sound server went away");
                    }
                    break;
                }
            };
            let handled = match message {
                Message::Data { channel, bytes } => {
                    self.take(channel, &bytes);
                    Ok(())
                }
                Message::Command { seq, command, rest } => self.command(seq, command, rest),
                Message::Idle => Ok(()),
            };
            // Now and then (and when quiet): choose again if the setting
            // changed.
            let handled = handled.and_then(|()| {
                if looked.elapsed() < IDLE {
                    return Ok(());
                }
                looked = Instant::now();
                if self.filter.discord_left_out() != self.left_out_discord {
                    self.ask_list()
                } else {
                    Ok(())
                }
            });
            if let Err(err) = handled {
                warn!(%err, "screen audio");
            }
            if stop.load(Ordering::Relaxed) {
                break;
            }
        }
        info!(frames = self.mixer.sent, "screen audio: capture stopped");
    }

    fn ask_list(&mut self) -> Result<(), String> {
        if self.listing {
            self.list_again = true;
            return Ok(());
        }
        let seq = self.conn.send(protocol::Command::GetSinkInputInfoList)?;
        self.pending.insert(seq, Pending::List);
        self.listing = true;
        Ok(())
    }

    fn command(
        &mut self,
        seq: u32,
        command: protocol::Command,
        mut rest: std::io::Cursor<Vec<u8>>,
    ) -> Result<(), String> {
        match command {
            protocol::Command::Reply => match self.pending.remove(&seq) {
                Some(Pending::List) => {
                    self.listing = false;
                    let list: protocol::SinkInputInfoList = read(&mut rest, self.conn.version)?;
                    self.reconcile(&list)?;
                    if std::mem::take(&mut self.list_again) {
                        self.ask_list()?;
                    }
                }
                Some(Pending::Create(input)) => {
                    let made: protocol::CreateRecordStreamReply =
                        read(&mut rest, self.conn.version)?;
                    match self.inputs.get_mut(&input) {
                        Some(channel) => {
                            debug!(
                                input,
                                channel = made.channel,
                                "screen audio: recording a stream"
                            );
                            *channel = Some(made.channel);
                            self.channels.insert(made.channel, input);
                            self.mixer.add(made.channel);
                        }
                        // It ended while the record stream was being made.
                        None => {
                            self.conn
                                .send(protocol::Command::DeleteRecordStream(made.channel))?;
                        }
                    }
                }
                None => {}
            },
            protocol::Command::Error(err) => match self.pending.remove(&seq) {
                Some(Pending::Create(input)) => {
                    debug!(input, ?err, "screen audio: a stream could not be recorded");
                    self.inputs.remove(&input);
                }
                Some(Pending::List) => {
                    self.listing = false;
                    return Err(format!("listing playback: {err:?}"));
                }
                None => {}
            },
            protocol::Command::SubscribeEvent(event)
                if event.event_facility == protocol::SubscriptionEventFacility::SinkInput =>
            {
                self.ask_list()?;
            }
            protocol::Command::RecordStreamKilled(channel) => {
                self.mixer.remove(channel);
                if let Some(input) = self.channels.remove(&channel) {
                    self.inputs.remove(&input);
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Record what plays now and stop recording what ended.
    fn reconcile(&mut self, list: &[protocol::SinkInputInfo]) -> Result<(), String> {
        let discord = self.filter.discord_left_out();
        if discord != self.left_out_discord {
            info!(left_out = discord, "screen audio: Discord's sound");
            self.left_out_discord = discord;
        }
        let wanted: Vec<u32> = list
            .iter()
            .filter(|input| self.filter.lets_through(input, discord))
            .map(|input| input.index)
            .collect();
        let gone: Vec<(u32, Option<u32>)> = self
            .inputs
            .iter()
            .filter(|(input, _)| !wanted.contains(input))
            .map(|(input, channel)| (*input, *channel))
            .collect();
        for (input, channel) in gone {
            self.inputs.remove(&input);
            if let Some(channel) = channel {
                self.channels.remove(&channel);
                self.mixer.remove(channel);
                self.conn
                    .send(protocol::Command::DeleteRecordStream(channel))?;
            }
        }
        for input in wanted {
            if self.inputs.contains_key(&input) {
                continue;
            }
            let mut props = protocol::Props::new();
            props.set(protocol::Prop::MediaName, c"Screen share sound");
            let seq = self.conn.send(protocol::Command::CreateRecordStream(
                protocol::RecordStreamParams {
                    sample_spec: protocol::SampleSpec {
                        format: protocol::SampleFormat::S16Le,
                        channels: CHANNELS as u8,
                        sample_rate: RATE,
                    },
                    channel_map: protocol::ChannelMap::stereo(),
                    direct_on_input_index: Some(input),
                    // 10 ms at a time (the default is hundreds).
                    buffer_attr: protocol::stream::BufferAttr {
                        fragment_size: (SAMPLES * 2) as u32,
                        ..Default::default()
                    },
                    flags: protocol::stream::StreamFlags {
                        adjust_latency: true,
                        no_move: true,
                        ..Default::default()
                    },
                    props,
                    ..Default::default()
                },
            ))?;
            self.pending.insert(seq, Pending::Create(input));
            self.inputs.insert(input, None);
        }
        Ok(())
    }

    /// Samples for one stream: queued, then mixed while there is a frame.
    fn take(&mut self, channel: u32, bytes: &[u8]) {
        self.mixer.push(
            channel,
            bytes
                .chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]])),
            &mut self.out,
        );
    }
}

impl Filter {
    /// Whether a playback stream is shared: not this app's (always), not
    /// Discord's (with `discord` left out), on the sink asked for.
    fn lets_through(&self, input: &protocol::SinkInputInfo, discord: bool) -> bool {
        if self.only_sink.is_some_and(|sink| sink != input.sink_index) {
            return false;
        }
        let text = |prop: &CStr| {
            input
                .props
                .get_bytes(prop)
                .and_then(|bytes| std::str::from_utf8(bytes.strip_suffix(&[0]).unwrap_or(bytes)).ok())
                .map(str::to_owned)
        };
        let pid = text(c"application.process.id").and_then(|pid| pid.trim().parse::<u32>().ok());
        if pid == Some(self.exclude_pid) {
            return false;
        }
        !(discord && is_discord_stream(&stream_names(&text, pid)))
    }
}

/// Every name a playback stream's app goes by: what it told the server
/// (its name, program, app id; a Flatpak's id) and, where its process can
/// be seen, the program running (Electron apps often call themselves
/// "Chromium").
fn stream_names(text: &dyn Fn(&CStr) -> Option<String>, pid: Option<u32>) -> Vec<String> {
    let mut names: Vec<String> = [
        c"application.name",
        c"application.process.binary",
        c"application.id",
        c"pipewire.access.portal.app_id",
        c"application.icon_name",
    ]
    .into_iter()
    .filter_map(text)
    .collect();
    if let Some(pid) = pid {
        let proc = std::path::PathBuf::from(format!("/proc/{pid}"));
        if let Ok(comm) = std::fs::read_to_string(proc.join("comm")) {
            names.push(comm.trim().to_owned());
        }
        if let Ok(exe) = std::fs::read_link(proc.join("exe")) {
            names.push(exe.to_string_lossy().into_owned());
        }
    }
    names
}

/// Whether any of a stream's names is Discord's (or a Discord client's).
fn is_discord_stream(names: &[String]) -> bool {
    names.iter().any(|name| crate::is_discord_app(name))
}

/// A message from the server.
enum Message {
    /// Samples for a record stream.
    Data { channel: u32, bytes: Vec<u8> },
    /// A command (a reply's payload follows in `rest`).
    Command {
        seq: u32,
        command: protocol::Command,
        rest: std::io::Cursor<Vec<u8>>,
    },
    /// Nothing came for a while.
    Idle,
}

/// A blocking connection to the server, read and written by one thread.
pub(crate) struct Conn {
    sock: BufReader<UnixStream>,
    version: u16,
    seq: u32,
}

impl Conn {
    /// Connected and introduced (as this process: the server notes its id).
    pub fn open(name: &CStr) -> Result<Self, String> {
        let path = pulseaudio::socket_path_from_env().ok_or("no PulseAudio server")?;
        let sock = UnixStream::connect(path).map_err(|err| err.to_string())?;
        let cookie = pulseaudio::cookie_path_from_env()
            .and_then(|path| std::fs::read(path).ok())
            .unwrap_or_default();
        let mut conn = Self {
            sock: BufReader::new(sock),
            version: protocol::MAX_VERSION,
            seq: 0,
        };
        let seq = conn.send(protocol::Command::Auth(protocol::AuthParams {
            version: protocol::MAX_VERSION,
            supports_shm: false,
            supports_memfd: false,
            cookie,
        }))?;
        let mut rest = conn.reply(seq)?;
        let auth: protocol::AuthReply = read(&mut rest, conn.version)?;
        conn.version = protocol::MAX_VERSION.min(auth.version);
        let mut props = protocol::Props::new();
        props.set(protocol::Prop::ApplicationName, name);
        props.set(
            protocol::Prop::ApplicationProcessId,
            CString::new(std::process::id().to_string()).unwrap_or_default(),
        );
        let seq = conn.send(protocol::Command::SetClientName(props))?;
        conn.reply(seq)?;
        Ok(conn)
    }

    fn send(&mut self, command: protocol::Command) -> Result<u32, String> {
        self.seq = self.seq.wrapping_add(1);
        protocol::write_command_message(self.sock.get_mut(), self.seq, &command, self.version)
            .map_err(|err| err.to_string())?;
        Ok(self.seq)
    }

    /// The next message; with `idle`, [`Message::Idle`] once that long has
    /// gone by with none starting (a message once begun is read whole).
    fn next(&mut self, idle: Option<Duration>) -> Result<Message, String> {
        use std::io::BufRead as _;
        if let Some(idle) = idle
            && self.sock.buffer().is_empty()
        {
            let sock = self.sock.get_ref().try_clone().map_err(|err| err.to_string())?;
            sock.set_read_timeout(Some(idle))
                .map_err(|err| err.to_string())?;
            let waited = self.sock.fill_buf().map(|got| got.is_empty());
            sock.set_read_timeout(None).map_err(|err| err.to_string())?;
            match waited {
                Ok(true) => return Err("the sound server closed the connection".into()),
                Ok(false) => {}
                Err(err)
                    if matches!(
                        err.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    return Ok(Message::Idle);
                }
                Err(err) => return Err(err.to_string()),
            }
        }
        let desc = protocol::read_descriptor(&mut self.sock).map_err(|err| err.to_string())?;
        let mut bytes = vec![0u8; desc.length as usize];
        self.sock
            .read_exact(&mut bytes)
            .map_err(|err| err.to_string())?;
        if desc.channel != u32::MAX {
            return Ok(Message::Data {
                channel: desc.channel,
                bytes,
            });
        }
        let mut rest = std::io::Cursor::new(bytes);
        let (seq, command) = protocol::Command::read_tag_prefixed(&mut rest, self.version)
            .map_err(|err| err.to_string())?;
        Ok(Message::Command { seq, command, rest })
    }

    /// The reply to `seq` (anything else meanwhile is dropped: only used
    /// before streams exist).
    fn reply(&mut self, seq: u32) -> Result<std::io::Cursor<Vec<u8>>, String> {
        loop {
            if let Message::Command {
                seq: got,
                command,
                rest,
            } = self.next(None)?
                && got == seq
            {
                return match command {
                    protocol::Command::Reply => Ok(rest),
                    protocol::Command::Error(err) => Err(format!("{err:?}")),
                    other => Err(format!("unexpected {other:?}")),
                };
            }
        }
    }

    #[cfg(test)]
    pub fn load_module(&mut self, name: &str, arguments: &str) -> Result<u32, String> {
        let seq = self.send(protocol::Command::LoadModule(protocol::LoadModuleParams {
            name: CString::new(name).map_err(|err| err.to_string())?,
            arguments: Some(CString::new(arguments).map_err(|err| err.to_string())?),
        }))?;
        let mut rest = self.reply(seq)?;
        protocol::TagStructReader::new(&mut rest, self.version)
            .read_u32()
            .map_err(|err| err.to_string())
    }

    #[cfg(test)]
    pub fn unload_module(&mut self, index: u32) -> Result<(), String> {
        let seq = self.send(protocol::Command::UnloadModule(index))?;
        self.reply(seq).map(|_| ())
    }

    #[cfg(test)]
    pub fn sink_index(&mut self, name: &str) -> Result<u32, String> {
        let seq = self.send(protocol::Command::GetSinkInfoList)?;
        let mut rest = self.reply(seq)?;
        let sinks: protocol::SinkInfoList = read(&mut rest, self.version)?;
        sinks
            .into_iter()
            .find(|s| s.name.to_bytes() == name.as_bytes())
            .map(|s| s.index)
            .ok_or_else(|| format!("no sink {name}"))
    }
}

fn read<T: protocol::TagStructRead>(
    rest: &mut std::io::Cursor<Vec<u8>>,
    version: u16,
) -> Result<T, String> {
    protocol::TagStructReader::new(rest, version)
        .read()
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, Mutex},
        time::{Duration, Instant},
    };

    use super::*;
    use crate::imp::system_audio::{CHANNELS, FRAME, power_at};

    /// A sink of the test's own, playing nowhere, so the tones are not heard
    /// (removed again when dropped).
    struct NullSink {
        name: String,
        index: u32,
        module: u32,
        conn: super::Conn,
    }

    impl NullSink {
        fn new() -> Option<Self> {
            let mut conn = super::Conn::open(c"morpheus test sink").ok()?;
            let name = format!("morpheus_test_{}", std::process::id());
            let module = conn
                .load_module(
                    "module-null-sink",
                    &format!(
                        "sink_name={name} rate=48000 channels=2 sink_properties=device.description=Morpheus-test"
                    ),
                )
                .expect("null sink");
            let index = conn.sink_index(&name).expect("the null sink is listed");
            Some(Self {
                name,
                index,
                module,
                conn,
            })
        }
    }

    impl Drop for NullSink {
        fn drop(&mut self) {
            let _ = self.conn.unload_module(self.module);
        }
    }

    /// A sine of `hz` played to the sink named `sink` by this process
    /// (through cpal, as the app's chimes are) while the stream lives.
    fn play(sink: &str, hz: f32) -> cpal::Stream {
        use cpal::traits::{DeviceTrait as _, HostTrait as _, StreamTrait as _};
        let host = cpal::default_host();
        let device = host
            .output_devices()
            .expect("devices")
            .find(|d| d.id().is_ok_and(|id| id.id() == sink))
            .expect("the null sink as a device");
        let supported = device.default_output_config().expect("config");
        let channels = usize::from(supported.channels());
        let rate = supported.sample_rate() as f32;
        let mut config = supported.config();
        config.buffer_size = cpal::BufferSize::Fixed(480);
        let mut phase = 0f32;
        let step = hz * std::f32::consts::TAU / rate;
        let stream = device
            .build_output_stream(
                config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    for frame in data.chunks_exact_mut(channels) {
                        phase = (phase + step) % std::f32::consts::TAU;
                        frame.fill(phase.sin() * 0.25);
                    }
                },
                |err| eprintln!("test tone: {err}"),
                None,
            )
            .expect("stream");
        stream.play().expect("plays");
        stream
    }

    /// CPU time the capture thread has used (Linux's per-thread counters).
    fn capture_thread_cpu() -> Option<Duration> {
        for task in std::fs::read_dir("/proc/self/task").ok()?.flatten() {
            let comm = std::fs::read_to_string(task.path().join("comm")).unwrap_or_default();
            if !comm.starts_with("morpheus-screen") {
                continue;
            }
            // Nanoseconds on a CPU, the first figure.
            let stat = std::fs::read_to_string(task.path().join("schedstat")).ok()?;
            let ns: u64 = stat.split_whitespace().next()?.parse().ok()?;
            return Some(Duration::from_nanos(ns));
        }
        None
    }

    const OURS: f32 = 440.0;
    const THEIRS: f32 = 1000.0;

    /// This process plays one tone, another process (the helper below)
    /// another, both to the test's null sink: only the other one is captured,
    /// from when it starts, and nothing once it ends.
    #[test]
    fn other_apps_are_captured_but_not_this_one() {
        let Some(sink) = NullSink::new() else {
            eprintln!("no PulseAudio or PipeWire server; skipping");
            return;
        };
        let got = Arc::new(Mutex::new(Vec::<(Instant, Vec<i16>)>::new()));
        let into = got.clone();
        let capture = start(
            Filter {
                exclude_pid: std::process::id(),
                only_sink: Some(sink.index),
                leave_out_discord: Some(true),
            },
            Box::new(move |frame| {
                into.lock()
                    .expect("lock")
                    .push((Instant::now(), frame.to_vec()))
            }),
        )
        .expect("capture starts");
        let _ours = play(&sink.name, OURS);
        std::thread::sleep(Duration::from_millis(500));
        // Another app starts after the capture: it is followed.
        let mut theirs = std::process::Command::new(std::env::current_exe().expect("test binary"))
            .args([
                "--exact",
                "imp::system_audio_pulse::tests::helper_plays_a_tone",
                "--ignored",
            ])
            .env("MORPHEUS_TEST_SINK", &sink.name)
            .env("MORPHEUS_TEST_HZ", THEIRS.to_string())
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect("the other app");
        let started = Instant::now();
        let cpu_before = capture_thread_cpu();
        std::thread::sleep(Duration::from_millis(3000));
        if let (Some(a), Some(b)) = (cpu_before, capture_thread_cpu()) {
            eprintln!(
                "capture thread CPU: {:.2}% of a core",
                (b - a).as_secs_f64() / 3.0 * 100.0
            );
        }
        let _ = theirs.kill();
        let _ = theirs.wait();
        let ended = Instant::now();
        std::thread::sleep(Duration::from_millis(1000));
        drop(capture);

        let got = got.lock().expect("lock");
        let between = |from: Instant, to: Instant| -> Vec<i16> {
            got.iter()
                .filter(|(at, _)| *at >= from && *at < to)
                .flat_map(|(_, f)| f.iter().copied())
                .collect()
        };
        // Before the other app: nothing of ours.
        let before = between(started - Duration::from_millis(450), started);
        let ours_before = power_at(&before, CHANNELS, 0, OURS);
        eprintln!("before: {} samples, ours {ours_before:e}", before.len());
        assert!(
            ours_before < 1e-6,
            "this process's own tone was captured: {ours_before:e}"
        );
        // While it plays: its tone, still none of ours.
        let first = got
            .iter()
            .find(|(at, f)| *at >= started && power_at(f, CHANNELS, 0, THEIRS) > 1e-3)
            .map(|(at, _)| at.duration_since(started));
        eprintln!("the other app heard after {first:?}");
        let first = first.expect("the other app's tone was never captured");
        assert!(
            first < Duration::from_millis(1500),
            "followed after {first:?}"
        );
        let during = between(started + Duration::from_millis(1500), ended);
        assert!(
            during.len() >= 100 * FRAME * CHANNELS,
            "{} samples while it played",
            during.len()
        );
        let (theirs_p, ours_p) = (
            power_at(&during, CHANNELS, 0, THEIRS),
            power_at(&during, CHANNELS, 0, OURS),
        );
        eprintln!("during: theirs {theirs_p:e}, ours {ours_p:e}");
        assert!(theirs_p > 1e-3, "the other app's tone: {theirs_p:e}");
        assert!(
            ours_p < theirs_p / 1e4,
            "this process's own tone leaked in: {ours_p:e} vs {theirs_p:e}"
        );
        // Once it ended: nothing of it.
        let after = between(
            ended + Duration::from_millis(300),
            ended + Duration::from_millis(1000),
        );
        let theirs_after = power_at(&after, CHANNELS, 0, THEIRS);
        assert!(
            theirs_after < 1e-6,
            "still captured after it ended: {theirs_after:e}"
        );
    }

    /// The other app of the test above: plays `MORPHEUS_TEST_HZ` to
    /// `MORPHEUS_TEST_SINK` for 5 s.
    #[test]
    #[ignore = "helper, run by other_apps_are_captured_but_not_this_one"]
    fn helper_plays_a_tone() {
        let (Ok(sink), Ok(hz)) = (
            std::env::var("MORPHEUS_TEST_SINK"),
            std::env::var("MORPHEUS_TEST_HZ"),
        ) else {
            return;
        };
        let _stream = play(&sink, hz.parse().expect("hz"));
        std::thread::sleep(Duration::from_secs(5));
    }

    const DISCORDS: f32 = 700.0;

    /// Another app plays a tone, and "Discord" (a helper calling itself so)
    /// another, both to the test's null sink, captured twice at once: with
    /// Discord left out only the other app's tone is shared; let in, both
    /// are.
    #[test]
    fn discord_is_left_out_unless_let_in() {
        let Some(sink) = NullSink::new() else {
            eprintln!("no PulseAudio or PipeWire server; skipping");
            return;
        };
        let capture = |leave_out: bool| {
            let got = Arc::new(Mutex::new(Vec::<i16>::new()));
            let into = got.clone();
            let running = start(
                Filter {
                    exclude_pid: std::process::id(),
                    only_sink: Some(sink.index),
                    leave_out_discord: Some(leave_out),
                },
                Box::new(move |frame| into.lock().expect("lock").extend_from_slice(frame)),
            )
            .expect("capture starts");
            (running, got)
        };
        let (left_out, without) = capture(true);
        let (let_in, with) = capture(false);
        let helper = |name: &str, hz: f32| {
            std::process::Command::new(std::env::current_exe().expect("test binary"))
                .args([
                    "--exact",
                    &format!("imp::system_audio_pulse::tests::{name}"),
                    "--ignored",
                ])
                .env("MORPHEUS_TEST_SINK", &sink.name)
                .env("MORPHEUS_TEST_HZ", hz.to_string())
                .stdout(std::process::Stdio::null())
                .spawn()
                .expect("helper")
        };
        let mut discord = helper("helper_plays_a_tone_as_discord", DISCORDS);
        let mut other = helper("helper_plays_a_tone", THEIRS);
        std::thread::sleep(Duration::from_millis(1500));
        // From here on (both followed by now).
        let from = (without.lock().expect("lock").len(), with.lock().expect("lock").len());
        std::thread::sleep(Duration::from_millis(2000));
        for child in [&mut discord, &mut other] {
            let _ = child.kill();
            let _ = child.wait();
        }
        drop((left_out, let_in));
        let without = without.lock().expect("lock")[from.0..].to_vec();
        let with = with.lock().expect("lock")[from.1..].to_vec();
        assert!(without.len() >= 100 * FRAME * CHANNELS, "{} samples", without.len());
        assert!(with.len() >= 100 * FRAME * CHANNELS, "{} samples", with.len());
        let power = |samples: &[i16], hz| power_at(samples, CHANNELS, 0, hz);
        eprintln!(
            "Discord left out: other {:e}, Discord {:e}; let in: other {:e}, Discord {:e}",
            power(&without, THEIRS),
            power(&without, DISCORDS),
            power(&with, THEIRS),
            power(&with, DISCORDS)
        );
        assert!(power(&without, THEIRS) > 1e-3, "the other app is shared");
        assert!(
            power(&without, DISCORDS) < power(&without, THEIRS) / 1e4,
            "Discord was shared though left out"
        );
        assert!(power(&with, THEIRS) > 1e-3, "the other app is shared");
        assert!(power(&with, DISCORDS) > 1e-3, "Discord is shared when let in");
    }

    /// "Discord" for the test above: a PulseAudio client of that name
    /// playing `MORPHEUS_TEST_HZ` to `MORPHEUS_TEST_SINK` for 5 s.
    #[test]
    #[ignore = "helper, run by discord_is_left_out_unless_let_in"]
    fn helper_plays_a_tone_as_discord() {
        use pulseaudio::AsPlaybackSource as _;
        let (Ok(sink), Ok(hz)) = (
            std::env::var("MORPHEUS_TEST_SINK"),
            std::env::var("MORPHEUS_TEST_HZ"),
        ) else {
            return;
        };
        let step = hz.parse::<f32>().expect("hz") * std::f32::consts::TAU / RATE as f32;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(async move {
            let client = pulseaudio::Client::from_env(c"Discord").expect("connects");
            let params = protocol::PlaybackStreamParams {
                sample_spec: protocol::SampleSpec {
                    format: protocol::SampleFormat::S16Le,
                    channels: CHANNELS as u8,
                    sample_rate: RATE,
                },
                channel_map: protocol::ChannelMap::stereo(),
                sink_name: Some(CString::new(sink).expect("sink name")),
                ..Default::default()
            };
            let mut phase = 0f32;
            let source = move |buf: &mut [u8]| {
                for pair in buf.chunks_exact_mut(4) {
                    phase = (phase + step) % std::f32::consts::TAU;
                    let s = ((phase.sin() * 0.25) * 32767.0) as i16;
                    pair[..2].copy_from_slice(&s.to_le_bytes());
                    pair[2..].copy_from_slice(&s.to_le_bytes());
                }
                buf.len() / 4 * 4
            };
            let stream = client
                .create_playback_stream(params, source.as_playback_source())
                .await
                .expect("plays");
            let _ = tokio::time::timeout(Duration::from_secs(5), stream.play_all()).await;
        });
    }
}
