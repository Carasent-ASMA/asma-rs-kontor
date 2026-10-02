//! Deterministic interruption points for the artifact transaction.
//!
//! Qualification needs to stop the installer at every boundary — before the
//! lock, between stage and swap, after one artifact swapped, before cleanup —
//! and then prove recovery reaches exactly one whole state. The production
//! path uses [`NoFaults`]; tests inject failures and side effects without a
//! second code path in the installer.

use std::fmt;
use std::path::Path;

use crate::artifact::install::InstallError;
use crate::artifact::manifest::ArtifactName;
use crate::client::ClientId;

/// A named boundary in one artifact transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailPoint {
    /// Before the exclusive install lock is taken.
    BeforeLock,
    /// After the lock is held, before the journal is read.
    AfterLock,
    /// After the manifest and source set are accepted.
    BeforeStage,
    /// After one artifact has been written into the staging directory.
    AfterStage(ArtifactName),
    /// Before one artifact's compare-before-swap check.
    BeforeSwap(ArtifactName),
    /// After one artifact has been swapped into place.
    AfterSwap(ArtifactName),
    /// After every artifact has swapped, before the journal is finalized.
    BeforeFinalize,
    /// After finalization and cleanup.
    AfterFinalize,
    /// Before one client configuration file is replaced. The path handed to
    /// [`FaultInjector::hit`] is the client configuration file itself.
    BeforeClientWrite(ClientId),
    /// After one client configuration file has been replaced, before it is
    /// read back.
    AfterClientWrite(ClientId),
}

/// Decides whether a transaction continues at each boundary.
///
/// The install root is passed to `hit` so an injector can also *observe* the
/// boundary — for example, overwrite a target between stage and swap to prove
/// the compare-before-swap check refuses a concurrent writer.
pub trait FaultInjector {
    /// Called at every boundary. `Err` aborts the transaction with
    /// [`InstallError::Injected`], leaving the journal for recovery.
    ///
    /// # Errors
    /// Whatever the injector decides.
    fn hit(&mut self, point: FailPoint, install_root: &Path) -> Result<(), InstallError>;
}

/// The production injector: every boundary continues.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoFaults;

impl FaultInjector for NoFaults {
    fn hit(&mut self, _point: FailPoint, _install_root: &Path) -> Result<(), InstallError> {
        Ok(())
    }
}

type ObserveAction = Box<dyn FnMut(&Path) -> Result<(), InstallError>>;

enum Rule {
    Fail(FailPoint),
    Observe(FailPoint, ObserveAction),
}

/// A scripted injector for tests: either aborts at one boundary, or performs
/// one side effect there and continues.
#[derive(Default)]
pub struct ScriptedFaults {
    rule: Option<Rule>,
    visits: usize,
}

impl fmt::Debug for ScriptedFaults {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rule = match &self.rule {
            Some(Rule::Fail(point)) => format!("fail at {point:?}"),
            Some(Rule::Observe(point, _)) => format!("observe at {point:?}"),
            None => "none".to_owned(),
        };
        formatter
            .debug_struct("ScriptedFaults")
            .field("rule", &rule)
            .field("visits", &self.visits)
            .finish()
    }
}

impl ScriptedFaults {
    /// Abort exactly at `point`.
    #[must_use]
    pub fn failing_at(point: FailPoint) -> Self {
        Self {
            rule: Some(Rule::Fail(point)),
            visits: 0,
        }
    }

    /// Run `action` exactly at `point` and continue.
    pub fn observing(
        point: FailPoint,
        action: impl FnMut(&Path) -> Result<(), InstallError> + 'static,
    ) -> Self {
        Self {
            rule: Some(Rule::Observe(point, Box::new(action))),
            visits: 0,
        }
    }

    /// How many boundaries were visited before the script finished.
    #[must_use]
    pub fn visits(&self) -> usize {
        self.visits
    }
}

impl FaultInjector for ScriptedFaults {
    fn hit(&mut self, point: FailPoint, install_root: &Path) -> Result<(), InstallError> {
        self.visits += 1;
        match &mut self.rule {
            Some(Rule::Fail(target)) if *target == point => Err(InstallError::Injected { point }),
            Some(Rule::Observe(target, action)) if *target == point => action(install_root),
            _ => Ok(()),
        }
    }
}
