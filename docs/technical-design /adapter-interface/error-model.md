# Error handling at the adapter boundary

```rust
enum AdapterError {
    ConnectionFailed(String),
    Disconnected,
    Timeout,
    AuthenticationFailed,
    DeviceNotFound(String),
    CommandRejected(String),
    ParseError(String),
}
```

The daemon handles each error type differently. `Disconnected` triggers a reconnect loop with exponential backoff. `Timeout` on a command increments the retry counter. `AuthenticationFailed` is a fatal startup error — log clearly and exit, don't retry silently. `ParseError` logs the raw payload and drops the event — never crash on a malformed event from a device.