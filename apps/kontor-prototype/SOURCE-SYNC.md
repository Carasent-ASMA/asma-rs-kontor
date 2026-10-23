# Prototype source-sync audit

Pinned source: `5072b403a60d3302c8dbad59afd1e64b8d277023`.
Initial sync: `07fe9e50` pinned `61f6b075`; the current pin advances it to installed MAIN.
Reference: `b88a7b94` (`feat/ASMA-8541-start-switch-real-prototype`).
This app imports the pinned plugin directly. The initial import carried only the
prototype app and workspace lock additions; the later pin advance uses the exact
merged plugin source, without local plugin behavior changes.

## Delta

The initial sync recovered these 32 plugin commits missing from the reference, oldest first. Their current source
comes from the pinned base, not from the reference branch.

| Commit | Change |
| --- | --- |
| `6ccffea45c1e108976ccee7e810f5a44d76b1729` | ASMA-8541 add opt-in keyboard Back and shortcut testing (#383) |
| `0fbe92533734a2d2864176b5c571036dcaff783d` | ASMA-8551 add navigator-only plugin mode (#384) |
| `0015f5192b445b5b07bcbd0ffa96b441f3ee304a` | test(paseo-plugin): Use neutral aliases and a synthetic home in fixtures (#388) |
| `e03206defe6ef723f20cd55ff1a88bd6a1a1c464` | ASMA-8541 session navigator integration (#389) |
| `ea56b9ffe821a52a6dea81cf025d1133d49fa717` | feat(paseo-plugin): Add mobile filter page and quick filter toolbar (#393) |
| `9e4524be043056c0b215ab48d592593fe6baa30b` | feat(paseo-plugin): Add reviewed session recovery and archive lifecycle (#396) |
| `1bc64bc6cf48b983462d4dbc0ee548b38b6575a2` | ASMA-8541 Add native desks and plan workstreams (#405) |
| `23338c3203c2ca6aa8dc8963aa3718f6a4f0a14a` | feat(ASMA-8541): integrate responsive Legend with current navigator (#409) |
| `4d4fc0ced34189d5389c1276db9ea6465e7c50b9` | fix(ASMA-8541): guard browser listeners on native (#412) |
| `561fe42684c134d526fa9de1a6ae7d4a930df82b` | feat(ASMA-8541): unify session status icons and contextual controls (#416) |
| `f1223221de7c1735d0f30c1e5a9805c05c0f1592` | fix(ASMA-8577): preserve authoritative task-to-epic projection (#418) |
| `b6a81fff97055976b70f99ba90b26302451957ad` | feat(paseo-plugin): ASMA-8541 retain context across views (#421) |
| `55b81d9a2919af8f5e18941b613b54118889f523` | ASMA-8577 Explain epic identity for sessionless workspaces (#420) |
| `93043d929374bf1b3218ad87c9a718f9e40f1ddc` | ASMA-8507 Restore Paseo-direct workspace creation behind the Kontor guard (#423) |
| `b8aa3e7ec0c2c9e6490ba75909c02e1bf18a52d7` | feat(ASMA-8541): preserve plan context and resolve the workstream registry (#425) |
| `78d51513a91f3560faffdccaa9bcf267a1d30cdf` | ASMA-8541 Relate split-project rows and tell same-named projects apart (#426) |
| `6859776e80d910ed8592e144ea008420a202fcc5` | ASMA-8574 Remove ASMA identity from console, CI and scripts (#419) |
| `8263c655acb4cc4af7f8d014de093334b862698d` | fix(paseo-plugin): ASMA-8541 give compact Role session titles their whole line (#430) |
| `af0d685b92d68306bffc07d166dfbb6b83b7ec20` | fix(ASMA-8541): reconcile exact native session availability (#432) |
| `d32ae8082890f2d76e68a6ea8674712ca27dd6d9` | fix(ASMA-8541): distinguish compact Attention session titles (#434) |
| `70c427a14150507e3e12d8cb6cc3945653bd2d9c` | ASMA-8585 Sessions sort, My work and association selector (#436) |
| `e47df2564753c8392ab2a5d318235368912f0f4d` | fix(paseo-plugin): ASMA-8541 let compact tree-row titles wrap to where they differ (#439) |
| `bf97757d3c1dbdda65740f04593766c18f1f4eae` | ASMA-8585 feat(paseo-plugin): local My work, profile status and Kontor workflow status (#440) |
| `7c5a498a2232d142f54300c0788b7b47bb1f9294` | ASMA-8541 fix(paseo-plugin): restore Archive and same-session Resume on the combined Sessions view (#441) |
| `94d433ff6bae1921ce99b7c256ae7808e42f9ef5` | fix(paseo-plugin): ASMA-8541 stop the iOS Sessions crash after data load (#443) |
| `0b945eff40f9313b05ab7cde64244de477f02180` | fix(paseo-plugin): ASMA-8541 keep iOS loop callbacks on their own item (#444) |
| `15f0ab20a9031c62d24125d9a5092aa323cc85ce` | fix(paseo-plugin): ASMA-8541 Replay archive membership on PR444 (#445) |
| `0687c745fa52ee85ab01f507a9729b26505fd9a5` | fix(paseo-plugin): ASMA-8541 Replay tooltip R2 on archive membership (#446) |
| `827a6e56fe6bee0cd83e0f54939d0ca65046bda2` | feat(paseo-plugin): Replay independent layout on qualified base (ASMA-8588) (#442) |
| `bf048526d4dca502c596747d876e5f978cd989a9` | fix(paseo-plugin): Wrap compact Role session titles (ASMA-8588) (#447) |
| `139ce565484e030bde5b8c5bb35cf49f97f901f8` | fix(paseo-plugin): ASMA-8541 Archive X force-close with fenced, recoverable outcomes (#448) |
| `61f6b07518aadcf741ccdc60c127b0aff60e63f5` | perf(paseo-plugin): ASMA-8541 Archive X walks only active sessions (#449) |

The reference-only plugin commits `c1a78659`, `e67116f1` and `d552270d` are
excluded. They carried the old Back shortcut, Command Center return item and
prototype Test-shortcut delta. The pinned source already contains its reviewed
keyboard behavior via `6ccffea4`; the retained keyboard scenario exercises that
implementation. No reference `apps/paseo-plugin` file is copied or merged.

## Advance to installed MAIN

The following six merged plugin commits advance the pin. The plugin subtree is
byte-identical to `5072b403`; no local plugin behavior is added.

- `fd4def7b ASMA-8541 fix(paseo-plugin): label session tokens as the last report (#450)`
- `454c51e8 ASMA-8541 Keep session layout visible (#452)`
- `bafccb3a ASMA-8541 Add desk icons, scope chips and group state ratios (#451)`
- `14974094 ASMA-8541 Add Start/Switch and Recent sessions (#457)`
- `db4b7caa fix(paseo-plugin): Preserve explicit catalog epic mappings (ASMA-8541) (#456)`
- `5072b403 feat(paseo-plugin): ASMA-8541 Show the matching count in the mobile Filters header (#458)`

The qualified raw client is SHA-256
`c341a308eef4c9f2d3620d62472cb9d42f6f7d2cffe1674bac2d8d38137d1288`
(801,897 bytes). The React Native Web build has its own output hash because it
substitutes the fixture host. Raw artifact hashes and source-map parity are
recorded separately in operator-local evidence.

## Retained and dropped files

All 27 non-script reference prototype files are retained: HTML, package/Vite/TS
configuration, studio, scenarios, all seven original fixture modules and all
eleven host modules. Changes within those files adapt only host behavior,
API responses, isolation, documentation and ports. All 31 scenario IDs survive.

The two old scripts `availability-p1-browser.mjs` and
`lifecycle-accessibility-check.mjs` are dropped: their assertions require the
superseded Archive confirmation/Recover UI and attachment to an existing browser.
`browser-check.mjs` replaces them with current-control checks in a new isolated
browser. `test/fixtures.test.ts` checks fixture responses against the real plugin
contracts, running Archive behavior, retained history and uncertain-action fences.
`fixtures/responsibility.ts` supplies a fictional roster and issue assignments.

No root package or workspace declaration change is needed: `apps/*` already
includes the app. The lockfile adds the prototype importer and only missing web
dependency records; existing MAIN importers and snapshots remain unchanged.
Vitest reuses the version already pinned for the plugin. No store, node_modules,
build output, browser profile or run evidence is committed.

## Parity proof

Run from the repository root at the candidate HEAD:

```sh
git rev-parse HEAD
git diff --exit-code 5072b403a60d3302c8dbad59afd1e64b8d277023 HEAD -- apps/paseo-plugin
git diff --exit-code 5072b403a60d3302c8dbad59afd1e64b8d277023 -- apps/paseo-plugin
git ls-files --others --exclude-standard apps/paseo-plugin
```

Both diffs and the untracked-file list must be empty. The first diff proves the
committed source; the second also detects worktree edits. Record the exact
candidate HEAD and verification results in delivery evidence/PR, outside this
source document. The Vite host substitutions are listed in the app README.

## Remaining boundary

The reference scenario IDs stay unchanged. Start/Switch now comes from the
pinned production client; its options and creation RPCs use synthetic fixtures. Native runtime/server changes and production cutover are
outside this task. The synthetic lifecycle adapter is a UI fixture, not a second
production authority or an exhaustive replay of the server's failure semantics.
