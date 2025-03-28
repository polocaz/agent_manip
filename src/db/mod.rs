use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags, Row, Statement};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct Column {
    pub name: String,
    pub data_type: String,
}

#[derive(Debug, Clone)]
pub struct QueryResult {
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<String>>,
    pub affected_rows: usize,
}

impl QueryResult {
    pub fn new() -> Self {
        Self {
            columns: Vec::new(),
            rows: Vec::new(),
            affected_rows: 0,
        }
    }
}

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;
        Ok(Self { conn })
    }

    pub fn new_readonly<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
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

    /// Execute a SELECT query and return the results
    pub fn execute_query(&self, query: &str) -> Result<QueryResult> {
        // Ensure the query is a SELECT query to prevent modification
        let lowercase_query = query.trim().to_lowercase();
        if !lowercase_query.starts_with("select")
            && !lowercase_query.starts_with("pragma")
            && !lowercase_query.starts_with("explain")
        {
            return Err(anyhow::anyhow!(
                "Only SELECT, PRAGMA, and EXPLAIN queries are allowed in read-only mode"
            ));
        }

        let mut stmt = self
            .conn
            .prepare(query)
            .with_context(|| format!("Failed to prepare query: {}", query))?;

        let result = self
            .execute_statement(&mut stmt)
            .with_context(|| format!("Failed to execute query: {}", query))?;

        Ok(result)
    }

    /// List all tables in the database
    pub fn list_tables(&self) -> Result<Vec<String>> {
        let query = "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name";
        let mut stmt = self.conn.prepare(query)?;
        let tables = stmt
            .query_map([], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        Ok(tables)
    }

    /// Get the schema of a specific table
    pub fn get_table_schema(&self, table_name: &str) -> Result<Vec<Column>> {
        let query = format!("PRAGMA table_info({})", table_name);
        let mut stmt = self.conn.prepare(&query)?;

        let columns = stmt
            .query_map([], |row| {
                Ok(Column {
                    name: row.get(1)?,
                    data_type: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<Column>, _>>()?;

        Ok(columns)
    }

    // Helper method to execute a statement and collect the results
    fn execute_statement(&self, stmt: &mut Statement) -> Result<QueryResult> {
        let column_count = stmt.column_count();
        let mut columns = Vec::with_capacity(column_count);

        // Get column names and types
        for i in 0..column_count {
            let column_name = match stmt.column_name(i) {
                Ok(name) => name.to_string(),
                Err(_) => format!("Column_{}", i),
            };

            columns.push(Column {
                name: column_name,
                data_type: "".to_string(), // SQLite doesn't easily expose column types through the Rust API
            });
        }

        // Execute the query and collect rows
        let mut rows = Vec::new();
        let mut row_count = 0;

        let mut rows_iter = stmt.query([])?;

        while let Some(row) = rows_iter.next()? {
            rows.push(self.row_to_strings(row, column_count)?);
            row_count += 1;
        }

        Ok(QueryResult {
            columns,
            rows,
            affected_rows: row_count,
        })
    }

    // Convert a row to a vector of strings for display
    fn row_to_strings(&self, row: &Row, column_count: usize) -> Result<Vec<String>> {
        let mut result = Vec::with_capacity(column_count);

        for i in 0..column_count {
            let value = match row.get_ref(i)? {
                rusqlite::types::ValueRef::Null => "NULL".to_string(),
                rusqlite::types::ValueRef::Integer(i) => i.to_string(),
                rusqlite::types::ValueRef::Real(f) => f.to_string(),
                rusqlite::types::ValueRef::Text(t) => String::from_utf8_lossy(t).to_string(),
                rusqlite::types::ValueRef::Blob(b) => format!("<BLOB: {} bytes>", b.len()),
            };
            result.push(value);
        }

        Ok(result)
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

    #[test]
    fn test_execute_query() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db = Database::new(db_path).unwrap();
        db.init_schema().unwrap();

        // Insert test data
        db.conn
            .execute(
                "INSERT INTO agent_status (timestamp, status, details) VALUES (?, ?, ?)",
                ["2023-01-01 12:00:00", "Running", "Test details"],
            )
            .unwrap();

        // Test SELECT query
        let result = db.execute_query("SELECT * FROM agent_status").unwrap();
        assert_eq!(result.columns.len(), 4);
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][1], "2023-01-01 12:00:00");
        assert_eq!(result.rows[0][2], "Running");
    }
}
