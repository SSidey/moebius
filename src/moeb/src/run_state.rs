use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq)]
pub enum TaskStatus {
    Pending,
    Done,
    Skipped,
}

#[derive(Debug, Clone)]
pub struct Task {
    pub id: String,
    pub description: String,
    pub status: TaskStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RubricStatus {
    Pass,
    Fail,
    Na,
}

#[derive(Debug, Clone)]
pub struct RubricVerification {
    pub name: String,
    pub status: RubricStatus,
    pub note: Option<String>,
}

#[derive(Debug, Default)]
pub struct RunState {
    pub tasks: Vec<Task>,
    pub rubric_verifications: Vec<RubricVerification>,
}

impl RunState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn pending_tasks(&self) -> Vec<&Task> {
        self.tasks.iter().filter(|t| t.status == TaskStatus::Pending).collect()
    }

    pub fn task_list_created(&self) -> bool {
        !self.tasks.is_empty()
    }
}

pub type SharedRunState = Arc<Mutex<RunState>>;

pub fn new_shared_run_state() -> SharedRunState {
    Arc::new(Mutex::new(RunState::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_tasks_returns_only_pending() {
        let mut state = RunState::new();
        state.tasks.push(Task {
            id: "1".to_string(),
            description: "done task".to_string(),
            status: TaskStatus::Done,
        });
        state.tasks.push(Task {
            id: "2".to_string(),
            description: "pending task".to_string(),
            status: TaskStatus::Pending,
        });
        let pending = state.pending_tasks();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, "2");
    }

    #[test]
    fn task_list_created_false_when_empty() {
        let state = RunState::new();
        assert!(!state.task_list_created());
    }

    #[test]
    fn task_list_created_true_after_push() {
        let mut state = RunState::new();
        state.tasks.push(Task {
            id: "1".to_string(),
            description: "a task".to_string(),
            status: TaskStatus::Pending,
        });
        assert!(state.task_list_created());
    }
}
