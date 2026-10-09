# PortPilot

PortPilot is a reusable Rust toolkit and CLI for managing local ports and processes:

> Find it. Inspect it. Kill it.

## Install

### Library

```bash
cargo add portpilot
```

### CLI (from source)

```bash
cargo install --path . --features cli
```

## Quickstart (library)

```rust
use portpilot::{PortPilot, models::TerminationTarget};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pilot = PortPilot::default();

    for binding in pilot.list_ports()? {
        println!("{} {} {} {}", binding.port, binding.pid, binding.process, binding.status.unwrap_or_default());
    }

    let web = pilot.find_by_port(3000)?;
    println!("pid on 3000 = {}", web.pid);

    let nodes = pilot.find_by_process("node")?;
    println!("node bindings = {}", nodes.len());

    let details = pilot.inspect_pid(web.pid)?;
    println!("cmd = {}", details.command);

    // Non-interactive on purpose; CLI should confirm with the user.
    // pilot.terminate(TerminationTarget::Pid(web.pid))?;
    let _target = TerminationTarget::Port(3000);
    Ok(())
}
```

## CLI commands

```bash
portpilot list
portpilot list --search node
portpilot port 3000
portpilot process node
portpilot inspect 18231
portpilot kill 3000
portpilot watch --interval-ms 1000
portpilot dev
portpilot dev add rabbitmq
portpilot dev remove rabbitmq
portpilot dev up
portpilot dev down
```

Example `portpilot.toml`:

```toml
[[dev.services]]
name = "postgres"

[[dev.services]]
name = "redis"
```

Images and default ports come from `catalog.toml` (a `catalog.toml` in the current directory overrides the built-in one):

```toml
[[services]]
name = "redis"
image = "redis:latest"
default_port = 6379
```

`dev up` / `dev down` start/stop each service as Docker container `portpilot-<name>`.

## Design notes

- **Library-first** API for reuse in other tools.
- CLI is a thin wrapper (`src/bin/portpilot.rs`) with confirmation UX for `kill`.
- Public API is semver-oriented: `list_ports`, `find_by_port`, `find_by_process`, `inspect_pid`, `terminate`.

## Feature coverage
- **Library-first** API for reuse in other tools.
- Public API is semver-oriented: `list_ports`, `find_by_port`, `find_by_process`, `inspect_pid`, `terminate`.
- CLI is a thin wrapper (`src/bin/portpilot.rs`) with confirmation UX for `kill`.
- CPU/Memory: included in `list`, `process`, `port`, and `inspect` output.
- Interactive TUI: `portpilot watch` with navigation, inspect shortcut, and explicit kill confirmation.
- Dev services: `portpilot dev` reads `portpilot.toml` (auto-creates an empty service list) and reports running/stopped status by port.

## Testing

For local smoke tests:

```bash
cargo test
cargo run -- list
cargo run -- dev
```

For automated cross-platform validation, CI matrix is included at:

`/.github/workflows/cross-platform-ci.yml`

It runs on:
- `macos-latest`
- `ubuntu-latest`
- `windows-latest`
