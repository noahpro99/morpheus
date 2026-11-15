//! Tab and Shift+Tab move between a form's fields and buttons in visual
//! order, wrap inside a dialog, and Enter on a focused button presses it.

#![allow(clippy::expect_used)]

use std::time::Duration;

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn tab(app: &mut App, shift: bool) {
    app.key_combo("body", "Tab", false, false, shift)
        .await
        .expect("tab");
}

#[tokio::test]
async fn tab_walks_the_login_form_and_enter_presses_the_button() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    app.wait_for("form.login", DEFAULT_WAIT).await.expect("login form");
    let home = "form.login input[name=homeserver]";
    let user = "form.login input[name=username]";
    let pass = "form.login input[name=password]";
    app.wait_for_focus(user, DEFAULT_WAIT).await.expect("username focused");

    // Shift+Tab goes back to the server box, Tab forward again.
    tab(&mut app, true).await;
    app.wait_for_focus(home, DEFAULT_WAIT).await.expect("back to the server");
    tab(&mut app, false).await;
    app.wait_for_focus(user, DEFAULT_WAIT).await.expect("on to the username");

    // Typing lands in the focused field.
    app.keys("body", "alice").await.expect("type name");
    tab(&mut app, false).await;
    app.wait_for_focus(pass, DEFAULT_WAIT).await.expect("password focused");
    app.keys("body", "nope").await.expect("type password");
    assert_eq!(app.value(user).await.expect("user").as_deref(), Some("alice"));
    assert_eq!(app.value(pass).await.expect("pass").as_deref(), Some("nope"));

    // One more Tab reaches the sign-in button; Enter presses it (wrong
    // password: the error shows).
    tab(&mut app, false).await;
    app.key_combo("body", "Enter", false, false, false)
        .await
        .expect("enter");
    app.wait_for("form.login p.error", DEFAULT_WAIT)
        .await
        .expect("signing in was tried");
}

#[tokio::test]
async fn tab_wraps_inside_a_dialog_and_enter_presses_a_button() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    app.wait_for("form.login", DEFAULT_WAIT).await.expect("login form");
    app.type_into("form.login input[name=username]", "alice").await.expect("user");
    app.type_into("form.login input[name=password]", "alice-password").await.expect("pass");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".room", DEFAULT_WAIT).await.expect("rooms");
    app.open_room("general").await.expect("open general");

    app.click(".create-room-button").await.expect("plus");
    let name = ".create-room-dialog input[name=name]";
    let topic = ".create-room-dialog input[name=topic]";
    app.wait_for_focus(name, DEFAULT_WAIT).await.expect("name focused");
    tab(&mut app, false).await;
    app.wait_for_focus(topic, DEFAULT_WAIT).await.expect("topic next");
    tab(&mut app, true).await;
    app.wait_for_focus(name, DEFAULT_WAIT).await.expect("name again");

    // Topic, Create and Cancel, then round to the name: nothing behind the dialog.
    for _ in 0..4 {
        tab(&mut app, false).await;
    }
    app.wait_for_focus(name, Duration::from_secs(3)).await.expect("wrapped to the name");

    // Backwards from the name wraps to Cancel; Enter presses it.
    tab(&mut app, true).await;
    app.key_combo("body", "Enter", false, false, false)
        .await
        .expect("enter");
    app.wait_for_count(".create-room-dialog", 0, DEFAULT_WAIT)
        .await
        .expect("cancel closed the dialog");
}
