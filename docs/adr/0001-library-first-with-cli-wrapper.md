# Library-first crate with optional CLI wrapper

PortPilot is designed as a reusable library crate with a small CLI binary layered on top, instead of a CLI-only architecture. This keeps process/port logic testable and embeddable in other developer tooling while preserving a practical command-line experience for direct use.
