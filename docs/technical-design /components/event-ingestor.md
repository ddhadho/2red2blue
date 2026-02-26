# Sits at the boundary between HA and the system. Its job is normalization and deduplication.

## Trait Definition

```rust
trait EventIngestor {
    fn ingest(&mut self, raw: RawHAEvent) -> Result<Option<Event>, IngestError>;
}
```

## Deduplication and Normalization

The `ingest` method returns `Option<Event>` — it can swallow a raw event if it's a duplicate or not relevant. The deduplication window is 50ms — if the same device reports the same state twice within 50ms, the second one is dropped. HA can be chatty.

Normalization means mapping HA's entity model to the `DeviceId` and `AttributeKey` model. This is the only place in the system that knows what an HA `entity_id` looks like. Everything above this layer speaks our internal model only.

## Normalization Map

```rust
struct NormalizationMap {
    entries: HashMap<HAEntityId, (DeviceId, AttributeKey)>,
}
```

This map is configuration — loaded at startup, defines the relationship between HA's world and ours. When we replace HA with native adapters in V2, we replace this map and the raw event format. Nothing else changes.