//! The Rust half of push notifications. The Kotlin `MorpheusPushService`
//! (apps/mobile/android) writes the UnifiedPush endpoint to
//! `push_endpoint` in the data directory; this registers it with the
//! homeserver as a pusher (and drops a replaced one), and keeps
//! `room_names.json` current so notifications can name the room. Plain
//! files keep the two sides free of JNI calls.

use std::{
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::Duration,
};

use morpheus_core::Session;

/// How often the endpoint file and the room names are looked at.
const INTERVAL: Duration = Duration::from_secs(15);

/// The signed-in session the bridge serves; swapped on sign-in.
static ACTIVE: OnceLock<Mutex<Option<Session>>> = OnceLock::new();

/// Called for every session that becomes active. Starts the bridge thread
/// the first time.
pub fn on_session(session: Session, data_dir: PathBuf) {
    write_notices(session.clone(), data_dir.join(NOTICES_DIR));
    let first = ACTIVE.get().is_none();
    let slot = ACTIVE.get_or_init(|| Mutex::new(None));
    if let Ok(mut active) = slot.lock() {
        *active = Some(session);
    }
    if !first {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("morpheus-push".into())
        .spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                tracing::warn!("push bridge runtime failed");
                return;
            };
            runtime.block_on(async move {
                let mut empty_passes = 0;
                loop {
                    let session = slot.lock().ok().and_then(|active| active.clone());
                    if let Some(session) = session {
                        sync(&session, &data_dir, &mut empty_passes).await;
                    }
                    tokio::time::sleep(INTERVAL).await;
                }
            });
        });
    if let Err(err) = spawned {
        tracing::warn!(%err, "push bridge thread failed");
    }
}

fn read(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
}

/// Passes with an empty room list before registering anyway (an account
/// with no rooms): about a minute.
const EMPTY_PASSES_BEFORE_PUSH: u32 = 4;

/// One pass. `empty_passes` counts passes that found no rooms yet.
async fn sync(session: &Session, dir: &Path, empty_passes: &mut u32) {
    let mut have_rooms = false;
    if let Ok(names) = session.room_names().await {
        have_rooms = !names.is_empty();
        let map: serde_json::Map<String, serde_json::Value> = names
            .into_iter()
            .map(|(id, name)| (id, serde_json::Value::String(name)))
            .collect();
        let _ = std::fs::write(
            dir.join("room_names.json"),
            serde_json::Value::Object(map).to_string(),
        );
    }

    // Right after sign-in the room list has not synced yet: register once it
    // has, so the first notification can already name its room.
    if !have_rooms && *empty_passes < EMPTY_PASSES_BEFORE_PUSH {
        *empty_passes += 1;
        return;
    }

    let endpoint = read(&dir.join("push_endpoint"));
    // One record per account: each account on the device registers the
    // phone's endpoint for itself (switching accounts keeps the others').
    let user = session
        .user_id()
        .map(|id| id.to_string())
        .unwrap_or_default()
        .replace([':', '@'], "_");
    let registered_path = dir.join(format!("push_registered_{user}"));
    let registered = read(&registered_path);
    if endpoint == registered {
        return;
    }
    if let Some(old) = &registered
        && let Err(err) = session.unregister_push(old.clone()).await
    {
        tracing::warn!(%err, "removing the old pusher failed");
    }
    match endpoint {
        Some(endpoint) => match session
            .register_push(endpoint.clone(), "Morpheus on Android".into())
            .await
        {
            Ok(gateway) => {
                tracing::info!(%gateway, "push notifications registered");
                let _ = std::fs::write(&registered_path, &endpoint);
                let _ =
                    std::fs::write(dir.join("push_status"), format!("registered via {gateway}"));
            }
            Err(err) => {
                tracing::warn!(%err, "registering for push failed");
                let _ = std::fs::write(dir.join("push_status"), format!("failed: {err}"));
            }
        },
        None => {
            let _ = std::fs::remove_file(&registered_path);
        }
    }
}

/// How often to look for a room a tapped notification asked for.
const OPEN_ROOM_POLL: Duration = Duration::from_millis(400);

/// The notification trampoline (`OpenRoomActivity`) leaves the room id in
/// `open_room`; hand it to the shell, which shows that room.
pub fn watch_open_room(data_dir: PathBuf, commands: morpheus_ui::AppCommands) {
    let path = data_dir.join("open_room");
    let spawned = std::thread::Builder::new()
        .name("morpheus-open-room".into())
        .spawn(move || {
            loop {
                if let Some(room) = read(&path) {
                    let _ = std::fs::remove_file(&path);
                    commands.send(morpheus_ui::AppCommand::OpenRoom(room));
                }
                std::thread::sleep(OPEN_ROOM_POLL);
            }
        });
    if let Err(err) = spawned {
        tracing::warn!(%err, "open-room watcher failed");
    }
}

/// Where notices for the push service go, one file per message.
const NOTICES_DIR: &str = "notices";
/// Notices older than this are cleared away.
const NOTICE_TTL: Duration = Duration::from_secs(600);

/// File name for a message's notice: its event id, made filename-safe (the
/// Kotlin service does the same).
fn notice_file(event_id: &str) -> String {
    event_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

/// While the app runs it syncs, so it has every new message decrypted: write
/// who said what for each one that should notify, for the push service
/// (`MorpheusPushService`) to show when the matching push arrives. A push
/// alone carries only ids.
fn write_notices(session: Session, dir: PathBuf) {
    let spawned = std::thread::Builder::new()
        .name("morpheus-notices".into())
        .spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            runtime.block_on(async move {
                let Ok(mut updates) = session.notifications().await else {
                    return;
                };
                let _ = std::fs::create_dir_all(&dir);
                while let Some(notice) = updates.next().await {
                    let body = serde_json::json!({
                        "room_id": notice.room_id.to_string(),
                        "room": notice.room_name,
                        "sender": notice.sender_name,
                        "body": notice.body_summary,
                        "mention": notice.is_mention,
                    });
                    let _ =
                        std::fs::write(dir.join(notice_file(&notice.event_id)), body.to_string());
                    prune(&dir);
                }
            });
        });
    if let Err(err) = spawned {
        tracing::warn!(%err, "notice writer failed");
    }
}

fn prune(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > NOTICE_TTL);
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}
