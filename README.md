# Agent_Manip

A Rust-based GUI application for managing and monitoring the agent. Main goal is to provide a similar experience to the current utils SystrackSQL and LogView but for MacOS and Linux. Compatability with windows would be a plus but not the main goal.

## Planned features

- Real-time log monitoring with automatic updates
- Database management
  - Basic ops like reads and writes
  - Automatic string id mapping
- Efficient log file parsing and display
- Status monitoring of the agent
- Agent controls
  - Start
  - Stop
  - Read config
  - Inventory
  - Condense
- Agent statistics at a glance
  - avg cpu in past x time
  - Errors in past x time
  - iops 
- Configuration monitoring and overriding

## Prerequisites

- Rust 1.70.0 or higher
- Cargo package manager

## Installation

1. Clone the repository:
```bash
git clone <repository-url>
cd agent_manip
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