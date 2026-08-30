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

/// The captured 2026-08-30 Claude refusal shape.
const CLAUDE_LIMIT: &str = "You've reached your individual spend limit. Manage it at \
     https://console.anthropic.com/settings/limits — your session limit will reset at 10:40pm.";

fn claude() -> QuotaSignal {
    QuotaSignal {
        provider: "claude-work".to_owned(),
        basis: QuotaBasis::PlanAllowance,
        markers: vec![
            "individual spend limit".to_owned(),
            "/settings/limits".to_owned(),
            "session limit will reset".to_owned(),
        ],
        reset_prefix: Some("session limit will reset at ".to_owned()),
        reset_zone: Some("Europe/Oslo".to_owned()),
    }
}

fn oslo(text: &str) -> Timestamp {
    parse_utc_timestamp(text).expect("a canonical instant")
}

#[test]
fn a_time_only_reset_later_the_same_day_stays_on_that_day() {
    // The item was emitted 20:00 Oslo (18:00Z) on 2026-08-30; 22:40 is still
    // ahead of it, so the reset is that evening.
    let observed =
        classify(CLAUDE_LIMIT, &[claude()], oslo("2026-08-30T18:00:00Z")).expect("a quota refusal");
    assert_eq!(observed.provider, "claude-work");
    assert_eq!(observed.kind, ProviderQuotaKind::Exhausted);
    assert_eq!(
        observed.resets_at,
        Some(oslo("2026-08-30T20:40:00Z")),
        "22:40 Oslo in August is CEST, two hours ahead of UTC",
    );
}

#[test]
fn a_time_only_reset_already_past_takes_the_next_occurrence() {
    // The item was emitted 23:00 Oslo (21:00Z); 22:40 has already gone, so the
    // limit returns at 22:40 *tomorrow*. A reset in the past would unblock the
    // account immediately and walk straight back into the limit.
    let observed =
        classify(CLAUDE_LIMIT, &[claude()], oslo("2026-08-30T21:00:00Z")).expect("a quota refusal");
    assert_eq!(
        observed.resets_at,
        Some(oslo("2026-08-31T20:40:00Z")),
        "a stated time that has passed means the next occurrence",
    );
}

/// The basis is the item's instant, not the read's. A probe running the next
/// morning must still produce the reset the message described.
#[test]
fn a_delayed_probe_does_not_roll_the_reset_forward() {
    let from_item = classify(CLAUDE_LIMIT, &[claude()], oslo("2026-08-30T18:00:00Z"))
        .expect("a quota refusal")
        .resets_at;
    let from_much_later = classify(CLAUDE_LIMIT, &[claude()], oslo("2026-08-31T09:00:00Z"))
        .expect("a quota refusal")
        .resets_at;
    assert_eq!(from_item, Some(oslo("2026-08-30T20:40:00Z")));
    assert_ne!(
        from_item, from_much_later,
        "the two differ precisely because the basis matters -- which is why the \
         daemon must pass the item's instant and never its own clock",
    );
}

/// Across the autumn fall-back the offset changes, so the same wall clock is a
/// different instant. Reading it in the declared zone is what keeps it right.
#[test]
fn a_time_only_reset_honours_the_zone_across_a_dst_change() {
    // 2026-10-25 is the European fall-back. An item emitted the evening after
    // it is CET, one hour ahead of UTC, not two.
    let observed =
        classify(CLAUDE_LIMIT, &[claude()], oslo("2026-10-26T18:00:00Z")).expect("a quota refusal");
    assert_eq!(
        observed.resets_at,
        Some(oslo("2026-10-26T21:40:00Z")),
        "22:40 Oslo in late October is CET, one hour ahead of UTC",
    );
}

#[test]
fn a_twenty_four_hour_time_only_reset_is_not_shifted() {
    let mut signal = claude();
    signal.reset_prefix = Some("reset at ".to_owned());
    let observed = classify(
        "individual spend limit /settings/limits session limit will reset — reset at 22:40 today",
        &[signal],
        oslo("2026-08-30T18:00:00Z"),
    )
    .expect("a quota refusal");
    assert_eq!(observed.resets_at, Some(oslo("2026-08-30T20:40:00Z")));
}

#[test]
fn the_claude_fingerprint_needs_every_component() {
    for partial in [
        "You've reached your individual spend limit.",
        "see https://console.anthropic.com/settings/limits for details",
        "your session limit will reset at 10:40pm",
        "I'll handle the individual spend limit case and reset at the next step",
    ] {
        assert!(
            classify(partial, &[claude()], oslo("2026-08-30T18:00:00Z")).is_none(),
            "a partial fingerprint must not retire a seat: {partial:?}",
        );
    }
}
