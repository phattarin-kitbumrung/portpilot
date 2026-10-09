# PortPilot

PortPilot is a domain for discovering, understanding, and controlling local machine port occupancy by process identity.

## Language

**Port Binding**:
A local network port currently associated with a process on a host.
_Avoid_: Open port record, socket line

**Process Inspection**:
The retrieval of metadata about a process, including command, owner, and related ports.
_Avoid_: Process dump, process peek

**Termination Target**:
An explicit selector for the process to terminate, represented as either a PID or a bound port.
_Avoid_: Kill input, target value

**Platform Adapter**:
An operating system-specific implementation that performs process and socket operations behind a shared interface.
_Avoid_: OS helper, platform script

**Port Discovery**:
The act of listing active port bindings with protocol and process attribution.
_Avoid_: Scan, netstat wrapper

**Resource Snapshot**:
A point-in-time measurement of process resource usage, including CPU and memory.
_Avoid_: Live profile, trend

**Watch Session**:
An interactive terminal session that continuously refreshes and displays current port bindings.
_Avoid_: Dashboard mode, stream

**Dev Diagnostics**:
A targeted check for likely conflicts between common development services and currently occupied ports.
_Avoid_: Environment lint, stack scan
