//! ASMA-8282 B1b: every planning pair command records the exact intent the
//! service recorded before its fingerprint moved to the shared runtime core.
//!
//! Each stored receipt's canonical intent text and hash are compared with the
//! daemon's document at `8cd2305f`, copied verbatim except for where its
//! inputs are read: here, from the request a real credential sent and the
//! facts the realm then holds.

use super::*;
use kontor_core::id::CanonicalDocument;
use kontor_core::planning_pair::{MemberDisposition, PlanningPairDisposition};

/// The one stored receipt of `kind`: its intent text and hash.
fn stored_intent(realm: &PairRealm, kind: &str) -> Vec<(String, String)> {
    let connection = rusqlite::Connection::open(realm.world.directory.path().join("kontor.db"))
        .expect("the realm database opens");
    let mut statement = connection
        .prepare("SELECT intent, intent_hash FROM command_receipts WHERE kind = ?1 ORDER BY rowid")
        .expect("the receipts query prepares");
    statement
        .query_map([kind], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("the receipts read")
        .map(|row| row.expect("a receipt"))
        .collect()
}

fn canonical(document: &serde_json::Value) -> (String, String) {
    let document = CanonicalDocument::from_value(document).expect("a canonical document");
    (
        document.json().to_owned(),
        document.hash().as_str().to_owned(),
    )
}

fn strings(value: &serde_json::Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| item.as_str().expect("a string").to_owned())
                .collect()
        })
        .unwrap_or_default()
}

#[tokio::test]
async fn every_planning_pair_command_records_its_baseline_intent() {
    let realm = pair_realm("/tmp/kontor-asma8282-b1b-intents").await;
    let caller_generation = realm.hosted_generation(realm.caller);
    let mut body = realm.invoke_body(&realm.profile, "Intent plan").await;
    body["members"][1]["unavailable_accounts"] =
        serde_json::json!(["zz-unlisted", "aa-unlisted", "zz-unlisted"]);
    let invoked = realm
        .invoke_with(&body, realm.caller_token(), "b1b-intent-invoke")
        .await;
    assert_eq!(invoked.status, 200, "{}", invoked.body);
    let run = invoked.json();
    let member = |slot: &str| {
        let entry = run["members"]
            .as_array()
            .expect("two members")
            .iter()
            .find(|member| member["slot"] == slot)
            .expect("the member")
            .clone();
        SeatBindingId::parse(entry["seat_binding_id"].as_str().expect("a seat")).expect("a seat")
    };
    let pair = Pair {
        run: run["planning_pair_run_id"]
            .as_str()
            .expect("a run id")
            .to_owned(),
        seat_a: member("seat-a"),
        seat_b: member("seat-b"),
        invoked: run.clone(),
    };
    let run_text = pair.run.clone();

    let recover = realm.recover_body(&pair, pair.seat_a).await;
    let recovered = realm
        .recover_with(
            &pair,
            pair.seat_a,
            &recover,
            realm.caller_token(),
            "b1b-intent-recover",
        )
        .await;
    assert_eq!(recovered.status, 200, "{}", recovered.body);
    for (seat, key) in [(pair.seat_a, "b1b-intent-a"), (pair.seat_b, "b1b-intent-b")] {
        let recorded = realm.finding(&pair, seat, "Keep it whole.", key).await;
        assert_eq!(recorded.status, 200, "{}", recorded.body);
    }
    let asked = realm
        .write(
            &pair,
            "/clarification:request",
            &serde_json::json!({
                "question": "Which step reverts alone?",
                "addressed": ["seat-a"],
                "expected_revision": realm.revision(&pair).await,
            }),
            realm.caller_token(),
            "b1b-intent-ask",
        )
        .await;
    assert_eq!(asked.status, 200, "{}", asked.body);
    let answered = realm
        .write(
            &pair,
            "/answers:record",
            &serde_json::json!({
                "advice": "The schema step reverts alone.",
                "expected_revision": realm.revision(&pair).await,
            }),
            realm.member_token(pair.seat_a, 1),
            "b1b-intent-answer",
        )
        .await;
    assert_eq!(answered.status, 200, "{}", answered.body);
    let released = realm
        .read_with(&pair, Some(realm.caller_token()))
        .await
        .json();
    let finding = |slot: &str| {
        ContentHash::parse(
            released["findings"]
                .as_array()
                .expect("released findings")
                .iter()
                .find(|entry| entry["slot"] == slot)
                .expect("a released finding")["document_hash"]
                .as_str()
                .expect("a hash"),
        )
        .expect("a finding hash")
    };
    let answer = ContentHash::parse(
        released["clarification"]["answers"][0]["document_hash"]
            .as_str()
            .expect("an answer hash"),
    )
    .expect("an answer hash");
    let disposed = realm
        .write(
            &pair,
            "/disposition:record",
            &serde_json::json!({
                "members": [
                    {"slot": "seat-a", "finding": finding("seat-a"), "answer": answer,
                     "disposition": "accepted"},
                    {"slot": "seat-b", "finding": finding("seat-b"), "disposition": "rejected"},
                ],
                "rationale": "Keep the dissent.",
                "expected_revision": realm.revision(&pair).await,
            }),
            realm.caller_token(),
            "b1b-intent-dispose",
        )
        .await;
    assert_eq!(disposed.status, 200, "{}", disposed.body);

    let caller = realm.caller.to_string();
    // Invocation, as at 8cd2305f.
    let members = body["members"].as_array().expect("the requested members");
    let invoke = serde_json::json!({
        "schema_version": 1,
        "operation": "invoke_planning_pair_run",
        "project": realm.project.clone(),
        "epic": realm.epic.clone(),
        "protocol": "planning_pair@1",
        "profile": [
            body["profile"]["id"],
            body["profile"]["version"],
            body["profile"]["definition_hash"],
        ],
        "topic": body["topic"],
        "question": body["question"],
        "task_id": serde_json::Value::Null,
        "members": members.iter().map(|member| serde_json::json!({
            "slot": member["slot"],
            "binding_key": member["binding_key"],
            "unavailable_accounts": strings(&member["unavailable_accounts"]).into_iter().collect::<std::collections::BTreeSet<_>>(),
            "excluded_vendors": strings(&member["excluded_vendors"]).into_iter().collect::<std::collections::BTreeSet<_>>(),
        })).collect::<Vec<_>>(),
        "caller_seat_binding_id": caller,
        "caller_occupancy_generation": caller_generation,
    });
    assert_eq!(
        invoke["members"][1]["unavailable_accounts"],
        serde_json::json!(["aa-unlisted", "zz-unlisted"])
    );
    // Recovery, as at 8cd2305f.
    let claim = realm
        .known_claim(&pair, pair.seat_a)
        .expect("seat A's claim");
    let recovery = serde_json::json!({
        "schema_version": 1,
        "operation": "recover_planning_pair_seat",
        "project": realm.project.clone(),
        "run": run_text,
        "member_seat_binding_id": pair.seat_a.to_string(),
        "slot": "seat-a",
        "caller_seat_binding_id": caller,
        "caller_occupancy_generation": caller_generation,
        "expected_run_revision": recover["expected_run_revision"],
        "expected_member_occupancy_generation": recover["expected_member_occupancy_generation"],
        "expected_native_identity": {
            "runtime_kind": recover["expected_native_identity"]["runtime_kind"],
            "host": recover["expected_native_identity"]["host"],
            "generation": recover["expected_native_identity"]["generation"],
            "native_id": recover["expected_native_identity"]["native_id"],
        },
        "expected_provider_session_id": recover["expected_provider_session_id"],
        "member_context_hash": claim.context_hash.as_str(),
        "placement_hash": claim.placement_hash.as_str(),
    });
    // Findings and the answer, as at 8cd2305f.
    let contribution = |operation: &str, slot: &str, seat: SeatBindingId, advice: &str| {
        serde_json::json!({
            "schema_version": 1,
            "operation": operation,
            "project": realm.project.clone(),
            "run": run_text,
            "slot": slot,
            "advice": advice,
            "member_seat_binding_id": seat.to_string(),
            "member_occupancy_generation": 1,
        })
    };
    // The clarification and the disposition, as at 8cd2305f.
    let clarification = serde_json::json!({
        "schema_version": 1,
        "operation": "request_planning_pair_clarification",
        "project": realm.project.clone(),
        "run": run_text,
        "question": "Which step reverts alone?",
        "addressed": ["seat-a"],
        "caller_seat_binding_id": caller,
        "caller_occupancy_generation": caller_generation,
    });
    let disposition = PlanningPairDisposition {
        members: vec![
            MemberDisposition {
                slot: PlanningPairSlot::SeatA,
                finding: finding("seat-a"),
                answer: Some(answer),
                disposition: kontor_core::consultation::AdviceDisposition::Accepted,
            },
            MemberDisposition {
                slot: PlanningPairSlot::SeatB,
                finding: finding("seat-b"),
                answer: None,
                disposition: kontor_core::consultation::AdviceDisposition::Rejected,
            },
        ],
        rationale: kontor_core::id::BoundedText::parse("Keep the dissent.").expect("a rationale"),
    };
    let disposition = serde_json::json!({
        "schema_version": 1,
        "operation": "record_planning_pair_disposition",
        "project": realm.project.clone(),
        "run": run_text,
        "disposition": disposition,
        "caller_seat_binding_id": caller,
        "caller_occupancy_generation": caller_generation,
    });

    let expected = [
        ("invoke_planning_pair_run", vec![canonical(&invoke)]),
        ("recover_planning_pair_seat", vec![canonical(&recovery)]),
        (
            "record_planning_pair_finding",
            vec![
                canonical(&contribution(
                    "record_planning_pair_finding",
                    "seat-a",
                    pair.seat_a,
                    "Keep it whole.",
                )),
                canonical(&contribution(
                    "record_planning_pair_finding",
                    "seat-b",
                    pair.seat_b,
                    "Keep it whole.",
                )),
            ],
        ),
        (
            "request_planning_pair_clarification",
            vec![canonical(&clarification)],
        ),
        (
            "record_planning_pair_answer",
            vec![canonical(&contribution(
                "record_planning_pair_answer",
                "seat-a",
                pair.seat_a,
                "The schema step reverts alone.",
            ))],
        ),
        (
            "record_planning_pair_disposition",
            vec![canonical(&disposition)],
        ),
    ];
    for (kind, intents) in expected {
        assert_eq!(stored_intent(&realm, kind), intents, "{kind}");
    }
}
