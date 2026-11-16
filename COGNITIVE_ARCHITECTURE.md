# NeuroGenesis AI — Cognitive Architecture Specification
## Milestone 1.3 & 1.3.1: Cognitive Control Foundation & Runtime Integration

---

## 1. Vision & Engineering Principles

NeuroGenesis AI is designed as an interdisciplinary, brain-inspired scientific intelligence architecture. 

### Non-Negotiable Boundaries
- **No LLM Wrapper:** NeuroGenesis AI does not rely on an external language model to manage internal control flow, goals, or memory.
- **No Chatbot Networks:** We reject multi-agent conversational chatter disguised as cognitive reasoning.
- **No Prompt Engineering as Architecture:** Cognitive control is implemented as typed, deterministic Rust software, not prompt chaining.
- **No RAG Marketed as Cognition:** Retrieval is an epistemic input, not a substitute for computational cognition.
- **Independent State Ownership:** NeuroGenesis owns its state transitions, memory replacement policies, attention scoring, goal verification, and action execution.
- **Functional Engineering Objective:** These computational mechanisms are inspired by cognitive science paradigms (e.g. Baddeley's working memory model, Norman & Shallice's supervisory attentional system), but are functional engineering approximations—we do NOT claim to have created biological cognition or consciousness.
- **Hosted Model Independence:** The cognitive engine functions deterministically offline without requiring hosted API credentials.

---

## 2. Architectural Subsystems

```
                                  ┌────────────────────────┐
                                  │   Primary Objective    │
                                  └───────────┬────────────┘
                                              │
                                              ▼
┌────────────────────────────────────────────────────────────────────────────────────────┐
│                                   COGNITIVE STATE                                      │
│  ┌────────────────────────┐   ┌────────────────────────┐   ┌────────────────────────┐  │
│  │     Goal Manager       │   │     Working Memory     │   │      Hypotheses        │  │
│  │ (Hierarchies, Status,  │   │  (Bounded, Activation, │   │ (Epistemic Status,     │  │
│  │  Completion Criteria)  │   │   Pinned Constraints)  │   │  Supporting/Contra)    │  │
│  └───────────┬────────────┘   └───────────┬────────────┘   └───────────┬────────────┘  │
│              │                            │                            │               │
│              ▼                            ▼                            ▼               │
│  ┌──────────────────────────────────────────────────────────────────────────────────┐  │
│  │                           SELECTIVE ATTENTION ENGINE                             │  │
│  │   (Deterministic scoring: Base Priority + Readiness + Contradiction Bonus +      │  │
│  │    Uncertainty Bonus + Missing Info Bonus with Deterministic Tie-Breaking)       │  │
│  └────────────────────────────────────────┬─────────────────────────────────────────┘  │
│                                           │                                            │
│                                           ▼                                            │
│  ┌──────────────────────────────────────────────────────────────────────────────────┐  │
│  │                               EXECUTIVE CONTROLLER                               │  │
│  │   Control Cycle:                                                                 │  │
│  │   Observe -> Update State -> Evaluate Goals -> Attention -> Select Action        │  │
│  │   -> Validate Preconditions -> Execute Action -> Evaluate Outcome                │  │
│  │   -> Update Working Memory -> Metacognitive Evaluation -> Continue / Stop        │  │
│  └────────────────────────────────────────┬─────────────────────────────────────────┘  │
│                                           │                                            │
│                                           ▼                                            │
│  ┌──────────────────────────────────────────────────────────────────────────────────┐  │
│  │                                ACTION EXECUTOR                                   │  │
│  │   Allowlist: Tool Execution, Specialist Request, Independent Rust Verification,   │  │
│  │   Goal Decomposition, Contradiction Resolution, Hypothesis Comparison, Synthesis  │  │
│  └──────────────────────────────────────────────────────────────────────────────────┘  │
└───────────────────────────────────────────┬────────────────────────────────────────────┘
                                            │
                                            ▼
                           ┌─────────────────────────────────┐
                           │      Rust SQLite Store v2       │
                           │  - Cognitive State Snapshots    │
                           │  - Append-Only Cognitive Events │
                           └─────────────────────────────────┘
```

---

## 3. Cognitive State Model (`src/cognition/state.rs`)

`CognitiveState` is a strongly-typed, versioned (`schema_version: "1.0"`), serializable data model representing the complete internal cognitive state of an investigation:

1. **Investigation Context (`InvestigationContext`):**
   - `investigation_id`: Unique UUID.
   - `primary_objective`: High-level scientific question or goal.
   - `constraints`: Invariants and physical limitations.
   - `relevant_entities`: Domain entities under investigation.
   - `known_variables` & `unknown_variables`: Epistemic parameter tracking.
   - `current_stage`: Human-readable operational stage.

2. **Goal Manager (`GoalManager`):**
   - Tracks hierarchical goal trees, dependencies, preconditions, completion criteria, and execution agenda.

3. **Hypotheses (`HashMap<String, Hypothesis>`):**
   - Explicit hypothesis statements with references to supporting and contradicting observations, assumptions, and epistemic statuses (`verified`, `calculated`, `evidence_supported`, `hypothesis`, `uncertain`, `contradicted`).

4. **Knowledge Items (`HashMap<String, KnowledgeItem>`):**
   - Discrete cognitive items with typed categories: `observation`, `fact`, `hypothesis`, `calculation`, `evidence`, `decision`, `open_question`, `constraint`.
   - Full provenance tracking (`source_type`, `source_id`, `generated_by`, `timestamp`).

5. **Working Memory (`WorkingMemory`):**
   - Active memory items currently held in working memory with activation levels and retention scores.

6. **Executive Controller State (`ExecutiveControllerState`):**
   - `current_cycle`: Discrete loop count.
   - `current_phase`: Current step in the 10-phase control loop.
   - `selected_action`: The pending or actively executing action.
   - `previous_action_outcome`: Result of the most recently executed action.
   - `remaining_action_budget`: Enforced limit on action executions.
   - `stop_reason`: Terminal reason when execution ceases.

7. **Contradictions & Metacognitive State:**
   - `contradictions`: Tracked records of conflicting claims or contradicted calculation results.
   - `consecutive_failures`: Loop and failure detection metric.

---

## 4. Working Memory Engine (`src/cognition/working_memory.rs`)

### Computational Approximation
Working memory is bounded to a configurable capacity (default: 64 items, minimum: 4, maximum: 512). It provides active item storage, activation decay, retrieval reinforcement, and deterministic replacement.

### Retention Score Formula
When working memory reaches capacity, items are evaluated for eviction using a deterministic retention score:
$$\text{Score} = (\text{activation} \times 40.0) + (\text{priority} \times 0.3) + \text{epistemic\_bonus} + \text{unresolved\_bonus}$$

Where:
- **Activation:** Value in $[0.0, 1.0]$. Decays by $0.05$ per executive cycle for non-pinned items; reinforced by $+0.1$ on retrieval.
- **Priority:** Value in $[0, 100]$.
- **Epistemic Bonus:**
  - `Contradicted`: $+25.0$ (ensures anomalies are retained to be resolved).
  - `Verified`: $+20.0$.
  - `Calculated` / `EvidenceSupported`: $+15.0$.
  - `Hypothesis`: $+10.0$.
  - `Uncertain`: $+5.0$.
- **Unresolved Bonus:** $+20.0$ for open questions or unresolved conflicts.

### Eviction Protection Rules
1. **Pinned Items:** Items with `pinned: true` (e.g. constraints, critical invariants) are **never evicted**.
2. **Unresolved Contradictions:** Items with `unresolved: true` or status `Contradicted` are **protected from eviction**.
3. **Deterministic Eviction:** Among non-pinned, resolved items, the item with the lowest retention score is evicted. Ties are broken deterministically by:
   $$\text{lowest access count} \to \text{oldest last\_accessed} \to \text{lexicographical ID}$$
4. If all items are pinned or protected, an explicit capacity error is returned rather than silently dropping critical constraints.

---

## 5. Selective Attention Engine (`src/cognition/attention.rs`)

The attention subsystem decides which goal and working memory items receive cognitive processing in each executive cycle.

### Scoring Policy
For every candidate non-terminal goal $g$:
$$\text{AttentionScore}(g) = (\text{Readiness} \times 40.0) + (\text{ContradictionBonus} \times 25.0) + (\text{Priority} \times 20.0) + (\text{UncertaintyBonus} \times 10.0) + (\text{MissingInfoBonus} \times 5.0)$$

Where:
- **Readiness ($40.0$ max):** $1.0$ if all goal dependencies are completed; $0.0$ if blocked.
- **Contradiction Bonus ($25.0$ max):** $1.0$ if the goal addresses an active unresolved contradiction; otherwise $0.0$.
- **Base Priority ($20.0$ max):** Normalized $\text{priority} / 100.0$.
- **Uncertainty Bonus ($10.0$ max):** $0.8$ if the goal has criteria to test hypotheses.
- **Missing Information Bonus ($5.0$ max):** $0.6$ if the goal explicitly references tracked unknown variables.

### Deterministic Tie-Breaking
Ties are resolved strictly by:
$$\text{Total Score (desc)} \to \text{Priority (desc)} \to \text{Created Timestamp (asc)} \to \text{Goal ID (asc)}$$

All attention allocations produce a structured `AttentionDecisionRecord` containing the ranking scores, selected goal, focused items, and a deterministic rationale.

---

## 6. Goal & Agenda Management (`src/cognition/goals.rs`)

### Hierarchical Goals
Goals form a directed tree with `parent_goal` relationships and acyclic dependency graphs (`dependencies`).

### Goal Status Transitions
$$\text{Pending} \xrightarrow{\text{all dependencies completed}} \text{Active} \xrightarrow{\text{all criteria verified}} \text{Completed}$$
$$\text{Pending / Active} \xrightarrow{\text{dependency failed}} \text{Blocked} \xrightarrow{\text{unresolvable}} \text{Failed}$$
$$\text{Any Non-Terminal} \xrightarrow{\text{parent cancelled}} \text{Cancelled (recursive)}$$

### Strict Completion Criteria Enforcement
**A goal can NEVER be marked completed merely because an agent asserts it is complete.**
Completion requires evaluating explicit typed criteria:
- `ToolExecuted { tool_name }`: Working memory must contain verified or calculated tool execution output for `tool_name`.
- `CalculationVerified { subject_id }`: Rust independent verification checker must have evaluated the output and assigned `Verified` status.
- `SubGoalsCompleted`: All child subgoals must have achieved `Completed` status.
- `ObservationsRecorded { min_count }`: Working memory must contain at least `min_count` observations.
- `HypothesesTested`: All hypotheses must have transitioned out of `Hypothesis` status.

---

## 7. Executive Controller (`src/cognition/executive.rs`)

The Executive Controller governs the 10-phase control cycle:

1. **Observe:** Inspect cognitive state, working memory, and event logs.
2. **Evaluate Goals:** Check completion criteria for active goals. Goals meeting all criteria transition to `Completed`.
3. **Update State:** Evaluate goal dependency graphs. Goals whose dependencies are satisfied transition from `Pending` / `Blocked` to `Active`.
4. **Attention & Prioritisation:** Score all candidate goals and select the current focus of attention.
5. **Select Action:** Select the highest priority valid action for the focused goal.
6. **Validate Preconditions:** Verify that the chosen action satisfies strict domain preconditions (allowlisted tools, active goal validity, non-terminal parents).
7. **Execute Action:** Dispatch to local scientific computation, independent verification, or state update.
8. **Evaluate Outcome:** Process action outcome, measure duration, update failure metrics.
9. **Update Working Memory:** Store results, apply activation decay, evict low-retention stale items if capacity is exceeded.
10. **Metacognitive Monitoring:** Check for repeated action failures, unresolved contradictions, or unverified calculations; decide to proceed, shift attention, or stop.

### Safety Guards & Stopping Criteria
- `max_cycles`: Default 50 cycles.
- `max_actions`: Default 100 actions.
- `timeout`: Default 30s wall-clock duration.
- `Infinite Loop Detector`: Monitors action signatures over a rolling window. If identical actions fail repeatedly without state change, triggers `MetacognitiveAbort`.
- `Deadlock Detection`: If goals remain uncompleted but no eligible actions can be scheduled, halts with `DeadlockNoExecutableActions`.
- `AllGoalsCompleted`: Halts normally when all goals have satisfied their completion criteria.

---

## 8. Persistence Architecture (`src/store.rs`)

SQLite store upgraded to schema version `2`:
- **`investigations` Table:** Added column `cognitive_state_json TEXT`. Full snapshots of `CognitiveState` are persisted on state transitions.
- **`cognitive_events` Table (Append-Only):**
  ```sql
  CREATE TABLE IF NOT EXISTS cognitive_events (
      id TEXT PRIMARY KEY NOT NULL,
      investigation_id TEXT NOT NULL,
      cycle INTEGER NOT NULL,
      timestamp TEXT NOT NULL,
      event_type TEXT NOT NULL,
      payload_json TEXT NOT NULL
  );
  CREATE INDEX IF NOT EXISTS idx_cognitive_events_inv 
      ON cognitive_events(investigation_id, cycle, timestamp);
  ```
- **Cross-Process Resumption:** An investigation paused or persisted to SQLite can be restored into a fresh `ExecutiveController` instance. It restores working memory items, goal statuses, cycle counts, budget consumption, and loop detection history, continuing execution with the next eligible action without repeating already completed steps.

---

## 9. Cognitive Runtime & Epistemic Integrity (Milestone 1.3.1)

In Milestone 1.3.1, the cognitive subsystems were integrated into the primary execution loop:
- **`CognitiveRuntime` & `AsyncActionExecutor`:** Drives the investigation lifecycle, executing real scientific computations via the Python worker, and managing atomic SQLite state persistence after each cycle.
- **Non-Bypassable Epistemic Integrity:** Epistemic statuses cannot be self-certified by external models. `Verified` status strictly requires independent deterministic Rust checker validation within numerical tolerance.
- **Structured Contradiction Arbitration:** Contradictions require verified independent evidence and persist as structured `ContradictionResolution` records.
- **Budget & Loop Detection Preservation:** Resumed investigations preserve `total_actions_executed` and historical loop signatures across server restarts.
- **Reference:** See `docs/COGNITIVE_RUNTIME.md` for full architecture specifications.
