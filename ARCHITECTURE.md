# NeuroGenesis AI Architecture
## Milestone 1.3 & 1.3.1: Cognitive Control Foundation & Runtime Integration

---

## 1. System Boundaries & Component Ownership

```
┌────────────────────────────────────────────────────────────────────────┐
│                        Rust Core (Axum / Tokio)                        │
│                                                                        │
│  ┌───────────────────────┐   ┌──────────────────────────────────────┐  │
│  │   HTTP & Static API   │   │      Investigation Orchestrator      │  │
│  │    (Contracts 1.0)    │   │  - Planning & Synthesis Routing      │  │
│  └───────────┬───────────┘   │  - Dynamic DAG Dependency Scheduler  │  │
│              │               │    (Tokio JoinSet, Concurrency 1..5) │  │
│              │               └──────────────────┬───────────────────┘  │
│              │                                  │                      │
│              │       ┌──────────────────────────┴───────────────┐      │
│              │       │       Cognitive Control Subsystem        │      │
│              │       │  - Bounded Working Memory Engine         │      │
│              │       │  - Hierarchical Goal & Agenda Manager    │      │
│              │       │  - Selective Attention & Prioritisation  │      │
│              │       │  - 10-Phase Executive Controller Loop    │      │
│              │       │  - Metacognitive Health Monitor          │      │
│              │       └──────────────────────────┬───────────────┘      │
│              │                                  │                      │
│              │       ┌──────────────────────────┴───────────────┐      │
│              │       │       Independent Verification Engine    │      │
│              │       │  - Newtonian Ballistics Kinematics       │      │
│              │       │  - Exact Arithmetic Validation           │      │
│              │       │  - Descriptive Statistics Invariants     │      │
│              │       │  - Power-Law Least-Squares Normal Eqns   │      │
│              │       └──────────────────────────┬───────────────┘      │
│              │                                  │                      │
│  ┌───────────▼───────────┐                      │                      │
│  │   SQLite Store v2     │◄─────────────────────┘                      │
│  │   (rusqlite, events)  │                                             │
│  └───────────────────────┘                                             │
└──────────────┬──────────────────────────────────┬──────────────────────┘
               │                                  │
     (Versioned JSON Stdin/Stdout)         (HTTP / JSON Mode)
               │                                  │
               ▼                                  ▼
┌───────────────────────────────┐  ┌─────────────────────────────────────┐
│  Python Scientific Worker     │  │  Model Provider                     │
│  (CPython 3.13 Virtualenv)    │  │  - OpenAI-compatible hosted API     │
│  - NumPy, SciPy, SymPy        │  │  - Offline deterministic mock       │
│  - Allowlisted Tools Only     │  │  - Bounded retries & token budget   │
└───────────────────────────────┘  └─────────────────────────────────────┘
```

### Component Roles

1. **Rust Core Application:**
   - **Cognitive Runtime & Executive Controller (`src/cognition/`):** Authoritative discrete cognitive loop (`CognitiveRuntime`), async execution dispatch (`AsyncActionExecutor`), working memory, attention prioritisation, hierarchical goals, completion criteria verification, and loop prevention.
   - **Investigation Orchestrator (`src/orchestrator.rs`):** Translates research plans into initial cognitive state, sets up structured goals and agenda items, and delegates lifecycle execution to the `CognitiveRuntime`.
   - **Deterministic Verification (`src/verification.rs`):** Independently evaluates computational outputs in native Rust without model self-verification.
   - **Persistence (`src/store.rs`):** Exclusively owns SQLite database reads, writes, schema migrations (version 2), and auditable append-only cognitive events.
2. **Python Scientific Computation Worker (`engine/app.py`):**
   - Pure mathematical calculations (NumPy, SciPy, SymPy) executed in a local subprocess over stdin/stdout.
   - Zero database permissions, zero network access, and zero credential storage.
3. **Model Provider Transport (`src/provider.rs`):**
   - Supplies structured hypothesis proposals and language decomposition.
   - Strictly subservient to Rust validation and cognitive state boundaries.

---

## 2. Cognitive Control Flow

The cognitive architecture operates a discrete 10-phase control cycle:

```
  ┌──────────────┐
  │   OBSERVE    │ ── Inspect working memory, active goals, and contradictions
  └──────┬───────┘
         ▼
  ┌──────────────┐
  │ EVALUATE     │ ── Evaluate completion criteria for active goals
  │ GOALS        │    (Requires verified calculations, tools, or subgoals)
  └──────┬───────┘
         ▼
  ┌──────────────┐
  │ UPDATE       │ ── Transition goals: Pending -> Active (deps satisfied)
  │ STATE        │    or Pending -> Blocked (deps failed)
  └──────┬───────┘
         ▼
  ┌──────────────┐
  │ ATTENTION &  │ ── Score candidate goals via readiness, contradictions,
  │ PRIORITY     │    priority, uncertainty, and unknown variables
  └──────┬───────┘
         ▼
  ┌──────────────┐
  │ SELECT       │ ── Pick highest priority valid action from agenda or goal criteria
  │ ACTION       │    (Check rolling loop detector)
  └──────┬───────┘
         ▼
  ┌──────────────┐
  │ VALIDATE     │ ── Enforce strict precondition allowlist
  │ PRECONDS     │
  └──────┬───────┘
         ▼
  ┌──────────────┐
  │ EXECUTE      │ ── Dispatch calculation, independent verification, or state update
  │ ACTION       │
  └──────┬───────┘
         ▼
  ┌──────────────┐
  │ EVALUATE     │ ── Record action result, measure latency, track failure counts
  │ OUTCOME      │
  └──────┬───────┘
         ▼
  ┌──────────────┐
  │ UPDATE       │ ── Insert knowledge items, decay stale activations,
  │ MEMORY       │    and deterministically evict low-retention items (capacity bounded)
  └──────┬───────┘
         ▼
  ┌──────────────┐
  │ METACOG      │ ── Evaluate progress, detect repeated failures, and emit audit event;
  │ MONITOR      │    continue or stop on terminal condition
  └──────────────┘
```

---

## 3. Epistemic Classification Standards

NeuroGenesis AI strictly enforces non-negotiable epistemic rules:
- **`verified`:** Validated by deterministic closed-form Rust checkers or formal logic. Models are barred from self-assigning verified status.
- **`calculated`:** Generated by an allowlisted deterministic tool (NumPy, SciPy, SymPy) without independent secondary verification.
- **`evidence_supported`:** Backed by primary scholarly literature with verified DOI/PMCID provenance.
- **`hypothesis`:** Model-generated or candidate proposition lacking independent verification.
- **`uncertain`:** Explicitly identified boundary ambiguity or missing data.
- **`contradicted`:** Mathematical inconsistency or empirical discrepancy.

---

## 4. Current Milestone Status & Boundaries

- **Implemented:** Cognitive control loop, bounded working memory (retention scoring), hierarchical goals (completion criteria), selective attention, deterministic verification, asynchronous specialist scheduling with DAG dependency propagation, SQLite version 2 schema migration, cross-process persistence recovery, and responsive UI visibility.
- **Deferred to Milestone 1.4+:** OpenAlex/Crossref/PubMed primary literature integration, Lean 4 formal mathematical proof checking, Zep conversational memory, and Mol* 3D molecular visualization.
- **Permanently Excluded:** Notion integration.
