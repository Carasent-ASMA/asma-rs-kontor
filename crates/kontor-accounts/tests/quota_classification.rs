//! Reading a provider's own refusal text.
//!
//! The mutants this suite exists to kill:
//!
//! * recording a plan allowance as a drained balance when the instant cannot be
//!   read, which asserts that money is the remedy and blocks forever;
//! * reading a bare wall clock as UTC when the vendor printed local time, which
//!   moves the reset by the offset;
//! * shifting a 24-hour clock by twelve hours because a meridiem was assumed;
//! * classifying an ordinary runtime error as a quota refusal.

use kontor_accounts::{QuotaBasis, QuotaSignal, classify};
use kontor_core::id::{Timestamp, parse_utc_timestamp};
use kontor_core::spec::ProviderQuotaKind;

/// The text Codex actually produced on 2026-08-21, from the report Igor filed.
const CODEX_LIMIT: &str = "[System Error] You've hit your usage limit. Visit \
     https://chatgpt.com/codex/settings/usage to purchase more credits or try \
     again at Aug 23rd, 2026 9:35 AM.";

fn codex() -> QuotaSignal {
    QuotaSignal {
        provider: "codex".to_owned(),
        basis: QuotaBasis::PlanAllowance,
        markers: vec!["usage limit".to_owned()],
        reset_prefix: Some("try again at ".to_owned()),
        // Codex prints the seat's local wall clock with no zone.
        reset_zone: Some("Europe/Oslo".to_owned()),
    }
}

fn openrouter() -> QuotaSignal {
    QuotaSignal {
        provider: "openrouter".to_owned(),
        basis: QuotaBasis::CreditBalance,
        markers: vec!["insufficient".to_owned(), "credits".to_owned()],
        reset_prefix: None,
        reset_zone: None,
    }
}

#[test]
fn the_codex_limit_message_yields_the_instant_it_states() {
    let observed = classify(
        CODEX_LIMIT,
        &[codex(), openrouter()],
        parse_utc_timestamp("2026-08-21T07:00:00Z").expect("a canonical instant"),
    )
    .expect("a quota refusal");
    assert_eq!(observed.provider, "codex");
    assert_eq!(observed.kind, ProviderQuotaKind::Exhausted);
    // 09:35 in Oslo during August is CEST, two hours ahead of UTC.
    assert_eq!(
        observed.resets_at,
        Some(parse_utc_timestamp("2026-08-23T07:35:00Z").expect("a canonical instant"))
    );
}

#[test]
fn the_stated_zone_is_honoured_rather_than_assumed_utc() {
    let mut utc = codex();
    utc.reset_zone = None;
    let observed = classify(
        CODEX_LIMIT,
        &[utc],
        parse_utc_timestamp("2026-08-21T07:00:00Z").expect("a canonical instant"),
    )
    .expect("a quota refusal");
    assert_eq!(
        observed.resets_at,
        Some(parse_utc_timestamp("2026-08-23T09:35:00Z").expect("a canonical instant")),
        "with no zone configured the wall clock is UTC, which is two hours from the Oslo answer"
    );
}

#[test]
fn a_drained_balance_carries_no_instant() {
    let text = "402 Payment Required: insufficient credits for this request";
    let observed = classify(
        text,
        &[codex(), openrouter()],
        parse_utc_timestamp("2026-08-21T07:00:00Z").expect("a canonical instant"),
    )
    .expect("a quota refusal");
    assert_eq!(observed.provider, "openrouter");
    assert_eq!(observed.kind, ProviderQuotaKind::Drained);
    assert_eq!(observed.resets_at, None);
}

#[test]
fn an_allowance_whose_instant_cannot_be_read_is_unknown_and_never_drained() {
    // The wording moved: the marker still matches, the instant does not parse.
    let reworded = "You've hit your usage limit. Try again after the weekly window rolls over.";
    let observed = classify(
        reworded,
        &[codex()],
        parse_utc_timestamp("2026-08-21T07:00:00Z").expect("a canonical instant"),
    )
    .expect("a quota refusal");
    assert_eq!(
        observed.kind,
        ProviderQuotaKind::Unknown,
        "a plan allowance with no readable instant is unresolved, not paid-for"
    );
    assert_eq!(observed.resets_at, None);
}

#[test]
fn a_twenty_four_hour_clock_is_not_shifted() {
    let text = "You've hit your usage limit. Try again at Aug 23, 2026 21:35";
    let observed = classify(
        text,
        &[codex()],
        parse_utc_timestamp("2026-08-21T07:00:00Z").expect("a canonical instant"),
    )
    .expect("a quota refusal");
    assert_eq!(
        observed.resets_at,
        Some(parse_utc_timestamp("2026-08-23T19:35:00Z").expect("a canonical instant")),
        "21:35 is already evening; assuming a meridiem would move it"
    );
}

#[test]
fn an_ordinary_runtime_error_is_not_a_quota_refusal() {
    for text in [
        "the tool call failed: connection reset",
        "compilation error in crates/kontor-core/src/spec.rs",
        "",
    ] {
        assert!(
            classify(
                text,
                &[codex(), openrouter()],
                parse_utc_timestamp("2026-08-21T07:00:00Z").expect("a canonical instant")
            )
            .is_none(),
            "{text:?} is not a quota refusal"
        );
    }
}

#[test]
fn a_signal_with_no_markers_never_matches() {
    // An empty marker list would otherwise match every text, since `all` over an
    // empty iterator is true -- and would classify the whole fleet as exhausted.
    let empty = QuotaSignal {
        markers: Vec::new(),
        ..codex()
    };
    assert!(
        classify(
            CODEX_LIMIT,
            &[empty],
            parse_utc_timestamp("2026-08-21T07:00:00Z").expect("a canonical instant")
        )
        .is_none()
    );
}

/// The head-boundary defect: lowercasing is not length-preserving, so a byte
/// offset found in a lowercased copy does not address the same position in the
/// original. A single non-ASCII character *before* the prefix was enough to
/// shift the slice and lose — or corrupt — the instant the vendor stated.
#[test]
fn a_non_ascii_character_before_the_prefix_does_not_shift_the_reset() {
    // `İ` is one char that lowercases to two, so every byte offset after it
    // moves in the lowercased copy.
    let text = "\u{130} [System Error] You've hit your usage limit. Try again at \
                Aug 23rd, 2026 9:35 AM.";
    let observed = classify(
        text,
        &[codex()],
        parse_utc_timestamp("2026-08-21T07:00:00Z").expect("a canonical instant"),
    )
    .expect("a quota refusal");
    assert_eq!(observed.kind, ProviderQuotaKind::Exhausted);
    assert_eq!(
        observed.resets_at,
        Some(parse_utc_timestamp("2026-08-23T07:35:00Z").expect("a canonical instant")),
        "the instant is read from the original text, not a shifted copy",
    );
}

/// The same defect on the marker side: containment must find ASCII wording
/// regardless of surrounding non-ASCII content.
#[test]
fn markers_match_through_surrounding_non_ascii_content() {
    let text = "\u{130}\u{130}\u{130} you've hit your USAGE LIMIT \u{e5}\u{f8}";
    assert!(
        classify(
            text,
            &[codex()],
            parse_utc_timestamp("2026-08-21T07:00:00Z").expect("a canonical instant")
        )
        .is_some()
    );
}

// ---------------------------------------------------------------------------
// A bare time of day, read from the provider item's own instant.
// ---------------------------------------------------------------------------

/// The message Claude actually produced on 2026-08-30, verbatim, repeated to
/// all three ASMA-7869 builders when the `claude-work` allowance was spent.
const CLAUDE_LIMIT: &str = "You've hit your individual spend limit · ask your admin to raise it \
     at claude.ai/settings/usage?from=cc_cli_limit_message · your session limit resets 10:40pm \
     (Europe/Chisinau)";

fn claude_alias(alias: &str) -> QuotaSignal {
    QuotaSignal {
        provider: alias.to_owned(),
        basis: QuotaBasis::PlanAllowance,
        markers: vec![
            "individual spend limit".to_owned(),
            "claude.ai/settings/usage?from=cc_cli_limit_message".to_owned(),
            "your session limit resets".to_owned(),
        ],
        reset_prefix: Some("your session limit resets ".to_owned()),
        reset_zone: Some("Europe/Chisinau".to_owned()),
    }
}

fn claude() -> QuotaSignal {
    claude_alias("claude-work")
}

fn chisinau(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical instant")
}

/// The incident itself: the exact captured sentence, on both logins, yielding
/// the exact reset the control plane recorded — 22:40 Chisinau is EEST in
/// August, three hours ahead of UTC.
#[test]
fn the_captured_message_classifies_on_both_claude_logins() {
    for alias in ["claude-work", "claude-personal"] {
        let observed = classify(
            CLAUDE_LIMIT,
            &[claude_alias(alias)],
            chisinau("2026-08-30T18:00:00Z"),
        )
        .unwrap_or_else(|| panic!("{alias} classifies the captured refusal"));
        assert_eq!(observed.provider, alias);
        assert_eq!(observed.kind, ProviderQuotaKind::Exhausted);
        assert_eq!(
            observed.resets_at,
            Some(chisinau("2026-08-30T19:40:00Z")),
            "the reset the control plane recorded for this incident",
        );
    }
}

/// Near misses. Each drops or alters exactly one distinctive component, and
/// none may retire a seat.
#[test]
fn near_matches_of_the_captured_message_do_not_classify() {
    for near in [
        // The settings URL, but the older guessed path rather than the captured
        // one. This is the fingerprint that was wrong before the capture.
        "You've hit your individual spend limit · raise it at claude.ai/settings/limits · \
         your session limit resets 10:40pm (Europe/Chisinau)",
        // "will reset at" instead of the captured "resets".
        "You've hit your individual spend limit · claude.ai/settings/usage?from=cc_cli_limit_message \
         · your session limit will reset at 10:40pm",
        // No spend-limit wording.
        "Rate limited · claude.ai/settings/usage?from=cc_cli_limit_message · your session limit \
         resets 10:40pm",
        // No settings endpoint.
        "You've hit your individual spend limit · your session limit resets 10:40pm",
        // An assistant discussing the feature.
        "I'll handle the individual spend limit case; your session limit resets are parsed from \
         the provider item.",
    ] {
        assert!(
            classify(near, &[claude()], chisinau("2026-08-30T18:00:00Z")).is_none(),
            "a near match must not retire a seat: {near:?}",
        );
    }
}

/// The trailing `(Europe/Chisinau)` is an annotation, not part of the clock.
/// Present or absent, the instant is the same -- because it *agrees* with the
/// declared zone.
#[test]
fn the_trailing_parenthesised_zone_does_not_change_the_instant() {
    let without = "You've hit your individual spend limit · \
         claude.ai/settings/usage?from=cc_cli_limit_message · your session limit resets 10:40pm";
    let with_annotation = classify(CLAUDE_LIMIT, &[claude()], chisinau("2026-08-30T18:00:00Z"))
        .expect("a quota refusal")
        .resets_at;
    let plain = classify(without, &[claude()], chisinau("2026-08-30T18:00:00Z"))
        .expect("a quota refusal")
        .resets_at;
    assert_eq!(with_annotation, plain);
    assert_eq!(plain, Some(chisinau("2026-08-30T19:40:00Z")));
}

#[test]
fn a_time_only_reset_later_the_same_day_stays_on_that_day() {
    // The item was emitted 21:00 Chisinau (18:00Z); 22:40 is still ahead.
    let observed = classify(CLAUDE_LIMIT, &[claude()], chisinau("2026-08-30T18:00:00Z"))
        .expect("a quota refusal");
    assert_eq!(observed.resets_at, Some(chisinau("2026-08-30T19:40:00Z")));
}

#[test]
fn a_time_only_reset_already_past_takes_the_next_occurrence() {
    // Emitted 23:00 Chisinau (20:00Z): 22:40 has gone, so the limit returns at
    // 22:40 tomorrow. A reset in the past would unblock the account at once and
    // walk it straight back into the limit.
    let observed = classify(CLAUDE_LIMIT, &[claude()], chisinau("2026-08-30T20:00:00Z"))
        .expect("a quota refusal");
    assert_eq!(
        observed.resets_at,
        Some(chisinau("2026-08-31T19:40:00Z")),
        "a stated time that has passed means the next occurrence",
    );
}

/// The basis is the item's instant, not the read's. A probe running the next
/// morning must not roll the reset forward a day.
#[test]
fn a_delayed_probe_does_not_roll_the_reset_forward() {
    let from_item = classify(CLAUDE_LIMIT, &[claude()], chisinau("2026-08-30T18:00:00Z"))
        .expect("a quota refusal")
        .resets_at;
    let from_much_later = classify(CLAUDE_LIMIT, &[claude()], chisinau("2026-08-31T09:00:00Z"))
        .expect("a quota refusal")
        .resets_at;
    assert_eq!(from_item, Some(chisinau("2026-08-30T19:40:00Z")));
    assert_ne!(
        from_item, from_much_later,
        "the two differ precisely because the basis matters, which is why the \
         daemon passes the item's instant and never its own clock",
    );
}

/// Across the autumn fall-back Chisinau moves EEST -> EET, so the same wall
/// clock is a different instant. Reading it in the declared zone keeps it right.
#[test]
fn a_time_only_reset_honours_the_zone_across_a_dst_change() {
    // 2026-10-25 is the European fall-back; the evening after it is EET, two
    // hours ahead of UTC rather than three.
    let observed = classify(CLAUDE_LIMIT, &[claude()], chisinau("2026-10-26T18:00:00Z"))
        .expect("a quota refusal");
    assert_eq!(
        observed.resets_at,
        Some(chisinau("2026-10-26T20:40:00Z")),
        "22:40 Chisinau in late October is EET",
    );
}

#[test]
fn a_twenty_four_hour_time_only_reset_is_not_shifted() {
    let text = "You've hit your individual spend limit · \
         claude.ai/settings/usage?from=cc_cli_limit_message · your session limit resets 22:40";
    let observed =
        classify(text, &[claude()], chisinau("2026-08-30T18:00:00Z")).expect("a quota refusal");
    assert_eq!(observed.resets_at, Some(chisinau("2026-08-30T19:40:00Z")));
}

/// The stated zone is checked, not ignored. A message whose own parenthesised
/// zone disagrees with the declared one must not be converted as though it
/// said the declared zone: that lands an hour wrong and silently.
///
/// No instant is produced, so the account still blocks — as `Unknown`, which is
/// the classifier's visible prompt to fix the signal — rather than blocking
/// until a wrong time.
#[test]
fn a_stated_zone_that_disagrees_with_the_declared_one_yields_no_instant() {
    let oslo_annotated = "You've hit your individual spend limit · ask your admin to raise it at \
         claude.ai/settings/usage?from=cc_cli_limit_message · your session limit resets 10:40pm \
         (Europe/Oslo)";
    let observed = classify(
        oslo_annotated,
        &[claude()],
        chisinau("2026-08-30T18:00:00Z"),
    )
    .expect("it is still recognisably a quota refusal");
    assert_eq!(
        observed.kind,
        ProviderQuotaKind::Unknown,
        "a disagreeing zone blocks without inventing an instant",
    );
    assert_eq!(observed.resets_at, None);
    // And emphatically not the Chisinau conversion.
    assert_ne!(observed.resets_at, Some(chisinau("2026-08-30T19:40:00Z")));
}

/// An abbreviation is not an IANA name and cannot be compared, so it is left
/// alone rather than guessed at — the declared zone still governs.
#[test]
fn a_non_iana_zone_abbreviation_is_left_alone() {
    let abbreviated = "You've hit your individual spend limit · ask your admin to raise it at \
         claude.ai/settings/usage?from=cc_cli_limit_message · your session limit resets 10:40pm \
         (EEST)";
    let observed = classify(abbreviated, &[claude()], chisinau("2026-08-30T18:00:00Z"))
        .expect("a quota refusal");
    assert_eq!(observed.resets_at, Some(chisinau("2026-08-30T19:40:00Z")));
}
