# Memory Architecture Design — NeuroGenesis AI

## 1. Storage Ownership Architecture

Rust (`src/store.rs`) exclusively owns all database interactions, schema definitions, and migrations using `rusqlite` with Write-Ahead Logging (WAL) and foreign keys enabled. Python is strictly an isolated worker process for computational tool execution (`engine/app.py`) and has zero database access or credentials.

## 2. Milestone 1.3 Memory Subsystems

### Working Memory Subsystem (`src/cognition/working_memory.rs`)
- **Bounded Capacity:** Configurable capacity (default: 64 items, bounds: 4..=512) preventing memory bloat.
- **Activation Dynamics:** Items possess an activation value in $[0.0, 1.0]$. Activation decays across executive cycles and is reinforced upon retrieval.
- **Retention & Deterministic Eviction:** Eviction is governed by a deterministic retention formula combining activation, explicit priority, epistemic status, and unresolved status.
- **Protection Rules:** Pinned constraints and unresolved contradictions are strictly protected from eviction.
- **Snapshot & Restoration:** Working memory state can be serialized into a `WorkingMemorySnapshot` and faithfully restored across process restarts.

### Episodic & Event Memory (`src/cognition/events.rs` & `src/store.rs`)
- **Append-Only Auditable Event Log:** Every state transition, goal status update, memory eviction, and executive action produces a typed `CognitiveEvent` persisted to SQLite's `cognitive_events` table.
- **Versioned Persistence & Recovery (M1.3.1):** SQLite schema version 2 persists `cognitive_state_json` alongside investigation history, supporting seamless resumption of interrupted investigations. `from_existing_state` preserves historical execution budgets (`total_actions_executed`) and loop detection history to prevent loop recurrence upon restart.

## 3. Future Memory Horizons (Milestones 1.4+)

- **Semantic Memory:** Persistent scientific concepts and literature findings linked by epistemic support.
- **Procedural Memory:** Reusable problem-decomposition schemas and action sequences distilled from successful investigations.
- **Zep Integration:** Zep integration remains slated for future milestones when primary literature retrieval is established. When integrated, Zep v4 conventions (`zep-cloud`, `ZEP_API_KEY`) will be used to support long-term conversational memory, preserving local SQLite as the canonical ground truth. No Zep credentials are required for local operation.
- **Notion Exclusion:** Notion is permanently excluded.
