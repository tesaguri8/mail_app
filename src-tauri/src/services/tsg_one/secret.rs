//! ログインの控え（更新用のトークンとログインした人）の置き場。**OS 金庫（keyring）だけに置く。**
//!
//! アクセストークン（15 分）はメモリにだけ持ち、ここには置かない。平文のファイルには何も書かない。
//! 試験では [`SecretStore`] を差し替え、本物の金庫に触れない。

use serde::{Deserialize, Serialize};

use crate::models::TsgOneUser;

/// 金庫の項目名（サービス名はアプリの識別子）。メールのアカウントはアドレスを項目名にするので、
/// `@` を含まないこの名前とはぶつからない。
pub const KEYRING_ENTRY: &str = "tsg-one";

/// 金庫に置くログインの控え。**`Debug` はトークンを伏せる。**
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedLogin {
    /// 30 日・使い捨て（更新のたびに替わる）。端末の鍵に結ばれている。
    pub refresh_token: String,
    /// 期限（UNIX 時刻のミリ秒）。
    pub refresh_token_expires_at: i64,
    pub user: TsgOneUser,
}

impl std::fmt::Debug for SavedLogin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SavedLogin")
            .field("refresh_token_expires_at", &self.refresh_token_expires_at)
            .field("user", &self.user)
            .finish_non_exhaustive()
    }
}

/// 金庫を読めない・書けない。
#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("OS の資格情報ストアを使えません: {0}")]
    Keyring(#[from] keyring::Error),
    #[error("資格情報ストアの TSG One の控えの形が違います: {0}")]
    Format(#[from] serde_json::Error),
}

/// ログインの控えの置き場。
pub trait SecretStore: Send + Sync {
    /// 控えを読む（無ければ `None`）。
    fn load(&self) -> Result<Option<SavedLogin>, SecretError>;
    /// 控えを置く（前の分は置き換える）。
    fn save(&self, login: &SavedLogin) -> Result<(), SecretError>;
    /// 控えを消す（無くてもよい）。
    fn clear(&self) -> Result<(), SecretError>;
}

/// OS 金庫（Win: Credential Manager / mac: Keychain / Linux: Secret Service）。
pub struct KeyringStore {
    /// サービス名（アプリの識別子 `tesaguri.rondine.dev`）。
    service: String,
}

impl KeyringStore {
    /// アプリの識別子をサービス名にして開く。
    pub fn new(service: &str) -> KeyringStore {
        KeyringStore {
            service: service.to_string(),
        }
    }

    fn entry(&self) -> Result<keyring::Entry, keyring::Error> {
        keyring::Entry::new(&self.service, KEYRING_ENTRY)
    }
}

impl SecretStore for KeyringStore {
    fn load(&self) -> Result<Option<SavedLogin>, SecretError> {
        match self.entry()?.get_password() {
            Ok(json) => Ok(Some(serde_json::from_str(&json)?)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn save(&self, login: &SavedLogin) -> Result<(), SecretError> {
        let json = serde_json::to_string(login)?;
        Ok(self.entry()?.set_password(&json)?)
    }

    fn clear(&self) -> Result<(), SecretError> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

/// 試験用の置き場（メモリだけ）。
#[cfg(test)]
#[derive(Default)]
pub struct MemoryStore(pub std::sync::Mutex<Option<SavedLogin>>);

#[cfg(test)]
impl SecretStore for MemoryStore {
    fn load(&self) -> Result<Option<SavedLogin>, SecretError> {
        Ok(self.0.lock().expect("試験の中だけで使う").clone())
    }

    fn save(&self, login: &SavedLogin) -> Result<(), SecretError> {
        *self.0.lock().expect("試験の中だけで使う") = Some(login.clone());
        Ok(())
    }

    fn clear(&self) -> Result<(), SecretError> {
        *self.0.lock().expect("試験の中だけで使う") = None;
        Ok(())
    }
}
