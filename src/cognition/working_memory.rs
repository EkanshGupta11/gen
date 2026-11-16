use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::contracts::EpistemicStatus;

pub const DEFAULT_WORKING_MEMORY_CAPACITY: usize = 64;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeItemType {
    Observation,
    Fact,
    Hypothesis,
    Calculation,
    Evidence,
    Decision,
    OpenQuestion,
    Constraint,
}

impl KnowledgeItemType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Observation => "observation",
            Self::Fact => "fact",
            Self::Hypothesis => "hypothesis",
            Self::Calculation => "calculation",
            Self::Evidence => "evidence",
            Self::Decision => "decision",
            Self::OpenQuestion => "open_question",
            Self::Constraint => "constraint",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WorkingMemoryItem {
    pub id: String,
    pub item_type: KnowledgeItemType,
    pub content: Value,
    pub epistemic_status: EpistemicStatus,
    pub activation: f64,
    pub priority: u32,
    pub pinned: bool,
    pub related_goals: Vec<String>,
    pub unresolved: bool,
    pub created_at: String,
    pub last_accessed: String,
    pub access_count: u32,
}

impl WorkingMemoryItem {
    pub fn new(
        id: impl Into<String>,
        item_type: KnowledgeItemType,
        content: Value,
        epistemic_status: EpistemicStatus,
    ) -> Self {
        let now = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        let pinned = item_type == KnowledgeItemType::Constraint;
        Self {
            id: id.into(),
            item_type,
            content,
            epistemic_status,
            activation: 1.0,
            priority: 50,
            pinned,
            related_goals: Vec::new(),
            unresolved: false,
            created_at: now.clone(),
            last_accessed: now,
            access_count: 1,
        }
    }

    pub fn with_priority(mut self, priority: u32) -> Self {
        self.priority = priority;
        self
    }

    pub fn with_pinned(mut self, pinned: bool) -> Self {
        self.pinned = pinned;
        self
    }

    pub fn with_unresolved(mut self, unresolved: bool) -> Self {
        self.unresolved = unresolved;
        self
    }

    pub fn with_related_goals(mut self, goals: Vec<String>) -> Self {
        self.related_goals = goals;
        self
    }

    /// Deterministic retention score for eviction decisions.
    /// Higher score = more important to retain in working memory.
    pub fn retention_score(&self) -> f64 {
        let base_activation = self.activation.clamp(0.0, 1.0) * 40.0;
        let priority_score = (self.priority.min(100) as f64) * 0.3; // 0..30
        let verification_bonus = match self.epistemic_status {
            EpistemicStatus::Verified => 20.0,
            EpistemicStatus::Calculated | EpistemicStatus::EvidenceSupported => 15.0,
            EpistemicStatus::Hypothesis => 10.0,
            EpistemicStatus::Uncertain => 5.0,
            EpistemicStatus::Contradicted => 25.0, // Retain contradictions to ensure they are addressed!
        };
        let unresolved_bonus = if self.unresolved { 20.0 } else { 0.0 };

        base_activation + priority_score + verification_bonus + unresolved_bonus
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WorkingMemorySnapshot {
    pub capacity: usize,
    pub items: Vec<WorkingMemoryItem>,
    pub current_focus: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WorkingMemory {
    capacity: usize,
    items: HashMap<String, WorkingMemoryItem>,
    pub current_focus: Option<String>,
}

impl Default for WorkingMemory {
    fn default() -> Self {
        Self::new(DEFAULT_WORKING_MEMORY_CAPACITY)
    }
}

impl WorkingMemory {
    pub fn new(capacity: usize) -> Self {
        let effective_capacity = capacity.clamp(4, 512);
        Self {
            capacity: effective_capacity,
            items: HashMap::new(),
            current_focus: None,
        }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn contains(&self, id: &str) -> bool {
        self.items.contains_key(id)
    }

    pub fn insert(&mut self, mut item: WorkingMemoryItem) -> Result<Option<String>, String> {
        if item.id.trim().is_empty() {
            return Err("Memory item ID cannot be empty".into());
        }

        let now = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        item.last_accessed = now;

        // If updating an existing item, replace in-place
        if self.items.contains_key(&item.id) {
            self.items.insert(item.id.clone(), item);
            return Ok(None);
        }

        // Check if capacity is reached
        let mut evicted_id = None;
        if self.items.len() >= self.capacity {
            // Find lowest retention item that is not pinned and not unresolved contradiction
            let candidate = self
                .items
                .values()
                .filter(|it| !it.pinned && !it.unresolved)
                .min_by(|a, b| {
                    a.retention_score()
                        .partial_cmp(&b.retention_score())
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| a.access_count.cmp(&b.access_count))
                        .then_with(|| a.last_accessed.cmp(&b.last_accessed))
                        .then_with(|| a.id.cmp(&b.id))
                })
                .map(|it| it.id.clone());

            match candidate {
                Some(id_to_evict) => {
                    self.items.remove(&id_to_evict);
                    evicted_id = Some(id_to_evict);
                }
                None => {
                    return Err(
                        "Working memory capacity exhausted: all items are pinned or protected"
                            .into(),
                    );
                }
            }
        }

        self.items.insert(item.id.clone(), item);
        Ok(evicted_id)
    }

    pub fn get(&mut self, id: &str) -> Option<&WorkingMemoryItem> {
        let now = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        if let Some(item) = self.items.get_mut(id) {
            item.last_accessed = now;
            item.access_count += 1;
            item.activation = (item.activation + 0.1).min(1.0);
        }
        self.items.get(id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut WorkingMemoryItem> {
        let now = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        if let Some(item) = self.items.get_mut(id) {
            item.last_accessed = now;
            item.access_count += 1;
            item.activation = (item.activation + 0.1).min(1.0);
        }
        self.items.get_mut(id)
    }

    pub fn peek(&self, id: &str) -> Option<&WorkingMemoryItem> {
        self.items.get(id)
    }

    pub fn update_activation(&mut self, id: &str, delta: f64) -> Result<(), String> {
        let item = self
            .items
            .get_mut(id)
            .ok_or_else(|| format!("Memory item '{id}' not found"))?;
        item.activation = (item.activation + delta).clamp(0.0, 1.0);
        Ok(())
    }

    pub fn pin(&mut self, id: &str, pinned: bool) -> Result<(), String> {
        let item = self
            .items
            .get_mut(id)
            .ok_or_else(|| format!("Memory item '{id}' not found"))?;
        item.pinned = pinned;
        Ok(())
    }

    pub fn remove(&mut self, id: &str) -> Option<WorkingMemoryItem> {
        self.items.remove(id)
    }

    pub fn deactivate_stale(&mut self, decay: f64) {
        for item in self.items.values_mut() {
            if !item.pinned && !item.unresolved {
                item.activation = (item.activation - decay).clamp(0.0, 1.0);
            }
        }
    }

    pub fn get_active_items(&self, threshold: f64) -> Vec<&WorkingMemoryItem> {
        let mut active: Vec<_> = self
            .items
            .values()
            .filter(|it| it.pinned || it.unresolved || it.activation >= threshold)
            .collect();
        active.sort_by(|a, b| {
            b.retention_score()
                .partial_cmp(&a.retention_score())
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.id.cmp(&b.id))
        });
        active
    }

    pub fn get_unresolved_contradictions(&self) -> Vec<&WorkingMemoryItem> {
        self.items
            .values()
            .filter(|it| it.unresolved || it.epistemic_status == EpistemicStatus::Contradicted)
            .collect()
    }

    pub fn snapshot(&self) -> WorkingMemorySnapshot {
        let mut items: Vec<_> = self.items.values().cloned().collect();
        items.sort_by(|a, b| a.id.cmp(&b.id));
        WorkingMemorySnapshot {
            capacity: self.capacity,
            items,
            current_focus: self.current_focus.clone(),
        }
    }

    pub fn restore_snapshot(&mut self, snapshot: WorkingMemorySnapshot) {
        self.capacity = snapshot.capacity;
        self.items.clear();
        for item in snapshot.items {
            self.items.insert(item.id.clone(), item);
        }
        self.current_focus = snapshot.current_focus;
    }
}
