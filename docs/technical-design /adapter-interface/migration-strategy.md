When ready to drop HA, you write this adapter, update one line in config:

```rust
[adapter]
kind = "zigbee2mqtt"
url = "mqtt://localhost:1883"
```