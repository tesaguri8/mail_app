use super::Store;
use crate::models::TagSummary;
use rusqlite::params;

impl Store {
    /// タグ一覧（使用件数つき、名前順）。kind='tag' のユーザー定義タグのみ返す。
    pub fn list_tags(&self) -> rusqlite::Result<Vec<TagSummary>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT t.id, t.name, t.color,
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
                count: r.get::<_, i64>(3)? as i32,
            })
        })?;
        rows.collect()
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

    /// タグを削除（メールとの紐づけも外す）。
    pub fn delete_tag(&self, id: i64) -> rusqlite::Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM email_tags WHERE tag_id = ?1", params![id])?;
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
