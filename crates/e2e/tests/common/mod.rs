//! Helpers shared by test files (`mod common;`), so no file has to include
//! another test file.

#![allow(dead_code, clippy::expect_used)]

use morpheus_e2e::{App, DEFAULT_WAIT};

/// Sign in through the login form and wait for the room list.
pub async fn login(app: &mut App, user: &str, password: &str) {
    app.wait_for("form.login", DEFAULT_WAIT)
        .await
        .expect("login form");
    app.type_into("form.login input[name=username]", user)
        .await
        .expect("username");
    app.type_into("form.login input[name=password]", password)
        .await
        .expect("password");
    app.submit("form.login").await.expect("submit");
    app.wait_for(".room", DEFAULT_WAIT)
        .await
        .expect("rooms listed");
}
