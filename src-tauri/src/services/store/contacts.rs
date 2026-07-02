use super::Store;
use crate::models::{
    ContactAddress, ContactAddressInput, ContactGroupSummary, ContactInput, ContactSummary,
    ContactValue, ContactValueInput, DuplicateGroup, ImportReport,
};
use crate::services::vcard::{ImportedContact, ParseResult};
use rusqlite::{params, Connection, OptionalExtension, Row};

/// contacts の 1 行を ContactSummary に写す（列順は CONTACT_COLS と対応）。
/// 複数値（emails/phones/addresses）は空で返し、詳細取得時に別途充填する。
fn row_to_contact(r: &Row) -> rusqlite::Result<ContactSummary> {
    Ok(ContactSummary {
        id: r.get::<_, i64>(0)? as i32,
        display_name: r.get(1)?,
        family_name: r.get(2)?,
        given_name: r.get(3)?,
        phonetic_family: r.get(4)?,
        phonetic_given: r.get(5)?,
        name_kana: r.get(6)?,
        email: r.get(7)?,
        phone: r.get(8)?,
        organization: r.get(9)?,
        org_title: r.get(10)?,
        org_department: r.get(11)?,
        address: r.get(12)?,
        birthday: r.get(13)?,
        note: r.get(14)?,
        is_favorite: r.get::<_, i64>(15)? != 0,
        is_business: r.get::<_, i64>(16)? != 0,
        allow_remote_images: r.get::<_, i64>(17)? != 0,
        emails: Vec::new(),
        phones: Vec::new(),
        addresses: Vec::new(),
        tags: Vec::new(),
    })
}

const CONTACT_COLS: &str = "id, display_name, family_name, given_name, phonetic_family, \
     phonetic_given, name_kana, email, phone, organization, org_title, org_department, \
     address, birthday, note, is_favorite, is_business, allow_remote_images";

impl Store {
    /// 連絡先一覧。`query` があれば名前/よみ/メール/組織を部分一致で絞り込む。
    /// お気に入りを先頭に、次いで よみ→表示名 で並べる。
    pub fn list_contacts(
        &self,
        query: Option<&str>,
        group: Option<i64>,
    ) -> rusqlite::Result<Vec<ContactSummary>> {
        let conn = self.conn.lock().unwrap();
        let order = "ORDER BY is_favorite DESC, \
             name_kana COLLATE NOCASE, display_name COLLATE NOCASE";
        let like = query
            .map(str::trim)
            .filter(|q| !q.is_empty())
            .map(|q| format!("%{}%", q.replace('%', "\\%").replace('_', "\\_")));

        // 条件を動的に組む（テキスト検索・タグ絞り込み）。プレースホルダは順に採番。
        let mut conds: Vec<String> = Vec::new();
        let mut binds: Vec<&dyn rusqlite::ToSql> = Vec::new();
        let mut n = 0;
        if let Some(l) = &like {
            n += 1;
            conds.push(format!(
                "(display_name LIKE ?{n} ESCAPE '\\' OR name_kana LIKE ?{n} ESCAPE '\\' \
                  OR email LIKE ?{n} ESCAPE '\\' OR organization LIKE ?{n} ESCAPE '\\')"
            ));
            binds.push(l);
        }
        if let Some(g) = &group {
            n += 1;
            conds.push(format!(
                "EXISTS (SELECT 1 FROM contact_tags ct \
                 WHERE ct.contact_id = contacts.id AND ct.tag_id = ?{n})"
            ));
            binds.push(g);
        }
        let where_sql = if conds.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conds.join(" AND "))
        };
        let sql = format!("SELECT {CONTACT_COLS} FROM contacts {where_sql} {order}");
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(binds), row_to_contact)?;
        rows.collect()
    }

    /// 単一の連絡先を取得（メール/電話/住所の複数値も充填する）。
    pub fn get_contact(&self, id: i64) -> rusqlite::Result<ContactSummary> {
        let conn = self.conn.lock().unwrap();
        let sql = format!("SELECT {CONTACT_COLS} FROM contacts WHERE id = ?1");
        let mut c = conn.query_row(&sql, params![id], row_to_contact)?;
        c.emails = load_values(&conn, "contact_emails", id)?;
        c.phones = load_values(&conn, "contact_phones", id)?;
        c.addresses = load_addresses(&conn, id)?;
        c.tags = load_tags(&conn, id)?;
        Ok(c)
    }

    /// 連絡先を作成または更新し、確定後の行を返す。`input.id` が None なら新規。
    pub fn upsert_contact(&self, input: &ContactInput) -> rusqlite::Result<ContactSummary> {
        let conn = self.conn.lock().unwrap();
        // flat 主値は「配列があればその先頭、無ければ flat 入力」から導出（一覧・重複判定用）。
        let first_value = |vs: &[ContactValueInput]| {
            vs.iter()
                .map(|v| v.value.trim().to_string())
                .find(|v| !v.is_empty())
        };
        let email_flat = first_value(&input.emails).or_else(|| input.email.clone());
        let phone_flat = first_value(&input.phones).or_else(|| input.phone.clone());
        let address_flat = input
            .addresses
            .iter()
            .find_map(|a| {
                let s = address_input_string(a);
                if s.is_empty() {
                    None
                } else {
                    Some(s)
                }
            })
            .or_else(|| input.address.clone());
        let id = match input.id {
            Some(id) => {
                conn.execute(
                    "UPDATE contacts SET \
                         display_name = ?1, family_name = ?2, given_name = ?3, \
                         phonetic_family = ?4, phonetic_given = ?5, name_kana = ?6, \
                         email = ?7, phone = ?8, organization = ?9, org_title = ?10, \
                         org_department = ?11, address = ?12, birthday = ?13, note = ?14, \
                         is_favorite = ?15, is_business = ?16, allow_remote_images = ?17, \
                         updated_at = CURRENT_TIMESTAMP \
                     WHERE id = ?18",
                    params![
                        input.display_name,
                        input.family_name,
                        input.given_name,
                        input.phonetic_family,
                        input.phonetic_given,
                        input.name_kana,
                        email_flat,
                        phone_flat,
                        input.organization,
                        input.org_title,
                        input.org_department,
                        address_flat,
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
                         (display_name, family_name, given_name, phonetic_family, phonetic_given, \
                          name_kana, email, phone, organization, org_title, org_department, \
                          address, birthday, note, is_favorite, is_business, allow_remote_images) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, \
                          ?16, ?17)",
                    params![
                        input.display_name,
                        input.family_name,
                        input.given_name,
                        input.phonetic_family,
                        input.phonetic_given,
                        input.name_kana,
                        email_flat,
                        phone_flat,
                        input.organization,
                        input.org_title,
                        input.org_department,
                        address_flat,
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
        // 複数値が来ていれば子テーブルを作り直し、無ければ主値のみ反映（追加値は温存＝後方互換）。
        if input.emails.is_empty() {
            set_primary_value(&conn, "contact_emails", id, input.email.as_deref())?;
        } else {
            rebuild_input_values(&conn, "contact_emails", id, &input.emails)?;
        }
        if input.phones.is_empty() {
            set_primary_value(&conn, "contact_phones", id, input.phone.as_deref())?;
        } else {
            rebuild_input_values(&conn, "contact_phones", id, &input.phones)?;
        }
        if input.addresses.is_empty() {
            set_primary_address(&conn, id, input.address.as_deref())?;
        } else {
            rebuild_input_addresses(&conn, id, &input.addresses)?;
        }
        // タグ（グループ）メンバーシップを input.tags に一致させる（編集時のみ触る）。
        set_contact_tags(&conn, id, &input.tags)?;
        drop(conn);
        self.get_contact(id)
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

    /// 重複候補を record linkage で束ねて返す（2 件以上のみ、確信度順）。
    /// 検出ロジックは services::dedupe（正規化＋ブロッキング＋Union-Find）。
    pub fn find_duplicate_groups(&self) -> rusqlite::Result<Vec<DuplicateGroup>> {
        Ok(crate::services::dedupe::group(&self.list_contacts(None, None)?))
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
                // keep→drop の順に、子テーブルの全メール/電話/住所を value で重複排除して統合。
                let ids: Vec<i64> = std::iter::once(keep_id)
                    .chain(drop_ids.iter().copied())
                    .collect();
                let mut emails: Vec<(Option<String>, String)> = Vec::new();
                let mut phones: Vec<(Option<String>, String)> = Vec::new();
                let mut addresses: Vec<ContactAddress> = Vec::new();

                // スカラー項目は keep を先頭に空き埋め。フラグは OR。
                let mut name_kana: Option<String> = None;
                let mut organization: Option<String> = None;
                let mut org_title: Option<String> = None;
                let mut org_department: Option<String> = None;
                let mut birthday: Option<String> = None;
                let mut note: Option<String> = None;
                let mut fav = false;
                let mut biz = false;
                let mut remote = false;

                for id in &ids {
                    for v in load_values(&tx, "contact_emails", *id)? {
                        if !emails.iter().any(|(_, x)| x.eq_ignore_ascii_case(&v.value)) {
                            emails.push((v.label, v.value));
                        }
                    }
                    for v in load_values(&tx, "contact_phones", *id)? {
                        if !phones.iter().any(|(_, x)| x == &v.value) {
                            phones.push((v.label, v.value));
                        }
                    }
                    for a in load_addresses(&tx, *id)? {
                        let same = addresses.iter().any(|x| {
                            (&x.postal, &x.region, &x.city, &x.street)
                                == (&a.postal, &a.region, &a.city, &a.street)
                        });
                        if !same {
                            addresses.push(a);
                        }
                    }
                    let row: Option<MergeScalars> = tx
                        .query_row(
                            "SELECT name_kana, organization, org_title, org_department, \
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
                                ))
                            },
                        )
                        .optional()?;
                    if let Some((kana, org, ot, od, bday, nt, f, b, rm)) = row {
                        name_kana = name_kana.or(kana);
                        organization = organization.or(org);
                        org_title = org_title.or(ot);
                        org_department = org_department.or(od);
                        birthday = birthday.or(bday);
                        note = note.or(nt);
                        fav |= f != 0;
                        biz |= b != 0;
                        remote |= rm != 0;
                    }
                }

                let email = emails.first().map(|(_, v)| v.clone());
                let phone = phones.first().map(|(_, v)| v.clone());
                let address = addresses.first().map(address_string);

                tx.execute(
                    "UPDATE contacts SET \
                     email = ?1, phone = ?2, organization = ?3, org_title = ?4, \
                     org_department = ?5, name_kana = ?6, address = ?7, birthday = ?8, note = ?9, \
                     is_favorite = ?10, is_business = ?11, allow_remote_images = ?12, \
                     updated_at = CURRENT_TIMESTAMP \
                 WHERE id = ?13",
                    params![
                        email,
                        phone,
                        organization,
                        org_title,
                        org_department,
                        name_kana,
                        address,
                        birthday,
                        note,
                        fav as i64,
                        biz as i64,
                        remote as i64,
                        keep_id,
                    ],
                )?;

                // 統合後の全メール/電話/住所を keep の子テーブルへ書き直す。
                rebuild_pairs(&tx, "contact_emails", keep_id, &emails)?;
                rebuild_pairs(&tx, "contact_phones", keep_id, &phones)?;
                tx.execute(
                    "DELETE FROM contact_addresses WHERE contact_id = ?1",
                    params![keep_id],
                )?;
                for (i, a) in addresses.iter().enumerate() {
                    tx.execute(
                        "INSERT INTO contact_addresses \
                             (contact_id, label, postal, region, city, street, extended, country, \
                              is_primary, position) \
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                        params![
                            keep_id,
                            a.label,
                            a.postal,
                            a.region,
                            a.city,
                            a.street,
                            a.extended,
                            a.country,
                            (i == 0) as i64,
                            i as i64,
                        ],
                    )?;
                }

                // drop 側のタグを keep に移し、drop 行を削除（子テーブルは CASCADE）。
                for id in drop_ids {
                    tx.execute(
                        "UPDATE OR IGNORE contact_tags SET contact_id = ?1 WHERE contact_id = ?2",
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

/// merge_contacts のスカラー行（name_kana, organization, org_title, org_department,
/// birthday, note, is_favorite, is_business, allow_remote_images）。
type MergeScalars = (
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

/// (label, value) の列で子テーブルを作り直す（先頭を primary）。
fn rebuild_pairs(
    tx: &rusqlite::Transaction,
    table: &str,
    cid: i64,
    values: &[(Option<String>, String)],
) -> rusqlite::Result<()> {
    tx.execute(
        &format!("DELETE FROM {table} WHERE contact_id = ?1"),
        params![cid],
    )?;
    for (i, (label, value)) in values.iter().enumerate() {
        tx.execute(
            &format!(
                "INSERT INTO {table} (contact_id, label, value, is_primary, position) \
                 VALUES (?1, ?2, ?3, ?4, ?5)"
            ),
            params![cid, label, value, (i == 0) as i64, i as i64],
        )?;
    }
    Ok(())
}

/// 入力のラベル付き値（空値は除く）で子テーブルを作り直す（先頭を primary）。
fn rebuild_input_values(
    conn: &Connection,
    table: &str,
    cid: i64,
    values: &[ContactValueInput],
) -> rusqlite::Result<()> {
    conn.execute(
        &format!("DELETE FROM {table} WHERE contact_id = ?1"),
        params![cid],
    )?;
    for (pos, v) in values
        .iter()
        .filter(|v| !v.value.trim().is_empty())
        .enumerate()
    {
        conn.execute(
            &format!(
                "INSERT INTO {table} (contact_id, label, value, is_primary, position) \
                 VALUES (?1, ?2, ?3, ?4, ?5)"
            ),
            params![cid, v.label, v.value.trim(), (pos == 0) as i64, pos as i64],
        )?;
    }
    Ok(())
}

/// 住所入力が全項目空か。
fn address_input_empty(a: &ContactAddressInput) -> bool {
    [
        &a.postal,
        &a.region,
        &a.city,
        &a.street,
        &a.extended,
        &a.country,
    ]
    .iter()
    .all(|v| v.as_deref().map(str::trim).unwrap_or("").is_empty())
}

/// 入力の構造化住所（全項目空は除く）で contact_addresses を作り直す。
fn rebuild_input_addresses(
    conn: &Connection,
    cid: i64,
    addrs: &[ContactAddressInput],
) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM contact_addresses WHERE contact_id = ?1",
        params![cid],
    )?;
    for (pos, a) in addrs.iter().filter(|a| !address_input_empty(a)).enumerate() {
        conn.execute(
            "INSERT INTO contact_addresses \
                 (contact_id, label, postal, region, city, street, extended, country, \
                  is_primary, position) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                cid,
                a.label,
                a.postal,
                a.region,
                a.city,
                a.street,
                a.extended,
                a.country,
                (pos == 0) as i64,
                pos as i64,
            ],
        )?;
    }
    Ok(())
}

/// 入力住所を1行の文字列へ（flat 主値の導出用）。
fn address_input_string(a: &ContactAddressInput) -> String {
    [
        a.postal.as_deref(),
        a.region.as_deref(),
        a.city.as_deref(),
        a.street.as_deref(),
        a.extended.as_deref(),
        a.country.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(str::trim)
    .filter(|s| !s.is_empty())
    .collect::<Vec<_>>()
    .join(" ")
}

/// 構造化住所を1行の文字列へ（flat 保存・一覧用）。
fn address_string(a: &ContactAddress) -> String {
    [
        a.postal.as_deref(),
        a.region.as_deref(),
        a.city.as_deref(),
        a.street.as_deref(),
        a.extended.as_deref(),
        a.country.as_deref(),
    ]
    .into_iter()
    .flatten()
    .filter(|s| !s.is_empty())
    .collect::<Vec<_>>()
    .join(" ")
}

/// インポート 1 件を新規挿入。flat 列は主(primary)値、子テーブルへ全件を保存。
fn insert_from_import(tx: &rusqlite::Transaction, c: &ImportedContact) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO contacts \
             (display_name, family_name, given_name, phonetic_family, phonetic_given, \
              name_kana, email, phone, organization, org_title, org_department, address, \
              birthday, note, source, external_id) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        params![
            c.display_name,
            c.family_name,
            c.given_name,
            c.phonetic_family,
            c.phonetic_given,
            c.name_kana,
            c.email,
            c.phone,
            c.organization,
            c.org_title,
            c.org_department,
            c.address,
            c.birthday,
            c.note,
            c.source,
            c.external_id,
        ],
    )?;
    let id = tx.last_insert_rowid();
    write_import_children(tx, id, c)?;
    Ok(())
}

/// ImportedContact のラベル付き複数値を子テーブルへ書き込む（全件置き換え）。
fn write_import_children(
    tx: &rusqlite::Transaction,
    id: i64,
    c: &ImportedContact,
) -> rusqlite::Result<()> {
    rebuild_labeled(tx, "contact_emails", id, &c.all_emails)?;
    rebuild_labeled(tx, "contact_phones", id, &c.all_phones)?;
    // タグ（ラベル）を付与（冪等。取り込みは追加のみ）。
    for label in &c.labels {
        add_contact_tag(tx, id, label)?;
    }
    tx.execute(
        "DELETE FROM contact_addresses WHERE contact_id = ?1",
        params![id],
    )?;
    for (i, a) in c.all_addresses.iter().enumerate() {
        tx.execute(
            "INSERT INTO contact_addresses \
                 (contact_id, label, postal, region, city, street, extended, country, \
                  is_primary, position) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                id,
                a.label,
                a.postal,
                a.region,
                a.city,
                a.street,
                a.extended,
                a.country,
                (i == 0) as i64,
                i as i64,
            ],
        )?;
    }
    Ok(())
}

/// ラベル付き値（メール/電話）で子テーブルを作り直す。
fn rebuild_labeled(
    tx: &rusqlite::Transaction,
    table: &str,
    cid: i64,
    values: &[crate::services::vcard::ImportedValue],
) -> rusqlite::Result<()> {
    tx.execute(
        &format!("DELETE FROM {table} WHERE contact_id = ?1"),
        params![cid],
    )?;
    for (i, v) in values.iter().enumerate() {
        tx.execute(
            &format!(
                "INSERT INTO {table} (contact_id, label, value, is_primary, position) \
                 VALUES (?1, ?2, ?3, ?4, ?5)"
            ),
            params![cid, v.label, v.value, (i == 0) as i64, i as i64],
        )?;
    }
    Ok(())
}

/// ラベル付き複数値（メール/電話）を読み出す（主→position→id 順）。
fn load_values(conn: &Connection, table: &str, cid: i64) -> rusqlite::Result<Vec<ContactValue>> {
    let sql = format!(
        "SELECT id, label, value, is_primary FROM {table} \
         WHERE contact_id = ?1 ORDER BY is_primary DESC, position, id"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![cid], |r| {
        Ok(ContactValue {
            id: r.get::<_, i64>(0)? as i32,
            label: r.get(1)?,
            value: r.get(2)?,
            is_primary: r.get::<_, i64>(3)? != 0,
        })
    })?;
    rows.collect()
}

/// 構造化住所を読み出す。
fn load_addresses(conn: &Connection, cid: i64) -> rusqlite::Result<Vec<ContactAddress>> {
    let mut stmt = conn.prepare(
        "SELECT id, label, postal, region, city, street, extended, country, is_primary \
         FROM contact_addresses WHERE contact_id = ?1 ORDER BY is_primary DESC, position, id",
    )?;
    let rows = stmt.query_map(params![cid], |r| {
        Ok(ContactAddress {
            id: r.get::<_, i64>(0)? as i32,
            label: r.get(1)?,
            postal: r.get(2)?,
            region: r.get(3)?,
            city: r.get(4)?,
            street: r.get(5)?,
            extended: r.get(6)?,
            country: r.get(7)?,
            is_primary: r.get::<_, i64>(8)? != 0,
        })
    })?;
    rows.collect()
}

/// 連絡先のタグ名を読み出す（メール共通の tags を使用。名前順）。
fn load_tags(conn: &Connection, cid: i64) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT t.name FROM tags t \
         JOIN contact_tags ct ON ct.tag_id = t.id \
         WHERE ct.contact_id = ?1 ORDER BY t.name COLLATE NOCASE",
    )?;
    let rows = stmt.query_map(params![cid], |r| r.get(0))?;
    rows.collect()
}

/// タグ名から id を得る（無ければ作成。メール/連絡先共通の tags）。
fn find_or_create_tag(conn: &Connection, name: &str) -> rusqlite::Result<i64> {
    if let Some(id) = conn
        .query_row("SELECT id FROM tags WHERE name = ?1", params![name], |r| {
            r.get::<_, i64>(0)
        })
        .optional()?
    {
        return Ok(id);
    }
    conn.execute(
        "INSERT INTO tags (name, kind) VALUES (?1, 'tag')",
        params![name],
    )?;
    Ok(conn.last_insert_rowid())
}

/// 連絡先にタグを付与（冪等）。
fn add_contact_tag(conn: &Connection, cid: i64, name: &str) -> rusqlite::Result<()> {
    let name = name.trim();
    if name.is_empty() {
        return Ok(());
    }
    let tid = find_or_create_tag(conn, name)?;
    conn.execute(
        "INSERT OR IGNORE INTO contact_tags (contact_id, tag_id) VALUES (?1, ?2)",
        params![cid, tid],
    )?;
    Ok(())
}

/// 連絡先のタグ集合を names にそろえる（既存を消して張り直す）。
fn set_contact_tags(conn: &Connection, cid: i64, names: &[String]) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM contact_tags WHERE contact_id = ?1",
        params![cid],
    )?;
    for name in names {
        add_contact_tag(conn, cid, name)?;
    }
    Ok(())
}

/// 主(primary)値を1件だけ張り替える（既存 primary を消して入れ直す。追加値は温存）。
fn set_primary_value(
    conn: &Connection,
    table: &str,
    cid: i64,
    value: Option<&str>,
) -> rusqlite::Result<()> {
    conn.execute(
        &format!("DELETE FROM {table} WHERE contact_id = ?1 AND is_primary = 1"),
        params![cid],
    )?;
    if let Some(v) = value {
        let v = v.trim();
        if !v.is_empty() {
            conn.execute(
                &format!(
                    "INSERT INTO {table} (contact_id, value, is_primary, position) \
                     VALUES (?1, ?2, 1, 0)"
                ),
                params![cid, v],
            )?;
        }
    }
    Ok(())
}

/// 主住所を1件だけ張り替える（単一文字列は street に格納）。
fn set_primary_address(conn: &Connection, cid: i64, street: Option<&str>) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM contact_addresses WHERE contact_id = ?1 AND is_primary = 1",
        params![cid],
    )?;
    if let Some(s) = street {
        let s = s.trim();
        if !s.is_empty() {
            conn.execute(
                "INSERT INTO contact_addresses (contact_id, street, is_primary, position) \
                 VALUES (?1, ?2, 1, 0)",
                params![cid, s],
            )?;
        }
    }
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
    fn child_tables_populated_on_import_and_upsert() {
        let s = store();
        // 追加メール2件を持つ vCard を取り込み → contact_emails に3件、うち1件が primary。
        let p = vcard::parse(
            "BEGIN:VCARD\nVERSION:3.0\nFN:多重 花子\nEMAIL;type=pref:a@x.jp\nEMAIL:b@x.jp\nEMAIL:c@x.jp\nTEL:090-1\nEND:VCARD\n",
        );
        s.import_contacts(&p).unwrap();
        let c = s.list_contacts(None, None).unwrap().remove(0);
        let got = s.get_contact(c.id as i64).unwrap();
        assert_eq!(got.emails.len(), 3, "追加メールも子テーブルに入る");
        assert!(got.emails[0].is_primary);
        assert_eq!(got.emails[0].value, "a@x.jp");
        assert_eq!(got.phones.len(), 1);

        // 編集で主メールを変更しても追加メールは温存される。
        s.upsert_contact(&ContactInput {
            id: Some(c.id),
            display_name: got.display_name.clone(),
            name_kana: None,
            email: Some("new@x.jp".into()),
            phone: got.phone.clone(),
            organization: None,
            address: None,
            birthday: None,
            note: None,
            is_favorite: false,
            is_business: false,
            allow_remote_images: false,
            ..Default::default()
        })
        .unwrap();
        let after = s.get_contact(c.id as i64).unwrap();
        let primaries: Vec<_> = after.emails.iter().filter(|e| e.is_primary).collect();
        assert_eq!(primaries.len(), 1);
        assert_eq!(primaries[0].value, "new@x.jp");
        assert!(
            after.emails.iter().any(|e| e.value == "b@x.jp"),
            "追加メールは残る"
        );
    }

    #[test]
    fn upsert_with_arrays_writes_all_child_values() {
        let s = store();
        let input = ContactInput {
            display_name: "配列 太郎".into(),
            emails: vec![
                ContactValueInput {
                    label: Some("自宅".into()),
                    value: "home@x.jp".into(),
                },
                ContactValueInput {
                    label: Some("職場".into()),
                    value: "work@x.jp".into(),
                },
            ],
            phones: vec![ContactValueInput {
                label: Some("携帯".into()),
                value: "090-1".into(),
            }],
            addresses: vec![ContactAddressInput {
                label: Some("自宅".into()),
                region: Some("沖縄県".into()),
                city: Some("那覇市".into()),
                ..Default::default()
            }],
            org_title: Some("部長".into()),
            ..Default::default()
        };
        let saved = s.upsert_contact(&input).unwrap();
        let c = s.get_contact(saved.id as i64).unwrap();
        assert_eq!(c.emails.len(), 2);
        assert_eq!(c.emails[0].label.as_deref(), Some("自宅"));
        assert_eq!(c.email.as_deref(), Some("home@x.jp")); // flat 主値も導出…はフロント。ここは flat 未指定
        assert_eq!(c.phones.len(), 1);
        assert_eq!(c.addresses.len(), 1);
        assert_eq!(c.addresses[0].region.as_deref(), Some("沖縄県"));
        assert_eq!(c.org_title.as_deref(), Some("部長"));
    }

    #[test]
    fn contact_tags_import_edit_and_filter() {
        let s = store();
        // CATEGORIES 付き vCard を取り込み → 共通 tags に入る。
        let p = vcard::parse(
            "BEGIN:VCARD\nVERSION:3.0\nFN:タグ 太郎\nCATEGORIES:施主,設計事務所\nEND:VCARD\n",
        );
        s.import_contacts(&p).unwrap();
        let id = s.list_contacts(None, None).unwrap()[0].id as i64;
        let c = s.get_contact(id).unwrap();
        assert!(c.tags.contains(&"施主".to_string()));
        assert!(c.tags.contains(&"設計事務所".to_string()));

        // 編集でタグを置き換え。
        let input = ContactInput {
            id: Some(c.id),
            display_name: c.display_name.clone(),
            tags: vec!["VIP".to_string()],
            ..Default::default()
        };
        s.upsert_contact(&input).unwrap();
        assert_eq!(s.get_contact(id).unwrap().tags, vec!["VIP".to_string()]);

        // タグ ID で絞り込み（メール共通の tags を参照）。
        let tag_id: i64 = {
            let conn = s.conn.lock().unwrap();
            conn.query_row("SELECT id FROM tags WHERE name = 'VIP'", [], |r| r.get(0))
                .unwrap()
        };
        let filtered = s.list_contacts(None, Some(tag_id)).unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, c.id);
    }

    #[test]
    fn import_keeps_all_phones_and_structured_address() {
        let s = store();
        let p = vcard::parse(
            "BEGIN:VCARD\nVERSION:3.0\nFN:多値 太郎\nTEL;type=CELL:090-1111\nTEL;type=WORK:03-2222\nTEL:03-3333\nADR;type=HOME:;;番地1;那覇市;沖縄県;9000001;日本\nTITLE:部長\nORG:テスト社;営業部\nEND:VCARD\n",
        );
        s.import_contacts(&p).unwrap();
        let id = s.list_contacts(None, None).unwrap()[0].id as i64;
        let c = s.get_contact(id).unwrap();
        // 電話3件（1件目=CELL が主）。
        assert_eq!(c.phones.len(), 3, "全電話を保持");
        assert!(c.phones[0].is_primary);
        assert_eq!(c.phones[0].label.as_deref(), Some("携帯"));
        // 住所は構造化。
        assert_eq!(c.addresses.len(), 1);
        assert_eq!(c.addresses[0].region.as_deref(), Some("沖縄県"));
        assert_eq!(c.addresses[0].city.as_deref(), Some("那覇市"));
        assert_eq!(c.addresses[0].postal.as_deref(), Some("9000001"));
        assert_eq!(c.addresses[0].label.as_deref(), Some("自宅"));
        // 組織の役職・部署。
        assert_eq!(c.org_title.as_deref(), Some("部長"));
        assert_eq!(c.org_department.as_deref(), Some("営業部"));
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
        let c = s.list_contacts(None, None).unwrap().remove(0);
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
            ..Default::default()
        })
        .unwrap();

        // 同じメールで再取り込み（組織名が変わり、電話が増えた）。
        let second = vcard::parse(
            "BEGIN:VCARD\nVERSION:3.0\nFN:山田太郎\nEMAIL:taro@example.com\nORG:新社名\nTEL:09011112222\nEND:VCARD\n",
        );
        let r2 = s.import_contacts(&second).unwrap();
        assert_eq!((r2.total, r2.imported, r2.updated), (1, 0, 1));

        // 重複は増えず、フラグは温存、フィールドは更新されている。
        let all = s.list_contacts(None, None).unwrap();
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
        assert_eq!(s.list_contacts(None, None).unwrap().len(), 2);
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
                ..Default::default()
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
                ..Default::default()
            })
            .unwrap()
            .id as i64;

        let groups = s.find_duplicate_groups().unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].contacts.len(), 2);

        let merged = s.merge_contacts(id_a, &[id_b]).unwrap();
        assert_eq!(s.list_contacts(None, None).unwrap().len(), 1);
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
                ..Default::default()
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
            ..Default::default()
        })
        .unwrap();
        assert_eq!(s.list_contacts(None, None).unwrap().len(), 2);

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
            ..Default::default()
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
        let all = s.list_contacts(None, None).unwrap();
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
             display_name    = ?1, \
             family_name     = COALESCE(?2, family_name), \
             given_name      = COALESCE(?3, given_name), \
             phonetic_family = COALESCE(?4, phonetic_family), \
             phonetic_given  = COALESCE(?5, phonetic_given), \
             name_kana       = COALESCE(?6, name_kana), \
             email           = COALESCE(?7, email), \
             phone           = COALESCE(?8, phone), \
             organization    = COALESCE(?9, organization), \
             org_title       = COALESCE(?10, org_title), \
             org_department  = COALESCE(?11, org_department), \
             address         = COALESCE(?12, address), \
             birthday        = COALESCE(?13, birthday), \
             note            = COALESCE(?14, note), \
             source          = ?15, \
             external_id     = COALESCE(?16, external_id), \
             updated_at      = CURRENT_TIMESTAMP \
         WHERE id = ?17",
        params![
            c.display_name,
            c.family_name,
            c.given_name,
            c.phonetic_family,
            c.phonetic_given,
            c.name_kana,
            c.email,
            c.phone,
            c.organization,
            c.org_title,
            c.org_department,
            c.address,
            c.birthday,
            c.note,
            c.source,
            c.external_id,
            id,
        ],
    )?;
    // 子テーブルは取り込み値で作り直す（このソースの最新値を反映）。
    if !c.all_emails.is_empty() || !c.all_phones.is_empty() || !c.all_addresses.is_empty() {
        write_import_children(tx, id, c)?;
    }
    Ok(())
}
