# ASMA-8196 F-1 closure confirmation

> **Date:** 2026-10-03 09:44 CEST
> **Status:** 🟢 Approved — F-1 closed
> **Author:** Codex AUD seat
> **Category:** report
> **Scope:** Narrow review of `a59d398a11bd915e73845f7de1e36dd39bfe975e`, parent `cc11087c75638a5ccefbca0ddfd9e61b4aea7159`
> **Summary:** Confirms that the only finding in the ASMA-8196 independent acceptance review was repaired by two rustfmt reflows with no semantic change. Acceptance extends to `a59d398a`; no new finding was identified.

---

## When to Load

**Load this document when:**

- checking whether F-1 from the ASMA-8196 independent acceptance review is closed;
- identifying the accepted source head after the formatting repair.

**Do NOT load for:** deployment, merge, integration, Jira, permission, admission, or live-runtime claims.

---

## Verdict

`accept`

F-1 is closed. The prior behavioral acceptance extends to accepted head `a59d398a11bd915e73845f7de1e36dd39bfe975e`, with no new findings.

## Delta confirmation

`git diff cc11087c..a59d398a` changes exactly one file, `crates/kontor-daemon/tests/loopback_api.rs`, with 7 additions and 2 deletions. The patch contains only:

1. rustfmt expanding `get_hosted_seat_role_persona(project, lsa, current_generation)` across lines;
2. rustfmt placing the two leading `assert_eq!` arguments on separate lines.

No identifier, argument, argument order, delimiter, assertion, literal, or behavior changed. This was checked three ways:

- the ordinary patch was inspected at both changed sites;
- removing all whitespace from each complete parent/child file produced the same SHA-256, `a6c846a2114c20ebb743f8d007a9be4874275d19e01aab1bc8c84cf59ea2f404`;
- formatting the parent file with `rustfmt --edition 2024 --emit stdout` produced SHA-256 `dadc26919b3568ea07e12d84b1b10379dd730c35fce47a530173039067fe1646`, exactly matching the child file.

## Exact checks

```text
git diff --stat cc11087c75638a5ccefbca0ddfd9e61b4aea7159..a59d398a11bd915e73845f7de1e36dd39bfe975e
git diff --name-status cc11087c75638a5ccefbca0ddfd9e61b4aea7159..a59d398a11bd915e73845f7de1e36dd39bfe975e
git diff --unified=20 cc11087c75638a5ccefbca0ddfd9e61b4aea7159..a59d398a11bd915e73845f7de1e36dd39bfe975e -- crates/kontor-daemon/tests/loopback_api.rs
```

Result: one modified file, +7/-2, exactly the two reflows above.

```text
git show cc11087c75638a5ccefbca0ddfd9e61b4aea7159:crates/kontor-daemon/tests/loopback_api.rs | tr -d '[:space:]' | shasum -a 256
git show a59d398a11bd915e73845f7de1e36dd39bfe975e:crates/kontor-daemon/tests/loopback_api.rs | tr -d '[:space:]' | shasum -a 256
```

Result: identical non-whitespace streams, SHA-256 `a6c846a2114c20ebb743f8d007a9be4874275d19e01aab1bc8c84cf59ea2f404`.

```text
git show cc11087c75638a5ccefbca0ddfd9e61b4aea7159:crates/kontor-daemon/tests/loopback_api.rs | rustfmt --edition 2024 --emit stdout | shasum -a 256
git show a59d398a11bd915e73845f7de1e36dd39bfe975e:crates/kontor-daemon/tests/loopback_api.rs | shasum -a 256
```

Result: both SHA-256 values are `dadc26919b3568ea07e12d84b1b10379dd730c35fce47a530173039067fe1646`.

```text
cargo fmt --all -- --check
```

Result: PASS — exit 0, no output.

```text
cargo test -p kontor-daemon --test loopback_api -- --test-threads=1 --exact a_claim_superseding_a_launched_occupancy_reports_a_null_roster_persona
```

Result: PASS — 1 passed, 0 failed, 0 ignored, 484 filtered out.

```text
cargo test -p kontor-daemon --test loopback_api -- --test-threads=1 --exact a_promotion_creates_one_epic_and_hands_the_work_to_its_lsa
```

Result: PASS — 1 passed, 0 failed, 0 ignored, 484 filtered out.

```text
git diff --check cc11087c75638a5ccefbca0ddfd9e61b4aea7159..a59d398a11bd915e73845f7de1e36dd39bfe975e
```

Result: PASS.

## Boundaries

- No source or existing evidence file was edited by this review.
- No push, PR, merge, Jira action, deployment, integration, admission, permission, or external gate action was performed.
