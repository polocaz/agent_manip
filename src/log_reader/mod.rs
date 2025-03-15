use anyhow::Result;
use crate::error::LogManagerError;
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

    pub fn read_latest_entries(&self, count: usize) -> Vec<LogEntry> {
        File::open(&self.path).map_or_else(|_| Vec::new(), |file| {
            let lines: Vec<_> = BufReader::new(file)
                .lines()
                .collect::<Result<Vec<_>, _>>()
                .unwrap_or_default();

            lines.iter()
                .rev()
                .take(count)
                .enumerate()
                .filter_map(|(line_num, line)| {
                    match Self::parse_log_line(line, Some(line_num + 1)) {
                        Ok(entry) => Some(entry),
                        Err(LogManagerError::ParseError { message, .. }) => {
                            eprintln!("Parse error at line {}: {}", line_num + 1, message);
                            None
                        }
                        Err(_) => None,
                    }
                })
                .collect()
        })
    }

    fn parse_log_line(line: &str, line_num: Option<usize>) -> Result<LogEntry, LogManagerError> {
        let parts: Vec<&str> = line.splitn(3, ' ').collect();
        
        if parts.len() != 3 {
            return Err(LogManagerError::ParseError {
                message: "Invalid log entry format: expected 3 parts (timestamp, level, message)".to_string(),
                line: line_num,
            });
        }

        DateTime::parse_from_rfc3339(parts[0])
            .map_err(|e| LogManagerError::ParseError {
                message: format!("Invalid timestamp format: {e}"),
                line: line_num,
            })
            .map(|timestamp| LogEntry {
                timestamp: timestamp.into(),
                level: parts[1].to_string(),
                message: parts[2].to_string(),
            })
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
        let entries = reader.read_latest_entries(2);
        
        assert_eq!(entries.len(), 2);
    }
} 