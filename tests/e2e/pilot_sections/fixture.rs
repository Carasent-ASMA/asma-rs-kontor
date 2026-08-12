//! The disposable project fixture, as data.
//!
//! The shape is the pilot's own: nothing in `kontor-core` reads it. It exists so
//! the five tasks, their modules and their worktrees are stated once, in a file
//! an inspector can read, rather than scattered through the driver as literals.

use serde::Deserialize;

/// The whole `project.json` document.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PilotProject {
    /// The disposable project's identity.
    pub(crate) project: ProjectSeed,
    /// Every task, including the deliberate collision contender.
    pub(crate) tasks: Vec<TaskSeed>,
}

/// The project row to create.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ProjectSeed {
    /// Display name.
    pub(crate) name: String,
    /// The unique root path. Suffixed per run so two runs never collide.
    pub(crate) root_path: String,
}

/// One pilot task.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct TaskSeed {
    /// Stable fixture key, used in evidence paths.
    pub(crate) key: String,
    /// Display title.
    pub(crate) title: String,
    /// The module this task contends for.
    pub(crate) module: String,
    /// Which pack the profile comes from: `bundled` or `incident`.
    pub(crate) pack: String,
    /// The pack category to resolve.
    pub(crate) category: String,
    /// The verified worktree, or `None` for the non-isolated contender.
    pub(crate) worktree: Option<String>,
    /// Whether this task is expected to be admissible at all.
    pub(crate) isolated: bool,
    /// The phases its profile must declare, in order.
    pub(crate) expected_phases: Vec<String>,
}

impl PilotProject {
    /// Parse the fixture.
    ///
    /// # Panics
    /// Panics when the fixture does not deserialize, which is a fixture bug and
    /// not a finding about the tree.
    #[must_use]
    pub(crate) fn parse(json: &str) -> Self {
        serde_json::from_str(json).expect("the pilot project fixture deserializes")
    }

    /// Every task expected to reach the scheduler as admissible work.
    pub(crate) fn isolated(&self) -> impl Iterator<Item = &TaskSeed> {
        self.tasks.iter().filter(|task| task.isolated)
    }

    /// The single deliberate collision contender.
    ///
    /// # Panics
    /// Panics when the fixture declares anything other than exactly one.
    #[must_use]
    pub(crate) fn contender(&self) -> &TaskSeed {
        let mut contenders = self.tasks.iter().filter(|task| !task.isolated);
        let contender = contenders
            .next()
            .expect("the fixture declares a collision contender");
        assert!(
            contenders.next().is_none(),
            "one contender proves the refusal; a second only proves it twice"
        );
        contender
    }
}
