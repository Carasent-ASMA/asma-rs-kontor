//! Typed native-name rendering contract (ASMA-7967).

use kontor_core::backlog_identity::ConfirmedJiraKey;
use kontor_core::id::ExternalId;
use kontor_core::naming::{
    AiShortName, NameSeparator, NativeNameSegment, NativeNameTemplate, NativeNameToken,
    NativeNameValues,
};

fn tokens(tokens: &[NativeNameToken]) -> NativeNameTemplate {
    NativeNameTemplate::from_segments(
        tokens
            .iter()
            .copied()
            .map(NativeNameSegment::Token)
            .collect(),
    )
    .expect("the fixture template is valid")
}

fn values(area: &str, jira: &str, backlog: &str) -> NativeNameValues {
    NativeNameValues::new()
        .with_area_code(area)
        .with_jira_code(jira)
        .with_kontor_backlog_code(backlog)
}

#[test]
fn item_code_is_one_typed_native_name_value_not_two_recombined_tokens() {
    let template = tokens(&[NativeNameToken::AreaCode, NativeNameToken::ItemCode]);

    let rendered = template
        .render(
            &NameSeparator::parse(" · ").expect("approved separator"),
            &NativeNameValues::new()
                .with_area_code("ESW")
                .with_item_code("KOP-8001"),
        )
        .expect("typed item code renders");

    assert_eq!(rendered.as_str(), "ESW · KOP-8001");
}

#[test]
fn the_v1_matrix_renders_exact_bullet_separated_bytes() {
    let separator = NameSeparator::default();
    assert_eq!(separator.as_str().as_bytes(), " • ".as_bytes());

    let epic = tokens(&[
        NativeNameToken::AreaCode,
        NativeNameToken::JiraCode,
        NativeNameToken::KontorBacklogCode,
    ]);
    let ticket = tokens(&[
        NativeNameToken::AreaCode,
        NativeNameToken::KontorBacklogCode,
    ]);

    for (area, jira, backlog, expected) in [
        ("ESW", "ASMA-7675", "QNR-P1", "ESW • ASMA-7675 • QNR-P1"),
        ("ECP", "ASMA-7675", "QNR-P1", "ECP • ASMA-7675 • QNR-P1"),
        (
            "TSW",
            "ASMA-7676",
            "QNR-P1-01",
            "TSW • ASMA-7676 • QNR-P1-01",
        ),
        ("LSA", "ASMA-7869", "OP", "LSA • ASMA-7869 • OP"),
    ] {
        assert_eq!(
            epic.render(&separator, &values(area, jira, backlog))
                .expect("the complete values render")
                .as_str(),
            expected
        );
    }
    assert_eq!(
        ticket
            .render(&separator, &values("SWE", "ASMA-7967", "OP-19"))
            .expect("the ticket seat renders")
            .as_str(),
        "SWE • OP-19"
    );
}

#[test]
fn the_backlog_code_wins_when_a_descriptive_ai_short_name_is_also_present() {
    let template = tokens(&[
        NativeNameToken::AreaCode,
        NativeNameToken::JiraCode,
        NativeNameToken::KontorBacklogCode,
    ]);
    let ai_short_name =
        AiShortName::parse("Nonprod Delivery").expect("the descriptive label is valid");
    let values = values("ESW", "ASMA-7675", "QNR-P1").with_ai_short_name(&ai_short_name);

    assert_eq!(
        template
            .render(&NameSeparator::default(), &values)
            .expect("the explicit backlog code renders")
            .as_str(),
        "ESW • ASMA-7675 • QNR-P1"
    );
}

#[test]
fn a_separator_only_revision_changes_every_join_without_mutating_v1() {
    let template = tokens(&[
        NativeNameToken::AreaCode,
        NativeNameToken::JiraCode,
        NativeNameToken::KontorBacklogCode,
    ]);
    let values = values("ESW", "ASMA-7675", "QNR-P1");
    let v1 = NameSeparator::default();
    let v2 = NameSeparator::parse(" / ").expect("a specification may choose another separator");

    let first = template.render(&v1, &values).expect("v1 renders");
    assert_eq!(
        template.render(&v2, &values).expect("v2 renders").as_str(),
        "ESW / ASMA-7675 / QNR-P1"
    );
    assert_eq!(
        template.render(&v1, &values).expect("v1 rerenders"),
        first,
        "the same pinned revision remains byte-identical"
    );
}

#[test]
fn every_missing_token_fails_closed_and_names_the_missing_contract() {
    let separator = NameSeparator::default();
    for (token, expected) in [
        (NativeNameToken::AreaCode, "AREA_CODE"),
        (NativeNameToken::JiraCode, "JIRA_CODE"),
        (NativeNameToken::KontorBacklogCode, "KONTOR_BACKLOG_CODE"),
        (NativeNameToken::ItemCode, "ITEM_CODE"),
        (NativeNameToken::AiShortName, "AI_SHORT_NAME"),
        (NativeNameToken::EpicJiraKey, "EPIC_JIRA_KEY"),
        (NativeNameToken::TaskJiraKey, "TASK_JIRA_KEY"),
        (NativeNameToken::ScopeJiraKey, "SCOPE_JIRA_KEY"),
    ] {
        let error = tokens(&[token])
            .render(&separator, &NativeNameValues::new())
            .expect_err("missing identity must never be inferred");
        assert!(
            error.to_string().contains(expected),
            "the refusal identifies {expected}: {error}"
        );
    }
}

#[test]
fn ai_short_names_are_trimmed_two_keyword_values_and_preserve_unicode_bytes() {
    let accepted = AiShortName::parse("QNR levering").expect("two keywords are accepted");
    assert_eq!(accepted.as_str(), "QNR levering");

    for rejected in [
        "QNR",
        " QNR levering",
        "QNR levering ",
        "QNR  levering",
        "QNR levering stage",
        "QNR •",
        "QNR ·",
        "QNR\nlevering",
    ] {
        assert!(
            AiShortName::parse(rejected).is_err(),
            "`{}` must be refused",
            rejected.escape_debug()
        );
    }
    assert!(AiShortName::parse(&format!("Q {}", "x".repeat(64))).is_err());
}

#[test]
fn every_token_keeps_its_exact_serialized_spelling() {
    // The whole closed vocabulary, pinned by value. An old token that silently
    // changed spelling would break every pinned revision that renders from it,
    // and a new token that shipped under an unintended spelling would be
    // published into immutable documents before anyone noticed.
    let expected = [
        (NativeNameToken::Prefix, "PREFIX"),
        (NativeNameToken::EpicItemCode, "EPIC_ITEM_CODE"),
        (NativeNameToken::TaskItemCode, "TASK_ITEM_CODE"),
        (NativeNameToken::ScopeItemCode, "SCOPE_ITEM_CODE"),
        (NativeNameToken::EpicJiraKey, "EPIC_JIRA_KEY"),
        (NativeNameToken::TaskJiraKey, "TASK_JIRA_KEY"),
        (NativeNameToken::ScopeJiraKey, "SCOPE_JIRA_KEY"),
        (NativeNameToken::Topic, "TOPIC"),
        (NativeNameToken::RoleCode, "ROLE_CODE"),
        (NativeNameToken::SlotDisplayName, "SLOT_DISPLAY_NAME"),
        (NativeNameToken::AreaCode, "AREA_CODE"),
        (NativeNameToken::JiraCode, "JIRA_CODE"),
        (NativeNameToken::KontorBacklogCode, "KONTOR_BACKLOG_CODE"),
        (NativeNameToken::ItemCode, "ITEM_CODE"),
        (NativeNameToken::AiShortName, "AI_SHORT_NAME"),
    ];
    assert_eq!(
        expected.len(),
        NativeNameToken::ALL.len(),
        "a token was added or removed without pinning its spelling here"
    );
    for (token, spelling) in expected {
        assert_eq!(token.as_str(), spelling);
        assert_eq!(
            NativeNameToken::parse(spelling).expect("the spelling parses"),
            token
        );
    }
}

#[test]
fn a_confirmed_jira_key_is_admitted_only_in_its_canonical_spelling() {
    let key = ConfirmedJiraKey::parse(&ExternalId::parse("ASMA-8117").expect("an external id"))
        .expect("a canonical confirmed key");
    assert_eq!(key.as_str(), "ASMA-8117");
    assert_eq!(key.number(), "8117");

    // Nothing derived from a title, a bare number, a legacy item code, a UUID
    // or a separator glyph may pass for a confirmed binding.
    for rejected in [
        "ASMA-08117",
        "ASMA-0",
        "ASMA-X",
        "8117",
        "-8117",
        "asma-8117",
        "ASMA•8117",
        "ASMA-8117•",
        "01a07722-c376-77f2-bee9-609793e172de",
    ] {
        let external = ExternalId::parse(rejected).expect("a structurally valid external id");
        assert!(
            ConfirmedJiraKey::parse(&external).is_err(),
            "`{rejected}` must not be admitted as a confirmed Jira key"
        );
    }

    // The ASMA-8050 confirmed-key contract admits a hyphen inside the project
    // key, so `KOP-8117-1` splits into project `KOP-8117` and suffix `1`.
    // ASMA-8117 renders that contract's exact output and deliberately does not
    // tighten it: a live epic already bound to such a key must keep rendering.
    // OQ-8117-02 records the question for the resolver scope that owns it.
    let hyphenated = ExternalId::parse("KOP-8117-1").expect("a structurally valid external id");
    let admitted = ConfirmedJiraKey::parse(&hyphenated).expect("the historical contract admits it");
    assert_eq!(admitted.as_str(), "KOP-8117-1");
    assert_eq!(admitted.number(), "1");
}

#[test]
fn a_jira_key_token_renders_its_exact_confirmed_bytes() {
    let key = ConfirmedJiraKey::parse(&ExternalId::parse("ASMA-8117").expect("an external id"))
        .expect("a canonical confirmed key");
    let rendered = tokens(&[NativeNameToken::TaskJiraKey])
        .render(
            &NameSeparator::default(),
            &NativeNameValues::new().with_task_jira_key(&key),
        )
        .expect("the confirmed key renders");
    assert_eq!(
        rendered.as_str(),
        "ASMA-8117",
        "a rendered key is the confirmed binding itself, never a projection of it"
    );
}
