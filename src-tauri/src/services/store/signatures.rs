use super::Store;
use crate::models::SignatureSummary;
use rusqlite::params;

impl Store {
    pub fn list_signatures(&self) -> rusqlite::Result<Vec<SignatureSummary>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, name, body FROM signatures ORDER BY id")?;
        let rows = stmt.query_map([], |r| {
            Ok(SignatureSummary {
                id: r.get::<_, i64>(0)? as i32,
                name: r.get(1)?,
                body: r.get(2)?,
            })
        })?;
        rows.collect()
    }

    pub fn insert_signature(&self, name: &str, body: &str) -> rusqlite::Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO signatures (name, body) VALUES (?1, ?2)",
            params![name, body],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn update_signature(&self, id: i64, name: &str, body: &str) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE signatures SET name = ?1, body = ?2 WHERE id = ?3",
            params![name, body, id],
        )?;
        Ok(())
    }

    pub fn delete_signature(&self, id: i64) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        // 参照していたアカウントの紐づけを外す（ON DELETE SET NULL の保険）。
        conn.execute(
            "UPDATE accounts SET signature_id = NULL WHERE signature_id = ?1",
            params![id],
        )?;
        conn.execute("DELETE FROM signatures WHERE id = ?1", params![id])?;
        Ok(())
    }
}
