//! ASMA-8282 W3a: the invocation coordinator, moved into the shared runtime
//! application, answers every invocation outcome with the same full body.
//!
//! Each scenario drives a real caller credential through the registered
//! route. It asserts its own invariants, then prints one normalized
//! `PARITY` line. Generated ids, hashes and instants are replaced by stable
//! placeholders in order of first appearance, so the same test run against
//! the `8c706eec` source and the candidate source gives byte-identical lines
//! exactly when the bodies agree.

use std::collections::BTreeMap;

use super::*;
use kontor_api::error::{ApiError, ApiErrorCode};
use kontor_core::id::AggregateRevision;
use kontor_runtime::planning_pair::MandatoryMemberField;

/// `answer` is exactly `expected`, status and whole body.
fn assert_answered(answer: &Answer, expected: &ApiError, why: &str) {
    assert_eq!(
        answer.status.as_u16(),
        expected.code.status().as_u16(),
        "{why}: {}",
        answer.body
    );
    assert_eq!(
        answer.json(),
        serde_json::to_value(expected.body()).expect("a refusal body"),
        "{why}"
    );
}

/// Placeholders for generated values, by first appearance.
#[derive(Default)]
struct Normalizer {
    seen: BTreeMap<String, String>,
}

impl Normalizer {
    fn placeholder(&mut self, kind: &str, value: &str) -> String {
        let next = self.seen.len();
        self.seen
            .entry(value.to_owned())
            .or_insert_with(|| format!("<{kind}{next}>"))
            .clone()
    }

    fn string(&mut self, text: &str) -> String {
        let bytes = text.as_bytes();
        let mut out = String::new();
        let mut index = 0;
        while index < bytes.len() {
            if let Some(len) = uuid_at(bytes, index) {
                out.push_str(&self.placeholder("id", &text[index..index + len]));
                index += len;
            } else if let Some(len) = hex64_at(bytes, index) {
                out.push_str(&self.placeholder("hash", &text[index..index + len]));
                index += len;
            } else if let Some(len) = instant_at(bytes, index) {
                out.push_str("<instant>");
                index += len;
            } else {
                out.push(char::from(bytes[index]));
                index += 1;
            }
        }
        out
    }

    fn value(&mut self, value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::String(text) => serde_json::Value::String(self.string(text)),
            serde_json::Value::Array(items) => {
                serde_json::Value::Array(items.iter().map(|item| self.value(item)).collect())
            }
            serde_json::Value::Object(fields) => serde_json::Value::Object(
                fields
                    .iter()
                    .map(|(key, field)| (key.clone(), self.value(field)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }
}

fn is_hex(byte: u8) -> bool {
    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
}

fn uuid_at(bytes: &[u8], at: usize) -> Option<usize> {
    let shape = [8, 4, 4, 4, 12];
    let mut index = at;
    for (group, width) in shape.iter().enumerate() {
        if group > 0 {
            if bytes.get(index) != Some(&b'-') {
                return None;
            }
            index += 1;
        }
        let end = index + width;
        if end > bytes.len() || !bytes[index..end].iter().all(|&byte| is_hex(byte)) {
            return None;
        }
        index = end;
    }
    Some(index - at)
}

fn hex64_at(bytes: &[u8], at: usize) -> Option<usize> {
    let end = at + 64;
    (end <= bytes.len()
        && bytes[at..end].iter().all(|&byte| is_hex(byte))
        && bytes.get(end).is_none_or(|&byte| !is_hex(byte)))
    .then_some(64)
}

/// An RFC 3339 instant: `YYYY-MM-DDTHH:MM:SS`, an optional fraction, then `Z`.
fn instant_at(bytes: &[u8], at: usize) -> Option<usize> {
    let digits = |from: usize, count: usize| {
        from + count <= bytes.len() && bytes[from..from + count].iter().all(u8::is_ascii_digit)
    };
    let fixed = digits(at, 4)
        && bytes.get(at + 4) == Some(&b'-')
        && digits(at + 5, 2)
        && bytes.get(at + 7) == Some(&b'-')
        && digits(at + 8, 2)
        && bytes.get(at + 10) == Some(&b'T')
        && digits(at + 11, 2)
        && bytes.get(at + 13) == Some(&b':')
        && digits(at + 14, 2)
        && bytes.get(at + 16) == Some(&b':')
        && digits(at + 17, 2);
    if !fixed {
        return None;
    }
    let mut index = at + 19;
    if bytes.get(index) == Some(&b'.') {
        index += 1;
        while bytes.get(index).is_some_and(u8::is_ascii_digit) {
            index += 1;
        }
    }
    (bytes.get(index) == Some(&b'Z')).then_some(index + 1 - at)
}

/// Print one scenario's normalized answer and return it for local checks.
fn parity(normalizer: &mut Normalizer, scenario: &str, answer: &Answer) -> serde_json::Value {
    let normalized = normalizer.value(&answer.json());
    println!(
        "PARITY {scenario} {} {}",
        answer.status.as_u16(),
        serde_json::to_string(&normalized).expect("a body")
    );
    normalized
}

fn launches(calls: &[AdapterCall]) -> usize {
    calls
        .iter()
        .filter(|call| matches!(call, AdapterCall::LaunchConsultation(_)))
        .count()
}

/// Created, replayed, and every invocation refusal the sequence reaches on a
/// running realm.
#[tokio::test]
async fn every_invocation_outcome_answers_its_full_baseline_body() {
    let realm = pair_realm("/tmp/kontor-asma8282-w3a-parity").await;
    let world = &realm.world;
    let mut normalizer = Normalizer::default();

    let body = realm.invoke_body(&realm.profile, "Parity plan").await;
    world.fake.take_calls();
    let created = realm
        .invoke_with(&body, realm.caller_token(), "w3a-invoke")
        .await;
    assert_eq!(created.status, 200, "{}", created.body);
    assert_eq!(created.json()["receipt"]["applied"], "created");
    parity(&mut normalizer, "created", &created);
    assert_eq!(
        launches(&world.fake.take_calls()),
        2,
        "both members launched"
    );
    let replayed = realm
        .invoke_with(&body, realm.caller_token(), "w3a-invoke")
        .await;
    assert_eq!(replayed.json()["receipt"]["applied"], "unchanged");
    parity(&mut normalizer, "replayed", &replayed);
    assert_eq!(
        launches(&world.fake.take_calls()),
        0,
        "a replay launches nothing"
    );

    let mut moved = realm.invoke_body(&realm.profile, "Moved plan").await;
    moved["profile"]["definition_hash"] = ContentHash::of(b"another").as_str().into();
    let refused = realm
        .invoke_with(&moved, realm.caller_token(), "w3a-document")
        .await;
    let deny = |code, rule| ApiError::new(world.realm_id(), code, rule);
    assert_answered(
        &refused,
        &deny(
            ApiErrorCode::InvalidRequest,
            "the pinned planning pair document hash is not the published one",
        ),
        "document moved",
    );
    parity(&mut normalizer, "document-moved", &refused);

    let mut stale = realm.invoke_body(&realm.profile, "Stale plan").await;
    let revision = stale["expected_revision"].as_u64().expect("a revision");
    stale["expected_revision"] = (revision + 1).into();
    let refused = realm
        .invoke_with(&stale, realm.caller_token(), "w3a-stale")
        .await;
    assert_answered(
        &refused,
        &deny(
            ApiErrorCode::RevisionConflict,
            "the epic moved since the planning pair invocation was prepared",
        )
        .with_revision(Some(
            AggregateRevision::parse(revision).expect("a revision"),
        )),
        "epic moved",
    );
    parity(&mut normalizer, "epic-moved", &refused);

    let duplicate = realm.invoke_body(&realm.profile, "Parity plan").await;
    let refused = realm
        .invoke_with(&duplicate, realm.caller_token(), "w3a-duplicate")
        .await;
    let existing = created.json()["planning_pair_run_id"]
        .as_str()
        .expect("the created run")
        .to_owned();
    assert_answered(
        &refused,
        &deny(
            ApiErrorCode::IdempotencyConflict,
            "consultation_semantic_duplicate: this planning pair scope and topic already has one run",
        )
        .about("consultation semantic identity")
        .located_at(format!("consultation-runs/{existing}"))
        .advising("read or resume the existing consultation run"),
        "semantic duplicate",
    );
    parity(&mut normalizer, "semantic-duplicate", &refused);

    let tpm = realm.seat_token(realm.tpm, realm.hosted_generation(realm.tpm));
    let other = realm.invoke_body(&realm.profile, "Role plan").await;
    let refused = realm.invoke_with(&other, tpm, "w3a-role").await;
    assert_answered(
        &refused,
        &deny(
            ApiErrorCode::Forbidden,
            "the authenticated seat's role may not convene this planning pair",
        ),
        "caller role",
    );
    parity(&mut normalizer, "caller-role", &refused);

    world.fake.withholding_planning_pair_members_on("cursor");
    world.fake.take_calls();
    let route = realm.invoke_body(&realm.profile, "Route plan").await;
    let refused = realm
        .invoke_with(&route, realm.caller_token(), "w3a-route")
        .await;
    assert_answered(
        &refused,
        &deny(
            ApiErrorCode::UnsupportedCapability,
            "this runtime cannot establish the closed planning pair member surface for a member route",
        )
        .about("planning pair member route")
        .located_at("providers/cursor/not_composed"),
        "route unsupported",
    );
    parity(&mut normalizer, "route-unsupported", &refused);
    assert!(
        world.fake.take_calls().iter().all(|call| !matches!(
            call,
            AdapterCall::PrepareContainer(_) | AdapterCall::LaunchConsultation(_)
        )),
        "no native effect before the route refusal"
    );
}

/// A frozen pair whose launch was interrupted resumes under its key; a member
/// kept unqualified is never relaunched, and its replay answers the same
/// refusal.
#[tokio::test]
async fn interrupted_and_unqualified_invocations_answer_their_full_baseline_bodies() {
    let realm = pair_realm("/tmp/kontor-asma8282-w3a-parity-resume").await;
    let world = &realm.world;
    let mut normalizer = Normalizer::default();

    let seat_a = kontor_core::id::RoleSlotId::parse("seat-a").expect("a slot");
    world.fake.refusing_launch_of(&seat_a);
    let body = realm.invoke_body(&realm.profile, "Resume plan").await;
    let interrupted = realm
        .invoke_with(&body, realm.caller_token(), "w3a-resume")
        .await;
    assert_ne!(interrupted.status, 200, "{}", interrupted.body);
    parity(&mut normalizer, "interrupted", &interrupted);
    world.fake.allowing_launch_of(&seat_a);
    world.fake.take_calls();
    let resumed = realm
        .invoke_with(&body, realm.caller_token(), "w3a-resume")
        .await;
    assert_eq!(resumed.status, 200, "{}", resumed.body);
    parity(&mut normalizer, "resumed", &resumed);

    let (kept_pair, kept_body) = unqualified_pair(
        &realm,
        PlanningPairSlot::SeatB,
        MandatoryMemberField::ToolRestrictions,
        "Kept plan",
        "w3a-kept",
    )
    .await;
    world.fake.take_calls();
    let kept = realm
        .invoke_with(&kept_body, realm.caller_token(), "w3a-kept")
        .await;
    let native = realm
        .known_claim(&kept_pair, kept_pair.seat_b)
        .expect("the kept claim")
        .identity
        .native_id
        .as_str()
        .to_owned();
    assert_answered(
        &kept,
        &ApiError::new(
            world.realm_id(),
            ApiErrorCode::Unavailable,
            "the planning pair member's readback did not observe its closed tool restriction",
        )
        .about("planning pair member readback")
        .located_at(format!("native/{native}"))
        .advising("confirmation unknown: the member's native session is kept unqualified; its caller may requalify that same session with kontor_planning_pair_seat_recover"),
        "kept unqualified",
    );
    parity(&mut normalizer, "kept-unqualified", &kept);
    assert_eq!(
        launches(&world.fake.take_calls()),
        0,
        "a kept member is never relaunched"
    );
}
