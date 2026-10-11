//! Typed native-name rendering contract (ASMA-7967).

use kontor_core::backlog_identity::ConfirmedJiraKey;
use kontor_core::id::{ExternalId, ExternalName, MAX_EXTERNAL_NAME_LEN};
use kontor_core::naming::{
    AiShortName, NameSeparator, NativeNameSegment, NativeNameTemplate, NativeNameToken,
    NativeNameValues, render_retired_name,
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
        (NativeNameToken::DeskName, "DESK_NAME"),
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

fn retired_name(name: &str) -> ExternalName {
    render_retired_name(
        &ExternalName::parse(name).expect("valid fixture name"),
        "STALE",
        &NameSeparator::default(),
        60,
    )
    .expect("the selected retirement policy renders")
}

#[test]
fn retired_names_keep_exact_identity_text_under_one_selected_prefix() {
    assert_eq!(
        retired_name("TPM • ASMA-8278 • Shared orchestration").as_str(),
        "STALE • TPM • ASMA-8278 • Shared orchestration"
    );
}

#[test]
fn retired_names_strip_all_repeated_leading_markers_and_are_idempotent() {
    let expected = "STALE • TPM • ASMA-8278";
    for name in [
        "TPM • ASMA-8278",
        "STALE • TPM • ASMA-8278",
        "STALE • STALE • STALE • TPM • ASMA-8278",
    ] {
        let rendered = retired_name(name);
        assert_eq!(rendered.as_str(), expected);
        assert_eq!(retired_name(rendered.as_str()), rendered);
    }
    let shortened = retired_name(&"界".repeat(80));
    assert_eq!(retired_name(shortened.as_str()), shortened);
}

#[test]
fn retired_names_preserve_interior_marker_text_and_nonmatching_prefixes() {
    for name in [
        "TPM • STALE • topic",
        "STALE · TPM",
        "STALE TPM",
        "UNSTALE • TPM",
    ] {
        assert_eq!(retired_name(name).as_str(), format!("STALE • {name}"));
    }
}

#[test]
fn retired_names_preserve_the_exact_sixty_scalar_boundary() {
    for base_length in [51, 52] {
        let base = "x".repeat(base_length);
        let rendered = retired_name(&base);
        assert_eq!(rendered.as_str(), format!("STALE • {base}"));
        assert_eq!(rendered.as_str().chars().count(), 8 + base_length);
    }
    assert_eq!(
        retired_name(&"x".repeat(53)).as_str(),
        format!("STALE • {}…", "x".repeat(51))
    );
}

#[test]
fn retired_names_truncate_unicode_scalars_instead_of_utf8_bytes() {
    let exact = retired_name(&"🦀".repeat(52));
    assert_eq!(exact.as_str(), format!("STALE • {}", "🦀".repeat(52)));
    assert_eq!(exact.as_str().chars().count(), 60);

    let shortened = retired_name(&"🦀".repeat(53));
    assert_eq!(shortened.as_str(), format!("STALE • {}…", "🦀".repeat(51)));
    assert_eq!(shortened.as_str().chars().count(), 60);

    // This contract counts scalars, not graphemes; a combining mark consumes
    // one scalar and no Unicode normalization rewrites the original bytes.
    let combined = retired_name(&"e\u{301}".repeat(27));
    assert_eq!(
        combined.as_str(),
        format!("STALE • {}e…", "e\u{301}".repeat(25))
    );
    assert_eq!(combined.as_str().chars().count(), 60);
}

#[test]
fn retired_names_take_marker_separator_and_cap_from_the_selected_policy() {
    let separator = NameSeparator::parse(" / ").expect("selected separator");
    let rendered = render_retired_name(
        &ExternalName::parse("Seat preservation").expect("fixture name"),
        "RETIRED",
        &separator,
        20,
    )
    .expect("alternative pinned naming policy");
    assert_eq!(rendered.as_str(), "RETIRED / Seat pres…");
    assert_eq!(rendered.as_str().chars().count(), 20);
    assert_eq!(
        render_retired_name(&rendered, "RETIRED", &separator, 20).expect("same policy rerenders"),
        rendered
    );
}

#[test]
fn retired_names_reject_empty_controls_and_separator_bearing_markers() {
    let name = ExternalName::parse("TPM").expect("fixture name");
    for marker in [
        "",
        " ",
        " STALE",
        "STALE ",
        "STALE\n",
        "STALE • OTHER",
        "STALE · OTHER",
        "STA…LE",
    ] {
        assert!(render_retired_name(&name, marker, &NameSeparator::default(), 60).is_err());
    }
    assert!(
        render_retired_name(
            &name,
            "OLD / COPY",
            &NameSeparator::parse(" / ").expect("selected separator"),
            60,
        )
        .is_err()
    );
    // A truncation suffix must not synthesize a complete leading marker on
    // the next call, which would strip the entire shortened title.
    assert!(
        render_retired_name(
            &ExternalName::parse("ABCD").expect("fixture name"),
            "A",
            &NameSeparator::parse("…").expect("valid general separator"),
            4,
        )
        .is_err()
    );
}

#[test]
fn retired_names_reject_caps_that_cannot_preserve_the_marker_and_base() {
    let name = ExternalName::parse("TPM").expect("fixture name");
    for cap in [0, 7, 8, 9, MAX_EXTERNAL_NAME_LEN + 1, usize::MAX] {
        assert!(render_retired_name(&name, "STALE", &NameSeparator::default(), cap).is_err());
    }
    let minimal = render_retired_name(&name, "STALE", &NameSeparator::default(), 10)
        .expect("prefix, one base scalar and ellipsis fit");
    assert_eq!(minimal.as_str(), "STALE • T…");
}

#[test]
fn retired_names_refuse_a_marker_only_or_untrimmed_base() {
    for name in ["STALE|", "STALE|STALE|", "STALE| TPM"] {
        assert!(
            render_retired_name(
                &ExternalName::parse(name).expect("valid external name before stripping"),
                "STALE",
                &NameSeparator::parse("|").expect("selected separator"),
                60,
            )
            .is_err()
        );
    }
}
