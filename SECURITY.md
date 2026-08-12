# Security Policy

Kontor coordinates agent runtimes, local credentials and project automation.
Please report security issues privately and give maintainers time to investigate
before public disclosure.

## Supported versions

Kontor is pre-1.0. Security fixes are made on the latest `master` and, when a
release exists, the latest published pre-1.0 release. Older commits and private
forks are not supported branches.

The current MVP is local-only. Remote exposure, multi-user operation and use as
an unattended production scheduler are unsupported even if a downstream fork
removes the loopback guard.

## Reporting a vulnerability

Use GitHub's private vulnerability reporting flow from the repository
**Security** tab when it is available. If it is unavailable, contact a
Carasent-ASMA repository maintainer through an established private Carasent
channel and ask for a secure reporting route **without including exploit or
secret details in the initial public message**.

Do not open a public issue for:

- authentication or authorization bypass;
- secret, token, credential-reference or transcript disclosure;
- non-loopback exposure or Host/Origin validation bypass;
- command replay, duplicate native effects or forged confirmation evidence;
- cross-realm, cross-project, cross-worktree or cross-account isolation failure;
- unsafe runtime admission, permission escalation or external-ticket mutation;
- dependency or build-pipeline compromise.

Include, when possible:

- affected commit/version and platform;
- minimal reproduction steps;
- expected and observed authority boundary;
- impact and whether secrets or external effects were involved;
- logs with all credentials, tokens, user data and private paths removed;
- a proposed fix or test, if you have one.

Maintainers will acknowledge the report through the private channel, validate
scope, coordinate a fix and agree on disclosure timing. No fixed response or
resolution SLA is promised during pre-1.0 development.

## Security boundaries

The supported MVP assumes:

- the daemon binds loopback only;
- the local operating-system account and Kontor state-root permissions are
  trusted;
- every non-health API request presents a realm credential;
- agent runtimes and `asma` tools are installed separately and reached only
  through supported interfaces;
- secrets stay in their native credential stores and Kontor persists only
  opaque references;
- project repositories and agent-generated content are untrusted input.

Vulnerabilities in Paseo, Agent Orchestrator, Codex, another provider/runtime or
the `asma` CLI should also be reported to that project's maintainers. Report to
Kontor as well when its adapter or policy fails to contain the issue.

## Safe research

Use disposable realms, repositories and provider accounts. Do not test against
production data, other users' sessions or infrastructure without explicit
authorization. Never include live tokens, credentials, patient/customer data or
private transcripts in a report or fixture.
