//! Morpheus's private account data (your friends, nicknames and notes) kept
//! so the homeserver cannot read it: each `dev.morpheus.<name>` content is
//! also stored sealed (ChaCha20-Poly1305) as `dev.morpheus.<name>.enc`, under
//! a 32-byte account key that lives in secret storage (SSSS, the same place
//! as the cross-signing keys, under the secret name
//! [`SECRET_NAME`]) and, once a device has it, in a file of the account's
//! folder.
//!
//! Nothing is ever lost on the way:
//! - turning encryption on seals what is there and leaves the plaintext
//!   copies, which devices that do not have the key (or are older) keep
//!   reading and writing;
//! - while a sealed copy exists, a device without the key does not write
//!   (it would be overwritten by a stale plaintext copy on the others);
//!   reading falls back to the plaintext copy, or says "locked" once
//!   [`remove_plaintext_copies`](Session::remove_plaintext_copies) emptied
//!   it, never an empty list;
//! - the plaintext copies are removed only on request, after the sealed ones
//!   are written and read back equal.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chacha20poly1305::{
    ChaCha20Poly1305, Key, KeyInit, Nonce,
    aead::{Aead, Payload},
};
use matrix_sdk::ruma::{
    events::{AnyGlobalAccountDataEventContent, GlobalAccountDataEventType},
    serde::Raw,
};
use rand::RngCore as _;
use serde_json::{Value, json};

use crate::{CoreFuture, Error, Result, Session, people::read_account_data};

/// The secret in secret storage that holds the account key (base64).
pub const SECRET_NAME: &str = "dev.morpheus.account_key";
/// Account data saying the plaintext copies were removed.
const STATE_TYPE: &str = "dev.morpheus.enc.state";
const KEY_FILE: &str = "account_key";
const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

/// What is kept sealed.
pub const KINDS: [&str; 3] = [
    "dev.morpheus.friends",
    "dev.morpheus.nicknames",
    "dev.morpheus.notes",
];

/// Where the sealed copy of `kind` lives.
pub fn sealed_type(kind: &str) -> String {
    format!("{kind}.enc")
}

/// `content` sealed under `key`, bound to `kind`.
pub fn seal(key: &[u8; KEY_LEN], kind: &str, content: &Value) -> Result<Value> {
    let cipher = ChaCha20Poly1305::new(&Key::from(*key));
    let mut nonce = [0u8; NONCE_LEN];
    rand::rng().fill_bytes(&mut nonce);
    let plaintext = serde_json::to_vec(content)?;
    let sealed = cipher
        .encrypt(&Nonce::from(nonce), Payload { msg: &plaintext, aad: kind.as_bytes() })
        .map_err(|_| Error::SessionCrypto)?;
    Ok(json!({
        "v": 1,
        "alg": "chacha20-poly1305",
        "n": STANDARD.encode(nonce),
        "ct": STANDARD.encode(sealed),
    }))
}

/// Whether `content` is a sealed blob.
pub fn is_sealed(content: &Value) -> bool {
    content.get("v").is_some() && content.get("ct").is_some() && content.get("n").is_some()
}

/// The content inside a sealed blob; an error for the wrong key, another
/// kind's blob or a damaged one.
pub fn open(key: &[u8; KEY_LEN], kind: &str, blob: &Value) -> Result<Value> {
    let bytes = |field: &str| {
        blob.get(field)
            .and_then(Value::as_str)
            .and_then(|s| STANDARD.decode(s).ok())
            .ok_or(Error::SessionCrypto)
    };
    if blob.get("v") != Some(&json!(1)) {
        return Err(Error::Other("a newer version of Morpheus sealed this data".into()));
    }
    let nonce: [u8; NONCE_LEN] = bytes("n")?.try_into().map_err(|_| Error::SessionCrypto)?;
    let ciphertext = bytes("ct")?;
    let plaintext = ChaCha20Poly1305::new(&Key::from(*key))
        .decrypt(&Nonce::from(nonce), Payload { msg: &ciphertext, aad: kind.as_bytes() })
        .map_err(|_| Error::SessionCrypto)?;
    Ok(serde_json::from_slice(&plaintext)?)
}

fn load_key(dir: &std::path::Path) -> Option<[u8; KEY_LEN]> {
    let text = crate::fs::read(&dir.join(KEY_FILE)).ok()?;
    let text = String::from_utf8(text).ok()?;
    STANDARD.decode(text.trim()).ok()?.try_into().ok()
}

fn save_key(dir: &std::path::Path, key: &[u8; KEY_LEN]) -> Result<()> {
    let path = dir.join(KEY_FILE);
    crate::fs::write(&path, STANDARD.encode(key).as_bytes())?;
    #[cfg(all(unix, not(target_arch = "wasm32")))]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

async fn write_raw(client: &matrix_sdk::Client, kind: &str, content: &Value) -> Result<()> {
    let raw: Raw<AnyGlobalAccountDataEventContent> =
        Raw::from_json(serde_json::value::to_raw_value(content)?);
    client
        .account()
        .set_account_data_raw(GlobalAccountDataEventType::from(kind), raw)
        .await?;
    Ok(())
}

async fn read_value(client: &matrix_sdk::Client, kind: &str) -> Result<Option<Value>> {
    Ok(read_account_data(client, kind)
        .await?
        .and_then(|raw| raw.deserialize_as::<Value>().ok()))
}

async fn plaintext_removed(client: &matrix_sdk::Client) -> bool {
    read_value(client, STATE_TYPE)
        .await
        .ok()
        .flatten()
        .is_some_and(|s| s.get("plaintext_removed") == Some(&json!(true)))
}

/// The content of `kind` as this device may read it: the sealed copy when
/// it holds the key, else the plaintext copy, else `Locked`.
pub(crate) async fn read_private(
    client: &matrix_sdk::Client,
    dir: &std::path::Path,
    kind: &str,
) -> Result<Option<Value>> {
    let sealed = read_value(client, &sealed_type(kind))
        .await?
        .filter(is_sealed);
    match (sealed, load_key(dir)) {
        (Some(blob), Some(key)) => open(&key, kind, &blob).map(Some),
        (Some(_), None) => {
            if plaintext_removed(client).await {
                Err(Error::PrivateDataLocked)
            } else {
                read_value(client, kind).await
            }
        }
        (None, _) => read_value(client, kind).await,
    }
}

/// Stores `content` as `kind`: sealed (and, until the plaintext copies are
/// removed, in plaintext too) when this device has the key; refused while a
/// sealed copy exists that this device cannot update.
pub(crate) async fn write_private(
    client: &matrix_sdk::Client,
    dir: &std::path::Path,
    kind: &str,
    content: &Value,
) -> Result<()> {
    match load_key(dir) {
        Some(key) => {
            write_raw(client, &sealed_type(kind), &seal(&key, kind, content)?).await?;
            if !plaintext_removed(client).await {
                write_raw(client, kind, content).await?;
            }
            Ok(())
        }
        None => {
            let sealed_exists = read_value(client, &sealed_type(kind))
                .await?
                .is_some_and(|v| is_sealed(&v));
            if sealed_exists {
                return Err(Error::PrivateDataLocked);
            }
            write_raw(client, kind, content).await
        }
    }
}

/// Gets the account key out of secret storage (making it, with `create`,
/// when there is none) and keeps it on this device.
pub(crate) async fn adopt_key(
    client: &matrix_sdk::Client,
    dir: &std::path::Path,
    recovery_key: &str,
    create: bool,
) -> Result<bool> {
    let store = client
        .encryption()
        .secret_storage()
        .open_secret_store(recovery_key.trim())
        .await
        .map_err(|e| Error::Other(e.to_string()))?;
    let name = matrix_sdk::ruma::events::secret::request::SecretName::from(SECRET_NAME);
    let existing = store
        .get_secret(name.clone())
        .await
        .map_err(|e| Error::Other(e.to_string()))?
        .and_then(|s| STANDARD.decode(s.trim()).ok())
        .and_then(|b| <[u8; KEY_LEN]>::try_from(b).ok());
    let key = match existing {
        Some(key) => key,
        None if create => {
            let mut key = [0u8; KEY_LEN];
            rand::rng().fill_bytes(&mut key);
            store
                .put_secret(name, &STANDARD.encode(key))
                .await
                .map_err(|e| Error::Other(e.to_string()))?;
            key
        }
        None => return Ok(false),
    };
    save_key(dir, &key)?;
    Ok(true)
}

/// Seals what each kind holds now (nothing for an empty one).
async fn seal_all(client: &matrix_sdk::Client, dir: &std::path::Path) -> Result<()> {
    let key = load_key(dir).ok_or(Error::PrivateDataLocked)?;
    for kind in KINDS {
        // The readable copy: sealed already, else the plaintext one.
        let Some(content) = read_private(client, dir, kind).await? else {
            continue;
        };
        write_raw(client, &sealed_type(kind), &seal(&key, kind, &content)?).await?;
    }
    Ok(())
}

/// What this device knows about the sealing of the account's data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrivateDataState {
    /// Sealed copies exist (it was turned on, on some device).
    pub sealed: bool,
    /// This device holds the key and can read and write them.
    pub unlocked: bool,
    /// The plaintext copies were emptied.
    pub plaintext_removed: bool,
}

impl Session {
    /// Where sealing stands.
    pub fn private_data_state(&self) -> CoreFuture<PrivateDataState> {
        let (client, dir) = (self.client.clone(), self.dir.clone());
        self.core.run(async move {
            let mut sealed = false;
            for kind in KINDS {
                sealed |= read_value(&client, &sealed_type(kind))
                    .await?
                    .is_some_and(|v| is_sealed(&v));
            }
            Ok(PrivateDataState {
                sealed,
                unlocked: load_key(&dir).is_some(),
                plaintext_removed: plaintext_removed(&client).await,
            })
        })
    }

    /// Turn sealing on (or finish it on this device): the account key is
    /// read from (or put in) secret storage with the recovery key, kept
    /// here, and the friends, nicknames and notes sealed. The plaintext
    /// copies stay until [`Self::remove_plaintext_copies`].
    pub fn enable_private_encryption(&self, recovery_key: String) -> CoreFuture<()> {
        let (client, dir) = (self.client.clone(), self.dir.clone());
        self.core.run(async move {
            adopt_key(&client, &dir, &recovery_key, true).await?;
            seal_all(&client, &dir).await
        })
    }

    /// Empty the plaintext copies, so only the sealed ones are left. Every
    /// kind is sealed and read back first; nothing is emptied otherwise.
    /// Devices without the key see "locked" afterwards until they have
    /// recovered with the recovery key.
    pub fn remove_plaintext_copies(&self) -> CoreFuture<()> {
        let (client, dir) = (self.client.clone(), self.dir.clone());
        self.core.run(async move {
            let key = load_key(&dir).ok_or(Error::PrivateDataLocked)?;
            seal_all(&client, &dir).await?;
            // Read back: each sealed copy opens to what is readable.
            for kind in KINDS {
                if let Some(plain) = read_value(&client, kind).await?
                    && !is_sealed(&plain)
                {
                    let blob = read_value(&client, &sealed_type(kind))
                        .await?
                        .ok_or_else(|| Error::Other("the sealed copy did not arrive".into()))?;
                    if open(&key, kind, &blob)? != plain {
                        return Err(Error::Other(
                            "the sealed copy differs from the data; nothing was removed".into(),
                        ));
                    }
                }
            }
            write_raw(&client, STATE_TYPE, &json!({ "plaintext_removed": true })).await?;
            for kind in KINDS {
                let empty = if kind == "dev.morpheus.friends" {
                    json!({ "users": [] })
                } else {
                    json!({})
                };
                write_raw(&client, kind, &empty).await?;
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sealed_data_opens_only_with_its_key_and_kind() {
        let key = [7u8; KEY_LEN];
        let content = json!({ "@bob:x": "Bobby" });
        let blob = seal(&key, "dev.morpheus.nicknames", &content).expect("seal");
        assert!(is_sealed(&blob));
        assert!(!blob.to_string().contains("Bobby"));
        assert_eq!(open(&key, "dev.morpheus.nicknames", &blob).expect("open"), content);
        assert!(open(&[8u8; KEY_LEN], "dev.morpheus.nicknames", &blob).is_err(), "wrong key");
        assert!(open(&key, "dev.morpheus.notes", &blob).is_err(), "bound to its kind");
        let mut damaged = blob.clone();
        damaged["ct"] = json!("AAAA");
        assert!(open(&key, "dev.morpheus.nicknames", &damaged).is_err());
        let again = seal(&key, "dev.morpheus.nicknames", &content).expect("seal");
        assert_ne!(blob["n"], again["n"], "a fresh nonce each time");
        assert!(!is_sealed(&content));
        assert_eq!(sealed_type("dev.morpheus.notes"), "dev.morpheus.notes.enc");
    }

    #[test]
    fn a_newer_version_is_not_misread() {
        let key = [1u8; KEY_LEN];
        let mut blob = seal(&key, "dev.morpheus.notes", &json!({})).expect("seal");
        blob["v"] = json!(2);
        assert!(open(&key, "dev.morpheus.notes", &blob).is_err());
    }
}
