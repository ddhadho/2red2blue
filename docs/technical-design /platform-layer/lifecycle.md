Purpose: Document daemon start/stop and startup sequence.

## Daemon Lifecycle Interface

The daemon is a single binary. No frameworks or service meshes.

```rust
struct DaemonLifecycle {
    fn run(config: Config) -> Result<(), DaemonError>;
    fn shutdown(&self, reason: ShutdownReason);
}
```

## Shutdown Reasons

```rust
enum ShutdownReason {
    Signal(Signal),      // SIGTERM, SIGINT
    FatalError(String),  // unrecoverable internal error
    WatchdogTimeout,     // watchdog kicked us
}
```

## Startup Sequence

1.  Load and validate `config.toml`.
2.  Load device registry from `devices.toml`.
3.  Open WAL database.
4.  Load latest snapshot.
5.  Replay WAL from snapshot sequence.
6.  Load and validate `rules.toml` against device registry.
7.  Connect to adapter layer — wait with retry.
8.  Boot reconciliation.
9.  Start UI server.
10. Start watchdog.
11. Enter main event loop.
12. On `SIGTERM` → flush WAL → close DB → exit clean.
