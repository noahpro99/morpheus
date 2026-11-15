//! Memory stays bounded (issue #10): an account in 30 rooms full of people
//! and pictures opens every room, and what the app keeps of them (decoded
//! pictures in GPUI's cache and atlas, its own picture caches, the
//! timelines of rooms left) stays under each cache's bound. The breakdown
//! is printed (`[memory] ...`) and appended to target/memory.txt
//! (MEMORY_LABEL names a run).

#![allow(clippy::expect_used)]

use std::{collections::HashMap, process::Command, time::Duration};

use morpheus_e2e::{App, DEFAULT_WAIT, ScrollTo};
use serde_json::{Value, json};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

const MB: u64 = 1024 * 1024;

/// The client-server API with one login per user (the setup makes hundreds of calls).
struct Api {
    hs: String,
    tokens: HashMap<String, String>,
}

impl Api {
    fn curl(args: &[&str]) -> Value {
        let (body, code) = loop {
            let out = Command::new("curl")
                .args(["-s", "-w", "\n%{http_code}"])
                .args(args)
                .output()
                .expect("curl");
            let text = String::from_utf8_lossy(&out.stdout).into_owned();
            let (body, code) = text.rsplit_once('\n').unwrap_or(("", &text));
            // Joins to one room are rate limited (rc_joins_per_room): wait as told.
            if code == "429" {
                let wait = serde_json::from_str::<Value>(body)
                    .ok()
                    .and_then(|v| v["retry_after_ms"].as_u64());
                std::thread::sleep(Duration::from_millis(wait.unwrap_or(500) + 50));
                continue;
            }
            break (body.to_owned(), code.to_owned());
        };
        let (body, code) = (body.as_str(), code.as_str());
        assert!(
            code.starts_with('2'),
            "curl {:?} failed: {code} {body}",
            args.iter()
                .filter(|a| !a.starts_with("Authorization"))
                .collect::<Vec<_>>(),
        );
        serde_json::from_str(body).unwrap_or(Value::Null)
    }

    fn token(&mut self, user: &str) -> String {
        if let Some(token) = self.tokens.get(user) {
            return token.clone();
        }
        let login = Self::curl(&[
            "-X",
            "POST",
            &format!("{}/_matrix/client/v3/login", self.hs),
            "-d",
            &json!({ "type": "m.login.password", "identifier": { "type": "m.id.user", "user": user }, "password": format!("{user}-password") }).to_string(),
        ]);
        let token = login["access_token"].as_str().expect("token").to_owned();
        self.tokens.insert(user.to_owned(), token.clone());
        token
    }

    fn call(&mut self, user: &str, method: &str, path: &str, body: Value) -> Value {
        let auth = format!("Authorization: Bearer {}", self.token(user));
        let url = format!("{}/_matrix/client/v3/{path}", self.hs);
        Self::curl(&["-X", method, "-H", &auth, &url, "-d", &body.to_string()])
    }

    fn upload(&mut self, user: &str, png: &[u8]) -> String {
        let auth = format!("Authorization: Bearer {}", self.token(user));
        let dir = std::env::temp_dir().join("morpheus-e2e-memory");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let file = dir.join(format!("{user}-{}.png", png.len()));
        std::fs::write(&file, png).expect("write picture");
        let url = format!("{}/_matrix/media/v3/upload?filename=p.png", self.hs);
        let uploaded = Self::curl(&[
            "-X",
            "POST",
            "-H",
            &auth,
            "-H",
            "Content-Type: image/png",
            "--data-binary",
            &format!("@{}", file.display()),
            &url,
        ]);
        let _ = std::fs::remove_file(&file);
        uploaded["content_uri"]
            .as_str()
            .expect("uploaded")
            .to_owned()
    }

    fn logout_all(&mut self) {
        for token in std::mem::take(&mut self.tokens).into_values() {
            let _ = Command::new("curl")
                .args(["-sf", "-X", "POST", "-H"])
                .arg(format!("Authorization: Bearer {token}"))
                .arg(format!("{}/_matrix/client/v3/logout", self.hs))
                .output();
        }
    }
}

/// A smooth picture (small to upload, full size once decoded), different for each `seed`.
fn picture(w: u32, h: u32, seed: u32) -> Vec<u8> {
    let buffer = image::RgbImage::from_fn(w, h, |x, y| {
        image::Rgb([
            ((x * 255 / w + seed * 37) % 256) as u8,
            ((y * 255 / h + seed * 91) % 256) as u8,
            ((seed * 53) % 256) as u8,
        ])
    });
    let mut png = std::io::Cursor::new(Vec::new());
    buffer
        .write_to(&mut png, image::ImageFormat::Png)
        .expect("encode");
    png.into_inner()
}

fn room_path(id: &str) -> String {
    id.replace('!', "%21").replace(':', "%3A")
}

fn held(report: &Value, key: &str) -> (u64, u64) {
    (
        report[key]["entries"].as_u64().unwrap_or(0),
        report[key]["bytes"].as_u64().unwrap_or(0),
    )
}

fn breakdown(report: &Value) -> String {
    let mb = |b: u64| b as f64 / MB as f64;
    let (dn, db) = held(report, "gpui_decoded");
    let (an, ab) = held(report, "any_size");
    let (sn, sb) = held(report, "squared");
    let (wn, wb) = held(report, "drawn");
    format!(
        "rss {:.0} MB (anon {:.0}, lazily freed {:.0}) | gpui decoded {dn} / {:.1} MB | atlas {} tiles ({} colour) / {:.1} MB | drawn {wn} / {:.1} MB | shared {an} / {:.1} MB | cut {sn} / {:.1} MB | card colours {} | blurs {} | timelines {} (weight {}) | video {} | views {} timelines {} members",
        report["rss_kb"].as_u64().unwrap_or(0) as f64 / 1024.0,
        report["anon_kb"].as_u64().unwrap_or(0) as f64 / 1024.0,
        report["lazy_free_kb"].as_u64().unwrap_or(0) as f64 / 1024.0,
        mb(db),
        report["atlas"]["tiles"],
        report["atlas"]["colour_tiles"],
        mb(report["atlas"]["bytes"].as_u64().unwrap_or(0)),
        mb(wb),
        mb(ab),
        mb(sb),
        report["card_colors"],
        report["blurs"],
        report["timelines"],
        report["timeline_weight"],
        report["video_pictures"],
        report["timeline_views"],
        report["member_views"],
    )
}

fn note(line: &str) {
    eprintln!("[memory] {line}");
    let label = std::env::var("MEMORY_LABEL").unwrap_or_default();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/memory.txt");
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        use std::io::Write as _;
        let _ = writeln!(file, "{label} {line}");
    }
}

/// Bounds the app keeps to (see apps/desktop/src/media.rs and shell.rs).
/// Pictures decoded for drawing: the budget plus what one screen shows.
const DECODED_MAX: u64 = 96 * MB;
/// The app's own copies (encoded) of pictures.
const SHARED_MAX: u64 = 24 * MB;
/// Timelines kept: the open room and those just left.
const TIMELINES_MAX: u64 = 10;

#[tokio::test]
async fn many_rooms_and_pictures_keep_the_caches_bounded() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut api = Api {
        hs: hs.clone(),
        tokens: HashMap::new(),
    };
    let me = morpheus_e2e::new_account(&hs, "mem").expect("an account");
    let tag = me.trim_start_matches("mem-").to_owned();
    let tag = &tag[tag.len().saturating_sub(5)..];
    // People with big profile pictures (as uploaded from a phone camera).
    let mut people = Vec::new();
    for i in 0..12u32 {
        let user = morpheus_e2e::new_account(&hs, "memp").expect("a person");
        let url = api.upload(&user, &picture(1024, 1024, 1000 + i));
        api.call(
            &user,
            "PUT",
            &format!("profile/%40{user}%3Alocalhost/avatar_url"),
            json!({ "avatar_url": url }),
        );
        people.push(user);
    }
    const ROOMS: usize = 30;
    let names: Vec<String> = (0..ROOMS)
        .map(|i| format!("mem{tag} room {i:02}"))
        .collect();
    let mut ids = Vec::new();
    for (i, name) in names.iter().enumerate() {
        let created = api.call(
            &me,
            "POST",
            "createRoom",
            json!({ "name": name, "preset": "public_chat" }),
        );
        let id = created["room_id"].as_str().expect("room id").to_owned();
        let room = room_path(&id);
        let avatar = api.upload(&me, &picture(800, 800, i as u32));
        api.call(
            &me,
            "PUT",
            &format!("rooms/{room}/state/m.room.avatar"),
            json!({ "url": avatar }),
        );
        // The first room has everyone in it; the others four people each.
        let members: Vec<String> = if i == 0 {
            people.clone()
        } else {
            (0..4)
                .map(|k| people[(i + k) % people.len()].clone())
                .collect()
        };
        for (k, person) in members.iter().enumerate() {
            api.call(person, "POST", &format!("join/{room}"), json!({}));
            api.call(
                person,
                "PUT",
                &format!("rooms/{room}/send/m.room.message/t{k}"),
                json!({ "msgtype": "m.text", "body": format!("{name} hello from {k}") }),
            );
        }
        for k in 0..3u32 {
            let seed = (i as u32) * 10 + k;
            let url = api.upload(&members[0], &picture(1600, 1200, seed));
            api.call(
                &members[k as usize % members.len()],
                "PUT",
                &format!("rooms/{room}/send/m.room.message/p{k}"),
                json!({ "msgtype": "m.image", "body": format!("{name} picture {k}"), "url": url,
                        "info": { "mimetype": "image/png", "w": 1600, "h": 1200 } }),
            );
        }
        ids.push(id);
    }
    api.logout_all();

    let mut app = App::launch(&hs).await.expect("launch app");
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    app.type_into("form.login input[name=username]", &me)
        .await
        .expect("username");
    app.type_into("form.login input[name=password]", &format!("{me}-password"))
        .await
        .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for_text(".room", &names[ROOMS - 1], Duration::from_secs(60))
        .await
        .expect("the rooms listed");

    tokio::time::sleep(Duration::from_secs(3)).await;
    let start = app.memory().await.expect("memory report");
    note(&format!("signed in, no room open: {}", breakdown(&start)));

    let mut most_decoded = 0u64;
    for (i, name) in names.iter().enumerate() {
        app.open_room(name).await.expect("open the room");
        app.wait_for(".timeline .message", DEFAULT_WAIT)
            .await
            .unwrap_or_else(|e| panic!("{name}'s messages: {e}"));
        // Every picture of the room drawn: from the newest up to the first.
        for k in (0..3).rev() {
            app.reveal_element(
                &format!("img.image[alt=\"{name} picture {k}\"]"),
                DEFAULT_WAIT,
            )
            .await
            .unwrap_or_else(|e| panic!("{name} picture {k} shown: {e}"));
        }
        app.scroll_to(".timeline", ScrollTo::Top)
            .await
            .expect("scroll up");
        tokio::time::sleep(Duration::from_millis(300)).await;
        let report = app.memory().await.expect("memory report");
        most_decoded = most_decoded.max(held(&report, "gpui_decoded").1);
        if i % 10 == 9 {
            note(&format!("after {} rooms: {}", i + 1, breakdown(&report)));
        }
    }
    // Settled: what freed memory the allocator gives back is gone.
    tokio::time::sleep(Duration::from_secs(8)).await;
    let report = app.memory().await.expect("memory report");
    note(&format!("settled: {}", breakdown(&report)));
    if std::env::var("MEMORY_SMAPS").is_ok() {
        let pid = app.pid().expect("pid");
        let smaps = std::fs::read_to_string(format!("/proc/{pid}/smaps")).unwrap_or_default();
        let mut regions: Vec<(u64, String)> = Vec::new();
        let mut head = String::new();
        for line in smaps.lines() {
            if line
                .split_whitespace()
                .next()
                .is_some_and(|f| f.contains('-') && !f.ends_with(':'))
            {
                head = line.to_owned();
            } else if let Some(v) = line.strip_prefix("Anonymous:") {
                let kb: u64 = v
                    .split_whitespace()
                    .next()
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(0);
                if kb > 2048 {
                    regions.push((kb, head.clone()));
                }
            }
        }
        regions.sort_by_key(|r| std::cmp::Reverse(r.0));
        for (kb, head) in regions.iter().take(40) {
            eprintln!("[smaps] {:>7} KB {head}", kb);
        }
        eprintln!(
            "[smaps] {} regions over 2 MB, {} MB",
            regions.len(),
            regions.iter().map(|r| r.0).sum::<u64>() / 1024
        );
    }
    note(&format!(
        "most decoded on the way: {:.1} MB",
        most_decoded as f64 / MB as f64
    ));

    // Rooms opened long ago still open whole, pictures and all.
    app.open_room(&names[0])
        .await
        .expect("back to the first room");
    app.reveal_element(
        &format!("img.image[alt=\"{} picture 0\"]", names[0]),
        DEFAULT_WAIT,
    )
    .await
    .expect("its first picture is drawn again");
    app.wait_for(".members .member img.avatar", DEFAULT_WAIT)
        .await
        .expect("its people's pictures");

    let (_, decoded) = held(&report, "gpui_decoded");
    assert!(
        most_decoded <= DECODED_MAX,
        "GPUI held {:.1} MB of decoded pictures (bound {} MB)",
        most_decoded as f64 / MB as f64,
        DECODED_MAX / MB
    );
    assert!(decoded <= DECODED_MAX, "{decoded} bytes decoded at the end");
    let atlas = report["atlas"]["bytes"].as_u64().unwrap_or(0);
    assert!(
        atlas <= DECODED_MAX + 16 * MB,
        "the atlas holds {:.1} MB",
        atlas as f64 / MB as f64
    );
    let shared = held(&report, "any_size").1 + held(&report, "squared").1;
    assert!(
        shared <= SHARED_MAX,
        "the app's own picture copies hold {:.1} MB",
        shared as f64 / MB as f64
    );
    let timelines = report["timelines"].as_u64().unwrap_or(0);
    assert!(
        timelines <= TIMELINES_MAX,
        "{timelines} timelines kept after visiting {ROOMS} rooms"
    );
    let rss = report["rss_kb"].as_u64().unwrap_or(0) * 1024;
    // Headless e2e build (no GPU memory); 697 MB before issue #10, about 460 after.
    assert!(rss < 600 * MB, "{} MB resident", rss / MB);
    let _ = ids;
}
