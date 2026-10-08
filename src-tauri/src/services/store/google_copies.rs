//! ファイルで取り込んだ Google 連絡先の「写し」の片付け（docs/CONTACTS_SYNC.md §2）。
//!
//! People API の同期を入れる前は、Google の連絡先を CSV／vCard で書き出して取り込んでいた
//! （`source = 'google'`）。これは一度きりの写しで、Google 側の変更は届かない。同期に
//! 置き換えたあとも残っていると、同じ人が「写し」と「同期」の 2 件になる。
//!
//! 片付けの対象は **どの Google アカウントの台帳（`contact_identities`）にも紐付いていない**
//! `source = 'google'` の連絡先だけ。台帳に紐付いた連絡先は同期の対象なので触らない —
//! 紐付いた連絡先を消すと、次の同期で **Google 側の連絡先まで消える**（push が削除を送る）。
//! 紐付いていない連絡先は push の対象外なので、消しても Google には何も送られない。
//!
//! 「住所録へ反映」を先に済ませると、写しのうち高確信で一致したものは台帳に紐付いて
//! 同期の側へ移る（お気に入り・タグ・メモは残る）。残った写しだけがここの対象になる。

use super::Store;
use rusqlite::params;

/// 片付けの対象: ファイルで取り込んだ Google 連絡先のうち、同期とつながっていないもの。
const UNSYNCED_GOOGLE_COPY: &str = "source = 'google' AND deleted_at IS NULL \
     AND NOT EXISTS (SELECT 1 FROM contact_identities ci WHERE ci.contact_id = contacts.id)";

impl Store {
    /// 同期とつながっていない Google の写しの件数を返す。
    ///
    /// # Errors
    /// DB の読み出しに失敗したとき。
    pub fn count_unsynced_google_copies(&self) -> rusqlite::Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            &format!("SELECT count(*) FROM contacts WHERE {UNSYNCED_GOOGLE_COPY}"),
            [],
            |r| r.get(0),
        )
    }

    /// 同期とつながっていない Google の写しをゴミ箱へ移し、移した件数を返す。
    ///
    /// 論理削除（`deleted_at`）なので住所録のゴミ箱から戻せる。`dirty` は立てない —
    /// 台帳に紐付いていないので Google へ送るものが無い。
    ///
    /// # Errors
    /// DB の書き込みに失敗したとき。
    pub fn trash_unsynced_google_copies(&self) -> rusqlite::Result<i64> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            &format!(
                "UPDATE contacts SET deleted_at = CURRENT_TIMESTAMP \
                 WHERE {UNSYNCED_GOOGLE_COPY}"
            ),
            params![],
        )?;
        Ok(n as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 連絡先を 1 件入れて id を返す（出どころだけ指定）。
    fn add_contact(s: &Store, name: &str, source: &str) -> i64 {
        let conn = s.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO contacts (display_name, source) VALUES (?1, ?2)",
            params![name, source],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    /// 台帳に紐付ける（同期でつながった連絡先にする）。
    fn link(s: &Store, contact_id: i64, external_id: &str) {
        let conn = s.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO google_accounts (provider, email) VALUES ('google', 'a@gmail.com') \
             ON CONFLICT DO NOTHING",
            [],
        )
        .unwrap();
        let account_id: i64 = conn
            .query_row("SELECT id FROM google_accounts", [], |r| r.get(0))
            .unwrap();
        conn.execute(
            "INSERT INTO contact_identities (provider, account_id, external_id, contact_id) \
             VALUES ('google', ?1, ?2, ?3)",
            params![account_id, external_id, contact_id],
        )
        .unwrap();
    }

    #[test]
    fn trashes_only_unsynced_google_copies() {
        let s = Store::open_in_memory_for_test();
        let copy = add_contact(&s, "写し", "google");
        let synced = add_contact(&s, "同期", "google");
        let icloud = add_contact(&s, "iCloud", "icloud");
        let local = add_contact(&s, "Rondine", "local");
        link(&s, synced, "people/c1");

        assert_eq!(s.count_unsynced_google_copies().unwrap(), 1);
        assert_eq!(s.trash_unsynced_google_copies().unwrap(), 1);
        assert_eq!(s.count_unsynced_google_copies().unwrap(), 0);

        let deleted = |id: i64| -> bool {
            let conn = s.conn.lock().unwrap();
            conn.query_row(
                "SELECT deleted_at IS NOT NULL FROM contacts WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert!(deleted(copy));
        // 同期でつながった連絡先を消すと Google 側まで消えるので、触らない。
        assert!(!deleted(synced));
        assert!(!deleted(icloud));
        assert!(!deleted(local));
    }

    #[test]
    fn trashing_does_not_queue_a_push() {
        let s = Store::open_in_memory_for_test();
        let copy = add_contact(&s, "写し", "google");
        s.trash_unsynced_google_copies().unwrap();
        let conn = s.conn.lock().unwrap();
        let dirty: i64 = conn
            .query_row("SELECT dirty FROM contacts WHERE id = ?1", params![copy], |r| r.get(0))
            .unwrap();
        assert_eq!(dirty, 0);
    }
}
