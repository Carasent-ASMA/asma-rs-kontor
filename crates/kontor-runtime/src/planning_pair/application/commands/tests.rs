//! Source-level owner traces; no runtime or deployment qualification.

use std::sync::Mutex;

use super::contribution::ContributionOwner;
use super::recovery::{Answer, Input, RecoveryOwner};
use super::*;
use crate::planning_pair::PlanningPairLaunchContext;
use crate::planning_pair::application::{Applied, tests::command_pair};
use crate::planning_pair::recovery::{RecoveryRefusal, WithdrawReason};
use crate::{RuntimeAdapter, RuntimeError};
use kontor_core::consultation::ConsultationRunState;
use kontor_core::id::BoundedText;
use kontor_core::planning_pair::{PlanningPairDisposition, PlanningPairRound, PlanningPairSpec};
use kontor_core::receipt::CommandKind;
use kontor_core::repository::{
    PlanningPairMemberReadback, StoredConsultationSeat, StoredPlanningPairContribution,
};
use kontor_core::state::NativeRuntimeIdentity;

struct Fake {
    authenticated: bool,
    trace: Mutex<Vec<&'static str>>,
}

impl Fake {
    fn step(&self, name: &'static str) {
        self.trace.lock().expect("trace").push(name);
    }
    fn authenticate(&self, name: &'static str) -> Result<(), String> {
        self.step(name);
        if self.authenticated {
            Ok(())
        } else {
            Err("fenced".to_owned())
        }
    }
    fn unexpected<T>(&self, name: &'static str) -> Result<T, String> {
        self.step(name);
        Err(format!("unexpected {name}"))
    }
    fn receipt() -> CommandReceiptId {
        CommandReceiptId::parse("01991c00-0000-7000-8000-000000000099").expect("receipt")
    }
}

impl CommandOwner for Fake {
    type Error = String;
    type Actor = ();
    fn domain_error(&self, error: &DomainError) -> String {
        error.to_string()
    }
    fn now(&self) -> Timestamp {
        command_pair().run.updated_at
    }
    fn stored_run(
        &self,
        _: ProjectId,
        _: PlanningPairRunId,
    ) -> Result<StoredConsultationRun, String> {
        self.step("run");
        Ok(command_pair().run)
    }
    fn pair_state(&self, _: &StoredConsultationRun) -> Result<PairState, String> {
        self.step("restore");
        Ok(command_pair())
    }
    fn presented(&self, (): ()) -> SeatGeneration {
        self.step("presented");
        SeatGeneration {
            seat_binding_id: command_pair().seats[0].seat_binding_id,
            occupancy_generation: 1,
        }
    }
    fn canonical(&self, document: &serde_json::Value) -> Result<CanonicalDocument, String> {
        self.step("intent");
        CanonicalDocument::from_value(document).map_err(|error| error.to_string())
    }
    fn replayed(
        &self,
        _: &IdempotencyKey,
        _: &CanonicalDocument,
        _: &AggregateRef,
    ) -> Result<Option<Replay>, String> {
        self.step("replay");
        Ok(Some(Replay {
            id: Self::receipt(),
            revision: AggregateRevision::INITIAL,
        }))
    }
    fn expect_revision(
        &self,
        _: &StoredConsultationRun,
        _: AggregateRevision,
    ) -> Result<(), String> {
        self.unexpected("revision")
    }
}

impl ContributionOwner for Fake {
    type Output = (Viewer, Applied);
    fn authenticate_member(&self, _: &PairState, (): ()) -> Result<PlanningPairSlot, String> {
        self.authenticate("member")?;
        Ok(PlanningPairSlot::SeatA)
    }
    fn authenticate_caller(&self, _: &StoredConsultationRun, (): ()) -> Result<(), String> {
        self.authenticate("caller")
    }
    fn missing_clarification(&self) -> String {
        "missing clarification".to_owned()
    }
    fn append_revision(
        &self,
        _: &PairState,
        _: ConsultationRunState,
        _: Option<&StoredPlanningPairContribution>,
    ) -> Result<StoredConsultationRun, String> {
        self.unexpected("append")
    }
    fn record(
        &self,
        _: &IdempotencyKey,
        _: ProjectId,
        _: CommandKind,
        _: AggregateRef,
        _: AggregateRevision,
        _: &CanonicalDocument,
    ) -> Result<CommandReceiptId, String> {
        self.unexpected("record")
    }
    fn render(
        &self,
        _: &PairState,
        viewer: Viewer,
        receipt: CommandReceiptId,
        applied: Applied,
    ) -> Result<Self::Output, String> {
        self.step("render");
        assert_eq!(receipt, Self::receipt());
        Ok((viewer, applied))
    }
}

#[async_trait::async_trait]
impl RecoveryOwner for Fake {
    type Activity = ();
    type Output = ();
    fn begin_native_activity(&self) -> Result<(), String> {
        self.step("activity");
        Ok(())
    }
    fn authenticate_recovering_caller(
        &self,
        _: &StoredConsultationRun,
        _: &PlanningPairSpec,
        (): (),
    ) -> Result<(), String> {
        self.authenticate("recovering caller")
    }
    fn no_such_member(&self) -> String {
        "no such member".to_owned()
    }
    fn member_context(
        &self,
        _: &StoredConsultationRun,
        _: &PairState,
        _: &StoredConsultationSeat,
        _: PlanningPairSlot,
    ) -> Result<PlanningPairLaunchContext, String> {
        self.unexpected("context")
    }
    fn recovery_refusal(&self, refusal: RecoveryRefusal) -> String {
        format!("{refusal:?}")
    }
    fn ensure_state(&self) -> Result<(), String> {
        self.unexpected("state")
    }
    fn runtime(&self) -> Result<std::sync::Arc<dyn RuntimeAdapter>, String> {
        self.unexpected("runtime")
    }
    fn require_member_routes(&self, _: &dyn RuntimeAdapter, _: &PairState) -> Result<(), String> {
        self.unexpected("routes")
    }
    fn runtime_error(&self, error: &RuntimeError) -> String {
        error.to_string()
    }
    fn disqualify(&self, _: &PlanningPairMemberReadback) -> Result<(), String> {
        self.unexpected("disqualify")
    }
    fn withdrawn(&self, reason: WithdrawReason, _: &NativeRuntimeIdentity) -> String {
        format!("{reason:?}")
    }
    async fn recovery_hold_point(&self) {
        self.step("hold");
    }
    fn requalify(
        &self,
        _: &PlanningPairMemberReadback,
        _: &IdempotencyKey,
        _: AggregateRef,
        _: AggregateRevision,
        _: &CanonicalDocument,
    ) -> Result<(StoredConsultationRun, CommandReceiptId, bool), String> {
        self.unexpected("requalify")
    }
    fn render(&self, _: Answer<'_>) -> Result<(), String> {
        self.unexpected("recovery render")
    }
}

#[derive(Clone, Copy)]
enum Command {
    Finding,
    Answer,
    Clarification,
    Disposition,
}
const COMMANDS: [Command; 4] = [
    Command::Finding,
    Command::Answer,
    Command::Clarification,
    Command::Disposition,
];

fn run_command(fake: &Fake, command: Command) -> Result<(Viewer, Applied), String> {
    let pair = command_pair();
    let project = pair.run.project_id;
    let kontor_core::consultation::ConsultationRunId::PlanningPair(run) = pair.run.id else {
        panic!("pair")
    };
    let key = IdempotencyKey::parse("command-key").expect("key");
    let text = BoundedText::parse("One contribution").expect("text");
    let revision = AggregateRevision::INITIAL;
    match command {
        Command::Finding | Command::Answer => contribution::record(
            fake,
            &key,
            project,
            run,
            (),
            if matches!(command, Command::Finding) {
                PlanningPairRound::Findings
            } else {
                PlanningPairRound::Clarification
            },
            &text,
            revision,
        ),
        Command::Clarification => contribution::clarify(
            fake,
            &key,
            project,
            run,
            (),
            &text,
            &[PlanningPairSlot::SeatA],
            revision,
        ),
        Command::Disposition => contribution::decide(
            fake,
            &key,
            project,
            run,
            (),
            &PlanningPairDisposition {
                members: Vec::new(),
                rationale: text,
            },
            revision,
        ),
    }
}

#[test]
fn a_fenced_actor_never_reaches_replay_for_any_contribution_command() {
    for command in COMMANDS {
        let fake = Fake {
            authenticated: false,
            trace: Mutex::new(Vec::new()),
        };
        assert_eq!(run_command(&fake, command), Err("fenced".to_owned()));
        let role = if matches!(command, Command::Finding | Command::Answer) {
            "member"
        } else {
            "caller"
        };
        assert_eq!(*fake.trace.lock().expect("trace"), ["run", "restore", role]);
    }
}

#[test]
fn each_exact_replay_is_authorized_and_never_reaches_revision_or_write() {
    for command in COMMANDS {
        let fake = Fake {
            authenticated: true,
            trace: Mutex::new(Vec::new()),
        };
        let member = matches!(command, Command::Finding | Command::Answer);
        let viewer = if member {
            Viewer::Member(PlanningPairSlot::SeatA)
        } else {
            Viewer::Caller
        };
        assert_eq!(
            run_command(&fake, command),
            Ok((viewer, Applied::Unchanged))
        );
        assert_eq!(
            *fake.trace.lock().expect("trace"),
            [
                "run",
                "restore",
                if member { "member" } else { "caller" },
                "presented",
                "intent",
                "replay",
                "render"
            ]
        );
    }
}

#[tokio::test]
async fn a_fenced_recovering_caller_never_reaches_context_or_runtime() {
    let fake = Fake {
        authenticated: false,
        trace: Mutex::new(Vec::new()),
    };
    let pair = command_pair();
    let kontor_core::consultation::ConsultationRunId::PlanningPair(run) = pair.run.id else {
        panic!("pair")
    };
    let seat = &pair.seats[0];
    let input = Input {
        expected_revision: pair.run.revision,
        expected_member_generation: 1,
        expected_identity: seat.native_identity.as_ref().expect("native"),
        expected_provider_session_id: None,
    };
    let key = IdempotencyKey::parse("recovery-key").expect("key");
    assert_eq!(
        recovery::recover(
            &fake,
            &key,
            pair.run.project_id,
            run,
            seat.seat_binding_id,
            (),
            input
        )
        .await,
        Err("fenced".to_owned())
    );
    assert_eq!(
        *fake.trace.lock().expect("trace"),
        ["activity", "run", "restore", "recovering caller"]
    );
}
