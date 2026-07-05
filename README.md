# agent_manip — LSI Agent Manager (lsman)

`agent_manip` (binary: `lsman`) is a command-line tool for debugging and managing
the Lakeside Software (SysTrack) telemetry agent. It has two faces:

- **Subcommands** — script-friendly access to daemon status, logs, trace levels,
  the agent database, and crash reports.
- **TUI** (run with no arguments) — a retro Pip-Boy style dashboard with
  real-time metrics, network state, log viewing, and daemon control.

Many agent files (`/Library/Application Support/Lakeside Software` on macOS,
`/var/opt/lsiagent` on Linux) are root-owned; run with `sudo` for full access.
`lsman` works without root but tells you what it can't see.

## Install

```bash
cargo build --release
# binary at target/release/lsman
```

## CLI usage

```bash
lsman status                 # daemon state, pid, resources, uplink, config, log freshness
lsman logs                   # list the agent's log files (name, size, freshness)
lsman logs agent -n 100      # last 100 lines of the main daemon log
lsman logs agent -f          # follow (tail -f), handles rotation
lsman logs webcom -e         # only error/crash lines from the uplink log
lsman errors                 # triage scan across the main agent logs
lsman config                 # print lsiagent.cfg
lsman trace                  # show current trace level
sudo lsman trace trace5 --restart   # set [Debug] LogLevel2=8 and bounce the daemon
sudo lsman db                # list tables in collect.sqlite3 (always read-only)
sudo lsman db "SELECT count(*) FROM SAAPP"
lsman crashes                # recent lsiagentd/LsiStatusBar crash reports (macOS)
lsman crashes --show latest  # print the newest one
sudo lsman net               # the daemon's open sockets (uplink check)
sudo lsman start|stop|restart  # via launchctl (macOS) / systemctl (Linux)
```

Notes for debugging sessions:

- **Log timestamps are UTC** — convert before matching customer-reported times.
- Each agent log line embeds `<file>(<line>)` — a direct pointer into the agent
  source; grep the message text in the `Agent/` tree to find the emitting code.
- Trace levels: 0=none, 1=error, 2=warning, 3=info (default), 4–8=trace1–trace5.
  A non-zero local `LogLevel2` overrides the master-pushed config.
- `lsman db` always opens the database read-only, so it is safe against a live
  agent.

## TUI usage

```bash
sudo ./target/release/lsman
```

Keyboard shortcuts

- Tab navigation:
  - `F1`..`F6`: Jump to specific tabs (Overview, Resources, Network, Logs, Config, Settings)
  - `Tab` / `Shift+Tab`, `h` / `l`: Cycle tabs
- Scrolling: `Up`/`Down`, `PageUp`/`PageDown`, `j`/`k` (half-page)
- Logs tab: `←`/`→` switch log file, `0-9` jump to the Nth available log
- Daemon control: `s` start, `x` stop, `r` refresh
- `q` / `Esc`: Quit

## Daemon detection & management

`lsman` finds the agent process (`lsiagentd` / anything matching `lsiagent`) via
system process inspection and manages it through the platform service manager:

- **macOS**: `launchctl` against `system/com.lakesidesoftware.lsiagentd`
  (bootstrap/bootout/kickstart). The daemon is a KeepAlive LaunchDaemon, so
  killing the process directly would just get it respawned — stop goes through
  `bootout`.
- **Linux**: `systemctl` against the `lsiagent` service when systemd is present.
- Fallback: direct spawn / SIGTERM.

## Architecture

- `main.rs` — clap CLI parsing; no subcommand → TUI event loop
- `cli.rs` — all subcommand implementations
- `paths.rs` — platform facts: base dir, log map, config/db paths, launchd label
- `app.rs` — TUI state and input handling
- `daemon.rs` — process discovery and service-manager control
- `network.rs` — real socket state via `lsof`
- `ui.rs` — rendering with `ratatui`

## Development

```bash
cargo test
cargo clippy -- -D warnings
```

## License

MIT
