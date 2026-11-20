//! Bridges (#107): connecting other networks through the mautrix bridges the
//! homeserver runs, as Settings > Bridges does on the desktop. The words of
//! the bots' replies and the login's steps are `morpheus-view-model::bridges`
//! (the same code); this runs the conversation with a bot as one Rust task and
//! pushes what the page shows (the status, the question, the QR code's
//! picture) as whole [`BridgeSnapshot`]s.

use std::sync::Arc;

use morpheus_core::OwnedUserId;
use morpheus_view_model::bridges::{self as vm, Login, Status};
use tokio::{sync::mpsc, task::JoinHandle};

use crate::{FfiError, Morpheus, runtime};

const SAVED: &str = "bridge_bots.json";

/// One bridge bot the server has.
#[derive(Debug, Clone, uniffi::Record)]
pub struct BridgeBot {
    pub bot: String,
    /// The network's id (`discord`), empty for a bot that is not one listed.
    pub network: String,
    /// The network's name, or the bot's ID.
    pub name: String,
    /// What to do on the phone to link it (an English key for `translate`).
    pub steps: String,
}

/// Where a bridge's login stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum BridgePhase {
    Unknown,
    Checking,
    LoggedOut,
    Starting,
    /// A QR code is up ([`BridgeSnapshot::qr`]).
    Qr,
    /// The bot asks for something typed ([`BridgeSnapshot::detail`] is the question).
    Prompt,
    LoggedIn,
    NeedsLogin,
    Error,
}

/// What a bridge's row shows now.
#[derive(Debug, Clone, uniffi::Record)]
pub struct BridgeSnapshot {
    pub phase: BridgePhase,
    /// The status in a word, an English key for `translate`.
    pub label: String,
    /// The question, whom it is logged in as, or the error.
    pub detail: Option<String>,
    /// The QR code's picture (the bot's image), while one is up.
    pub qr: Option<Vec<u8>>,
}

#[uniffi::export(with_foreign)]
pub trait BridgeListener: Send + Sync {
    fn on_bridge(&self, snapshot: BridgeSnapshot);
}

fn snapshot(status: &Status, qr: &Option<Vec<u8>>) -> BridgeSnapshot {
    let (phase, detail) = match status {
        Status::Unknown => (BridgePhase::Unknown, None),
        Status::Checking => (BridgePhase::Checking, None),
        Status::LoggedOut => (BridgePhase::LoggedOut, None),
        Status::Starting => (BridgePhase::Starting, None),
        Status::Qr => (BridgePhase::Qr, None),
        Status::Prompt(question) => (BridgePhase::Prompt, Some(question.clone())),
        Status::LoggedIn(who) => (BridgePhase::LoggedIn, who.clone()),
        Status::NeedsLogin(why) => (BridgePhase::NeedsLogin, Some(why.clone())),
        Status::Error(why) => (BridgePhase::Error, Some(why.clone())),
    };
    BridgeSnapshot { phase, label: status.label().to_owned(), detail, qr: qr.clone() }
}

fn load_extra(dir: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(dir.join(SAVED))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save_extra(dir: &std::path::Path, bots: &[String]) {
    if let Ok(text) = serde_json::to_string(bots) {
        let _ = std::fs::write(dir.join(SAVED), text);
    }
}

enum Command {
    Check,
    Start,
    LogOut,
    Cancel,
    Answer(String),
}

/// One bot's conversation: the commands go in, snapshots come out. Stops when dropped.
#[derive(uniffi::Object)]
pub struct BridgeHandle {
    commands: mpsc::UnboundedSender<Command>,
    task: std::sync::Mutex<Option<JoinHandle<()>>>,
}

impl Drop for BridgeHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

#[uniffi::export]
impl BridgeHandle {
    /// Ask the bot where the login stands.
    pub fn check(&self) {
        let _ = self.commands.send(Command::Check);
    }

    /// Start a login.
    pub fn connect(&self) {
        let _ = self.commands.send(Command::Start);
    }

    pub fn log_out(&self) {
        let _ = self.commands.send(Command::LogOut);
    }

    /// Stop a login in progress.
    pub fn cancel(&self) {
        let _ = self.commands.send(Command::Cancel);
    }

    /// Answer the bot's question (a phone number, a code, a password).
    pub fn answer(&self, typed: String) {
        let _ = self.commands.send(Command::Answer(typed));
    }

    /// End the conversation's task.
    pub fn stop(&self) {
        if let Some(task) = self.task.lock().ok().and_then(|mut t| t.take()) {
            task.abort();
        }
    }
}

#[uniffi::export]
impl Morpheus {
    /// The bridge bots this server has: the usual names, the ones typed in before and the ones
    /// the bridged rooms name.
    pub async fn bridge_bots(&self) -> Result<Vec<BridgeBot>, FfiError> {
        let session = self.session()?;
        let me = session.user_id().ok_or(FfiError::SignedOut)?;
        let named = session.room_bridges().await.map(|b| vm::bots_in_rooms(&b)).unwrap_or_default();
        let extra: Vec<String> = load_extra(&self.dir).into_iter().chain(named).collect();
        let candidates = vm::candidates(me.server_name().as_str(), &extra);
        let asks: Vec<_> = candidates.iter().map(|c| session.find_user(c.bot.clone())).collect();
        let answers = futures_util::future::join_all(asks).await;
        Ok(candidates
            .into_iter()
            .zip(answers)
            .filter(|(_, answer)| answer.is_ok())
            .map(|(c, _)| {
                let network = c.network.and_then(vm::network);
                BridgeBot {
                    name: network.map_or_else(|| c.bot.clone(), |n| n.name.to_owned()),
                    steps: network.map_or(vm::GENERIC_STEPS, |n| n.steps).to_owned(),
                    network: c.network.unwrap_or_default().to_owned(),
                    bot: c.bot,
                }
            })
            .collect())
    }

    /// Remember a bot typed in (a name such as `discordbot` or `@bot:example.org`) and say which
    /// it is. Fails with words when it is not a name or the server has no such user.
    pub async fn add_bridge_bot(&self, typed: String) -> Result<String, FfiError> {
        let session = self.session()?;
        let me = session.user_id().ok_or(FfiError::SignedOut)?;
        let bot = vm::normalise_bot(&typed, me.server_name().as_str())
            .ok_or_else(|| FfiError::Failed { reason: format!("\"{}\" is not a bot name.", typed.trim()) })?;
        session.find_user(bot.clone()).await.map_err(|_| FfiError::Failed {
            reason: format!("No bridge bot was found on this server called {bot}."),
        })?;
        let mut extra = load_extra(&self.dir);
        if !extra.contains(&bot) {
            extra.push(bot.clone());
            save_extra(&self.dir, &extra);
        }
        Ok(bot)
    }

    /// Open the conversation with a bot and push its snapshots to `listener`. Nothing is sent
    /// until a command is given.
    pub fn open_bridge(&self, bot: String, listener: Arc<dyn BridgeListener>) -> Result<Arc<BridgeHandle>, FfiError> {
        let session = self.session()?;
        let user = OwnedUserId::try_from(bot.as_str())
            .map_err(|_| FfiError::Failed { reason: format!("\"{bot}\" is not a bot name.") })?;
        let network = vm::network_of_bot(&bot).map(|n| n.id);
        let (tx, mut rx) = mpsc::unbounded_channel::<Command>();
        let task = runtime().spawn(async move {
            let mut login = Login::new(network);
            let mut qr: Option<Vec<u8>> = None;
            let mut seen: std::collections::HashMap<String, String> = std::collections::HashMap::new();
            let mut open: Option<(morpheus_core::TimelineHandle, morpheus_core::TimelineUpdates)> = None;
            let fail = |login: &mut Login, qr: &mut Option<Vec<u8>>, why: String| {
                login.status = Status::Error(why);
                *qr = None;
                listener.on_bridge(snapshot(&login.status, qr));
            };
            loop {
                tokio::select! {
                    command = rx.recv() => {
                        let Some(command) = command else { break };
                        let sends = match command {
                            Command::Check => login.check(),
                            Command::Start => login.start(),
                            Command::LogOut => login.log_out(),
                            Command::Cancel => login.cancel(),
                            Command::Answer(text) => login.answer(&text),
                        };
                        qr = None;
                        listener.on_bridge(snapshot(&login.status, &qr));
                        if open.is_none() {
                            let opened = async {
                                let room = session.bridge_dm_with(user.clone()).await?;
                                session.timeline(room).await
                            }
                            .await;
                            match opened {
                                Ok(mut timeline) => {
                                    // What was said before is not an answer: the first batch is the history.
                                    if let Some(first) = timeline.updates.next().await {
                                        for diff in &first {
                                            for message in vm::messages_in(diff, &bot) {
                                                seen.insert(message.0, vm::fingerprint(&message.1));
                                            }
                                        }
                                    }
                                    open = Some((timeline.handle, timeline.updates));
                                }
                                Err(err) => {
                                    fail(&mut login, &mut qr, err.to_string());
                                    continue;
                                }
                            }
                        }
                        if let Some((handle, _)) = &open {
                            for text in sends {
                                if let Err(err) = handle.send_text(text).await {
                                    fail(&mut login, &mut qr, err.to_string());
                                    break;
                                }
                            }
                        }
                    }
                    diffs = async {
                        match open.as_mut() {
                            Some((_, updates)) => updates.next().await,
                            None => std::future::pending().await,
                        }
                    } => {
                        let Some(diffs) = diffs else {
                            open = None;
                            continue;
                        };
                        for diff in &diffs {
                            for (id, kind) in vm::messages_in(diff, &bot) {
                                if seen.insert(id, vm::fingerprint(&kind)).is_some_and(|before| before == vm::fingerprint(&kind)) {
                                    continue;
                                }
                                let Some((reply, media)) = vm::reply_of(&kind) else { continue };
                                let picture = match media {
                                    Some(media) => session.media_file(media).await.ok().flatten().map(|f| f.bytes),
                                    None => vm::local_qr(&reply),
                                };
                                if picture.is_some() {
                                    qr = picture;
                                }
                                let sends = login.on_reply(&reply);
                                if !matches!(login.status, Status::Qr) {
                                    qr = None;
                                }
                                listener.on_bridge(snapshot(&login.status, &qr));
                                if let Some((handle, _)) = &open {
                                    for text in sends {
                                        if let Err(err) = handle.send_text(text).await {
                                            fail(&mut login, &mut qr, err.to_string());
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });
        Ok(Arc::new(BridgeHandle { commands: tx, task: std::sync::Mutex::new(Some(task)) }))
    }
}
