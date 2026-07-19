# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

`agent_manip` builds a binary named **`lsman`** — a debugging tool for the Lakeside Software (SysTrack) telemetry agent (`lsiagentd`). Three modes: script-friendly CLI subcommands (`lsman status|logs|errors|trace|config|db|crashes|net|start|stop|restart|report|collect|where`), a web dashboard (`lsman serve`), and a Pip-Boy-styled ratatui TUI when run with no arguments.

The agent being debugged lives in `~/src/systrack-col1710/Agent/` (see its CLAUDE.md and the repo's `agent-bug-investigation` skill). Facts about the agent install (paths, launchd label, log names, config format) were verified against that source tree — **`src/paths.rs` is the single source of truth for them here**; never hardcode agent paths elsewhere.

## Commands

```bash
cargo build                    # debug build
cargo build --release          # release binary at target/release/lsman
cargo test                     # unit tests (cfg parsing, lsof parsing)
cargo test <test_name>         # run a single test
cargo clippy -- -D warnings    # lint; CI fails on any warning
```

CI (`.github/workflows/rust.yml`) runs build, test, and `clippy -- -D warnings` on Ubuntu for pushes/PRs to `main`.

**Running:** works without root but most agent files (base dir `/Library/Application Support/Lakeside Software` on macOS, `/var/opt/lsiagent` on Linux) are root-owned, and sysinfo/lsof can't inspect the root daemon as a normal user — use `sudo` for real data. The TUI needs a real TTY. Quick smoke test on a machine with the agent installed: `./target/debug/lsman status`.

## Architecture

- `main.rs` — clap `Cli` parsing; a subcommand dispatches to `cli::run`, no subcommand enters the TUI event loop (60 FPS draw / 1ms input poll / 100ms tick, gated to 1s refresh in `App::on_tick`).
- `cli.rs` — subcommand implementations, plus the `lsiagent.cfg` trace-level parsing/editing (`read_log_level`/`set_log_level`, unit-tested) and `where` (log site → agent source checkout: `$LSMAN_AGENT_SRC`, `~/src/systrack/Agent`, `~/src/systrack-col1710/Agent`). Log reading is **streamed/bounded** — the main agent log can be hundreds of MB; never `read_to_string` an agent log.
- `triage.rs` — agent log-line parsing (timestamp/site/level) and error grouping by source site (unit-tested). This is where log-format knowledge lives.
- `report.rs` — `DiagSnapshot`: one gather pass serialized as markdown (`lsman report`), JSON (`serve`), or into the `collect` bundle. `scan_errors*` streams whole logs — never call it per poll/tick.
- `collect.rs` — support-case `.tar.gz` bundle (report + config + tail-capped log copies + crash reports + lsof/service state); shells out to `tar`.
- `serve.rs` + `assets/dashboard.html` (embedded via `include_str!`) — `tiny_http` on 4 threads. JSON endpoints (`/api/snapshot`, `/api/errors` incl. per-site trend buckets, `/api/log` — full tail or incremental follow via `?offset=`, `/api/crash?name=` and `/api/db/tables`+`/api/db/table?name=` — both resolve names against known lists only, never arbitrary paths/SQL), streamed file downloads (`/api/download?file=cfg|db|profile|log:<name>` — known keys only; the live DB is served as a `sqlite3 .backup` snapshot; `/api/collect` streams a full support bundle from a self-cleaning temp dir), daemon control (POST `/api/daemon/start|stop|restart`), and a config editor (GET/POST `/api/config`, POST `/api/config/trace?level=` — writes are atomic temp+rename and **refused with 409 while the daemon is running**, since the agent only reads lsiagent.cfg at startup). All mutating POSTs require an `X-Lsman-Control` header so cross-origin pages can't trigger them. Log tails read at most the last 1 MB per request.
- `paths.rs` — agent install facts: base dir, `known_logs()` (short name → path map, incl. dynamic per-user logs on macOS), config/db/profile/binary paths, launchd label, crash-report dirs.
- `daemon.rs` — `DaemonManager`: finds the process via sysinfo name match, controls it through `ServiceManager` (launchctl on macOS / systemctl on Linux / direct fallback). The macOS daemon is a **KeepAlive LaunchDaemon** — stop must go through `launchctl bootout`; a plain kill gets respawned. `update_status()` also caches the service-manager status text so the render loop never shells out.
- `network.rs` — real socket state via `lsof -nP -a -i -p <pid>` (parser unit-tested). An ESTABLISHED TCP connection is the practical "uplink to master is up" signal.
- `app.rs` — TUI state + input. `current_log_file` is an index into `paths::known_logs()`.
- `ui.rs` — rendering. Log tail reads are capped at 1 MB (`read_file_tail`).

### Agent facts baked into this tool

- Config `<base>/lsiagent.cfg`; trace level = `LogLevel2` in `[Debug]` (0=none, 1=err, 2=warn, 3=info default, 4-8=trace1..5); non-zero local value overrides master config.
- DB `<base>/database/collect.sqlite3` — always open read-only (`sqlite3 -readonly`) against a live agent.
- Master-delivered profile blob at `<base>/profile` (root-only perms; verified on a live macOS agent).
- Log timestamps are UTC; each line embeds `<file>(<line>)` pointing into the agent source; error lines carry ` -E `.
- Verified against a live agent: the site is logged **without the file extension** (`dbConnNix(1119)`, not `dbConnNix.cpp(1119)`), padded into a fixed-width column that **truncates long names** (`threadStatusBarBridge(75` — no closing paren, line digits cut). `triage.rs` handles both; `where` matches stems against `.cpp/.c/.cc/.h/.hpp/.m/.mm`.
- macOS crash reports land in `/Library/Logs/DiagnosticReports` (daemon) and `~/Library/Logs/DiagnosticReports` (LsiStatusBar / SysTrackManagementUser) — the agent installs no crash handler.

### Gotchas

- Mouse hit-testing in `app.rs::on_mouse` assumes fixed layout rows (main tabs rows 5–7, log-tab strip row 8) and mirrors label widths via `ui::log_tab_hit` — changing the layout in `ui.rs` breaks these; update together.
- Non-root on macOS: sysinfo returns zeroed stats (memory 0, start_time 0) for the root daemon and lsof returns nothing — code must treat those as "unavailable, hint sudo", not as real values.
- `src_old/` is a previous implementation kept for reference — not part of the build. Don't extend it.

## Reference material

- `docs/prd.md` — original PRD (aspirational; much of it is not implemented).
- `~/src/systrack-col1710/.claude/skills/agent-bug-investigation/SKILL.md` — the agent-side debugging runbook this tool automates.
