Every log line is JSON via Rust `tracing` crate.

## JSON Log Example

```json
{
  "timestamp": "2026-02-25T08:42:11Z",
  "level": "info",
  "component": "state_engine",
  "event": "device_state_updated",
  "device_id": "gate_main",
  "attribute": "state",
  "old_value": "open",
  "new_value": "closed",
  "confidence": 1.0,
  "sequence": 4821
}
```

## Logging Features and Benefits

*   **Enables advanced queries:** The JSON format allows for easy filtering, searching (`grep`), or querying using tools like `jq` or log analysis platforms, based on any field (e.g., `device_id`, `event`, `level`).
*   **Rotating logs on Raspberry Pi:** Logs are configured to rotate, with a maximum size of 50MB and keeping the last 3 rotations. This prevents logs from consuming excessive disk space.
*   **OpenWRT logging to syslog:** On OpenWRT devices, logs are directed to `syslog` to protect the limited flash memory from excessive writes.
