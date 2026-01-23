# agent_manip — LSI Agent Manager (lsman)

`agent_manip` (binary: `lsman`) is a terminal-based TUI for monitoring and managing the Lakeside Software telemetry agent. It provides a compact, retro-style Pip-Boy inspired interface for real-time system metrics, network/connection status, and log inspection, plus basic daemon control (start/stop/restart).

## Features

- **Real-time Process Monitoring**: CPU, memory, disk I/O, thread count, and file handles tracking
- **Daemon Management**: Start, stop, and restart telemetry daemons
- **Connection Monitoring**: WebSocket connection status and health metrics
- **Network Traffic Analysis**: Data flow verification and traffic statistics
- **Tabbed Interface**: Overview, Resources, Network, Logs, and Settings views
- **Keyboard Navigation**: Intuitive shortcuts for all operations
- **Systemd Integration**: Automatic systemctl usage on Linux systems for proper service management

## Prerequisites

- Rust (stable) and Cargo

## Installation

Clone and build:
```bash
git clone <repository-url>
cd agent_manip
cargo build --release
```

The release binary will be at `target/release/lsman`.

## Usage

Run the UI (use `sudo` if you need access to system log files):

```bash
./target/release/lsman
```

Keyboard shortcuts

- Tab navigation:
  - `F1`..`F6`: Jump to specific tabs
  - `Tab` / `Shift+Tab`: Cycle tabs
  - `h` / `l`: Move one tab left/right
- Scrolling:
  - `Up` / `Down`: Line-by-line scroll
  - `PageUp` / `PageDown`: Page scroll
  - `j` / `k`: Half-page down/up (in scrollable views)
- Daemon control:
  - `s`: Start daemon
  - `x`: Stop daemon
  - `r`: Manual refresh
- Misc:
  - `q` / `Esc`: Quit

## Daemon detection & management

`lsman` looks for common LSI agent process names and paths on each platform and surfaces service status in the Overview tab. On Linux with systemd, it will use `systemctl` when available; otherwise it falls back to direct process inspection.

## Architecture

High-level modules:

- `main.rs` — terminal setup and event loop
- `app.rs` — application state, input handling, and high-level behavior
- `daemon.rs` — process discovery and control
- `network.rs` — connection monitoring
- `ui.rs` — rendering with `ratatui`
- `log_reader` / cache — efficient log tailing and parsing

## Development

Contributions welcome. Run tests and linters with:

```bash
cargo test
cargo clippy -- -D warnings
```

If you plan to change UI layout, see `src/ui.rs` for rendering helpers and `src/app.rs` for input handling.

## License

MIT