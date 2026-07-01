use super::Store;
use crate::models::{
    ContactGroupSummary, ContactInput, ContactSummary, DuplicateGroup, ImportReport,
};
use crate::services::vcard::{ImportedContact, ParseResult};
use rusqlite::{params, OptionalExtension, Row};
use std::collections::BTreeMap;

/// contacts の 1 行を ContactSummary に写す（列順は SELECT と対応）。
fn row_to_contact(r: &Row) -> rusqlite::Result<ContactSummary> {
    Ok(ContactSummary {
        id: r.get::<_, i64>(0)? as i32,
        display_name: r.get(1)?,
        name_kana: r.get(2)?,
        email: r.get(3)?,
        phone: r.get(4)?,
        organization: r.get(5)?,
        address: r.get(6)?,
        birthday: r.get(7)?,
        note: r.get(8)?,
        is_favorite: r.get::<_, i64>(9)? != 0,
        is_business: r.get::<_, i64>(10)? != 0,
        allow_remote_images: r.get::<_, i64>(11)? != 0,
    })
}

const CONTACT_COLS: &str = "id, display_name, name_kana, email, phone, organization, \
     address, birthday, note, is_favorite, is_business, allow_remote_images";

impl Store {
    /// 連絡先一覧。`query` があれば名前/よみ/メール/組織を部分一致で絞り込む。
    /// お気に入りを先頭に、次いで よみ→表示名 で並べる。
    pub fn list_contacts(&self, query: Option<&str>) -> rusqlite::Result<Vec<ContactSummary>> {
        let conn = self.conn.lock().unwrap();
        let order = "ORDER BY is_favorite DESC, \
             name_kana COLLATE NOCASE, display_name COLLATE NOCASE";
        match query.map(str::trim).filter(|q| !q.is_empty()) {
            Some(q) => {
                let like = format!("%{}%", q.replace('%', "\\%").replace('_', "\\_"));
                let sql = format!(
                    "SELECT {CONTACT_COLS} FROM contacts \
                     WHERE display_name LIKE ?1 ESCAPE '\\' \
                        OR name_kana    LIKE ?1 ESCAPE '\\' \
                        OR email        LIKE ?1 ESCAPE '\\' \
                        OR organization LIKE ?1 ESCAPE '\\' \
                     {order}"
                );
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt.query_map(params![like], row_to_contact)?;
                rows.collect()
            }
            None => {
                let sql = format!("SELECT {CONTACT_COLS} FROM contacts {order}");
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt.query_map([], row_to_contact)?;
                rows.collect()
            }
        }
    }

    /// 単一の連絡先を取得。
    pub fn get_contact(&self, id: i64) -> rusqlite::Result<ContactSummary> {
        let conn = self.conn.lock().unwrap();
        let sql = format!("SELECT {CONTACT_COLS} FROM contacts WHERE id = ?1");
        conn.query_row(&sql, params![id], row_to_contact)
    }

    /// 連絡先を作成または更新し、確定後の行を返す。`input.id` が None なら新規。
    pub fn upsert_contact(&self, input: &ContactInput) -> rusqlite::Result<ContactSummary> {
        let conn = self.conn.lock().unwrap();
        let id = match input.id {
            Some(id) => {
                conn.execute(
                    "UPDATE contacts SET \
                         display_name = ?1, name_kana = ?2, email = ?3, phone = ?4, \
                         organization = ?5, address = ?6, birthday = ?7, note = ?8, \
                         is_favorite = ?9, is_business = ?10, allow_remote_images = ?11, \
                         updated_at = CURRENT_TIMESTAMP \
                     WHERE id = ?12",
                    params![
                        input.display_name,
                        input.name_kana,
                        input.email,
                        input.phone,
                        input.organization,
                        input.address,
                        input.birthday,
                        input.note,
                        input.is_favorite as i64,
                        input.is_business as i64,
                        input.allow_remote_images as i64,
                        id,
                    ],
                )?;
                id as i64
            }
            None => {
                conn.execute(
                    "INSERT INTO contacts \
                         (display_name, name_kana, email, phone, organization, address, \
                          birthday, note, is_favorite, is_business, allow_remote_images) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                    params![
                        input.display_name,
                        input.name_kana,
                        input.email,
                        input.phone,
                        input.organization,
                        input.address,
                        input.birthday,
                        input.note,
                        input.is_favorite as i64,
                        input.is_business as i64,
                        input.allow_remote_images as i64,
                    ],
                )?;
                conn.last_insert_rowid()
            }
        };
        let sql = format!("SELECT {CONTACT_COLS} FROM contacts WHERE id = ?1");
        conn.query_row(&sql, params![id], row_to_contact)
    }

    /// 連絡先を削除（グループ所属も外れる。ON DELETE CASCADE）。
    pub fn delete_contact(&self, id: i64) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM contacts WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// vCard パース結果を一括取り込み。UID（source+external_id）かメール一致で既存を更新し、
    /// 無ければ新規追加。お気に入り等のユーザーフラグは温存（COALESCE で既存値を消さない）。
    pub fn import_contacts(&self, parsed: &ParseResult) -> rusqlite::Result<ImportReport> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let mut imported = 0i32;
        let mut updated = 0i32;
        {
            for c in &parsed.contacts {
                // 既存 id を探す。安全側に倒し「別人の誤統合」を避ける:
                //  1) UID があれば source+external_id 一致（同期・再取り込み）
                //  2) メール＋表示名の両方一致（代表メール共有の同僚を別人として保つ）
                //  3) 電話＋表示名の両方一致（メール無し連絡先の完全重複を畳む）
                // 同一トランザクション内では直前に INSERT した行も見えるため、
                // ファイル内の完全重複もこの照合で 1 件に集約される。
                let existing: Option<i64> = if let Some(uid) = &c.external_id {
                    tx.query_row(
                        "SELECT id FROM contacts WHERE source = ?1 AND external_id = ?2 LIMIT 1",
                        params![c.source, uid],
                        |r| r.get(0),
                    )
                    .optional()?
                } else if let Some(email) = &c.email {
                    tx.query_row(
                        "SELECT id FROM contacts \
                         WHERE email IS NOT NULL AND lower(email) = lower(?1) \
                           AND display_name = ?2 LIMIT 1",
                        params![email, c.display_name],
                        |r| r.get(0),
                    )
                    .optional()?
                } else if let Some(phone) = &c.phone {
                    tx.query_row(
                        "SELECT id FROM contacts \
                         WHERE email IS NULL AND phone = ?1 AND display_name = ?2 LIMIT 1",
                        params![phone, c.display_name],
                        |r| r.get(0),
                    )
                    .optional()?
                } else {
                    None
                };

                match existing {
                    Some(id) => {
                        update_from_import(&tx, id, c)?;
                        updated += 1;
                    }
                    None => {
                        insert_from_import(&tx, c)?;
                        imported += 1;
                    }
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

    /// 重複候補を「正規化した表示名」でグループ化して返す（2 件以上のみ）。
    /// メール共有の同僚を誤って束ねないよう、メールではなく氏名でまとめて目視レビューに回す。
    /// 件数の多い順 → 名前順。
    pub fn find_duplicate_groups(&self) -> rusqlite::Result<Vec<DuplicateGroup>> {
        let all = self.list_contacts(None)?;
        let mut groups: BTreeMap<String, Vec<ContactSummary>> = BTreeMap::new();
        for c in all {
            let key = normalize_name(&c.display_name);
            if key.is_empty() {
                continue;
            }
            groups.entry(key).or_default().push(c);
        }
        let mut out: Vec<DuplicateGroup> = groups
            .into_values()
            .filter(|v| v.len() > 1)
            .map(|v| DuplicateGroup {
                label: v[0].display_name.clone(),
                contacts: v,
            })
            .collect();
        // 多い順、同数なら見出し名順。
        out.sort_by(|a, b| {
            b.contacts
                .len()
                .cmp(&a.contacts.len())
                .then_with(|| a.label.cmp(&b.label))
        });
        Ok(out)
    }

    /// 複数の連絡先を 1 件（keep_id）に統合する。メール/電話などを寄せ集め、
    /// お気に入り・取引先・外部画像許可は OR で残し、drop 側を削除する。統合後の行を返す。
    pub fn merge_contacts(
        &self,
        keep_id: i64,
        drop_ids: &[i64],
    ) -> rusqlite::Result<ContactSummary> {
        // ロックはこのブロック内に閉じ込め、末尾の get_contact で再ロックして
        // 自己デッドロックしないようにする（Mutex は非再入）。
        {
            let mut conn = self.conn.lock().unwrap();
            let tx = conn.transaction()?;
            {
                // keep と drop の全フィールドを集める。
                let ids: Vec<i64> = std::iter::once(keep_id)
                    .chain(drop_ids.iter().copied())
                    .collect();

                // メール（主＋追加 JSON）を出現順で統合・重複排除。
                let mut emails: Vec<String> = Vec::new();
                let mut push_email = |e: &str| {
                    let e = e.trim().to_string();
                    if !e.is_empty() && !emails.iter().any(|x| x.eq_ignore_ascii_case(&e)) {
                        emails.push(e);
                    }
                };

                // keep を先頭にして順に走査し、空き項目を埋める。フラグは OR。
                let mut phone: Option<String> = None;
                let mut name_kana: Option<String> = None;
                let mut organization: Option<String> = None;
                let mut address: Option<String> = None;
                let mut birthday: Option<String> = None;
                let mut note: Option<String> = None;
                let mut fav = false;
                let mut biz = false;
                let mut remote = false;

                for id in &ids {
                    let row: Option<MergeRow> = tx
                        .query_row(
                            "SELECT email, emails, phone, name_kana, organization, address, \
                                birthday, note, is_favorite, is_business, allow_remote_images \
                         FROM contacts WHERE id = ?1",
                            params![id],
                            |r| {
                                Ok((
                                    r.get(0)?,
                                    r.get(1)?,
                                    r.get(2)?,
                                    r.get(3)?,
                                    r.get(4)?,
                                    r.get(5)?,
                                    r.get(6)?,
                                    r.get(7)?,
                                    r.get(8)?,
                                    r.get(9)?,
                                    r.get(10)?,
                                ))
                            },
                        )
                        .optional()?;
                    let Some((em, ej, ph, kana, org, addr, bday, nt, f, b, rm)) = row else {
                        continue;
                    };
                    if let Some(e) = em {
                        push_email(&e);
                    }
                    if let Some(j) = ej {
                        if let Ok(list) = serde_json::from_str::<Vec<String>>(&j) {
                            for e in list {
                                push_email(&e);
                            }
                        }
                    }
                    phone = phone.or(ph);
                    name_kana = name_kana.or(kana);
                    organization = organization.or(org);
                    address = address.or(addr);
                    birthday = birthday.or(bday);
                    note = note.or(nt);
                    fav |= f != 0;
                    biz |= b != 0;
                    remote |= rm != 0;
                }

                let email = emails.first().cloned();
                let emails_json = if emails.len() > 1 {
                    serde_json::to_string(&emails[1..]).ok()
                } else {
                    None
                };

                tx.execute(
                    "UPDATE contacts SET \
                     email = ?1, emails = ?2, phone = ?3, name_kana = ?4, organization = ?5, \
                     address = ?6, birthday = ?7, note = ?8, \
                     is_favorite = ?9, is_business = ?10, allow_remote_images = ?11, \
                     updated_at = CURRENT_TIMESTAMP \
                 WHERE id = ?12",
                    params![
                        email,
                        emails_json,
                        phone,
                        name_kana,
                        organization,
                        address,
                        birthday,
                        note,
                        fav as i64,
                        biz as i64,
                        remote as i64,
                        keep_id,
                    ],
                )?;

                // drop 側のグループ所属を keep に移し、drop 行を削除。
                for id in drop_ids {
                    tx.execute(
                    "UPDATE OR IGNORE contact_group_members SET contact_id = ?1 WHERE contact_id = ?2",
                    params![keep_id, id],
                )?;
                    tx.execute("DELETE FROM contacts WHERE id = ?1", params![id])?;
                }
            }
            tx.commit()?;
        }
        self.get_contact(keep_id)
    }

    /// 連絡先グループ一覧（所属件数つき、名前順）。
    pub fn list_contact_groups(&self) -> rusqlite::Result<Vec<ContactGroupSummary>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT g.id, g.name, g.color, \
                    (SELECT count(*) FROM contact_group_members m WHERE m.group_id = g.id) AS cnt \
             FROM contact_groups g \
             ORDER BY g.name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(ContactGroupSummary {
                id: r.get::<_, i64>(0)? as i32,
                name: r.get(1)?,
                color: r.get(2)?,
                count: r.get::<_, i64>(3)? as i32,
            })
        })?;
        rows.collect()
    }
}

/// merge_contacts で 1 行分を取り出すタプル（email, emails_json, phone, kana, org,
/// address, birthday, note, is_favorite, is_business, allow_remote_images）。
type MergeRow = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    i64,
    i64,
    i64,
);

/// 表示名を突き合わせ用に正規化（前後空白除去・空白/全角空白を畳む・小文字化）。
fn normalize_name(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join("")
        .replace('\u{3000}', "")
        .to_lowercase()
}

/// インポート 1 件を新規挿入。
fn insert_from_import(tx: &rusqlite::Transaction, c: &ImportedContact) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO contacts \
             (display_name, name_kana, email, emails, phone, organization, address, \
              birthday, note, source, external_id) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            c.display_name,
            c.name_kana,
            c.email,
            c.emails_json,
            c.phone,
            c.organization,
            c.address,
            c.birthday,
            c.note,
            c.source,
            c.external_id,
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::vcard;

    fn store() -> Store {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        super::super::migrations::run(&conn).unwrap();
        Store {
            conn: std::sync::Mutex::new(conn),
            path: std::sync::Mutex::new(std::path::PathBuf::from(":memory:")),
        }
    }

    #[test]
    fn import_dedups_by_email_and_preserves_user_flags() {
        let s = store();

        // 初回取り込み（メールあり）。
        let first = vcard::parse(
            "BEGIN:VCARD\nVERSION:3.0\nFN:山田太郎\nEMAIL:taro@example.com\nORG:旧社名\nEND:VCARD\n",
        );
        let r1 = s.import_contacts(&first).unwrap();
        assert_eq!((r1.total, r1.imported, r1.updated), (1, 1, 0));

        // ユーザーがお気に入り＆取引先に設定。
        let c = s.list_contacts(None).unwrap().remove(0);
        s.upsert_contact(&ContactInput {
            id: Some(c.id),
            display_name: c.display_name.clone(),
            name_kana: None,
            email: c.email.clone(),
            phone: None,
            organization: c.organization.clone(),
            address: None,
            birthday: None,
            note: None,
            is_favorite: true,
            is_business: true,
            allow_remote_images: false,
        })
        .unwrap();

        // 同じメールで再取り込み（組織名が変わり、電話が増えた）。
        let second = vcard::parse(
            "BEGIN:VCARD\nVERSION:3.0\nFN:山田太郎\nEMAIL:taro@example.com\nORG:新社名\nTEL:09011112222\nEND:VCARD\n",
        );
        let r2 = s.import_contacts(&second).unwrap();
        assert_eq!((r2.total, r2.imported, r2.updated), (1, 0, 1));

        // 重複は増えず、フラグは温存、フィールドは更新されている。
        let all = s.list_contacts(None).unwrap();
        assert_eq!(all.len(), 1);
        let c = &all[0];
        assert!(c.is_favorite && c.is_business); // 温存
        assert_eq!(c.organization.as_deref(), Some("新社名")); // 更新
        assert_eq!(c.phone.as_deref(), Some("09011112222")); // 追記
    }

    #[test]
    fn shared_company_email_with_different_names_stays_separate() {
        let s = store();
        // 同じ代表メールを持つ別人 2 名（Google CSV 由来）は別レコードのまま。
        let csv = vcard::parse(
            "BEGIN:VCARD\nVERSION:3.0\nFN:田中一郎\nEMAIL:info@acme.co.jp\nEND:VCARD\n\
             BEGIN:VCARD\nVERSION:3.0\nFN:鈴木花子\nEMAIL:info@acme.co.jp\nEND:VCARD\n",
        );
        let r = s.import_contacts(&csv).unwrap();
        assert_eq!((r.imported, r.updated), (2, 0));
        assert_eq!(s.list_contacts(None).unwrap().len(), 2);
    }

    #[test]
    fn find_duplicates_groups_by_name_and_merge_unions_and_preserves_flags() {
        let s = store();
        // 同名（田中太郎）が 2 件、別メール・別電話。片方だけお気に入り。
        let id_a = s
            .upsert_contact(&ContactInput {
                id: None,
                display_name: "田中太郎".into(),
                name_kana: None,
                email: Some("taro@a.jp".into()),
                phone: Some("090-1111".into()),
                organization: None,
                address: None,
                birthday: None,
                note: None,
                is_favorite: true,
                is_business: false,
                allow_remote_images: false,
            })
            .unwrap()
            .id as i64;
        let id_b = s
            .upsert_contact(&ContactInput {
                id: None,
                display_name: "田中太郎".into(),
                name_kana: Some("タナカタロウ".into()),
                email: Some("taro@b.jp".into()),
                phone: None,
                organization: Some("B社".into()),
                address: None,
                birthday: None,
                note: None,
                is_favorite: false,
                is_business: true,
                allow_remote_images: false,
            })
            .unwrap()
            .id as i64;

        let groups = s.find_duplicate_groups().unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].contacts.len(), 2);

        let merged = s.merge_contacts(id_a, &[id_b]).unwrap();
        assert_eq!(s.list_contacts(None).unwrap().len(), 1);
        assert!(merged.is_favorite && merged.is_business); // OR で温存
        assert_eq!(merged.name_kana.as_deref(), Some("タナカタロウ")); // 空きを補完
        assert_eq!(merged.organization.as_deref(), Some("B社"));
        assert_eq!(merged.email.as_deref(), Some("taro@a.jp")); // keep の主メール
    }

    #[test]
    fn relocate_moves_db_and_updates_path() {
        // 一時フォルダに実ファイル DB を作り、別フォルダへ移動する。
        let root = std::env::temp_dir().join(format!("rondine_reloc_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let old_dir = root.join("old");
        let new_dir = root.join("new");
        let pointer = root.join(".data-location.txt");

        let s = Store::open(&old_dir.join("mail.db")).unwrap();
        let id = s
            .upsert_contact(&ContactInput {
                id: None,
                display_name: "移転 太郎".into(),
                name_kana: None,
                email: Some("a@b.jp".into()),
                phone: None,
                organization: None,
                address: None,
                birthday: None,
                note: None,
                is_favorite: false,
                is_business: false,
                allow_remote_images: false,
            })
            .unwrap()
            .id;

        s.relocate(&new_dir, &pointer).unwrap();

        // パスが新フォルダに更新され、データは無事、旧 mail.db は消えている。
        assert_eq!(s.path(), new_dir.join("mail.db"));
        assert!(new_dir.join("mail.db").exists());
        assert!(!old_dir.join("mail.db").exists());
        let got = s.get_contact(id as i64).unwrap();
        assert_eq!(got.display_name, "移転 太郎");
        // ポインタに新フォルダが記録されている。
        assert_eq!(
            std::fs::read_to_string(&pointer).unwrap().trim(),
            new_dir.to_string_lossy()
        );
        // 移動後も書き込める（接続が新DBへ差し替わっている）。
        s.upsert_contact(&ContactInput {
            id: None,
            display_name: "追加 花子".into(),
            name_kana: None,
            email: None,
            phone: None,
            organization: None,
            address: None,
            birthday: None,
            note: None,
            is_favorite: false,
            is_business: false,
            allow_remote_images: false,
        })
        .unwrap();
        assert_eq!(s.list_contacts(None).unwrap().len(), 2);

        drop(s);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rondine_uid_assigned_on_insert_and_kept_through_merge() {
        let s = store();
        let mk = |name: &str| ContactInput {
            id: None,
            display_name: name.into(),
            name_kana: None,
            email: None,
            phone: None,
            organization: None,
            address: None,
            birthday: None,
            note: None,
            is_favorite: false,
            is_business: false,
            allow_remote_images: false,
        };
        let keep = s.upsert_contact(&mk("同姓同名")).unwrap().id as i64;
        let drop = s.upsert_contact(&mk("同姓同名")).unwrap().id as i64;

        let uid = |id: i64| -> Option<String> {
            let conn = s.conn.lock().unwrap();
            conn.query_row("SELECT uid FROM contacts WHERE id = ?1", [id], |r| r.get(0))
                .unwrap()
        };
        let keep_uid = uid(keep).expect("uid assigned");
        assert!(keep_uid.len() == 36, "UUIDv4 形式");
        assert_ne!(keep_uid, uid(drop).unwrap(), "各行で一意");

        // 統合しても残した側の rondine-id は不変。
        s.merge_contacts(keep, &[drop]).unwrap();
        assert_eq!(uid(keep).as_deref(), Some(keep_uid.as_str()));
    }

    #[test]
    fn import_dedups_by_uid_across_changed_email() {
        let s = store();
        let a = vcard::parse(
            "BEGIN:VCARD\nVERSION:3.0\nPRODID:-//Apple Inc.//iOS//EN\nFN:A\nEMAIL:old@x.jp\nUID:U-1\nEND:VCARD\n",
        );
        s.import_contacts(&a).unwrap();
        // 同じ UID・別メールでも 1 件のまま更新される。
        let b = vcard::parse(
            "BEGIN:VCARD\nVERSION:3.0\nPRODID:-//Apple Inc.//iOS//EN\nFN:A\nEMAIL:new@x.jp\nUID:U-1\nEND:VCARD\n",
        );
        let r = s.import_contacts(&b).unwrap();
        assert_eq!((r.imported, r.updated), (0, 1));
        let all = s.list_contacts(None).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].email.as_deref(), Some("new@x.jp"));
    }
}

/// 既存連絡先へインポート値を反映。新値が NULL の項目は既存を残す（COALESCE）。
/// is_favorite / is_business / allow_remote_images は触らない（ユーザー設定を温存）。
fn update_from_import(
    tx: &rusqlite::Transaction,
    id: i64,
    c: &ImportedContact,
) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE contacts SET \
             display_name = ?1, \
             name_kana    = COALESCE(?2, name_kana), \
             email        = COALESCE(?3, email), \
             emails       = COALESCE(?4, emails), \
             phone        = COALESCE(?5, phone), \
             organization = COALESCE(?6, organization), \
             address      = COALESCE(?7, address), \
             birthday     = COALESCE(?8, birthday), \
             note         = COALESCE(?9, note), \
             source       = ?10, \
             external_id  = COALESCE(?11, external_id), \
             updated_at   = CURRENT_TIMESTAMP \
         WHERE id = ?12",
        params![
            c.display_name,
            c.name_kana,
            c.email,
            c.emails_json,
            c.phone,
            c.organization,
            c.address,
            c.birthday,
            c.note,
            c.source,
            c.external_id,
            id,
        ],
    )?;
    Ok(())
}
