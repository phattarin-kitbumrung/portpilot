# Explicit kill confirmation in interactive modes

PortPilot requires explicit user confirmation before terminating processes from interactive flows such as `watch` and `dev`-driven remediation hints. This prioritizes safety over speed to reduce accidental process termination when users are operating in high-churn local development environments.
