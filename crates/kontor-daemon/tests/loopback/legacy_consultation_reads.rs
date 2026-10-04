use super::*;

/// Reproduce only the pre-v94 shape in this test's disposable Realm. No
/// product operation may infer or backfill the discarded subject.
fn remove_fixture_subject(world: &World, run: &str) {
    let connection =
        rusqlite::Connection::open(world.directory.path().join(kontor_daemon::DATABASE_FILE))
            .unwrap();
    connection
        .execute("DROP TRIGGER consultation_run_inputs_are_frozen", [])
        .unwrap();
    assert_eq!(
        connection
            .execute(
                "UPDATE consultation_runs SET subject_kind = NULL, subject_task_id = NULL WHERE run_id = ?1",
                [run],
            )
            .unwrap(),
        1,
    );
}

/// Compare every persistent row, including findings, identities, receipts and
/// events. Read-only API access must not repair even the synthetic legacy row.
fn persistent_rows(world: &World) -> std::collections::BTreeMap<String, Vec<String>> {
    use rusqlite::types::ValueRef;
    let connection = rusqlite::Connection::open_with_flags(
        world.directory.path().join(kontor_daemon::DATABASE_FILE),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    connection
        .execute_batch("PRAGMA query_only = ON; BEGIN")
        .unwrap();
    let tables: Vec<String> = connection
        .prepare("SELECT name FROM sqlite_schema WHERE type = 'table' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    tables
        .into_iter()
        .map(|table| {
            let sql = format!("SELECT * FROM \"{}\"", table.replace('"', "\"\""));
            let mut statement = connection.prepare(&sql).unwrap();
            let columns = statement.column_count();
            let mut rows: Vec<String> = statement
                .query_map([], |row| {
                    let mut values = Vec::with_capacity(columns);
                    for column in 0..columns {
                        values.push(match row.get_ref(column)? {
                            ValueRef::Null => serde_json::Value::Null,
                            ValueRef::Integer(value) => serde_json::json!(value),
                            ValueRef::Real(value) => serde_json::json!(value),
                            ValueRef::Text(value) => serde_json::json!(
                                std::str::from_utf8(value).expect("fixture text is UTF-8")
                            ),
                            ValueRef::Blob(value) => serde_json::json!({"blob": value}),
                        });
                    }
                    Ok(serde_json::to_string(&values).unwrap())
                })
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap();
            rows.sort();
            (table, rows)
        })
        .collect()
}

async fn assert_legacy_read_preserves_everything(
    world: &World,
    path: String,
    mut expected: serde_json::Value,
) {
    world.fake.take_calls();
    let before = persistent_rows(world);
    let read = Call::get(path)
        .signed_as(world, "observer")
        .send(world)
        .await;
    assert_eq!(
        read.status, 200,
        "legacy archival GET must remain readable: {}",
        read.body
    );
    let mut actual = read.json();
    assert!(
        actual.get("container_name").is_none(),
        "an unknown subject has no canonical name"
    );
    assert!(
        actual["subject_evidence"].is_null(),
        "no subject evidence may be inferred"
    );
    for document in [&mut expected, &mut actual] {
        let object = document.as_object_mut().unwrap();
        object.remove("container_name");
        object.remove("subject_evidence");
    }
    assert_eq!(
        actual, expected,
        "topic, findings, result, revision and every identity must survive exactly"
    );
    assert_eq!(
        persistent_rows(world),
        before,
        "archival GET must not change any persisted row"
    );
    assert!(
        world.fake.take_calls().is_empty(),
        "archival GET must cause no runtime call"
    );
}

#[tokio::test]
async fn a_subjectless_settled_committee_remains_readable_without_rewriting_history() {
    let boundary = committee_verdict_boundary(
        "/tmp/kontor-asma8114-legacy-committee-read",
        "asma8114-legacy-committee",
    )
    .await;
    let world = &boundary.composed.world;
    let project = &boundary.composed.project;
    pin_jira_key_successor(world, project, &boundary.epic, "asma8114-legacy-committee").await;
    let path = format!(
        "/v1/projects/{project}/committee-runs/{}",
        boundary.committee_run
    );
    let original = Call::get(&path)
        .signed_as(world, "observer")
        .send(world)
        .await;
    assert_eq!(original.status, 200, "{}", original.body);
    let expected = original.json();
    assert_eq!(expected["state"], "settled");
    assert_eq!(
        expected["outcome"], "non_compliant",
        "a historical FAIL must not be rewritten"
    );
    assert_eq!(expected["findings"].as_array().unwrap().len(), 3);
    assert!(expected["container_name"].is_string());
    remove_fixture_subject(world, &boundary.committee_run);
    assert_legacy_read_preserves_everything(world, path, expected).await;
}

#[tokio::test]
async fn a_subjectless_settled_advisor_remains_readable_without_rewriting_advice() {
    let fixture = jira_key_consultation_fixture("/tmp/kontor-asma8114-legacy-advisor-read").await;
    let world = &fixture.composed.world;
    let project = &fixture.composed.project;
    let epic = &fixture.composed.epic;
    let epic_read = Call::get(format!("/v1/projects/{project}/epics/{epic}"))
        .signed_as(world, "observer")
        .send(world)
        .await;
    prepare_fake_provider_headroom(world, project).await;
    let invoked = Call::post(
        format!("/v1/projects/{project}/epics/{epic}/advisor-runs:invoke"),
        &serde_json::json!({
            "profile": {"id": ADVISOR_PROFILE, "version": 1},
            "topic": "Historical advice integrity",
            "question": "Can archived advice remain readable without an invented subject?",
            "caller_seat_binding_id": fixture.caller,
            "task_id": fixture.task_id,
            "expected_revision": epic_read.json()["revision"],
        }),
    )
    .signed_as(world, "operator")
    .with_key("asma8114-legacy-advisor-invoke")
    .send(world)
    .await;
    assert_eq!(invoked.status, 200, "{}", invoked.body);
    let run = invoked.json()["advisor_run_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let path = format!("/v1/projects/{project}/advisor-runs/{run}");
    let seats = invoked.json()["seats"].as_array().unwrap().clone();
    let mut revision = invoked.json()["receipt"]["revision"].clone();
    for (index, entry) in seats.iter().enumerate() {
        let seat = SeatBindingId::parse(entry["seat_binding_id"].as_str().unwrap()).unwrap();
        let output = Call::post(
            format!("{path}/settle"),
            &serde_json::json!({
                "output": format!("Independent advice {index}: preserve historical evidence and the unknown subject."),
                "expected_revision": revision,
            }),
        )
        .with_token(world.daemon.state().credentials().consultation_seat_credential(seat))
        .with_key(format!("asma8114-legacy-advisor-output-{index}"))
        .send(world)
        .await;
        assert_eq!(output.status, 200, "{}", output.body);
        revision = output.json()["receipt"]["revision"].clone();
    }
    let settled = Call::post(
        format!("{path}/settle"),
        &serde_json::json!({
            "disposition": "accepted",
            "rationale": "Keep the actual advice unchanged.",
            "expected_revision": revision,
        }),
    )
    .signed_as(world, "operator")
    .with_key("asma8114-legacy-advisor-disposition")
    .send(world)
    .await;
    assert_eq!(settled.status, 200, "{}", settled.body);
    let original = Call::get(&path)
        .signed_as(world, "observer")
        .send(world)
        .await;
    assert_eq!(original.status, 200, "{}", original.body);
    let expected = original.json();
    assert_eq!(expected["state"], "settled");
    assert_eq!(expected["advice"].as_array().unwrap().len(), seats.len());
    assert_eq!(
        expected["container_name"],
        format!("ASW • {} • Historical advice integrity", fixture.task_key)
    );
    remove_fixture_subject(world, &run);
    assert_legacy_read_preserves_everything(world, path, expected).await;
}
