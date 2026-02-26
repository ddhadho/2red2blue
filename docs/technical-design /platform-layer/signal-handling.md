Purpose: Explain **signal handling**.


---



```rust
// SIGTERM or SIGINT
// 1. Stop accepting new events
// 2. Finish in-flight events
// 3. Flush WAL
// 4. Write final snapshot
// 5. Close DB
// 6. Exit 0

// SIGUSR1
// Hot-reload rules.toml without restart

// SIGUSR2
// Dump current state to log

Clean shutdown ensures WAL consistency.