# LSAgent Manager

A Rust-based GUI application for managing and monitoring the LSAgent daemon program. This application provides a user-friendly interface for:

- Reading and monitoring log files (lsaigent1.log)
- Interacting with the SQLite database
- Managing agent settings and configuration

## Features

- Real-time log monitoring with automatic updates
- Database management interface
- Clean and modern UI built with egui
- Efficient log file parsing and display
- SQLite database integration

## Prerequisites

- Rust 1.70.0 or higher
- Cargo package manager

## Installation

1. Clone the repository:
```bash
git clone <repository-url>
cd lsagent_manager
```

2. Build the application:
```bash
cargo build --release
```

The compiled binary will be available in `target/release/lsagent_manager`

## Usage

1. Make sure you have the following files in your working directory:
   - `lsaigent1.log` - The log file to monitor
   - `agent.db` - The SQLite database file (will be created if it doesn't exist)

2. Run the application:
```bash
./target/release/lsagent_manager
```

## Development

- The application is structured into three main modules:
  - `db`: Database interaction layer
  - `log_reader`: Log file parsing and monitoring
  - `ui`: User interface components

- To run tests:
```bash
cargo test
```

## License

MIT License 