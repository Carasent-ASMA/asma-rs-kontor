# KON-OP-11 replacement closeout record

Date: 2026-08-23
Jira: ASMA-7880
Status: not closed

The previous closeout branch and Jira closure were not supported by the
acceptance evidence. That branch recorded red Clippy/test gates as passed, did
not run the required audit gates, and accepted the missing native Jira cutover
as a limitation. It must not be merged.

The replacement branch fixes the observed defects and has passed the local
format, Clippy, Rust, console, audit and deny gates. Three security/correctness
mutants were killed and reverted:

1. removing the cutover readback-hash guard;
2. allowing a duplicate Jira create-marker match; and
3. allowing the legacy ASMA CLI machine Jira writer to proceed.

ASMA-7880 remains open because the required independent final review is not
present. This record is deliberately not an approval and must not be used to
close the ticket. Close only after the merged revision is deployed, live
readback is captured, and an eligible independent reviewer approves the same
revision.
