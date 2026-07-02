use super::Store;
use crate::models::TagSummary;
use rusqlite::params;

impl Store {
    /// タグ一覧（使用件数つき、名前順）。kind='tag' のユーザー定義タグのみ返す。
    pub fn list_tags(&self) -> rusqlite::Result<Vec<TagSummary>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT t.id, t.name, t.color, t.parent_id,
                    (SELECT count(*) FROM email_tags et WHERE et.tag_id = t.id) AS cnt
             FROM tags t
             WHERE t.kind = 'tag'
             ORDER BY t.name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(TagSummary {
                id: r.get::<_, i64>(0)? as i32,
                name: r.get(1)?,
                color: r.get(2)?,
                parent_id: r.get::<_, Option<i64>>(3)?.map(|v| v as i32),
                count: r.get::<_, i64>(4)? as i32,
            })
        })?;
        rows.collect()
    }

    /// タグの親を設定（None でルートへ）。循環（自分の子孫を親にする）は拒否。
    pub fn set_tag_parent(&self, id: i64, parent: Option<i64>) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        if let Some(p) = parent {
            if p == id {
                return Ok(()); // 自分自身は親にできない（無視）
            }
            // p から祖先をたどり id に達したら循環なので拒否。
            let mut cur = Some(p);
            while let Some(c) = cur {
                if c == id {
                    return Ok(());
                }
                cur = conn
                    .query_row("SELECT parent_id FROM tags WHERE id = ?1", params![c], |r| {
                        r.get::<_, Option<i64>>(0)
                    })
                    .unwrap_or(None);
            }
        }
        conn.execute(
            "UPDATE tags SET parent_id = ?1 WHERE id = ?2",
            params![parent, id],
        )?;
        Ok(())
    }

    /// タグを新規作成し、作成した行を返す。
    pub fn insert_tag(&self, name: &str, color: Option<&str>) -> rusqlite::Result<TagSummary> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO tags (name, kind, color) VALUES (?1, 'tag', ?2)",
            params![name, color],
        )?;
        let id = conn.last_insert_rowid();
        Ok(TagSummary {
            id: id as i32,
            name: name.to_string(),
            color: color.map(str::to_string),
            parent_id: None,
            count: 0,
        })
    }

    /// タグの名前・色を更新。
    pub fn update_tag(&self, id: i64, name: &str, color: Option<&str>) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE tags SET name = ?1, color = ?2 WHERE id = ?3",
            params![name, color, id],
        )?;
        Ok(())
    }

    /// タグを削除（メール/連絡先との紐づけも外す）。子タグは削除タグの親へ繰り上げる。
    pub fn delete_tag(&self, id: i64) -> rusqlite::Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let parent: Option<i64> = tx
            .query_row("SELECT parent_id FROM tags WHERE id = ?1", params![id], |r| {
                r.get(0)
            })
            .unwrap_or(None);
        // 子タグを繰り上げ（フォルダ削除で孤立させない）。
        tx.execute(
            "UPDATE tags SET parent_id = ?1 WHERE parent_id = ?2",
            params![parent, id],
        )?;
        tx.execute("DELETE FROM email_tags WHERE tag_id = ?1", params![id])?;
        tx.execute("DELETE FROM contact_tags WHERE tag_id = ?1", params![id])?;
        tx.execute("DELETE FROM tags WHERE id = ?1", params![id])?;
        tx.commit()
    }

    /// 複数メールにタグを付与（冪等。既にあるものは無視）。
    pub fn add_tag_to_emails(&self, email_ids: &[i64], tag_id: i64) -> rusqlite::Result<()> {
        if email_ids.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        {
            let mut stmt =
                tx.prepare("INSERT OR IGNORE INTO email_tags (email_id, tag_id) VALUES (?1, ?2)")?;
            for id in email_ids {
                stmt.execute(params![id, tag_id])?;
            }
        }
        tx.commit()
    }

    /// 複数メールからタグを外す。
    pub fn remove_tag_from_emails(&self, email_ids: &[i64], tag_id: i64) -> rusqlite::Result<()> {
        if email_ids.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        {
            let mut stmt =
                tx.prepare("DELETE FROM email_tags WHERE email_id = ?1 AND tag_id = ?2")?;
            for id in email_ids {
                stmt.execute(params![id, tag_id])?;
            }
        }
        tx.commit()
    }
}
