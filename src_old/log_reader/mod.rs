use crate::error::LogManagerError;
use anyhow::Result;
use chrono::{DateTime, Datelike, Local, NaiveDateTime, TimeZone};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader};
use std::path::Path;

pub static THREAD_NAMES: [&str; 5] = ["Rules", "Cond", "Coll", "Logon", "Inv"];
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: DateTime<Local>,
    pub component: String,
    pub message: String,
    pub level: LogLevel,
    pub line_number: u32,
    pub thread_name: String,
}

pub struct LogReader {
    file: File,
    path: String,
    pub last_line_read: usize,
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
        let file = OpenOptions::new().read(true).open(new_path)?;

        self.file = file;
        self.path = path_str;
        Ok(())
    }

    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_str = path.as_ref().to_string_lossy().into_owned();
        let file = OpenOptions::new().read(true).open(path)?;

        Ok(Self {
            file,
            path: path_str,
            last_line_read: 0,
        })
    }

    // Read only new entries from the log file
    pub fn read_new_entries(&mut self) -> Vec<String> {
        let file = match File::open(&self.path) {
            Ok(file) => file,
            Err(_) => return Vec::new(),
        };

        let reader = BufReader::new(file);
        let mut new_lines = Vec::new();

        // Skip lines we've already read
        for (i, line) in reader.lines().enumerate() {
            if i < self.last_line_read {
                continue;
            }

            if let Ok(line) = line {
                new_lines.push(line);
                self.last_line_read = i + 1;
            }
        }

        new_lines
    }

    // Complete reloads
    pub fn read_latest_entries(&self, count: usize, earliest_first: bool) -> Vec<String> {
        let file = match File::open(&self.path) {
            Ok(file) => file,
            Err(_) => return Vec::new(),
        };

        let reader = BufReader::new(file);
        let lines: Vec<String> = reader.lines().filter_map(Result::ok).collect();

        if earliest_first {
            lines.into_iter().take(count).collect()
        } else {
            lines.into_iter().rev().take(count).collect()
        }

        // This method is pretty slow, so we will load raw log entries for now
        //
        // File::open(&self.path).map_or_else(
        //     |_| Vec::new(),
        //     |file| {
        //         let lines: Vec<_> = BufReader::new(file)
        //             .lines()
        //             .collect::<Result<Vec<_>, _>>()
        //             .unwrap_or_default();

        //         // Collect log entries in the order they are read
        //         let entries: Vec<LogEntry> = lines
        //             .iter()
        //             .enumerate()
        //             .filter_map(|(line_num, line)| {
        //                 match Self::parse_log_line(line, Some(line_num + 1)) {
        //                     Ok(entry) => Some(entry),
        //                     Err(LogManagerError::ParseError { message, .. }) => {
        //                         eprintln!("Parse error at line {}: {}", line_num + 1, message);
        //                         None
        //                     }
        //                     Err(_) => None,
        //                 }
        //             })
        //             .collect();

        //         // Determine the slice to return based on earliest_first
        //         if earliest_first {
        //             // Return the first `count` entries as they are read
        //             entries.into_iter().take(count).collect()
        //         } else {
        //             // Return the last `count` entries as they are read
        //             entries.into_iter().rev().take(count).collect()
        //         }
        //     },
        // )
    }

    fn parse_log_line(line: &str, line_num: Option<usize>) -> Result<LogEntry, LogManagerError> {
        let parts: Vec<&str> = line.split_whitespace().collect();

        if parts.len() < 5 {
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
        let timestamp = Local
            .from_local_datetime(&naive_dt)
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
            level => {
                return Err(LogManagerError::ParseError {
                    message: format!("Unknown log level: {}", level),
                    line: line_num,
                })
            }
        };

        // Get thread name and message
        let mut thrd_name = "".to_string();
        let mut message = "".to_string();
        if THREAD_NAMES.contains(&parts[4]) {
            thrd_name = parts[4].to_string();
            message = parts[5..].join(" ");
        } else {
            message = parts[4..].join(" ");
        }

        Ok(LogEntry {
            timestamp,
            component,
            message,
            level,
            line_number,
            thread_name: thrd_name,
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
        let entries = reader.read_latest_entries(2, true);

        assert_eq!(entries.len(), 2);
    }
}
