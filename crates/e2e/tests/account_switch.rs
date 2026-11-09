//! Several accounts on one device: "Switch account" lists the saved ones
//! and "Add an account" signs another in without signing out; switching
//! needs no password; the choice survives a restart; signing out of one
//! moves to the next.

#![allow(clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

fn homeserver() -> Option<String> {
    std::env::var("MORPHEUS_TEST_HOMESERVER").ok()
}

async fn sign_in(app: &mut App, user: &str) {
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    app.type_into("form.login input[name=username]", user)
        .await
        .expect("username");
    app.type_into(
        "form.login input[name=password]",
        &format!("{user}-password"),
    )
    .await
    .expect("password");
    app.submit("form.login").await.expect("submit");
    signed_in_as(app, user).await;
}

async fn signed_in_as(app: &mut App, user: &str) {
    app.wait_for(
        &format!(".sidebar-footer .profile-chip[title=\"@{user}:localhost\"]"),
        DEFAULT_WAIT,
    )
    .await
    .unwrap_or_else(|e| panic!("signed in as {user}: {e}"));
}

async fn account_menu(app: &mut App) {
    app.click(".profile-chip").await.expect("chip");
    app.wait_for(".context-menu", DEFAULT_WAIT)
        .await
        .expect("account menu");
}

/// The labels in the "Switch account" submenu, the ticked one marked `*`.
async fn switch_items(app: &mut App) -> Vec<String> {
    // A phone's sheet shows a submenu once its row is tapped.
    if app.is_phone() {
        app.click_text(".context-menu.sheet .context-item", "Switch account")
            .await
            .expect("open the submenu");
        app.wait_for(".context-menu.sheet .sheet-back", DEFAULT_WAIT)
            .await
            .expect("submenu shown");
        let items = app
            .eval("return [...document.querySelectorAll('.context-menu.sheet .sheet-items .context-item')].map(e => (e.classList.contains('checked') ? '*' : '') + e.textContent.trim());")
            .await
            .expect("submenu");
        // Back to the top level, where a desktop menu would still be.
        app.click(".context-menu.sheet .sheet-back")
            .await
            .expect("back");
        return serde_json::from_value(items).expect("a Switch account submenu");
    }
    let items = app
        .eval(
            "const p = [...document.querySelectorAll('.context-item.has-submenu')].find(el => el.querySelector('.menu-label')?.textContent === 'Switch account'); \
             if (!p) return null; \
             return [...p.querySelectorAll('.context-submenu .context-item')].map(e => (e.classList.contains('checked') ? '*' : '') + e.textContent.trim());",
        )
        .await
        .expect("submenu");
    serde_json::from_value(items).expect("a Switch account submenu")
}

async fn pick(app: &mut App, label: &str) {
    app.click_submenu("Switch account", label)
        .await
        .expect("pick");
}

#[tokio::test]
async fn add_switch_restart_and_sign_out_of_accounts() {
    let Some(hs) = homeserver() else {
        eprintln!("MORPHEUS_TEST_HOMESERVER not set; skipping e2e test");
        return;
    };
    let mut app = App::launch(&hs).await.expect("launch app");
    sign_in(&mut app, "alice").await;

    // One account: the submenu offers only adding another.
    account_menu(&mut app).await;
    let items = switch_items(&mut app).await;
    assert_eq!(
        items.last().map(String::as_str),
        Some("Add an account"),
        "{items:?}"
    );

    // Adding one shows the login screen, with a way back.
    pick(&mut app, "Add an account").await;
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form for the new account");
    app.wait_for_text("form.login button", "Back to", DEFAULT_WAIT)
        .await
        .expect("a way back to the signed-in account");
    sign_in(&mut app, "bob").await;

    // Both listed, bob ticked; switching to alice needs no password.
    account_menu(&mut app).await;
    let items = switch_items(&mut app).await;
    assert!(
        items
            .iter()
            .any(|i| i.starts_with('*') && i.contains("@bob:localhost")),
        "{items:?}"
    );
    assert!(
        items
            .iter()
            .any(|i| !i.starts_with('*') && i.contains("@alice:localhost")),
        "{items:?}"
    );
    pick(&mut app, "@alice:localhost").await;
    signed_in_as(&mut app, "alice").await;
    assert_eq!(
        app.count("form.login").await.expect("count"),
        0,
        "no login form"
    );
    app.wait_for(".room", DEFAULT_WAIT)
        .await
        .expect("alice's rooms");

    // A restart comes back to alice.
    let mut app = app.relaunch().await.expect("relaunch");
    signed_in_as(&mut app, "alice").await;

    // Signing out of alice moves to bob; signing out of bob ends at login.
    account_menu(&mut app).await;
    app.click_text(".context-item", "Sign out")
        .await
        .expect("sign out alice");
    app.wait_for(".confirm-dialog button.danger", DEFAULT_WAIT)
        .await
        .expect("asked");
    app.click(".confirm-dialog button.danger")
        .await
        .expect("confirm");
    signed_in_as(&mut app, "bob").await;
    account_menu(&mut app).await;
    let items = switch_items(&mut app).await;
    assert!(
        !items.iter().any(|i| i.contains("@alice")),
        "alice gone: {items:?}"
    );
    app.click_text(".context-item", "Sign out")
        .await
        .expect("sign out bob");
    app.wait_for(".confirm-dialog button.danger", DEFAULT_WAIT)
        .await
        .expect("asked");
    app.click(".confirm-dialog button.danger")
        .await
        .expect("confirm");
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("back at the login form");
    assert_eq!(
        app.count("form.login button.back-to-account")
            .await
            .expect("count"),
        0,
        "no way back when no account is left"
    );
}
