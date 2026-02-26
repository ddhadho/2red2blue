
## Directory Structure

Purpose: Explain **disk layout and paths**.

```
/data/smarthome/
├── config.toml          ← daemon config
├── rules.toml           ← rule definitions
├── devices.toml         ← device registry
├── wal/
│   ├── wal.db           ← SQLite WAL database
│   └── wal.db-wal       ← SQLite WAL journal
├── snapshots/
│   ├── snap_0001042.bin ← snapshot at sequence 1042
│   └── snap_0002187.bin ← snapshot at sequence 2187
└── logs/
    └── daemon.log
```

- `/data` maps to SSD on a Pi or eMMC on OpenWRT.  
- The daemon reads paths from config; upper layers never access disk directly.