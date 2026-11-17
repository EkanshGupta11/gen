use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    cognition::{
        actions::{ActionResult, ExecutiveAction, StopReason},
        goals::GoalManager,
        working_memory::{KnowledgeItemType, WorkingMemory},
    },
    contracts::EpistemicStatus,
};

pub const COGNITIVE_STATE_SCHEMA_VERSION: &str = "1.0";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutivePhase {
    Idle,
    Observe,
    UpdateState,
    EvaluateGoals,
    AttentionPrioritisation,
    SelectAction,
    ValidatePreconditions,
    ExecuteAction,
    EvaluateOutcome,
    UpdateWorkingMemory,
    MetacognitiveEvaluation,
    Stopped,
}

impl ExecutivePhase {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Observe => "observe",
            Self::UpdateState => "update_state",
            Self::EvaluateGoals => "evaluate_goals",
            Self::AttentionPrioritisation => "attention_prioritisation",
            Self::SelectAction => "select_action",
            Self::ValidatePreconditions => "validate_preconditions",
            Self::ExecuteAction => "execute_action",
            Self::EvaluateOutcome => "evaluate_outcome",
            Self::UpdateWorkingMemory => "update_working_memory",
            Self::MetacognitiveEvaluation => "metacognitive_evaluation",
            Self::Stopped => "stopped",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InvestigationContext {
    pub investigation_id: String,
    pub primary_objective: String,
    pub constraints: Vec<String>,
    pub relevant_entities: Vec<String>,
    pub known_variables: Vec<String>,
    pub unknown_variables: Vec<String>,
    pub current_stage: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Provenance {
    pub source_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_by: Option<String>,
    pub timestamp: String,
}

impl Provenance {
    pub fn new(source_type: impl Into<String>) -> Self {
        let timestamp = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        Self {
            source_type: source_type.into(),
            source_id: None,
            generated_by: None,
            timestamp,
        }
    }

    pub fn with_source_id(mut self, id: impl Into<String>) -> Self {
        self.source_id = Some(id.into());
        self
    }

    pub fn with_generated_by(mut self, generator: impl Into<String>) -> Self {
        self.generated_by = Some(generator.into());
        self
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Hypothesis {
    pub id: String,
    pub statement: String,
    pub supporting_observations: Vec<String>,
    pub contradicting_observations: Vec<String>,
    pub assumptions: Vec<String>,
    pub epistemic_status: EpistemicStatus,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KnowledgeItem {
    pub id: String,
    pub item_type: KnowledgeItemType,
    pub content: Value,
    pub provenance: Provenance,
    pub epistemic_status: EpistemicStatus,
    pub related_goals: Vec<String>,
    pub related_entities: Vec<String>,
    pub timestamp: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionOutcome {
    Resolved,
    PartiallyResolved,
    Unresolved,
    RequiresHumanReview,
}

impl ResolutionOutcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Resolved => "resolved",
            Self::PartiallyResolved => "partially_resolved",
            Self::Unresolved => "unresolved",
            Self::RequiresHumanReview => "requires_human_review",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ContradictionResolution {
    pub contradiction_id: String,
    pub conflicting_items: Vec<String>,
    pub proposed_resolution: String,
    pub supporting_evidence_refs: Vec<String>,
    pub verification_method: String,
    pub outcome: ResolutionOutcome,
    pub timestamp: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ContradictionRecord {
    pub id: String,
    pub description: String,
    pub conflicting_items: Vec<String>,
    pub resolved: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<ContradictionResolution>,
    pub detected_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ExecutiveControllerState {
    pub current_cycle: u32,
    pub current_phase: ExecutivePhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_action: Option<ExecutiveAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_action_outcome: Option<ActionResult>,
    #[serde(default)]
    pub total_actions_executed: u32,
    pub remaining_action_budget: u32,
    #[serde(default)]
    pub loop_detection_history: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<StopReason>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CognitiveState {
    pub schema_version: String,
    pub context: InvestigationContext,
    pub goals: GoalManager,
    pub hypotheses: HashMap<String, Hypothesis>,
    pub knowledge_items: HashMap<String, KnowledgeItem>,
    pub working_memory: WorkingMemory,
    pub executive: ExecutiveControllerState,
    pub contradictions: Vec<ContradictionRecord>,
    pub consecutive_failures: u32,
}

impl CognitiveState {
    pub fn new(
        investigation_id: impl Into<String>,
        primary_objective: impl Into<String>,
        working_memory_capacity: usize,
        action_budget: u32,
    ) -> Self {
        Self {
            schema_version: COGNITIVE_STATE_SCHEMA_VERSION.into(),
            context: InvestigationContext {
                investigation_id: investigation_id.into(),
                primary_objective: primary_objective.into(),
                constraints: Vec::new(),
                relevant_entities: Vec::new(),
                known_variables: Vec::new(),
                unknown_variables: Vec::new(),
                current_stage: "initialization".into(),
            },
            goals: GoalManager::new(),
            hypotheses: HashMap::new(),
            knowledge_items: HashMap::new(),
            working_memory: WorkingMemory::new(working_memory_capacity),
            executive: ExecutiveControllerState {
                current_cycle: 0,
                current_phase: ExecutivePhase::Idle,
                selected_action: None,
                previous_action_outcome: None,
                total_actions_executed: 0,
                remaining_action_budget: action_budget,
                loop_detection_history: Vec::new(),
                stop_reason: None,
            },
            contradictions: Vec::new(),
            consecutive_failures: 0,
        }
    }

    pub fn add_knowledge_item(&mut self, item: KnowledgeItem) {
        let wm_item = crate::cognition::working_memory::WorkingMemoryItem::new(
            item.id.clone(),
            item.item_type.clone(),
            item.content.clone(),
            item.epistemic_status.clone(),
        )
        .with_related_goals(item.related_goals.clone());

        let _ = self.working_memory.insert(wm_item);
        self.knowledge_items.insert(item.id.clone(), item);
    }

    pub fn record_contradiction(
        &mut self,
        id: impl Into<String>,
        description: impl Into<String>,
        conflicting_items: Vec<String>,
    ) {
        let now = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        let contradiction_id = id.into();
        self.contradictions.push(ContradictionRecord {
            id: contradiction_id.clone(),
            description: description.into(),
            conflicting_items,
            resolved: false,
            resolution: None,
            detected_at: now,
        });
    }

    pub fn unresolved_contradictions(&self) -> Vec<&ContradictionRecord> {
        self.contradictions.iter().filter(|c| !c.resolved).collect()
    }

    pub fn is_stopped(&self) -> bool {
        self.executive.stop_reason.is_some()
            || self.executive.current_phase == ExecutivePhase::Stopped
    }
}

/// Canonical Scientific Evidence Schema and Comparability Verification.
///
/// Established rule: A verified calculation does NOT automatically resolve a contradiction.
/// Evidence items are comparable ONLY when they concern the exact same scientific question,
/// executed using the same scientific tool, under identical condition inputs (within 1e-4 numeric tolerance),
/// compatible units, non-conflicting physical/model assumptions, and matching output quantity identity.
///
/// IMPORTANT: Contradictory result values are expected to differ in a contradiction.
/// Do NOT require output values to be equal in order to establish comparability.
///
/// FAILS CLOSED: Missing tool identity, arguments, units, assumptions, or mismatched measured quantities
/// must return an explicit Err, routing the contradiction to ResolutionOutcome::RequiresHumanReview.
pub fn check_contradiction_comparability(
    verified: &KnowledgeItem,
    other: &KnowledgeItem,
) -> Result<(), String> {
    // 1. Tool check: Both MUST explicitly specify a tool/operation, and they must match.
    // Specialist provenance (e.g. "physics") is NOT a substitute for tool identity.
    let tool_v = verified
        .content
        .get("tool")
        .or_else(|| verified.content.get("tool_name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let tool_o = other
        .content
        .get("tool")
        .or_else(|| other.content.get("tool_name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let (tv, to) = match (tool_v, tool_o) {
        (Some(tv), Some(to)) => (tv, to),
        (None, _) | (_, None) => {
            return Err("Tool or scientific operation specification missing in evidence".into());
        }
    };

    if tv != to {
        return Err(format!("Different tools used: '{tv}' vs '{to}'"));
    }

    // 2. Condition / Parameter comparison: Both MUST specify arguments/parameters object
    let args_v = verified
        .content
        .get("arguments")
        .or_else(|| verified.content.get("parameters"))
        .and_then(Value::as_object);
    let args_o = other
        .content
        .get("arguments")
        .or_else(|| other.content.get("parameters"))
        .and_then(Value::as_object);

    let (av, ao) = match (args_v, args_o) {
        (Some(av), Some(ao)) => (av, ao),
        (None, _) | (_, None) => {
            return Err("Condition parameters/arguments missing in evidence".into());
        }
    };

    fn is_metadata_key(k: &str) -> bool {
        matches!(k, "units" | "unit" | "assumptions")
    }

    // Key set comparison
    for k in av.keys() {
        if is_metadata_key(k) {
            continue;
        }
        if !ao.contains_key(k) {
            return Err(format!(
                "Mismatched condition parameters: parameter '{k}' missing from conflicting item"
            ));
        }
    }
    for k in ao.keys() {
        if is_metadata_key(k) {
            continue;
        }
        if !av.contains_key(k) {
            return Err(format!(
                "Mismatched condition parameters: parameter '{k}' missing from verified item"
            ));
        }
    }

    // Value comparison with justified numeric tolerance
    for (k, val_v) in av {
        if is_metadata_key(k) {
            continue;
        }
        let val_o = &ao[k];
        compare_condition_values(k, val_v, val_o)?;
    }

    // 3. Units comparison: Both MUST specify units, and units must be compatible
    let unit_v = extract_units(&verified.content);
    let unit_o = extract_units(&other.content);

    match (unit_v, unit_o) {
        (Some(uv), Some(uo)) => {
            match (&uv, &uo) {
                (Value::String(sv), Value::String(so)) => {
                    if sv.trim().to_lowercase() != so.trim().to_lowercase() {
                        return Err(format!("Incompatible units: {:?} vs {:?}", uv, uo));
                    }
                }
                (Value::Object(mv), Value::Object(mo)) => {
                    // Check that any shared keys have matching units
                    for (k, v) in mv {
                        if let Some(o_val) = mo.get(k).filter(|&o| o != v) {
                            return Err(format!(
                                "Incompatible units for '{k}': {:?} vs {:?}",
                                v, o_val
                            ));
                        }
                    }
                }
                _ => {
                    return Err(format!("Incompatible units: {:?} vs {:?}", uv, uo));
                }
            }
        }
        (None, _) | (_, None) => {
            return Err("Units specification missing in evidence".into());
        }
    }

    // 4. Assumptions comparison: Both MUST specify explicit assumptions, and they must not conflict
    let assumptions_v = extract_assumptions(&verified.content);
    let assumptions_o = extract_assumptions(&other.content);

    if assumptions_v.is_empty() || assumptions_o.is_empty() {
        return Err("Explicit scientific assumptions missing in evidence".into());
    }

    let v_has_air = assumptions_v
        .iter()
        .any(|a| a.contains("air") || a.contains("drag") || a.contains("resistance"));
    let o_has_air = assumptions_o
        .iter()
        .any(|a| a.contains("air") || a.contains("drag") || a.contains("resistance"));
    let v_has_vacuum = assumptions_v
        .iter()
        .any(|a| a.contains("vacuum") || a.contains("ideal") || a.contains("no drag"));
    let o_has_vacuum = assumptions_o
        .iter()
        .any(|a| a.contains("vacuum") || a.contains("ideal") || a.contains("no drag"));

    if (v_has_air && o_has_vacuum) || (v_has_vacuum && o_has_air) {
        return Err("Conflicting physical assumptions: air resistance vs ideal vacuum".into());
    }

    let v_has_rel = assumptions_v.iter().any(|a| a.contains("relativ"));
    let o_has_rel = assumptions_o.iter().any(|a| a.contains("relativ"));
    let v_has_class = assumptions_v
        .iter()
        .any(|a| a.contains("classical") || a.contains("newtonian"));
    let o_has_class = assumptions_o
        .iter()
        .any(|a| a.contains("classical") || a.contains("newtonian"));

    if (v_has_rel && o_has_class) || (v_has_class && o_has_rel) {
        return Err("Conflicting model assumptions: relativistic vs classical".into());
    }

    let v_has_isothermal = assumptions_v.iter().any(|a| a.contains("isothermal"));
    let o_has_isothermal = assumptions_o.iter().any(|a| a.contains("isothermal"));
    let v_has_adiabatic = assumptions_v.iter().any(|a| a.contains("adiabatic"));
    let o_has_adiabatic = assumptions_o.iter().any(|a| a.contains("adiabatic"));

    if (v_has_isothermal && o_has_adiabatic) || (v_has_adiabatic && o_has_isothermal) {
        return Err("Conflicting thermodynamic assumptions: isothermal vs adiabatic".into());
    }

    let v_has_laminar = assumptions_v.iter().any(|a| a.contains("laminar"));
    let o_has_laminar = assumptions_o.iter().any(|a| a.contains("laminar"));
    let v_has_turbulent = assumptions_v.iter().any(|a| a.contains("turbulent"));
    let o_has_turbulent = assumptions_o.iter().any(|a| a.contains("turbulent"));

    if (v_has_laminar && o_has_turbulent) || (v_has_turbulent && o_has_laminar) {
        return Err("Conflicting fluid assumptions: laminar vs turbulent".into());
    }

    let v_has_quantum = assumptions_v.iter().any(|a| a.contains("quantum"));
    let o_has_quantum = assumptions_o.iter().any(|a| a.contains("quantum"));

    if (v_has_quantum && o_has_class) || (v_has_class && o_has_quantum) {
        return Err("Conflicting model assumptions: quantum vs classical".into());
    }

    // Check that assumption sets are aligned
    if assumptions_v != assumptions_o {
        // If neither is a superset of standard compatible assumptions, reject
        let has_unknown_v = assumptions_v.iter().any(|a| !assumptions_o.contains(a));
        let has_unknown_o = assumptions_o.iter().any(|a| !assumptions_v.contains(a));
        if has_unknown_v && has_unknown_o {
            return Err(format!(
                "Incompatible scientific assumptions: {assumptions_v:?} vs {assumptions_o:?}"
            ));
        }
    }

    // 5. Output identity check: Both MUST have an output and measure the same physical quantities
    let out_v = verified
        .content
        .get("output")
        .or_else(|| verified.content.get("result"))
        .or_else(|| verified.content.get("value"));
    let out_o = other
        .content
        .get("output")
        .or_else(|| other.content.get("result"))
        .or_else(|| other.content.get("value"));

    let (ov, oo) = match (out_v, out_o) {
        (Some(ov), Some(oo)) if !ov.is_null() && !oo.is_null() => (ov, oo),
        _ => return Err("Output result field missing in evidence".into()),
    };

    match (ov.as_object(), oo.as_object()) {
        (Some(ov_map), Some(oo_map)) => {
            fn is_output_metadata_key(k: &str) -> bool {
                matches!(
                    k,
                    "epistemic_status"
                        | "method"
                        | "assumptions"
                        | "units"
                        | "unit"
                        | "operation"
                        | "error"
                        | "status"
                )
            }
            let mut quantities_v: Vec<&str> = ov_map
                .keys()
                .map(String::as_str)
                .filter(|k| !is_output_metadata_key(k))
                .collect();
            quantities_v.sort();

            let mut quantities_o: Vec<&str> = oo_map
                .keys()
                .map(String::as_str)
                .filter(|k| !is_output_metadata_key(k))
                .collect();
            quantities_o.sort();

            if quantities_v.is_empty() || quantities_o.is_empty() {
                return Err("No measured quantities identified in output".into());
            }

            if quantities_v != quantities_o {
                return Err(format!(
                    "Different output measured quantities: {quantities_v:?} vs {quantities_o:?}"
                ));
            }
        }
        (None, None) => {
            if ov.is_boolean() || oo.is_boolean() {
                return Err("Malformed output structure in evidence: boolean output".into());
            }
        }
        _ => {
            return Err(
                "Malformed output structure: mismatched scalar and object output shapes".into(),
            );
        }
    }

    Ok(())
}

fn compare_condition_values(key: &str, val_v: &Value, val_o: &Value) -> Result<(), String> {
    match (val_v, val_o) {
        (Value::Number(nv), Value::Number(no)) => {
            let f_v = nv.as_f64().unwrap_or(0.0);
            let f_o = no.as_f64().unwrap_or(0.0);
            if (f_v - f_o).abs() > 1e-4 {
                return Err(format!(
                    "Condition parameter mismatch for '{key}': {f_v} vs {f_o}"
                ));
            }
        }
        (Value::String(sv), Value::String(so)) => {
            if sv.trim() != so.trim() {
                return Err(format!(
                    "Condition parameter mismatch for '{key}': '{sv}' vs '{so}'"
                ));
            }
        }
        (Value::Bool(bv), Value::Bool(bo)) => {
            if bv != bo {
                return Err(format!(
                    "Condition parameter mismatch for '{key}': {bv} vs {bo}"
                ));
            }
        }
        (Value::Array(av), Value::Array(ao)) => {
            if av.len() != ao.len() {
                return Err(format!(
                    "Condition parameter array length mismatch for '{key}': {} vs {}",
                    av.len(),
                    ao.len()
                ));
            }
            for (idx, (item_v, item_o)) in av.iter().zip(ao.iter()).enumerate() {
                let subkey = format!("{key}[{idx}]");
                compare_condition_values(&subkey, item_v, item_o)?;
            }
        }
        (Value::Object(ov), Value::Object(oo)) => {
            for (sub_k, sub_v) in ov {
                if let Some(sub_o) = oo.get(sub_k) {
                    let subkey = format!("{key}.{sub_k}");
                    compare_condition_values(&subkey, sub_v, sub_o)?;
                } else {
                    return Err(format!(
                        "Condition parameter nested key '{sub_k}' missing from conflicting item"
                    ));
                }
            }
            for sub_k in oo.keys() {
                if !ov.contains_key(sub_k) {
                    return Err(format!(
                        "Condition parameter nested key '{sub_k}' missing from verified item"
                    ));
                }
            }
        }
        _ => {
            return Err(format!("Condition parameter type mismatch for '{key}'"));
        }
    }
    Ok(())
}

fn extract_units(content: &Value) -> Option<Value> {
    if let Some(u) = content
        .get("units")
        .or_else(|| content.get("unit"))
        .filter(|u| !u.is_null())
    {
        return Some(u.clone());
    }
    if let Some(u) = content
        .get("arguments")
        .and_then(Value::as_object)
        .and_then(|args| args.get("units").or_else(|| args.get("unit")))
        .filter(|u| !u.is_null())
    {
        return Some(u.clone());
    }
    if let Some(out) = content.get("output").and_then(Value::as_object) {
        if let Some(u) = out
            .get("units")
            .or_else(|| out.get("unit"))
            .filter(|u| !u.is_null())
        {
            return Some(u.clone());
        }
        // Infer from standard unit suffixes in output keys (e.g. _m, _s, _m_s, _j, _kg, _deg, _rad)
        let mut inferred_units = serde_json::Map::new();
        for key in out.keys() {
            if key.ends_with("_m") {
                inferred_units.insert(key.clone(), Value::String("m".into()));
            } else if key.ends_with("_s") {
                inferred_units.insert(key.clone(), Value::String("s".into()));
            } else if key.ends_with("_m_s") {
                inferred_units.insert(key.clone(), Value::String("m/s".into()));
            } else if key.ends_with("_j") {
                inferred_units.insert(key.clone(), Value::String("J".into()));
            } else if key.ends_with("_kg") {
                inferred_units.insert(key.clone(), Value::String("kg".into()));
            } else if key.ends_with("_rad") {
                inferred_units.insert(key.clone(), Value::String("rad".into()));
            } else if key.ends_with("_deg") {
                inferred_units.insert(key.clone(), Value::String("deg".into()));
            }
        }
        if !inferred_units.is_empty() {
            return Some(Value::Object(inferred_units));
        }
    }
    None
}

fn extract_assumptions(val: &Value) -> Vec<String> {
    let mut assumptions = Vec::new();
    let mut check_source = |source: Option<&Value>| {
        if let Some(src) = source {
            if let Some(arr) = src.get("assumptions").and_then(Value::as_array) {
                for item in arr {
                    if let Some(s) = item.as_str() {
                        let trimmed = s.trim().to_lowercase();
                        if !trimmed.is_empty() && !assumptions.contains(&trimmed) {
                            assumptions.push(trimmed);
                        }
                    }
                }
            } else if let Some(s) = src.get("assumptions").and_then(Value::as_str) {
                let trimmed = s.trim().to_lowercase();
                if !trimmed.is_empty() && !assumptions.contains(&trimmed) {
                    assumptions.push(trimmed);
                }
            }
        }
    };

    check_source(Some(val));
    check_source(val.get("output"));
    check_source(val.get("arguments"));

    assumptions.sort();
    assumptions
}
