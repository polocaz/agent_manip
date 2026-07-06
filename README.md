# agent_manip — LSI Agent Manager (lsman)

`agent_manip` (binary: `lsman`) is a command-line tool for debugging and managing
the Lakeside Software (SysTrack) telemetry agent. It has two faces:

- **Subcommands** — script-friendly access to daemon status, logs, trace levels,
  the agent database, and crash reports, plus ticket-workflow commands:
  grouped error triage, a pasteable diagnostic report, a support-case bundle,
  log-site → source resolution, and a local web dashboard.
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
lsman errors                 # error lines grouped by source site (count, first/last seen)
lsman errors --since 24h     # only the last 24h (also 90s / 30m / 7d)
lsman errors --raw           # classic behavior: raw matching lines
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

### Customer-case workflow

```bash
sudo lsman report --since 24h          # markdown diagnostic report -> paste into the ticket
sudo lsman report -o report.md         # or write it to a file
sudo lsman collect                     # full diag bundle: lsman-diag-<host>-<stamp>.tar.gz
sudo lsman collect --max-log-mb 20 --since 7d   # cap copied log tails; scope the triage
lsman where "dbConnNix(1119)"          # resolve a log site to the agent source (+ context)
lsman where "07-05 02:22:05 dbConnNix(1119) -E Coll 1 no such table"   # paste a whole line
sudo lsman serve                       # read-only web dashboard at http://127.0.0.1:7171
```

- `report` and `collect` automate the runbook's first triage pass: daemon
  state, uplink, trace level, error lines grouped by `<file>(<line>)` source
  site with counts and first/last-seen times, plus crash reports. `collect`
  packs it all (report, config, tail-capped log copies, crash reports, socket
  and service state) into one `.tar.gz` to attach to the case — there is no
  other mac/linux agent diag collector.
- `where` searches the agent source checkout (`$LSMAN_AGENT_SRC`, or
  `~/src/systrack/Agent`, or `~/src/systrack-col1710/Agent`) and prints the
  emitting code. Sites are matched extensionless (`dbConnNix` →
  `dbConnNix.cpp`), the way the agent actually logs them.
- `serve` is read-only (no daemon control, no config writes) and binds
  127.0.0.1 by default; use an SSH port forward to watch a remote box.
```bash
ssh -L 7171:127.0.0.1:7171 user@customer-box sudo lsman serve
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
- `cli.rs` — subcommand implementations, incl. `where` source resolution
- `triage.rs` — agent log-line parsing (UTC timestamps, `<file>(<line>)` sites)
  and error grouping
- `report.rs` — diagnostic snapshot: gathered once, rendered as markdown
  (`report`), JSON (`serve`), or bundled (`collect`)
- `collect.rs` — support-case `.tar.gz` bundle
- `serve.rs` + `assets/dashboard.html` — read-only web dashboard (`tiny_http`)
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
