use anyhow::Result;
use chrono::{DateTime, Local};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader};
use std::path::Path;

// Log entry struct TODO: add thread name
pub struct LogEntry {
    pub timestamp: DateTime<Local>,
    pub message: String,
    pub level: String,
}

pub struct LogReader {
    file: File,
    path: String,
}

impl LogReader {

    // Change log path based on user input and update the file handle
    pub fn change_log_path<P: AsRef<Path>>(&mut self, new_path: P) -> Result<()> {
        let path_str = new_path.as_ref().to_string_lossy().into_owned();
        let file = OpenOptions::new()
            .read(true)
            .open(new_path)?;

        self.file = file;
        self.path = path_str;
        Ok(())
    }

    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_str = path.as_ref().to_string_lossy().into_owned();
        let file = OpenOptions::new()
            .read(true)
            .open(path)?;

        Ok(Self {
            file,
            path: path_str,
        })
    }

    pub fn read_latest_entries(&mut self, count: usize) -> Result<Vec<LogEntry>> {
        let mut entries = Vec::new();
        let reader = BufReader::new(&self.file);
        
        // Read all lines and keep only the last 'count' entries
        let lines: Vec<String> = reader.lines()
            .filter_map(|line| line.ok())
            .collect();

        for line in lines.iter().rev().take(count) {
            if let Some(entry) = self.parse_log_line(line) {
                entries.push(entry);
            }
        }

        entries.reverse();
        Ok(entries)
    }

    fn parse_log_line(&self, line: &str) -> Option<LogEntry> {
        // This is a basic implementation - adjust based on your log format
        let parts: Vec<&str> = line.splitn(3, ' ').collect();
        if parts.len() < 3 {
            return None;
        }

        if let Ok(timestamp) = DateTime::parse_from_rfc3339(parts[0]) {
            Some(LogEntry {
                timestamp: timestamp.into(),
                level: parts[1].to_string(),
                message: parts[2].to_string(),
            })
        } else {
            None
        }
    }

    pub fn get_path(&self) -> &str {
        &self.path
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_log_reader() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("test.log");
        
        // Create a test log file
        let mut file = File::create(&log_path).unwrap();
        writeln!(file, "2024-02-20T10:00:00Z INFO Test message 1").unwrap();
        writeln!(file, "2024-02-20T10:01:00Z ERROR Test message 2").unwrap();
        
        let mut reader = LogReader::new(log_path).unwrap();
        let entries = reader.read_latest_entries(2).unwrap();
        
        assert_eq!(entries.len(), 2);
    }
} 