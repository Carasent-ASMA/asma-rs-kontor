//! Typed native-name rendering contract (ASMA-7967).

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
        (NativeNameToken::AiShortName, "AI_SHORT_NAME"),
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
