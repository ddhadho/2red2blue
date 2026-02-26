Purpose: Explain **Platform trait and implementations**.

```rust
trait Platform {
    fn storage_root(&self) -> PathBuf;
    fn log_output(&self) -> LogOutput;
    fn memory_limit_mb(&self) -> u32;
    fn watchdog(&self) -> Box<dyn Watchdog>;
    fn network_interface(&self) -> String;
}

struct LinuxPlatform {
    config: PlatformConfig,
}

struct OpenWRTPlatform {
    config: PlatformConfig,
}

impl Platform for LinuxPlatform { ... }
impl Platform for OpenWRTPlatform { ... }
```

Daemon receives Box<dyn Platform> at startup via dependency injection
Only one trait, two implementations — hardware-specific code is isolated