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

    async fn dm_menu(&mut self, item: &str) {
        let writer = self.writer.clone();
        self.app.right_click_exact(".room.dm .room-name", &writer).await.expect("right-click the DM");
        self.app.wait_for(".context-menu", DEFAULT_WAIT).await.expect("menu");
        self.app
            .click_text(".context-menu .context-item", item)
            .await
            .expect("menu item");
    }

    /// Lock the DM with `password` through its menu.
    async fn lock_dm(&mut self, password: &str) {
        self.dm_menu("Lock with a password").await;
        self.app.wait_for(".lock-dialog", DEFAULT_WAIT).await.expect("the dialog");
        self.app.type_into(".lock-dialog input[name=password]", password).await.expect("password");
        self.app.type_into(".lock-dialog input[name=again]", password).await.expect("again");
        self.app.click(".lock-dialog button[type=submit]").await.expect("set");
        self.app.wait_for_count(".lock-dialog", 0, DEFAULT_WAIT).await.expect("dialog closed");
    }

    async fn unlock(&mut self, password: &str) {
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
            assert!(std::time::Instant::now() < deadline, "the message should show: {shown}");
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
