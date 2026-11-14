//! A room or DM can be locked with a password on this device (client side
//! only): its menu offers "Lock with a password…"; a locked room shows a lock
//! screen instead of its timeline, and nothing else shows what is in it (the
//! notifications, the inbox, search, the cache).

#![allow(clippy::expect_used)]

use std::time::Duration;

use morpheus_e2e::{App, DEFAULT_WAIT, api_as, new_account};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

fn esc(id: &str) -> String {
    id.replace('@', "%40").replace(':', "%3A").replace('!', "%21").replace('$', "%24")
}

fn api(user: &str, hs: &str, method: &str, path: &str, body: serde_json::Value) -> serde_json::Value {
    api_as(hs, user, method, path, Some(body)).expect("api call")
}

async fn login(app: &mut App, user: &str) {
    app.wait_for("form.login", DEFAULT_WAIT).await.expect("login form");
    app.type_into("form.login input[name=username]", user).await.expect("username");
    app.type_into("form.login input[name=password]", &format!("{user}-password"))
        .await
        .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".room", DEFAULT_WAIT).await.expect("room listed");
}

struct Setup {
    app: App,
    hs: String,
    reader: String,
    writer: String,
    dm: String,
    /// What the DM says (a word nothing else has).
    secret: String,
}

/// `reader` is signed in; a DM with `writer` holds one message, and there is a
/// room of the reader's own to be in meanwhile.
async fn setup(hs: &str, name: &str, env: &[(&str, &str)]) -> Setup {
    let reader = new_account(hs, &format!("{name}r")).expect("reader");
    let writer = new_account(hs, &format!("{name}w")).expect("writer");
    let user = |u: &str| format!("@{u}:localhost");
    let dm = api(
        &writer,
        hs,
        "POST",
        "createRoom",
        serde_json::json!({ "is_direct": true, "preset": "trusted_private_chat", "invite": [user(&reader)] }),
    );
    let dm = dm["room_id"].as_str().expect("dm").to_owned();
    api(&reader, hs, "POST", &format!("rooms/{}/join", esc(&dm)), serde_json::json!({}));
    api(
        &reader,
        hs,
        "PUT",
        &format!("user/{}/account_data/m.direct", esc(&user(&reader))),
        serde_json::json!({ user(&writer): [dm] }),
    );
    api(&reader, hs, "POST", "createRoom", serde_json::json!({ "name": "lock elsewhere" }));
    let secret = format!("pomegranate{}", std::process::id());
    say(hs, &writer, &dm, &format!("the {secret} is ripe"));
    let mut app = App::launch_with_env(hs, env).await.expect("launch");
    login(&mut app, &reader).await;
    Setup { app, hs: hs.to_owned(), reader, writer, dm, secret }
}

fn say(hs: &str, user: &str, room: &str, text: &str) {
    api(
        user,
        hs,
        "PUT",
        &format!("rooms/{}/send/m.room.message/{}", esc(room), unique()),
        serde_json::json!({ "msgtype": "m.text", "body": text }),
    );
}

fn unique() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos())
}

impl Setup {
    async fn open_dm(&mut self) {
        let writer = self.writer.clone();
        self.app.open_room(&writer).await.expect("open the DM");
    }

    async fn leave_to_elsewhere(&mut self) {
        self.app.open_room("lock elsewhere").await.expect("the other room");
    }

    /// The DM list, shown (the rail's DMs).
    async fn show_dms(&mut self) {
        self.app.click(".rail-dms").await.expect("DMs");
        self.app.wait_for(".room.dm", DEFAULT_WAIT).await.expect("the DM is listed");
    }

    async fn dm_menu(&mut self, item: &str) {
        let writer = self.writer.clone();
        self.show_dms().await;
        self.app.right_click_exact(".room.dm .room-name", &writer).await.expect("right-click the DM");
        self.app.wait_for(".context-menu", DEFAULT_WAIT).await.expect("menu");
        self.app
            .click_text(".context-menu .context-item", item)
            .await
            .expect("menu item");
    }

    /// Lock the DM with `password` through its menu.
    async fn lock_dm(&mut self, password: &str) {
        self.open_dm().await;
        self.dm_menu("Lock with a password").await;
        self.app.wait_for(".lock-dialog", DEFAULT_WAIT).await.expect("the dialog");
        self.app.type_into(".lock-dialog input[name=password]", password).await.expect("password");
        self.app.type_into(".lock-dialog input[name=again]", password).await.expect("again");
        self.app.click(".lock-dialog button[type=submit]").await.expect("set");
        self.app.wait_for_count(".lock-dialog", 0, DEFAULT_WAIT).await.expect("dialog closed");
    }

    async fn unlock(&mut self, password: &str) {
        self.app.wait_for(".lock-screen input[name=password]", DEFAULT_WAIT).await.expect("the password box");
        self.app.type_into(".lock-screen input[name=password]", password).await.expect("type");
        self.app.click(".lock-screen .unlock").await.expect("Unlock");
    }

    async fn shows_message(&mut self) -> bool {
        self.app
            .texts(".message .body")
            .await
            .expect("bodies")
            .iter()
            .any(|text| text.contains(&self.secret))
    }

    async fn wait_message(&mut self, shown: bool) {
        let deadline = std::time::Instant::now() + DEFAULT_WAIT;
        while self.shows_message().await != shown {
            if std::time::Instant::now() >= deadline {
                let seen = self.app.texts("*").await.unwrap_or_default();
                panic!("the message should show: {shown}; on screen: {seen:?}");
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    async fn nothing_of_it_on_screen(&mut self, what: &str) {
        let mut seen = self.app.texts("*").await.expect("texts");
        seen.extend(self.app.attrs("*", "title").await.expect("titles").into_iter().flatten());
        assert!(
            !seen.iter().any(|text| text.contains(&self.secret)),
            "{what}: the message is on screen"
        );
    }
}

macro_rules! hs {
    () => {
        match homeserver() {
            Some(hs) => hs,
            None => {
                eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
                return;
            }
        }
    };
}

/// Lock a DM: its page is a lock screen with no messages; a wrong password
/// is refused, the right one opens it.
#[tokio::test]
async fn a_locked_dm_shows_a_lock_screen_until_the_password_is_entered() {
    let hs = hs!();
    let mut s = setup(&hs, "lk", &[]).await;
    s.open_dm().await;
    s.wait_message(true).await;

    // The form wants the password twice.
    s.dm_menu("Lock with a password").await;
    s.app.wait_for(".lock-dialog", DEFAULT_WAIT).await.expect("the dialog");
    let blurb = s.app.text(".lock-dialog").await.expect("text").unwrap_or_default();
    assert!(blurb.contains("they don't encrypt it further"), "the dialog says what a lock is: {blurb}");
    s.app.type_into(".lock-dialog input[name=password]", "tulip").await.expect("password");
    s.app.type_into(".lock-dialog input[name=again]", "tulop").await.expect("again");
    s.app.click(".lock-dialog button[type=submit]").await.expect("set");
    s.app.wait_for(".lock-dialog p.error", DEFAULT_WAIT).await.expect("a mismatch is refused");
    s.app.type_into(".lock-dialog input[name=again]", "tulip").await.expect("again");
    s.app.click(".lock-dialog button[type=submit]").await.expect("set");
    s.app.wait_for_count(".lock-dialog", 0, DEFAULT_WAIT).await.expect("closed");

    // Locked at once: a lock screen where the messages were.
    s.app.wait_for(".lock-screen", DEFAULT_WAIT).await.expect("the lock screen");
    s.wait_message(false).await;
    assert_eq!(s.app.count(".composer").await.expect("count"), 0, "no message box while locked");
    s.nothing_of_it_on_screen("right after locking").await;

    // Reopened, still locked.
    s.leave_to_elsewhere().await;
    s.app.wait_for_count(".lock-screen", 0, DEFAULT_WAIT).await.expect("other room");
    s.open_dm().await;
    s.app.wait_for(".lock-screen", DEFAULT_WAIT).await.expect("locked on reopening");
    s.nothing_of_it_on_screen("reopened").await;

    // A wrong password is refused.
    s.unlock("daffodil").await;
    s.app.wait_for(".lock-screen .error", DEFAULT_WAIT).await.expect("refused");
    assert_eq!(s.app.count(".message").await.expect("count"), 0, "still nothing shown");
    // The right one opens it.
    s.unlock("tulip").await;
    s.app.wait_for_count(".lock-screen", 0, DEFAULT_WAIT).await.expect("unlocked");
    s.wait_message(true).await;
}

/// An unlocked room stays so when you leave and come back in time, and locks
/// again after the timeout (a test variable shortens it to 4 s).
#[tokio::test]
async fn an_unlocked_room_locks_again_after_the_timeout() {
    let hs = hs!();
    let mut s = setup(&hs, "lt", &[("MORPHEUS_E2E_LOCK_SECS", "4")]).await;
    s.lock_dm("tulip").await;
    s.app.wait_for(".lock-screen", DEFAULT_WAIT).await.expect("locked");
    s.unlock("tulip").await;
    s.wait_message(true).await;

    // Away and back inside the timeout: open.
    s.leave_to_elsewhere().await;
    tokio::time::sleep(Duration::from_millis(1200)).await;
    s.open_dm().await;
    s.wait_message(true).await;
    assert_eq!(s.app.count(".lock-screen").await.expect("count"), 0, "still unlocked");

    // Away longer than the timeout: locked.
    s.leave_to_elsewhere().await;
    tokio::time::sleep(Duration::from_secs(6)).await;
    s.open_dm().await;
    s.app.wait_for(".lock-screen", DEFAULT_WAIT).await.expect("locked after the timeout");
    s.nothing_of_it_on_screen("after the timeout").await;
}

/// "Lock now" in an unlocked room's menu; "Remove lock" needs the password.
#[tokio::test]
async fn lock_now_and_remove_lock() {
    let hs = hs!();
    let mut s = setup(&hs, "lr", &[]).await;
    s.lock_dm("tulip").await;
    s.unlock("tulip").await;
    s.wait_message(true).await;

    s.dm_menu("Lock now").await;
    s.app.wait_for(".lock-screen", DEFAULT_WAIT).await.expect("locked at once");
    s.wait_message(false).await;

    // Remove: the password is asked, a wrong one refused.
    s.dm_menu("Remove lock").await;
    s.app.wait_for(".lock-dialog", DEFAULT_WAIT).await.expect("asks for the password");
    s.app.type_into(".lock-dialog input[name=password]", "nope").await.expect("wrong");
    s.app.click(".lock-dialog button[type=submit]").await.expect("remove");
    s.app.wait_for(".lock-dialog p.error", DEFAULT_WAIT).await.expect("refused");
    assert!(
        s.app.text(".lock-dialog").await.expect("text").unwrap_or_default().contains("signing out"),
        "the dialog says what to do when the password is forgotten"
    );
    s.app.type_into(".lock-dialog input[name=password]", "tulip").await.expect("right");
    s.app.click(".lock-dialog button[type=submit]").await.expect("remove");
    s.app.wait_for_count(".lock-dialog", 0, DEFAULT_WAIT).await.expect("closed");
    s.app.wait_for_count(".lock-screen", 0, DEFAULT_WAIT).await.expect("the room is open");
    s.wait_message(true).await;
    let kept = std::fs::read_to_string(s.app.data_dir().join("locks.json")).unwrap_or_default();
    assert!(!kept.contains(&s.dm), "the lock is gone from locks.json: {kept}");
}

fn shown(app: &App) -> Vec<serde_json::Value> {
    std::fs::read_to_string(app.data_dir().join("shown-notifications.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// While locked: the notification says only that there is a message, the
/// list shows the name and a dot (no count), the inbox leaves its mentions
/// out, the lock screen has no search, and the switcher still finds the name.
#[tokio::test]
async fn a_locked_room_gives_nothing_away_elsewhere() {
    let hs = hs!();
    let mut s = setup(&hs, "ln", &[]).await;
    s.lock_dm("tulip").await;
    s.leave_to_elsewhere().await;
    s.show_dms().await;
    s.app.wait_for(".room.dm.locked", DEFAULT_WAIT).await.expect("the row is marked locked");

    let before = shown(&s.app).len();
    let word = format!("rhubarb{}", std::process::id());
    let text = format!("{word} for you");
    api(
        &s.writer,
        &hs,
        "PUT",
        &format!("rooms/{}/send/m.room.message/{}", esc(&s.dm), unique()),
        serde_json::json!({
            "msgtype": "m.text", "body": text,
            "m.mentions": { "user_ids": [format!("@{}:localhost", s.reader)] },
        }),
    );
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    let notice = loop {
        if let Some(notice) = shown(&s.app).into_iter().nth(before) {
            break notice;
        }
        assert!(std::time::Instant::now() < deadline, "no notification: {:?}", shown(&s.app));
        tokio::time::sleep(Duration::from_millis(200)).await;
    };
    assert_eq!(notice["body"].as_str(), Some("New message in a locked chat"), "{notice}");
    let all = notice.to_string();
    assert!(!all.contains(&word) && !all.contains(&s.writer), "nothing of the message or sender: {all}");
    // The hidden log of announcements says no more.
    tokio::time::sleep(Duration::from_millis(500)).await;
    for line in s.app.texts(".notification-log li").await.expect("log") {
        assert!(!line.contains(&word) && !line.contains(&s.secret), "the log gives it away: {line}");
    }

    // The row keeps its name; no count.
    let names = s.app.texts(".room.dm .room-name").await.expect("names");
    assert!(names.iter().any(|n| n.trim() == s.writer), "the name is listed: {names:?}");
    assert_eq!(s.app.count(".room.dm .badge").await.expect("badges"), 0, "no count on a locked room");
    s.app.wait_for(".room.dm.locked .lock-dot", DEFAULT_WAIT).await.expect("a dot says there is news");

    // The inbox does not list the mention.
    s.app.click(".titlebar .inbox-button").await.expect("inbox");
    s.app.wait_for(".inbox-panel", DEFAULT_WAIT).await.expect("inbox panel");
    assert_eq!(s.app.count(".inbox-panel .inbox-mention").await.expect("count"), 0, "no mention of it");
    let seen = s.app.texts("*").await.expect("texts");
    assert!(!seen.iter().any(|t| t.contains(&word)), "the inbox gives the mention away");
    s.app.press("body", "Escape").await.expect("close inbox");
    s.app.wait_for_count(".inbox-panel", 0, DEFAULT_WAIT).await.expect("inbox closed");

    // Search is of the room open: the lock screen has none, and finds nothing.
    s.open_dm().await;
    s.app.wait_for(".lock-screen", DEFAULT_WAIT).await.expect("locked");
    assert_eq!(s.app.count(".chat-header input[name=search]").await.expect("count"), 0, "no search box");
    assert_eq!(s.app.count(".search-panel").await.expect("count"), 0, "no search panel");
    s.nothing_of_it_on_screen("the lock screen").await;

    // The switcher finds it by its name.
    s.leave_to_elsewhere().await;
    s.app.key_combo("body", "k", true, false, false).await.expect("Ctrl+K");
    s.app.wait_for(".switcher", DEFAULT_WAIT).await.expect("switcher");
    s.app.type_into(".switcher input", &s.writer.clone()).await.expect("type the name");
    s.app
        .wait_for_text(".switcher .switcher-item", &s.writer.clone(), DEFAULT_WAIT)
        .await
        .expect("found by name");
    s.nothing_of_it_on_screen("the switcher").await;
}

/// A lock outlives the app: after a restart the room is locked, and the file
/// holds a hash, not the password.
#[tokio::test]
async fn a_lock_is_kept_over_a_restart_and_holds_no_password() {
    let hs = hs!();
    let mut s = setup(&hs, "lp", &[]).await;
    s.lock_dm("correct horse").await;
    s.unlock("correct horse").await;
    s.wait_message(true).await;
    let file = s.app.data_dir().join("locks.json");
    let kept = std::fs::read_to_string(&file).expect("locks.json");
    assert!(kept.contains(&s.dm) && kept.contains("$argon2"), "a hash for the room: {kept}");
    assert!(!kept.contains("correct horse"), "never the password: {kept}");

    let (hs2, writer, secret) = (s.hs.clone(), s.writer.clone(), s.secret.clone());
    let mut app = s.app.relaunch().await.expect("relaunch");
    app.wait_for(".room", DEFAULT_WAIT).await.expect("rooms again");
    app.open_room(&writer).await.expect("open the DM");
    app.wait_for(".lock-screen", DEFAULT_WAIT).await.expect("locked after a restart");
    let seen = app.texts("*").await.expect("texts");
    assert!(!seen.iter().any(|t| t.contains(&secret)), "nothing of it shown");
    let _ = hs2;
}

/// Settings > Security > Locked chats: the timeout, 5 minutes unless changed,
/// kept in locks.json.
#[tokio::test]
async fn the_lock_timeout_is_a_setting() {
    let hs = hs!();
    let mut s = setup(&hs, "ls", &[]).await;
    s.app.click(".settings-button").await.expect("gear");
    s.app.wait_for_text(".settings-nav button", "Security", DEFAULT_WAIT).await.expect("settings");
    s.app.click_text(".settings-nav button", "Security").await.expect("Security");
    s.app
        .wait_for(".settings-page input[name=lock-timeout]", DEFAULT_WAIT)
        .await
        .expect("the timeout choices");
    let checked = s.app.checked(".settings-page input[name=lock-timeout]").await.expect("checked");
    assert_eq!(checked, vec![false, true, false, false, false], "5 minutes by default");
    // 15 minutes.
    s.app
        .click_text(".settings-page label.notify-choice", "15 minutes")
        .await
        .expect("choose 15 minutes");
    let file = s.app.data_dir().join("locks.json");
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let kept = std::fs::read_to_string(&file).unwrap_or_default();
        if kept.contains("\"timeout_secs\":900") {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "not kept: {kept}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let checked = s.app.checked(".settings-page input[name=lock-timeout]").await.expect("checked");
    assert_eq!(checked, vec![false, false, true, false, false], "15 minutes shows chosen");
}

/// Someone calling in a locked DM: the card and the system notification say
/// only that a call came in a locked chat (not who), answering works, and
/// the chat's messages stay hidden meanwhile.
#[tokio::test]
async fn a_call_in_a_locked_dm_does_not_name_the_caller() {
    let hs = hs!();
    let mut s = setup(&hs, "lc", &[]).await;
    s.lock_dm("tulip").await;
    s.leave_to_elsewhere().await;
    let before = shown(&s.app).len();

    // The writer calls from a second app (the DM is theirs too, and a room of their own is listed).
    api(&s.writer, &hs, "POST", "createRoom", serde_json::json!({ "name": "caller elsewhere" }));
    api(
        &s.writer,
        &hs,
        "PUT",
        &format!("user/{}/account_data/m.direct", esc(&format!("@{}:localhost", s.writer))),
        serde_json::json!({ format!("@{}:localhost", s.reader): [s.dm] }),
    );
    let mut caller = App::launch(&hs).await.expect("launch the caller");
    login(&mut caller, &s.writer).await;
    caller.click(".rail-dms").await.expect("DMs");
    caller.open_room(&s.reader).await.expect("open the DM");
    caller.wait_for(".chat-header .call-button", DEFAULT_WAIT).await.expect("call button");
    caller.click(".chat-header .call-button").await.expect("call");

    s.app.wait_for(".incoming-call", DEFAULT_WAIT).await.expect("it rings");
    let mut seen = s.app.texts(".incoming-calls *").await.expect("texts");
    seen.extend(s.app.attrs(".incoming-calls *", "title").await.expect("titles").into_iter().flatten());
    assert!(!seen.iter().any(|t| t.contains(&s.writer)), "the card names the caller: {seen:?}");
    assert_eq!(
        s.app.texts(".incoming-call .incoming-call-name").await.expect("name"),
        vec!["Incoming call in a locked chat".to_owned()]
    );
    let deadline = std::time::Instant::now() + DEFAULT_WAIT;
    loop {
        let all = shown(&s.app).into_iter().skip(before).map(|n| n.to_string()).collect::<Vec<_>>();
        assert!(!all.iter().any(|n| n.contains(&s.writer)), "a notification names the caller: {all:?}");
        if all.iter().any(|n| n.contains("Incoming call in a locked chat")) {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "no ring notification: {all:?}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    // Answering works; the chat itself stays behind the lock.
    s.app.click(".incoming-call .accept-call").await.expect("answer");
    s.app
        .wait_for_text(".voice-panel", "Voice connected", DEFAULT_WAIT)
        .await
        .expect("in the call");
    s.app.wait_for(".lock-screen", DEFAULT_WAIT).await.expect("still locked");
    for part in [".timeline", ".message", ".voice-chat", ".composer"] {
        assert_eq!(s.app.count(part).await.expect("count"), 0, "{part} shows while locked");
    }
    s.nothing_of_it_on_screen("the call in a locked chat").await;
}

/// Sends `body` as `user` into `room` (a reply in `root`'s thread when
/// given); the event's ID.
fn send_id(hs: &str, user: &str, room: &str, body: &str, root: Option<&str>) -> String {
    let mut content = serde_json::json!({ "msgtype": "m.text", "body": body });
    if let Some(root) = root {
        content["m.relates_to"] = serde_json::json!({
            "rel_type": "m.thread", "event_id": root, "is_falling_back": true,
            "m.in_reply_to": { "event_id": root }
        });
    }
    let sent = api(user, hs, "PUT", &format!("rooms/{}/send/m.room.message/{}", esc(room), unique()), content);
    sent["event_id"].as_str().expect("event id").to_owned()
}

impl Setup {
    /// Unlocks the DM and opens it; its messages show.
    async fn unlocked_and_open(&mut self, password: &str) {
        self.open_dm().await;
        self.unlock(password).await;
        self.wait_message(true).await;
    }

    /// With the clock at 3 s and the window not in front, the room locks by
    /// itself: nothing of what was open from it stays on screen.
    async fn wait_locked_clean(&mut self, what: &str) {
        self.app
            .wait_for(".lock-screen", Duration::from_secs(20))
            .await
            .unwrap_or_else(|err| panic!("{what}: the room never locked: {err}"));
        for part in [
            ".thread-panel",
            ".pinned-dialog",
            ".search-panel",
            ".reactions-dialog",
            ".lightbox-overlay",
            ".video-viewer",
            ".dialog",
            ".modal",
        ] {
            assert_eq!(self.app.count(part).await.expect("count"), 0, "{what}: {part} is left on screen");
        }
        self.nothing_of_it_on_screen(what).await;
    }
}

/// A thread panel, the pins dialog and the search results opened from the
/// room go when it locks by itself.
#[tokio::test]
async fn a_thread_the_pins_and_search_close_when_the_room_locks() {
    let hs = hs!();
    let mut s = setup(&hs, "lo", &[("MORPHEUS_E2E_LOCK_SECS", "3")]).await;
    let word = format!("kumquat{}", std::process::id());
    let root = send_id(&hs, &s.writer, &s.dm, &format!("{word} root"), None);
    send_id(&hs, &s.writer, &s.dm, &format!("{word} reply in the thread"), Some(&root));
    s.lock_dm("tulip").await;
    s.unlocked_and_open("tulip").await;

    // The thread panel.
    s.app.wait_for_text(".thread-chip", "repl", DEFAULT_WAIT).await.expect("thread chip");
    s.app.click_text(".thread-chip", "repl").await.expect("open the thread");
    s.app.wait_for(".thread-panel", DEFAULT_WAIT).await.expect("thread panel");
    s.wait_locked_clean("a thread panel").await;

    // The search results.
    s.unlock("tulip").await;
    s.wait_message(true).await;
    s.app.click(".chat-header .search-bar input[name=search]").await.expect("search box");
    s.app.type_into(".chat-header input[name=search]", &word).await.expect("type");
    s.app.press(".chat-header input[name=search]", "Enter").await.expect("search");
    s.app.wait_for(".search-panel .search-hit", DEFAULT_WAIT).await.expect("a hit");
    s.wait_locked_clean("the search results").await;

    // The pinned messages.
    s.unlock("tulip").await;
    s.wait_message(true).await;
    s.app.click(".pin-toggle").await.expect("pins");
    s.app.wait_for(".pinned-dialog", DEFAULT_WAIT).await.expect("pins dialog");
    s.wait_locked_clean("the pins dialog").await;
}

/// The reactions viewer and a picture opened full size go when it locks.
#[tokio::test]
async fn the_reactions_viewer_and_the_lightbox_close_when_the_room_locks() {
    let hs = hs!();
    let mut s = setup(&hs, "lp", &[("MORPHEUS_E2E_LOCK_SECS", "3")]).await;
    let word = format!("loquat{}", std::process::id());
    let message = send_id(&hs, &s.writer, &s.dm, &format!("{word} for reactions"), None);
    api(
        &s.reader,
        &hs,
        "PUT",
        &format!("rooms/{}/send/m.reaction/{}", esc(&s.dm), unique()),
        serde_json::json!({ "m.relates_to": { "rel_type": "m.annotation", "event_id": message, "key": "🎉" } }),
    );
    let wide: &[u8] = include_bytes!("../fixtures/wide.png");
    morpheus_e2e::send_image_as(&hs, &s.writer, &s.dm, wide, &format!("{word}.png"), None).expect("a picture");
    s.lock_dm("tulip").await;
    s.unlocked_and_open("tulip").await;

    // The reactions viewer.
    s.app.wait_for_text(".message .body", &word, DEFAULT_WAIT).await.expect("the message");
    s.app.right_click_text(".message .body", &word).await.expect("its menu");
    s.app.click_text(".context-item", "View reactions").await.expect("view");
    s.app.wait_for(".reactions-dialog", DEFAULT_WAIT).await.expect("viewer");
    s.wait_locked_clean("the reactions viewer").await;

    // The picture, full size.
    s.unlock("tulip").await;
    s.wait_message(true).await;
    s.app.reveal_element(".message img.image", DEFAULT_WAIT).await.expect("an image");
    s.app.click(".message img.image").await.expect("open it");
    s.app.wait_for(".lightbox-overlay", DEFAULT_WAIT).await.expect("lightbox");
    s.wait_locked_clean("the lightbox").await;
}

/// How many shared pictures the app holds in memory now.
async fn held(app: &mut App) -> usize {
    app.attr(".shell-body", "data-held-pictures")
        .await
        .expect("held")
        .and_then(|n| n.parse::<usize>().ok())
        .expect("a number of pictures")
}

/// The pictures of a room are held in memory beyond its views (shared
/// copies by media): locking the room drops them, going away does not.
#[tokio::test]
async fn locking_a_room_drops_its_pictures_from_memory() {
    let hs = hs!();
    let mut s = setup(&hs, "lm", &[]).await;
    let wide: &[u8] = include_bytes!("../fixtures/wide.png");
    morpheus_e2e::send_image_as(&hs, &s.writer, &s.dm, wide, "kept in memory.png", None).expect("a picture");
    s.lock_dm("tulip").await;
    s.unlocked_and_open("tulip").await;
    s.app.reveal_element(".message img.image", DEFAULT_WAIT).await.expect("the picture shows");
    let with_it = held(&mut s.app).await;
    assert!(with_it >= 1, "the shown picture is held: {with_it}");

    // Away without a lock: still held (the room is kept for coming back).
    s.leave_to_elsewhere().await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(held(&mut s.app).await >= with_it, "going away dropped it");

    // Locked: gone from the shared copies.
    s.open_dm().await;
    s.app.reveal_element(".message img.image", DEFAULT_WAIT).await.expect("the picture shows again");
    s.dm_menu("Lock now").await;
    s.app.wait_for(".lock-screen", DEFAULT_WAIT).await.expect("locked");
    tokio::time::sleep(Duration::from_millis(500)).await;
    let after = held(&mut s.app).await;
    assert!(after < with_it, "the locked room's picture is still held: {after} of {with_it}");
}
