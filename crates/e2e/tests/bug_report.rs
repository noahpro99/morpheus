//! Settings → Help & Feedback → Report a bug opens a new GitHub issue in
//! the bug template, filled in with this version, the device and the end of
//! this run's log (the person sees it all before posting).

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

#[tokio::test]
async fn report_a_bug_opens_a_filled_in_github_issue() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch");
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    app.type_into("form.login input[name=username]", "alice")
        .await
        .expect("username");
    app.type_into("form.login input[name=password]", "alice-password")
        .await
        .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".shell", DEFAULT_WAIT).await.expect("shell");

    app.click(".settings-button").await.expect("settings");
    app.wait_for(".settings-nav", DEFAULT_WAIT)
        .await
        .expect("settings open");
    app.click_text(".settings-nav button", "Help & Feedback")
        .await
        .expect("Help & Feedback in settings");
    app.wait_for(".settings-page .report-bug", DEFAULT_WAIT)
        .await
        .expect("Report a bug on the page");
    app.click(".settings-page .report-bug")
        .await
        .expect("Report a bug");
    app.wait_for(".link-log li", DEFAULT_WAIT)
        .await
        .expect("a link was opened");

    let report = app
        .eval(
            "const u = new URL([...document.querySelectorAll('.link-log li')].pop().textContent);\
             return { at: u.origin + u.pathname, template: u.searchParams.get('template'),\
               version: u.searchParams.get('version'), device: u.searchParams.get('device'),\
               logs: u.searchParams.get('logs') };",
        )
        .await
        .expect("read the link");
    assert_eq!(
        report["at"], "https://github.com/noahpro99/morpheus/issues/new",
        "a new issue on GitHub: {report}"
    );
    assert_eq!(report["template"], "bug_report.yml", "{report}");
    assert_eq!(report["version"], env!("CARGO_PKG_VERSION"), "{report}");
    let device = report["device"].as_str().unwrap_or_default();
    assert!(
        device.contains("Linux") && device.contains("x86_64"),
        "the device it runs on: {device:?}"
    );
    // The end of this run's log, as written to morpheus.log.
    let log = std::fs::read_to_string(app.data_dir().join("morpheus.log")).expect("the log file");
    let logs = report["logs"].as_str().unwrap_or_default();
    assert!(
        !logs.is_empty() && log.contains(logs.lines().last().unwrap_or("-")),
        "logs from this run: {logs:?}"
    );
    assert!(
        logs.len() < 6000,
        "kept to what fits in a link ({} chars)",
        logs.len()
    );
}

/// The Help button (top right) offers it too, beside the shortcuts.
#[tokio::test]
async fn the_help_dialog_reports_a_bug_too() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch");
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    app.type_into("form.login input[name=username]", "alice")
        .await
        .expect("username");
    app.type_into("form.login input[name=password]", "alice-password")
        .await
        .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".shell", DEFAULT_WAIT).await.expect("shell");

    app.click(".help-button").await.expect("Help");
    app.wait_for(".help-dialog .report-bug", DEFAULT_WAIT)
        .await
        .expect("Report a bug in the Help dialog");
    app.click(".help-dialog .report-bug")
        .await
        .expect("click it");
    app.wait_for(".link-log li", DEFAULT_WAIT)
        .await
        .expect("a link was opened");
    let at = app
        .eval("const u = new URL([...document.querySelectorAll('.link-log li')].pop().textContent); return u.origin + u.pathname + ' ' + u.searchParams.get('template');")
        .await
        .expect("read the link");
    assert_eq!(
        at, "https://github.com/noahpro99/morpheus/issues/new bug_report.yml",
        "the same filled-in issue"
    );
}
