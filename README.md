# Telemetry Daemon Manager

A terminal-based TUI application for monitoring and managing a Rust-based telemetry daemon. Built using the ratatui library, this tool provides real-time insights into the daemon's resource usage, connection status, and operational state, while offering management capabilities to start, stop, and troubleshoot the telemetry agent.

## Features

- **Real-time Process Monitoring**: CPU, memory, disk I/O, and network usage tracking
- **Daemon Management**: Start, stop, and restart telemetry daemons
- **Connection Monitoring**: WebSocket connection status and health metrics
- **Network Traffic Analysis**: Data flow verification and traffic statistics
- **Tabbed Interface**: Overview, Resources, Network, Logs, and Settings views
- **Keyboard Navigation**: Intuitive shortcuts for all operations
- **Systemd Integration**: Automatic systemctl usage on Linux systems for proper service management

## Prerequisites

- Rust 1.85.0 or higher
- Cargo package manager

## Installation

1. Clone the repository:
```bash
git clone <repository-url>
cd telemetry-daemon-manager
```

2. Build the application:
```bash
cargo build --release
```

The compiled binary will be available in `target/release/telemetry-daemon-manager`

## Usage

1. Run the application:
```bash
./target/release/telemetry-daemon-manager
```

### Keyboard Shortcuts

- **Tab Navigation**:
  - `F1-F5`: Switch between tabs (Overview, Resources, Network, Logs, Settings)
  - `Tab`: Cycle forward through tabs
  - `Shift+Tab`: Cycle backward through tabs
  - **Vim-style**: `h` (previous tab), `l` (next tab), `j` (next tab), `k` (previous tab)

- **Daemon Control**:
  - `S`: Start daemon
  - `X`: Stop daemon
  - `R`: Manual refresh

- **Application**:
  - `Q` or `Esc`: Quit application

## Daemon Management

The application automatically detects and manages the Lakeside Software agent:

- **Process Detection**: Automatically finds running processes by name:
  - Linux/macOS: `lsiagentd`
  - Windows: `LsiAgent.exe`
- **Cross-platform Paths**:
  - Linux: `/opt/lsiagent/bin/lsiagentd`
  - macOS: `/Library/Application Support/Lakeside Software/lsiagentd`
  - Windows: `C:\Program Files\Lakeside Software\LsiAgent.exe`
- **Systemd Integration**: Uses `systemctl start/stop/status lsiagent` on Linux systems
- **Fallback**: Direct process management on non-systemd systems

The service status is displayed in the Overview tab, showing real-time systemctl status information when available.

## Architecture

The application consists of several key modules:

- **Main**: Terminal setup and event loop
- **App**: Application state and event handling
- **Daemon**: Process monitoring and daemon management
- **Network**: WebSocket connection and traffic monitoring
- **UI**: Terminal user interface rendering
- **Error**: Custom error types and handling
  - Errors in past x time
  - iops 
- Configuration monitoring and overriding

## Prerequisites

- Rust 1.81.0 or higher
- Cargo package manager

## Installation

1. Clone the repository:
```bash
git clone <repository-url>
cd telemetry-daemon-manager
```

2. Build the application:
```bash
cargo build --release
```

The compiled binary will be available in `target/release/agent_manip`

## Usage

1. Run the application:
```bash
./target/release/agent_manip
```
2. The application will point to the default locations of agent files, but they can be chosen via file picker
  a. TODO: Load file paths from 

## Development

- The application is structured into four main modules:
  - `agent_monitor` : Agent service interaction
  - `db`: Database interaction layer
  - `log_reader`: Log file parsing and monitoring
  - `ui`: User interface components

- To run tests:
```bash
cargo test
```

## License

MIT License 