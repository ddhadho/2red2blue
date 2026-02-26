┌─────────────────────────────────────────────────────┐
│                   PHYSICAL WORLD                     │
│  Gates · Lights · Pumps · Cameras · Alarms · Plugs  │
└─────────────────────────────┬───────────────────────┘
                              │ zigbee / wifi / serial
┌─────────────────────────────▼───────────────────────┐
│              DEVICE ADAPTER LAYER                    │
│         Home Assistant (V1) / Native (V2)            │
│  Owns: device protocols, raw events, raw commands    │
│  Ignorant of: rules, state, WAL, business logic      │
└─────────────────────────────┬───────────────────────┘
                              │ normalized events
┌─────────────────────────────▼───────────────────────┐
│                EVENT INGESTOR                        │
│  Owns: normalization, dedup, event schema validation │
│  Ignorant of: rules, state, what device it came from │
└──────────────┬──────────────┬───────────────────────┘
               │              │
               ▼              ▼
┌──────────────────┐  ┌───────────────────────────────┐
│      WAL         │  │         STATE ENGINE           │
│                  │  │                                │
│ Owns: durable    │  │ Owns: world model              │
│ append-only log  │  │ desired vs actual              │
│ sequence numbers │  │ confidence per device          │
│ crash recovery   │  │ last known good state          │
│                  │  │                                │
│ Ignorant of:     │  │ Ignorant of: rules, commands   │
│ what events mean │  │ protocols, WAL internals       │
└──────────────────┘  └───────────────┬───────────────┘
                                      │ world model
                      ┌───────────────▼───────────────┐
                      │         RULE ENGINE            │
                      │                                │
                      │ Owns: evaluating rules against │
                      │ current world model            │
                      │ stateful rule tracking         │
                      │ producing candidate commands   │
                      │                                │
                      │ Ignorant of: how commands are  │
                      │ sent, device protocols, WAL    │
                      └───────────────┬───────────────┘
                                      │ candidate commands
                      ┌───────────────▼───────────────┐
                      │       CONFLICT RESOLVER        │
                      │                                │
                      │ Owns: priority model           │
                      │ deciding winner when rules     │
                      │ compete for same device        │
                      │ logging losers                 │
                      │                                │
                      │ Ignorant of: why rules exist   │
                      │ device protocols, WAL          │
                      └───────────────┬───────────────┘
                                      │ resolved commands
                      ┌───────────────▼───────────────┐
                      │      COMMAND DISPATCHER        │
                      │                                │
                      │ Owns: sending commands down    │
                      │ to adapter layer               │
                      │ tracking command confirmation  │
                      │ retry logic                    │
                      │ logging unconfirmed commands   │
                      │                                │
                      │ Ignorant of: rules, state,     │
                      │ conflict resolution            │
                      └───────────────┬───────────────┘
                                      │
                      ┌───────────────▼───────────────┐
                      │     RECONCILIATION LOOP        │
                      │                                │
                      │ Owns: boot sequence            │
                      │ WAL replay → desired state     │
                      │ device poll → actual state     │
                      │ diff → recovery commands       │
                      │ ongoing confidence monitoring  │
                      │                                │
                      │ Runs: on boot + continuously   │
                      └───────────────┬───────────────┘
                                      │
                      ┌───────────────▼───────────────┐
                      │       LOCAL UI SERVER          │
                      │                                │
                      │ Owns: serving web interface    │
                      │ live device state display      │
                      │ event log view                 │
                      │ rule management                │
                      │                                │
                      │ Ignorant of: everything inside │
                      │ the daemon. reads state only   │
                      │ via internal API               │
                      └───────────────────────────────┘


## Complete Internal Flow

```
HA websocket event arrives
→ EventIngestor.ingest() → normalized Event
→ WAL.append(event) → sequence number assigned
→ StateEngine.apply_event(event) → StateUpdate
→ RuleEngine.evaluate(update, state) → Vec<Command>
→ ConflictResolver.resolve(commands) → ResolvedCommands
→ WAL.append(conflict_records)
→ CommandDispatcher.dispatch(winners)
→ WAL.append(command, status=Pending)
→ HA REST API call
→ WAL.append(command, status=Sent)
→ ... HA confirms via websocket event ...
→ EventIngestor.ingest() → confirmation Event
→ WAL.append(confirmation)
→ StateEngine.apply_event(confirmation) → actual state updated
→ CommandDispatcher confirmation received → WAL status=Confirmed
```

### The boot sequence — this is where crash recovery live

Runs every single time the daemon starts, whether from a clean boot or a crash:

1. Open WAL → replay all entries → reconstruct desired state
2. Connect to device adapter layer → poll all devices → get actual state
3. Diff desired vs actual → generate reconciliation commands
4. Apply safe defaults for any device with unknown state
5. Begin normal event loop
