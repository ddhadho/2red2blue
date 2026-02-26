# UI Server

The `UIServer` provides a web interface for monitoring and managing the system.

## Trait Definition

```rust
trait UIServer {
    fn start(&self, port: u16);
}
```

## Endpoints

*   `GET /state` → all device states
*   `GET /state/:device_id` → single device state
*   `GET /events?from=seq` → WAL entries from sequence
*   `GET /rules` → all rules and their current state
*   `GET /conflicts` → recent conflict records
*   `POST /rules` → add or update a rule
*   `POST /devices/:id/command` → manual command (for UI control)

## UI Server Role

The UI server reads from the state engine and WAL. It never writes to the state engine directly — manual commands go through the normal command dispatcher pipeline so they're logged, confirmed, and conflict-resolved like any other command.

## V1 UI Scope

For V1 the UI is minimal — live device state, event log, rule list, conflict log. No dashboards, no graphs, no mobile app. Observability tooling first, polish later.

