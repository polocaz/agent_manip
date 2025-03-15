use anyhow::Result;
use rusqlite::Connection;
use std::path::Path;

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;
        Ok(Self { conn })
    }

    pub fn init_schema(&self) -> Result<()> {
        // Add your schema initialization here
        // This is a placeholder - modify according to your needs
        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS agent_status (
                id INTEGER PRIMARY KEY,
                timestamp TEXT NOT NULL,
                status TEXT NOT NULL,
                details TEXT
            )",
            [],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_database_creation() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db = Database::new(db_path).unwrap();
        assert!(db.init_schema().is_ok());
    }
} 