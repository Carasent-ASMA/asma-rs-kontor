//! Settled attention signals emitted by the store, never delivered here.
//!
//! The store owns durable facts and commits them; it does not own a destination
//! and never performs egress (CON-003). Every signal below is constructed
//! *after* the transaction that settled it has committed, and handed to an
//! optional observer the daemon installed. The observer is a plain function
//! call that must be cheap and non-blocking: the daemon's observer formats the
//! channel-neutral payload and spawns a detached one-shot delivery (CON-006).
//!
//! A panic in an observer is contained here. Observation may never change a
//! command's result (GUD-001), so it is the store that guarantees it rather than
//! trusting every future observer.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use kontor_core::id::{
    CommandReceiptId, ExternalName, MiniProjectId, ProjectId, RealmId, RoleKey, TaskId, Timestamp,
};
use kontor_core::receipt::CommandKind;
use kontor_core::state::TaskState;

/// The settled state one signal reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttentionSignalKind {
    /// A task entered `blocked`.
    TaskBlocked,
    /// A task entered `parked`.
    TaskParked,
    /// A task entered `needs_human`.
    TaskNeedsHuman,
    /// A task entered `failed`.
    TaskFailed,
    /// An abandon receipt was born `confirmed`.
    CommandConfirmed,
}

impl AttentionSignalKind {
    /// The signal a task transition into `state` raises, if any.
    #[must_use]
    pub const fn from_task_state(state: TaskState) -> Option<Self> {
        match state {
            TaskState::Blocked => Some(Self::TaskBlocked),
            TaskState::Parked => Some(Self::TaskParked),
            TaskState::NeedsHuman => Some(Self::TaskNeedsHuman),
            TaskState::Failed => Some(Self::TaskFailed),
            _ => None,
        }
    }

    /// The stable spelling used in diagnostics.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TaskBlocked => "task_blocked",
            Self::TaskParked => "task_parked",
            Self::TaskNeedsHuman => "task_needs_human",
            Self::TaskFailed => "task_failed",
            Self::CommandConfirmed => "command_confirmed",
        }
    }
}

/// One settled fact the daemon may turn into an alert.
///
/// Every field is already normalized. There is deliberately no runtime payload,
/// transcript text, prompt or model output here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttentionSignal {
    /// Owning Realm.
    pub realm_id: RealmId,
    /// Owning project.
    pub project_id: ProjectId,
    /// Owning epic, when the signal names one.
    pub mini_project_id: Option<MiniProjectId>,
    /// The task, when the signal names one.
    pub task_id: Option<TaskId>,
    /// The task's normalized human title.
    pub task_title: Option<ExternalName>,
    /// The seat/role code that owns or last worked the task, when known.
    pub seat: Option<RoleKey>,
    /// The command kind, when the signal names a receipt.
    pub command_kind: Option<CommandKind>,
    /// The receipt, when the signal names one.
    pub receipt_id: Option<CommandReceiptId>,
    /// What settled.
    pub kind: AttentionSignalKind,
    /// When it settled.
    pub occurred_at: Timestamp,
}

/// The daemon-installed observer.
///
/// It must be cheap and must not call back into the store: it runs while the
/// caller still holds the store lock, after the transaction has committed.
pub type AttentionObserver = Arc<dyn Fn(AttentionSignal) + Send + Sync>;

/// The optional observer slot on one store.
#[derive(Clone, Default)]
pub(crate) struct AttentionNotifier(std::sync::Arc<std::sync::Mutex<Option<AttentionObserver>>>);

impl std::fmt::Debug for AttentionNotifier {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let armed = self.0.lock().map(|slot| slot.is_some()).unwrap_or(false);
        formatter
            .debug_struct("AttentionNotifier")
            .field("armed", &armed)
            .finish()
    }
}

impl AttentionNotifier {
    pub(crate) fn install(&self, observer: Option<AttentionObserver>) {
        if let Ok(mut slot) = self.0.lock() {
            *slot = observer;
        }
    }

    pub(crate) fn notify(&self, signal: AttentionSignal) {
        let observer = self
            .0
            .lock()
            .ok()
            .and_then(|slot| slot.as_ref().map(Arc::clone));
        let Some(observer) = observer else {
            return;
        };
        let _ = catch_unwind(AssertUnwindSafe(move || observer(signal)));
    }
}
