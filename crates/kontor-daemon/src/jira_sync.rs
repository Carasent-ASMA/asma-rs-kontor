//! Resident Jira convergence controller.
//!
//! Notifications make convergence prompt; the periodic backstop makes it
//! correct after missed notifications and process restarts. Durable Kontor
//! state remains the queue, so this loop stores no second desired-state copy.

use std::sync::Arc;
use std::time::Duration;

use kontor_api::state::{ApiState, BarrierState};
use tokio::time::MissedTickBehavior;
use tracing::{info, warn};

use crate::applications::Services;

/// Maximum quiet interval before every Jira-bound subject is checked again.
pub const RECONCILE_INTERVAL: Duration = Duration::from_secs(30);

/// Reconcile after startup, on every committed control-plane wake, and on a
/// bounded timer until the daemon's shared shutdown signal is raised.
pub async fn poll_until_stopped(services: Arc<Services>, state: ApiState) {
    if state.barrier().settled().await != BarrierState::Open {
        warn!(
            realm_id = %state.realm_id(),
            "automatic Jira reconciliation stayed stopped because startup reconciliation failed"
        );
        return;
    }

    let mut appends = state.signals().appends();
    let mut stops = state.signals().stops();
    let mut backstop = tokio::time::interval(RECONCILE_INTERVAL);
    backstop.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        if state.signals().is_stopping() {
            return;
        }
        tokio::select! {
            changed = stops.changed() => {
                if changed.is_err() || *stops.borrow_and_update() {
                    return;
                }
                continue;
            }
            changed = appends.changed() => {
                if changed.is_err() {
                    return;
                }
                let _ = *appends.borrow_and_update();
            }
            _ = backstop.tick() => {}
        }

        let report = services.reconcile_jira_once().await;
        if report.blocked > 0 {
            warn!(
                realm_id = %state.realm_id(),
                task_subjects = report.task_subjects,
                epic_subjects = report.epic_subjects,
                converged = report.converged,
                applied = report.applied,
                blocked = report.blocked,
                "automatic Jira reconciliation completed with blocked subjects"
            );
        } else if report.task_subjects + report.epic_subjects > 0 {
            info!(
                realm_id = %state.realm_id(),
                task_subjects = report.task_subjects,
                epic_subjects = report.epic_subjects,
                converged = report.converged,
                applied = report.applied,
                "automatic Jira reconciliation completed"
            );
        }
    }
}
