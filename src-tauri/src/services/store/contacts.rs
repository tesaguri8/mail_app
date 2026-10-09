//! 住所録（連絡先）の読み書き。docs/CONTACT_MODEL.md。
//!
//! 行の読み出しは `contact_rows`、書き込みは `contact_write` に分け、ここは画面とファイル取り込みが
//! 使う操作（一覧・取得・保存・削除・取り込み）をまとめる。重複整理は `contact_dedupe`、
//! 組織カードは `organizations` / `org_tidy`、Google との同期は `contact_sync`。

use super::contact_rows::{load_all_full, load_contact, query_summaries, SUMMARY_ORDER};
use super::contact_tags::{add_tags, set_tags};
use super::contact_write::{mark_dirty, write_contact, OrgLinking, WriteOptions};
use super::Store;
use crate::models::{ContactFields, ContactInput, ContactListItem, ContactSummary, ImportReport};
use crate::services::contact_fields::fill_from_import;
use crate::services::name_norm::match_rank;
use crate::services::vcard::ParseResult;
use rusqlite::{params, Connection, OptionalExtension, ToSql};

/// あいまいヒットとして拾う最大件数（誤ヒットが末尾に大量に並ぶのを防ぐ上限）。
const FUZZY_LIMIT: usize = 30;

/// 事前に SQL で絞り込み・整列済みの `rows` を、検索語 `query` でさらに絞る。
///
/// 二段構え:
/// 1. **正規化部分一致**で名前/よみ/主メール/主の会社名を照合。異体字・カタカナ/ひらがな・
///    全角/半角・空白の違いを吸収する。入力順（お気に入り→よみ→表示名）を保つ。
/// 2. 直接ヒットしなかった連絡先のうち、名前/よみが検索語に**編集距離で近い**ものを補助的に
///    拾い、近い順に 1. の後ろへ付ける（斎藤↔斉藤 のような紛らわしい別字・打ち間違い向け）。
fn filter_contacts_by_query(rows: Vec<ContactSummary>, query: &str) -> Vec<ContactSummary> {
    let mut direct: Vec<ContactSummary> = Vec::new();
    let mut fuzzy: Vec<(usize, ContactSummary)> = Vec::new();
    for c in rows {
        let name = c.fields.display_name.as_str();
        let kana = c.sort_name.as_deref().unwrap_or_default();
        let email = c.primary_email.as_deref().unwrap_or_default();
        let org = c.primary_organization.as_deref().unwrap_or_default();
        match match_rank(query, &[name, kana, email, org], &[name, kana]) {
            Some(0) => direct.push(c),
            Some(d) => fuzzy.push((d, c)),
            None => {}
        }
    }
    // あいまいヒットは近い順（同距離なら元の整列＝よみ順を保つ安定ソート）に上限まで採る。
    fuzzy.sort_by_key(|(d, _)| *d);
    direct.extend(fuzzy.into_iter().take(FUZZY_LIMIT).map(|(_, c)| c));
    direct
}

/// ファイル取り込みの既存照合: メール＋表示名、無ければ電話＋表示名が一致する連絡先。
///
/// 安全側に倒し「別人の誤統合」を避ける（代表メール共有の同僚を別人として保つ）。同一
/// トランザクション内では直前に入れた行も見えるので、ファイル内の完全重複も 1 件に集約される。
fn find_import_target(conn: &Connection, c: &ContactFields) -> rusqlite::Result<Option<i64>> {
    let name = c.display_name.trim();
    if let Some(email) = c.emails.first() {
        return conn
            .query_row(
                "SELECT c.id FROM contacts c JOIN contact_emails ce ON ce.contact_id = c.id \
                 WHERE lower(ce.value) = lower(?1) AND c.display_name = ?2 \
                   AND c.deleted_at IS NULL LIMIT 1",
                params![email.value.trim(), name],
                |r| r.get(0),
            )
            .optional();
    }
    if let Some(phone) = c.phones.first() {
        return conn
            .query_row(
                "SELECT c.id FROM contacts c JOIN contact_phones cp ON cp.contact_id = c.id \
                 WHERE cp.value = ?1 AND c.display_name = ?2 AND c.deleted_at IS NULL \
                   AND NOT EXISTS (SELECT 1 FROM contact_emails ce WHERE ce.contact_id = c.id) \
                 LIMIT 1",
                params![phone.value.trim(), name],
                |r| r.get(0),
            )
            .optional();
    }
    Ok(None)
}

impl Store {
    /// 連絡先一覧（子テーブルは空・主値とつながりつき）。
    ///
    /// - `query`: 名前/よみ/主メール/主の会社名で絞り込む（異体字・表記ゆれを吸収。
    ///   [`filter_contacts_by_query`]）
    /// - `groups`: 非空なら、いずれかのタグを持つ連絡先に絞る（OR）
    /// - `include_deleted`: true なら論理削除済みも含める
    ///
    /// 参照専用の接続で読むので、同期などの書き込みが走っていても待たされない。
    ///
    /// # Errors
    /// DB の読み出しに失敗したとき。
    pub fn list_contacts(
        &self,
        query: Option<&str>,
        groups: &[i64],
        include_deleted: bool,
    ) -> rusqlite::Result<Vec<ContactSummary>> {
        let mut conds: Vec<String> = Vec::new();
        if !include_deleted {
            conds.push("c.deleted_at IS NULL".to_string());
        }
        if !groups.is_empty() {
            let holders = (1..=groups.len())
                .map(|n| format!("?{n}"))
                .collect::<Vec<_>>()
                .join(", ");
            conds.push(format!(
                "EXISTS (SELECT 1 FROM contact_tags ct \
                 WHERE ct.contact_id = c.id AND ct.tag_id IN ({holders}))"
            ));
        }
        let binds: Vec<&dyn ToSql> = groups.iter().map(|g| g as &dyn ToSql).collect();
        let rows = {
            let conn = self.read_conn.lock().unwrap();
            query_summaries(&conn, &conds.join(" AND "), SUMMARY_ORDER, &binds)?
        };
        Ok(match query.map(str::trim).filter(|q| !q.is_empty()) {
            Some(q) => filter_contacts_by_query(rows, q),
            None => rows,
        })
    }

    /// 連絡先一覧を、一覧に出す分だけの軽い形で返す（連絡先タブの一覧用）。
    /// 引数と絞り込みは [`Store::list_contacts`] と同じ。
    ///
    /// # Errors
    /// DB の読み出しに失敗したとき。
    pub fn list_contact_items(
        &self,
        query: Option<&str>,
        groups: &[i64],
        include_deleted: bool,
    ) -> rusqlite::Result<Vec<ContactListItem>> {
        Ok(self
            .list_contacts(query, groups, include_deleted)?
            .into_iter()
            .map(ContactListItem::from)
            .collect())
    }

    /// 書き出す連絡先の中身（子テーブル・タグまで）。ゴミ箱は含めない。
    ///
    /// `ids` が Some ならその人だけ（一覧で絞り込んでいる分）、None なら全員。並びは一覧と同じ。
    /// 参照専用の接続で読むので、同期の書き込みに待たされない。
    ///
    /// # Errors
    /// DB の読み出しに失敗したとき。
    pub fn contacts_for_export(&self, ids: Option<&[i64]>) -> rusqlite::Result<Vec<ContactFields>> {
        let all = {
            let conn = self.read_conn.lock().unwrap();
            load_all_full(&conn)?
        };
        let wanted: Option<std::collections::HashSet<i64>> =
            ids.map(|ids| ids.iter().copied().collect());
        Ok(all
            .into_iter()
            .filter(|c| wanted.as_ref().map_or(true, |w| w.contains(&(c.id as i64))))
            .map(|c| c.fields)
            .collect())
    }

    /// 1 人の連絡先を、子テーブル・タグ・つながりまで充填して返す。
    ///
    /// # Errors
    /// 該当が無いとき（`QueryReturnedNoRows`）や DB の読み出しに失敗したとき。
    pub fn get_contact(&self, id: i64) -> rusqlite::Result<ContactSummary> {
        let conn = self.conn.lock().unwrap();
        load_contact(&conn, id)
    }

    /// 指定メールアドレスを持つ（非削除の）連絡先を返す（一覧用の形）。メールアドレスの
    /// ＋/編集 アイコン切替と重複数の表示に使う。小文字の完全一致（式索引）。
    ///
    /// # Errors
    /// DB の読み出しに失敗したとき。
    pub fn lookup_contacts_by_email(&self, email: &str) -> rusqlite::Result<Vec<ContactSummary>> {
        let addr = email.trim();
        if addr.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.conn.lock().unwrap();
        query_summaries(
            &conn,
            "c.deleted_at IS NULL AND EXISTS (SELECT 1 FROM contact_emails ce \
             WHERE ce.contact_id = c.id AND lower(ce.value) = lower(?1))",
            SUMMARY_ORDER,
            &[&addr],
        )
    }

    /// 連絡先を作成または更新し、確定後の連絡先を返す（編集画面の保存）。
    ///
    /// 配列・タグは送られたもので置き換える。会社は候補から選べば（`org_id`）そのカードへ、
    /// 新しく入れた名前はカードが無ければ作ってつなぐ。つながっている全サービスへ送る印を立てる。
    ///
    /// # Errors
    /// DB の書き込みに失敗したとき。
    pub fn upsert_contact(&self, input: &ContactInput) -> rusqlite::Result<ContactSummary> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let id = write_contact(
            &tx,
            input.id.map(i64::from),
            &input.fields,
            WriteOptions {
                mark_dirty: true,
                org_linking: OrgLinking::Editor,
            },
        )?;
        set_tags(&tx, id, &input.fields.tags)?;
        tx.commit()?;
        load_contact(&conn, id)
    }

    /// 連絡先を論理削除する（ゴミ箱へ。保持期間後に完全削除）。つながっている全サービスから
    /// 次の同期で削除する。
    ///
    /// # Errors
    /// DB の書き込みに失敗したとき。
    pub fn delete_contact(&self, id: i64) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE contacts SET deleted_at = CURRENT_TIMESTAMP WHERE id = ?1",
            params![id],
        )?;
        mark_dirty(&conn, id)
    }

    /// 論理削除した連絡先を復元する。向こう側は消えている可能性があるので、次の同期で送り直す。
    ///
    /// # Errors
    /// DB の書き込みに失敗したとき。
    pub fn restore_contact(&self, id: i64) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE contacts SET deleted_at = NULL WHERE id = ?1",
            params![id],
        )?;
        mark_dirty(&conn, id)
    }

    /// 保持期間（日数）を過ぎたゴミ箱を完全削除する（連絡先・組織）。起動時などに呼ぶ。
    ///
    /// # Errors
    /// DB の書き込みに失敗したとき。
    pub fn purge_expired_trash(&self, retention_days: i64) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        let cutoff = format!("-{} days", retention_days.max(0));
        conn.execute(
            "DELETE FROM contacts WHERE deleted_at IS NOT NULL \
             AND deleted_at <= datetime('now', ?1)",
            params![cutoff],
        )?;
        conn.execute(
            "DELETE FROM organizations WHERE deleted_at IS NOT NULL \
             AND deleted_at <= datetime('now', ?1)",
            params![cutoff],
        )?;
        Ok(())
    }

    /// ファイル（vCard / Google CSV）のパース結果を一括取り込みする。
    ///
    /// 既存（メール＋表示名、または電話＋表示名が一致）があれば入ってきた値で埋めて更新し、
    /// 無ければ新規に作る。取り込んだ連絡先はどのサービスにもつながらない（Rondine の連絡先）。
    /// 会社は既存の組織カードと正規化名が一致したときだけつなぐ（カードは作らない）。
    ///
    /// # Errors
    /// DB の書き込みに失敗したとき（全体を巻き戻す）。
    pub fn import_contacts(&self, parsed: &ParseResult) -> rusqlite::Result<ImportReport> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let mut imported = 0i32;
        let mut updated = 0i32;
        let opts = WriteOptions {
            mark_dirty: true,
            org_linking: OrgLinking::ExistingOnly,
        };
        for c in &parsed.contacts {
            match find_import_target(&tx, c)? {
                Some(id) => {
                    let existing = load_contact(&tx, id)?.fields;
                    let merged = fill_from_import(&existing, c);
                    write_contact(&tx, Some(id), &merged, opts)?;
                    add_tags(&tx, id, &merged.tags)?;
                    updated += 1;
                }
                None => {
                    let id = write_contact(&tx, None, c, opts)?;
                    add_tags(&tx, id, &c.tags)?;
                    imported += 1;
                }
            }
        }
        tx.commit()?;
        Ok(ImportReport {
            total: parsed.total_cards as i32,
            imported,
            updated,
            skipped: parsed.total_cards as i32 - parsed.contacts.len() as i32,
        })
    }
}

#[cfg(test)]
mod tests;
