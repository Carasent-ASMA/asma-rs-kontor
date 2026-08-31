//! Durable epic backlog identity and Jira-derived item-code behavior.

use kontor_core::backlog_identity::{EpicBacklogCode, JiraItemCode};
use kontor_core::id::{ExternalId, ExternalName};

#[test]
fn automatic_epic_code_starts_with_title_initials() {
    let title = ExternalName::parse("Kontor Backlog Identities").expect("title");

    let code =
        EpicBacklogCode::allocate(&title, std::iter::empty::<&str>()).expect("the title allocates");

    assert_eq!(code.as_str(), "KBI");
}

#[test]
fn collisions_expand_title_characters_in_column_major_order_case_insensitively() {
    let title = ExternalName::parse("Kontor Backlog Identities").expect("title");

    let code = EpicBacklogCode::allocate(&title, ["kbi", "KBIO", "KBIOA"])
        .expect("the next deterministic candidate allocates");

    assert_eq!(code.as_str(), "KBIOAD");
}

#[test]
fn exhausted_title_characters_use_the_smallest_free_numeric_ordinal() {
    let title = ExternalName::parse("Alpha Beta").expect("title");
    let used = [
        "AB",
        "ABL",
        "ABLE",
        "ABLEP",
        "ABLEPT",
        "ABLEPTH",
        "ABLEPTHA",
        "ABLEPTHAA",
        "ABLEPTHAA2",
    ];

    let code = EpicBacklogCode::allocate(&title, used).expect("numeric fallback allocates");

    assert_eq!(code.as_str(), "ABLEPTHAA3");
}

#[test]
fn manual_epic_codes_are_canonical_namespaces_not_issue_numbers() {
    assert_eq!(
        EpicBacklogCode::parse("KOP").expect("override").as_str(),
        "KOP"
    );
    for rejected in ["K", "kop", "KO-P", "8050", &"K".repeat(33)] {
        assert!(EpicBacklogCode::parse(rejected).is_err(), "{rejected}");
    }
}

#[test]
fn automatic_allocation_refuses_titles_with_fewer_than_two_usable_characters() {
    let title = ExternalName::parse("Å A").expect("title");

    assert!(EpicBacklogCode::allocate(&title, std::iter::empty::<&str>()).is_err());
}

#[test]
fn item_code_uses_the_confirmed_jira_keys_canonical_numeric_suffix() {
    let backlog = EpicBacklogCode::parse("KOP").expect("backlog code");
    let jira = ExternalId::parse("ASMA-7869").expect("full Jira binding");

    let item = JiraItemCode::derive(&backlog, &jira).expect("derived projection");

    assert_eq!(item.as_str(), "KOP-7869");
    assert_eq!(
        jira.as_str(),
        "ASMA-7869",
        "the full binding remains intact"
    );
}

#[test]
fn item_code_refuses_noncanonical_or_missing_jira_numeric_identity() {
    let backlog = EpicBacklogCode::parse("KOP").expect("backlog code");

    for rejected in ["ASMA-07869", "ASMA-0", "ASMA-X", "7869", "-7869"] {
        let jira = ExternalId::parse(rejected).expect("structurally valid external id");
        assert!(JiraItemCode::derive(&backlog, &jira).is_err(), "{rejected}");
    }
}
