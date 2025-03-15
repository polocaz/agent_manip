use anyhow::Result;
use crate::error::LogManagerError;
use chrono::{DateTime, Local, Datelike, NaiveDateTime, TimeZone};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader};
use std::path::Path;

// Log entry struct TODO: add thread name
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: DateTime<Local>,
    pub component: String,
    pub message: String,
    pub level: LogLevel,
    pub line_number: usize,
    pub category: String,
}

pub struct LogReader {
    file: File,
    path: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LogLevel {
    Info, 
    Warning,
    Error,
    One,
    Two,
    Three,
    Four,
    Five,
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Info => write!(f, "Info"),
            LogLevel::Warning => write!(f, "Warning"),
            LogLevel::Error => write!(f, "Error"),
            LogLevel::One => write!(f, "-1"),
            LogLevel::Two => write!(f, "-2"),
            LogLevel::Three => write!(f, "-3"),
            LogLevel::Four => write!(f, "-4"),
            LogLevel::Five => write!(f, "-5"),
        }
    }
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
        let parts: Vec<&str> = line.split_whitespace().collect();
        
        if parts.len() < 6 {
            return Err(LogManagerError::ParseError {
                message: "Invalid log entry format".to_string(),
                line: line_num,
            });
        }

        // Parse date and time (03-14 18:31:38)
        let date = parts[0];
        let time = parts[1];
        let current_year = Local::now().year();
        let timestamp_str = format!("{}-{} {}", current_year, date, time);
        let naive_dt = NaiveDateTime::parse_from_str(&timestamp_str, "%Y-%m-%d %H:%M:%S")?;
        let timestamp = Local.from_local_datetime(&naive_dt)
            .single()
            .ok_or_else(|| LogManagerError::ParseError {
                message: "Invalid timestamp".to_string(),
                line: line_num,
            })?;

        // Parse component and line number (collThermalsNix(31))
        let component_line = parts[2];
        let (component, line_number) = component_line
            .split_once('(')
            .and_then(|(comp, id)| {
                id.trim_end_matches(')')
                    .parse::<u32>()
                    .ok()
                    .map(|num| (comp.to_string(), num))
            })
            .ok_or_else(|| LogManagerError::ParseError {
                message: "Invalid component/thread format".to_string(),
                line: line_num,
            })?;

        // Parse log level (-I or -W)
        let level = match parts[3] {
            "-I" => LogLevel::Info,
            "-W" => LogLevel::Warning,
            "-E" => LogLevel::Error,
            "-1" => LogLevel::One,
            "-2" => LogLevel::Two,
            "-3" => LogLevel::Three,
            "-4" => LogLevel::Four,
            "-5" => LogLevel::Five,
            level => return Err(LogManagerError::ParseError {
                message: format!("Unknown log level: {}", level),
                line: line_num,
            }),
        };

        // Get category and message
        // let category = parts[4].to_string();
        let category = "".to_string(); // TODO: add category
        let message = parts[4..].join(" ");

        Ok(LogEntry {
            timestamp,
            component,
            message,
            level,
            line_number: line_num.unwrap_or(0),
            category,
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