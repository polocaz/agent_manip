use thiserror::Error;
use chrono::ParseError;

#[derive(Error, Debug)]
pub enum LogManagerError {
    #[error("Failed to open log file at {path}: {source}")]
    FileOpenError {
        path: String,
        source: std::io::Error,
    },

    #[error("Invalid log file path: {0}")]
    InvalidPath(String),

    #[error("Failed to read log file: {0}")]
    ReadError(String),

    #[error("Permission denied for file: {0}")]
    PermissionDenied(String),

    #[error("Log file not found: {0}")]
    FileNotFound(String),

    #[error("Parse error: {message}{}", .line.map(|l| format!(" at line {l}")).unwrap_or_default())]
    ParseError {
        message: String,
        line: Option<usize>,
    },
}

impl From<ParseError> for LogManagerError {
    fn from(err: ParseError) -> Self {
        LogManagerError::ParseError {
            message: err.to_string(),
            line: None,
        }
    }
}

pub fn validate_log_path(path: &str) -> Result<(), LogManagerError> {
    let path = std::path::Path::new(path);
    
    if !path.exists() {
        return Err(LogManagerError::FileNotFound(path.display().to_string()));
    }

    if !path.is_file() {
        return Err(LogManagerError::InvalidPath(
            "Path does not point to a file".to_string()
        ));
    }

    // Check file permissions
    match std::fs::metadata(path) {
        Ok(metadata) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if metadata.permissions().mode() & 0o444 == 0 {
                    return Err(LogManagerError::PermissionDenied(
                        path.display().to_string()
                    ));
                }
            }
        }
        Err(e) => {
            return Err(LogManagerError::FileOpenError {
                path: path.display().to_string(),
                source: e,
            });
        }
    }

    Ok(())
}