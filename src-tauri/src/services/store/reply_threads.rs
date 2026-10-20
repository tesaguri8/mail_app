//! 返信のスレッド化に使う、親メールの手がかり（docs/THREADING.md §2・docs/COMPOSE.md）。
//!
//! 返信の In-Reply-To・References は、ふつう親の Message-ID から組む（`references_chain_for`）。
//! ただし 0.1.0-alpha.34 までの送信は Message-ID を付けておらず、自分の送信メールの控えには
//! Message-ID が無い（中継が後から付けた ID は控えに入らない）。そうした親へ返信するときは、
//! 親の In-Reply-To・References から References を組み、親の行（canonical_key）を手がかり
//! （thread_hints）として残す。スレッドの割当（取り込み・再構築）はこの手がかりで親と結ぶ（threads.rs）。

use super::Store;
use rusqlite::{params, OptionalExtension};

/// 返信に付けるスレッドのヘッダと、残すべき手がかり。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ReplyThreading {
    /// In-Reply-To（親の Message-ID。親に無ければ None）。
    pub in_reply_to: Option<String>,
    /// References（祖先 Message-ID を空白区切り・古い順。祖先が 1 件も無ければ None）。
    pub references: Option<String>,
    /// 親の行の canonical_key。親に Message-ID が無く、ヘッダでは親へ戻れないときだけ持つ。
    pub hint_parent_key: Option<String>,
}

/// 親メールの、返信のスレッド化に要る列。
struct Parent {
    message_id: Option<String>,
    in_reply_to: Option<String>,
    references_ids: Option<String>,
    canonical_key: String,
}

/// 空白区切りの ID 列を、重複を除いて古い順のまま足していく。
fn push_ids(chain: &mut Vec<String>, ids: Option<&str>) {
    for id in ids.unwrap_or("").split_whitespace() {
        let id = id.trim_start_matches('<').trim_end_matches('>');
        if !id.is_empty() && !chain.iter().any(|x| x == id) {
            chain.push(id.to_string());
        }
    }
}

impl Store {
    /// 返信元メール（emails.id）から、返信に付けるスレッドのヘッダを組む。
    ///
    /// - 親に Message-ID があれば: In-Reply-To＝親の ID、References＝親の References＋親の ID。
    /// - 無ければ: In-Reply-To は付けず、References＝親の References＋親の In-Reply-To。
    ///   親の行の canonical_key を `hint_parent_key` に入れる（[`Store::put_thread_hint`] で残す）。
    ///
    /// 戻り値: 親が見つからなければ None。
    ///
    /// # Errors
    /// DB の読み取りに失敗したとき。
    pub fn reply_threading(
        &self,
        parent_email_id: i64,
    ) -> rusqlite::Result<Option<ReplyThreading>> {
        let conn = self.read_conn.lock().unwrap();
        let parent = conn
            .query_row(
                "SELECT message_id, in_reply_to, references_ids, canonical_key
                 FROM emails WHERE id = ?1",
                params![parent_email_id],
                |r| {
                    Ok(Parent {
                        message_id: r.get(0)?,
                        in_reply_to: r.get(1)?,
                        references_ids: r.get(2)?,
                        canonical_key: r.get(3)?,
                    })
                },
            )
            .optional()?;
        let Some(p) = parent else {
            return Ok(None);
        };
        let own_id = p
            .message_id
            .as_deref()
            .map(|s| s.trim().trim_start_matches('<').trim_end_matches('>'))
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let mut chain = Vec::new();
        push_ids(&mut chain, p.references_ids.as_deref());
        let threading = match own_id {
            Some(mid) => {
                push_ids(&mut chain, Some(&mid));
                ReplyThreading {
                    in_reply_to: Some(mid),
                    references: Some(chain.join(" ")),
                    hint_parent_key: None,
                }
            }
            None => {
                push_ids(&mut chain, p.in_reply_to.as_deref());
                ReplyThreading {
                    in_reply_to: None,
                    references: (!chain.is_empty()).then(|| chain.join(" ")),
                    hint_parent_key: Some(p.canonical_key),
                }
            }
        };
        Ok(Some(threading))
    }

    /// 送った返信（Message-ID）の親の行（canonical_key）を残す（同じ ID なら上書き）。
    ///
    /// # Errors
    /// DB の書き込みに失敗したとき。
    pub fn put_thread_hint(
        &self,
        account_id: i64,
        message_id: &str,
        parent_key: &str,
    ) -> rusqlite::Result<()> {
        let mid = message_id
            .trim()
            .trim_start_matches('<')
            .trim_end_matches('>');
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO thread_hints (account_id, message_id, parent_key) VALUES (?1, ?2, ?3)
             ON CONFLICT(account_id, message_id) DO UPDATE SET parent_key = excluded.parent_key",
            params![account_id, mid, parent_key],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
