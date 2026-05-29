// SPDX-License-Identifier: Apache-2.0
//! Container / task lifecycle state machine.
//!
//! Behavioural reimplementation of the documented containerd task status
//! model (`api/types/task/task.proto` `Status` enum and the runc/OCI
//! lifecycle: `created -> running -> stopped`, with `paused`/`pausing` as a
//! suspended sub-state of running). Illegal transitions are rejected with a
//! typed error rather than silently accepted.
//!
//! Spec sources:
//!   * containerd task `Status` enum: `CREATED`, `RUNNING`, `STOPPED`,
//!     `PAUSED`, `PAUSING`.
//!   * OCI runtime-spec `runtime.md` container lifecycle (the `create` /
//!     `start` / `kill` / `delete` operations and their state effects).

use std::fmt;

/// The lifecycle state of a container task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    /// The bundle has been created but the process has not started.
    Created,
    /// The process is running.
    Running,
    /// The process is running but suspended (cgroup freezer).
    Paused,
    /// The process has exited (terminal). Carries the exit status.
    Stopped,
}

impl TaskState {
    /// The containerd `Status` token for this state.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Stopped => "stopped",
        }
    }

    /// True for the single terminal state.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Stopped)
    }
}

impl fmt::Display for TaskState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A lifecycle action requested against a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// `start` — run the created process.
    Start,
    /// `pause` — freeze a running process.
    Pause,
    /// `resume` — thaw a paused process.
    Resume,
    /// `stop` — terminate the process with the given exit code.
    Stop {
        /// The process exit code reported on stop.
        exit_code: u32,
    },
}

/// An illegal lifecycle transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IllegalTransition {
    /// The state the task was in.
    pub from: TaskState,
    /// The action that was rejected.
    pub action: Action,
}

impl fmt::Display for IllegalTransition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "illegal transition: cannot {:?} from {}", self.action, self.from)
    }
}

impl std::error::Error for IllegalTransition {}

/// A container task with a tracked lifecycle state and exit status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    state: TaskState,
    exit_code: Option<u32>,
}

impl Default for Task {
    fn default() -> Self {
        Self::new()
    }
}

impl Task {
    /// A freshly-created task (`created`, no exit code yet).
    #[must_use]
    pub const fn new() -> Self {
        Self { state: TaskState::Created, exit_code: None }
    }

    /// The current state.
    #[must_use]
    pub const fn state(&self) -> TaskState {
        self.state
    }

    /// The recorded exit code, present only once the task has stopped.
    #[must_use]
    pub const fn exit_code(&self) -> Option<u32> {
        self.exit_code
    }

    /// Applies a lifecycle action, returning the new state or rejecting an
    /// illegal transition. The legal edges are:
    ///
    /// ```text
    /// created --start--> running
    /// running --pause--> paused
    /// running --stop---> stopped
    /// paused  --resume-> running
    /// paused  --stop---> stopped
    /// ```
    ///
    /// Every other (state, action) pair is rejected. `stopped` is terminal.
    ///
    /// # Errors
    /// Returns [`IllegalTransition`] when `action` is not a legal edge out of
    /// the current state.
    // The Created->Running and Paused->Running edges share a target state but
    // are kept as distinct arms because they are distinct lifecycle
    // transitions; collapsing them would obscure the state machine.
    #[allow(clippy::match_same_arms)]
    pub const fn apply(&mut self, action: Action) -> Result<TaskState, IllegalTransition> {
        let next = match (self.state, action) {
            (TaskState::Created, Action::Start) => TaskState::Running,
            (TaskState::Running, Action::Pause) => TaskState::Paused,
            (TaskState::Paused, Action::Resume) => TaskState::Running,
            (TaskState::Running | TaskState::Paused, Action::Stop { exit_code }) => {
                self.exit_code = Some(exit_code);
                TaskState::Stopped
            }
            (from, action) => return Err(IllegalTransition { from, action }),
        };
        self.state = next;
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_task_is_created_without_exit_code() {
        let t = Task::new();
        assert_eq!(t.state(), TaskState::Created);
        assert!(t.exit_code().is_none());
    }

    #[test]
    fn created_starts_to_running() {
        let mut t = Task::new();
        assert_eq!(t.apply(Action::Start), Ok(TaskState::Running));
        assert_eq!(t.state(), TaskState::Running);
    }

    #[test]
    fn running_pauses_and_resumes() {
        let mut t = Task::new();
        t.apply(Action::Start).expect("start");
        assert_eq!(t.apply(Action::Pause), Ok(TaskState::Paused));
        assert_eq!(t.apply(Action::Resume), Ok(TaskState::Running));
    }

    #[test]
    fn running_stops_with_exit_code() {
        let mut t = Task::new();
        t.apply(Action::Start).expect("start");
        assert_eq!(t.apply(Action::Stop { exit_code: 137 }), Ok(TaskState::Stopped));
        assert_eq!(t.exit_code(), Some(137));
        assert!(t.state().is_terminal());
    }

    #[test]
    fn paused_can_stop_directly() {
        let mut t = Task::new();
        t.apply(Action::Start).expect("start");
        t.apply(Action::Pause).expect("pause");
        assert_eq!(t.apply(Action::Stop { exit_code: 0 }), Ok(TaskState::Stopped));
        assert_eq!(t.exit_code(), Some(0));
    }

    #[test]
    fn cannot_start_twice() {
        let mut t = Task::new();
        t.apply(Action::Start).expect("start");
        let err = t.apply(Action::Start).expect_err("double start");
        assert_eq!(err.from, TaskState::Running);
    }

    #[test]
    fn cannot_pause_created() {
        let mut t = Task::new();
        assert!(t.apply(Action::Pause).is_err());
        assert_eq!(t.state(), TaskState::Created);
    }

    #[test]
    fn cannot_resume_running() {
        let mut t = Task::new();
        t.apply(Action::Start).expect("start");
        assert!(t.apply(Action::Resume).is_err());
    }

    #[test]
    fn stopped_is_terminal_and_rejects_all_actions() {
        let mut t = Task::new();
        t.apply(Action::Start).expect("start");
        t.apply(Action::Stop { exit_code: 1 }).expect("stop");
        assert!(t.apply(Action::Start).is_err());
        assert!(t.apply(Action::Pause).is_err());
        assert!(t.apply(Action::Resume).is_err());
        assert!(t.apply(Action::Stop { exit_code: 0 }).is_err());
        // Exit code is not clobbered by a rejected action.
        assert_eq!(t.exit_code(), Some(1));
    }

    #[test]
    fn cannot_stop_created() {
        let mut t = Task::new();
        assert!(t.apply(Action::Stop { exit_code: 0 }).is_err());
    }
}
