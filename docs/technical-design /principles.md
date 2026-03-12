# Principles

- **Zero cloud** — all data is processed and stored locally, no internet required
- **Deterministic behaviour** — given the same inputs the system produces the same outputs
- **Crash recovery** — the system replays its WAL on boot and resumes from exactly where it stopped
- **Runs at the edge** — designed for low-power hardware in the home, not data center infrastructure
- **Confidence over assumption** — device state is tracked with a confidence score; uncertainty triggers safe defaults, not guesses
- **Safe defaults** — when the system cannot determine correct state, it fails safe rather than fails silently
- **Adapter abstraction** — device protocols are isolated behind a trait boundary; the connectivity layer is replaceable