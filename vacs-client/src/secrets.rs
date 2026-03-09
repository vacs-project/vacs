pub mod cookies;

use anyhow::Context;
use base64::prelude::*;
use keyring::Entry;
use keyring::error::Error::NoEntry;

pub enum SecretKey {
    CookieStoreEncryptionKey,
}

impl SecretKey {
    pub const fn as_str(&self) -> &'static str {
        match self {
            SecretKey::CookieStoreEncryptionKey => "cookie-store-encryption-key",
        }
    }
}

#[allow(dead_code)]
pub fn get(key: SecretKey) -> anyhow::Result<Option<String>> {
    match entry_for_key(key)?.get_password() {
        Ok(password) => Ok(Some(password)),
        Err(NoEntry) => Ok(None),
        Err(err) => Err(anyhow::anyhow!(err).context("Failed to get password")),
    }
}

pub fn get_binary(key: SecretKey) -> anyhow::Result<Option<Vec<u8>>> {
    match entry_for_key(key)?.get_password() {
        Ok(password) => Ok(Some(
            BASE64_STANDARD
                .decode(&password)
                .context("Failed to decode password")?,
        )),
        Err(NoEntry) => Ok(None),
        Err(err) => Err(anyhow::anyhow!(err).context("Failed to get password")),
    }
}

#[allow(dead_code)]
pub fn set(key: SecretKey, value: &str) -> anyhow::Result<()> {
    entry_for_key(key)?
        .set_password(value)
        .context("Failed to set password")
}

pub fn set_binary(key: SecretKey, value: &[u8]) -> anyhow::Result<()> {
    entry_for_key(key)?
        .set_password(BASE64_STANDARD.encode(value).as_str())
        .context("Failed to set password")
}

pub fn remove(key: SecretKey) -> anyhow::Result<()> {
    match entry_for_key(key)?.delete_credential() {
        Ok(()) => Ok(()),
        Err(NoEntry) => Ok(()),
        Err(err) => Err(anyhow::anyhow!(err).context("Failed to delete credential")),
    }
}

fn entry_for_key(key: SecretKey) -> anyhow::Result<Entry> {
    Entry::new(env!("CARGO_PKG_NAME"), key.as_str()).context("Failed to create entry")
}
