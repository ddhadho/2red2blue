Purpose: Explain **watchdog thread and behavior**.

## A lightweight thread monitors heartbeat from main loop. If two intervals pass with no heartbeat, the daemon is killed and restarted.

```rust
struct Watchdog {
    fn start(interval: Duration, sender: HeartbeatSender);
    fn heartbeat(&self);  // called by main loop every cycle
}
```

**On OpenWRT**: uses /dev/watchdog kernel timer
**On Linux**: systemd restart handled via Restart=always