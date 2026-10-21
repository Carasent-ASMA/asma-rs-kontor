/** Shared audit metadata only. Native sessions are simulated; full original documents are not embedded. */
import type { SourceRead } from "../../../paseo-plugin/shared/read-sources";
const rows: [string, string, string | null, string | null, string][] = [
  ["Hybrid GitHub Actions Runner and Workflow V2 Implementation Plan", "_docs/GITHUB_ACTIONS_HYBRID_RUNNER_IMPLEMENTATION_PLAN.md", "dev-platform", null, "cab38c95142619b3d2658c457f66547a53f9d87fc12d9f66470bea9997ded576"],
  ["Introduction", "_docs/GITHUB_SUBMODULE_ORCHESTRATION_IMPLEMENTATION_PLAN.md", null, null, "07d1913ab95bacf2fa247ced444c866f412c0369bed0a662b197d6cd1b29b538"],
  ["adopus — Hasura v2 Consolidation Plan (One Engine, All Tenants)", "_docs/adopus-graphql/plans/2026-07-11-00-14-plan-adopus-hasura2-consolidation.md", "adopus-graphql", null, "6713f575badd70cbfd43924b019ad1826890de83b93e04bf6eed920baa5cb75f"],
  ["adopus — HSR wrapper client fleet rollout (`SRV_AO_WRAPPER` → `HSR_AO_WRAPPER`)", "_docs/adopus-graphql/plans/2026-07-12-plan-adopus-hsr-wrapper-client-fleet-rollout.md", "adopus-graphql", "ASMA-7575", "1c43023a5172da1721d911784ad23fa1ab3ca4cba7e8621fd42aba62b2830d00"],
  ["ASMA-7827 — Stamp ChangeUser and Changedt on every AdOpus write", "_docs/adopus-graphql/plans/2026-10-16-20-27-plan-asma-7827-adopus-audit-column-stamping.md", "adopus-graphql", null, "d015ad5087c103b4feb85d76eac61ecf5973c1aff3777a490da8f0c5387989a7"],
  ["Deterministic Seat Context Window and Compaction MVP Plan", "_docs/ai-orchestration/plans/2026-08-13-09-43-plan-seat-context-window-compaction-mvp.md", "kontor", null, "313d5de821bb72bc8912d5d591c3b2c2198ba75ed109a6377e56e95c29e183fb"],
  ["Kontor Operational MVP Implementation Plan", "_docs/ai-orchestration/plans/2026-08-14-23-21-plan-kontor-operational-mvp.md", "kontor", null, "7d66e5eba5cb09b9be17548f18746ee4ea32c95c468e3acc01c18a672ad67e24"],
  ["Kontor Developer Activity Statistics", "_docs/ai-orchestration/plans/2026-08-15-00-18-plan-kontor-developer-activity-statistics.md", "kontor", null, "a4dc98664082bf48714bcee34e3f15c33dfe4bb0d5eda8c8c3c695f141f868fc"],
  ["Kontor Operator Surface and Attention Channel Implementation Plan", "_docs/ai-orchestration/plans/2026-08-18-17-17-plan-kontor-operator-surface-and-attention.md", "kontor", null, "98dacb63a79ab0ef48067c843c771feec7539bf58a73833b1c3cac99bb1baccb"],
  ["Quota-Blocked Seat Succession Implementation Plan", "_docs/ai-orchestration/plans/2026-08-23-22-31-plan-kontor-quota-blocked-seat-succession.md", "kontor", null, "38e62a9be357d2d46e82244fbbb63591841e36ab68b56419c2280aaedcd51222"],
  ["KON-OP-22 — Remove Kontor control-plane dead ends and manual fallbacks", "_docs/ai-orchestration/plans/2026-08-24-00-43-plan-kontor-op22-control-autonomy.md", "kontor", null, "9ed3fefb5ddf0a3df98f61cf02a827d81321a26a6e9ac64801eb949898c26495"],
  ["Carasent AI Workshop — Presentation Preparation", "_docs/ai-orchestration/plans/2026-08-24-19-00-plan-carasent-ai-workshop-preparation.md", "kontor", null, "224289ce0bb5117e34ef572810c6ff02352b31be55e214ab00989182fe534241"],
  ["OP-20 reconciliation & implementation plan — permission posture at spawn (LSA-authorized, 2026-08-30)", "_docs/ai-orchestration/plans/2026-08-30-15-05-plan-kontor-op20-permission-posture-reconciliation.md", "kontor", null, "560dc89c72b5a224ea5d717240499000227c121b1ba1545e08efb3e58fc5d829"],
  ["Kontor Backlog Identity Implementation Plan", "_docs/ai-orchestration/plans/2026-08-30-19-51-plan-kontor-backlog-identities.md", "kontor", null, "8cacac46429f0e2601ce41f676b352fcbfec1e7cdf0c2c716972220561de60ca"],
  ["Kontor Experience Memory Implementation Plan", "_docs/ai-orchestration/plans/2026-09-12-10-10-plan-kontor-experience-memory.md", "kontor", "ASMA-8155", "2ca303cee37625fa56d5942f025c392c5c506b316da99c0c405fd2d47dcbd99f"],
  ["Restore Autonomous Epic Delivery Implementation Plan", "_docs/ai-orchestration/plans/2026-09-16-22-09-plan-kontor-autonomous-epic-delivery.md", "kontor", null, "6188137aff9d0e9abed033965bf7cdba9c0780d6f2a5bdb66f95c6d03ed935f1"],
  ["ASMA-8188 completed plan", "_docs/ai-orchestration/plans/2026-09-20-21-01-plan-asma-8188-gate-rejection-role-slot-fence-recovery.md", "kontor", null, "4963f9a7c27bc16468163cb8a9f48f9b03327aca7f50e982cf51bbed4bb225c2"],
  ["Kontor isolated live acceptance fixture", "_docs/ai-orchestration/plans/2026-09-21-12-05-plan-kontor-isolated-live-acceptance.md", null, null, "5a217b6942b0faaa9c5a4d08ffad6ed0ad0291bafb0b1828dded6ed4caf03251"],
  ["Kontor Smart Takeover and Automatic Resume Implementation Plan", "_docs/ai-orchestration/plans/2026-09-23-18-36-plan-kontor-smart-takeover-and-resume.md", "kontor", null, "8588695514d8754d2b9441e20922e3699adf7c799977003d17601956dad69234"],
  ["Kontor Live Fleet Configuration Implementation Plan", "_docs/ai-orchestration/plans/2026-09-24-07-30-plan-kontor-live-fleet-configuration.md", "kontor", null, "69ff4f55498e8f73b8b96744dd3079239072c909e7b2604e2b17414e5032b164"],
  ["ASMA-8186 — Governed Core Team seat succession (DRAFT)", "_docs/ai-orchestration/plans/2026-09-27-08-11-plan-asma-8186-governed-core-team-seat-succession-draft.md", "kontor", "ASMA-8186", "4a419743df91555f4a1d7356279c2c7685d7fee8357602d087e028f38609ee01"],
  ["ASMA-8186 — Core Team seat succession", "_docs/ai-orchestration/plans/2026-09-27-15-51-plan-asma-8186-core-team-seat-succession.md", "kontor", "ASMA-8186", "3d970bb0c1ba3d837842befd7069fb817e04746a768f1af1cbcd0608fffaf2dd"],
  ["Shared Orchestration Workflow Implementation Plan", "_docs/ai-orchestration/plans/2026-09-27-20-14-plan-shared-orchestration-workflow.md", "roles-chain-models", null, "e3b784e870c1e21f9e4366a53f21220dc5d961ce90be266242ea523f5072aeea"],
  ["ASMA-8353 — Tokenomics collection, Directory ledger and Kontor dashboard proposal", "_docs/ai-orchestration/plans/2026-10-04-21-49-plan-tokenomics-collection-ledger-dashboard.md", "tokenomics", "ASMA-8353", "652633c5d4ef40c2171119a6853abad294eb9cb195219a153b63166a38c51f80"],
  ["ASMA-8371 — PR review desk and lean seat rules", "_docs/ai-orchestration/plans/2026-10-06-22-41-plan-pr-review-desk-and-lean-seat-rules.md", "pr-desk-rules", "ASMA-8371", "39bd9fd6ecb5ba43dcf25e9f2508e1379aee6142e638c54c49bd9d966b151118"],
  ["ASMA-8399 — PR lifecycle autopilot — merge on silence and takeover of abandoned drafts", "_docs/ai-orchestration/plans/2026-10-07-08-27-plan-pr-lifecycle-autopilot.md", "pr-lifecycle", "ASMA-8399", "05115e678fdc40e6995952546def215f19759cc9ccf29fa43fdd6fd8bd4b72e1"],
  ["kontord durable desks — ASMA-8449", "_docs/ai-orchestration/plans/2026-10-08-22-47-plan-kontord-durable-desks.md", "kontor", null, "9b04c5801ff432a387845e6aa98a85d02438b4945429a21f70505e7d6fe659e9"],
  ["Kontor relaunch and self-improvement — unified implementation proposal", "_docs/ai-orchestration/plans/2026-10-15-03-55-plan-kontor-relaunch-and-self-improvement.md", "kontor", null, "e781a1f768b1f68e5fe3bcc1c619f4bfac0a7e652da36ec0054bd29091f678a6"],
  ["Kontor PR desk — owner mode: review, fix, merge and close the cycle", "_docs/ai-orchestration/plans/2026-10-17-10-46-plan-kontor-pr-desk-owner-mode.md", "pr-lifecycle", "ASMA-8490", "217ddd939b21dd5f6ef0d03ca7ac6dd6dc99f872e8662302e69bb742f25dedc8"],
  ["Kontor workflow pack — publish the method so any project can adopt it", "_docs/ai-orchestration/plans/2026-10-17-16-15-plan-kontor-workflow-pack.md", "kontor", null, "458e5a44a81548f76bbb5f96bf1b707e970110f7c6261e7070e24ad95eac120b"],
  ["Kontor Paseo plugin — workflow automation and UI planes", "_docs/ai-orchestration/plans/2026-10-18-02-17-plan-kontor-paseo-plugin-automation-and-ui.md", "kontor", null, "e1cf7cb8aab17dae9c446374a552af30764acaf72db81a6104b947353fcf764d"],
  ["Submodule change tree & the Paseo submodule-awareness contribution — build plan", "_docs/asma-cli/plans/2026-08-04-18-29-plan-submodule-change-tree-and-paseo-contribution.md", "dev-platform", null, "29171212e52effcfb2dc599ff01b9bf6db2805d1f7d3ce78f203a81da1395db6"],
  ["Catalog workspace, drop pointer-sync, status tree", "_docs/asma-cli/plans/2026-08-22-12-13-plan-catalog-workspace-and-status-tree.md", "dev-platform", null, "c12e054a17f8f87034224ddfd1e05c4db4aac9bd44eba458086e02f5a0d1b1db"],
  ["CAT-07 — Drop pointer-sync workflows (architect contract)", "_docs/asma-cli/plans/2026-08-24-14-09-plan-cat-07-drop-pointer-sync-workflows.md", "dev-platform", null, "25866f19769f9c56a14525f523226241e35153a4fd3fa2e7463ce749e7d5f74f"],
  ["CAT-06 — verify-pointers / fleet / hooks (architect contract)", "_docs/asma-cli/plans/2026-08-24-14-30-plan-cat-06-verify-pointers-fleet-hooks.md", "dev-platform", null, "28abf2b76a75f8d95c60404575f57522d88d8a893119de2d2d6b4e1b9b00d3f5"],
  ["_docs Knowledge-Base Refactor — Single Source of Truth + Autodiscovery", "_docs/asma-platform/plans/2026-07-09-10-00-plan-docs-knowledge-base-refactor.md", "dev-platform", null, "bdfa014e1747b7407ecccae7a01ce89167a3c929c919b2f8d8cb53cdd226e902"],
  ["Proposal — Repository Guardrails, Secret Hygiene and Agent Experience", "_docs/asma-platform/plans/2026-08-07-00-45-plan-repo-guardrails-security-and-agent-dx.md", "dev-platform", null, "c3edd237843b08bc68ad1fd1fc4f7b7db04f99951d6c58d2625f458e491f0076"],
  ["Release Pipeline Integrity Remediation", "_docs/asma-platform/plans/2026-08-14-22-30-plan-release-pipeline-integrity-remediation.md", "dev-platform", "ASMA-7856", "849ec3406389494f62e53d4bd5d24c9995687000a1946eb5f2f95eff7a8335a5"],
  ["Release Pipeline Integrity — Production Rollout", "_docs/asma-platform/plans/2026-08-18-00-15-plan-release-pipeline-production-rollout.md", "dev-platform", "ASMA-7856", "4d5c5b8695601beca5f6b35958a76859ab4cc4c81a48b4f78ffc35edda39a12a"],
  ["Introduction", "_docs/cdc/plans/2026-05-10-09-26-plan-refactor-cdc-integration.md", "cdc", null, "3283db22847fee6eb04fa53b560340136de75de5af780be90b6bf6a6cc370795"],
  ["FDW Scaffold And db-per-tenant Source Group Implementation Plan", "_docs/cdc/plans/2026-05-10-10-35-plan-fdw-scaffold-db-per-tenant-source-group.md", "cdc", null, "f74d7a80ffeee156076556e4f14dac30236c58df5a38ba1e46b8fdf373c03c01"],
  ["Runtime Customer Monitoring Filter — Upgrade Plan", "_docs/cdc/plans/2026-05-25-18-17-plan-runtime-customer-monitor-filter-upgrade.md", "cdc", null, "aca998040d6ca387cb595c144e83a6269cafdb34bdcd5a2104410b1521ae7b2b"],
  ["Option 3 Plan — Full Source Registration Redesign", "_docs/cdc/plans/2026-05-25-23-10-plan-source-registration-full-redesign.md", "cdc", null, "34d87567d670db62c5e827d0057fa137d4c7b0dfb3adb347f7ab0af85c639b67"],
  ["Native CDC Lightweight Prechecks - Implementation Proposal", "_docs/cdc/plans/2026-05-26-22-29-plan-native-cdc-lightweight-prechecks.md", "cdc", null, "7779134360431a6745d4770dc7f860a5762094eaead4ef41b3949c5f61fb76ae"],
  ["Native CDC Adaptive Prechecks - Step-by-Step Implementation Plan", "_docs/cdc/plans/2026-05-26-22-50-plan-native-cdc-precheck-implementation.md", "cdc", null, "3da77987a4126577ae2e0939ed20d33108f1dbc9a02a591683d784bc09e8d9c0"],
  ["cdc CLI — `cdc rbac` Subcommand (Declarative RBAC Compiler)", "_docs/cdc/plans/2026-06-17-19-01-plan-cdc-cli-rbac-subcommand.md", "cdc", null, "0e8e94e4ef26ae2e5bf3e68c5cc6d58dd6a5a3c9cbef730d64d983c713ae27a3"],
  ["Introduction", "_docs/cdc/plans/2026-06-21-09-15-plan-journal-patient-responsible-canonical-mirror.md", "cdc", null, "b36daf0e672d8096baa664354231f9408c2d942736f80d4293a36f3282a0156d"],
  ["Introduction", "_docs/cdc/plans/2026-06-21-20-30-plan-cdc-cli-enrichment-scaffold.md", "cdc", null, "c353a5686f1d91ef58f62a22bcd9e32c115820f13351baf6e4fb72132a27e0b2"],
  ["AdOpus Søknad mirror for pathway reference events", "_docs/cdc/plans/2026-10-19-17-20-plan-adopus-soknad-mirror-for-pathway-anchors.md", "cdc", null, "b55046c0e843c2526d715c1f7740857db108ccfa7acfeb5d3e678c6833d08cc2"],
  ["Revman Release Packages, Version Groups and scheduled application", "_docs/directory/plans/2026-07-22-16-38-plan-revman-release-packages-version-groups.md", "revman", "ASMA-5585", "23dc0ad69cefe18f8bfcb8b19057792adb79107ed21e7d588b6abf053fdf7a6a"],
  ["Revoke participant access when primary/secondary responsible is removed", "_docs/directory/plans/2026-08-26-11-30-plan-revoke-participant-access-on-responsible-removal.md", "participants", null, "8725502959128a0f0049b363734b2d9a8e4f1b8fe3f51bde5826e7c215f93bdb"],
  ["ASMA-7485 — international phone numbers: Stage 2 SMS authorization and branch completion", "_docs/directory/plans/2026-09-05-17-25-plan-asma-7485-phone-e164-stage-2-and-branch-completion.md", "pathfinders", "ASMA-7485", "f603558206d6bac983a07deda40b2da5c6add62e2ac62f37114d90fcb398e2ea"],
  ["ASMA-7628 — Revman scheduled executor: `asma-bunjs-directory` service plan", "_docs/directory/plans/2026-09-14-11-18-plan-revman-scheduled-executor-service.md", "revman", "ASMA-5585", "dae9a0dbd80cf3a2b60542d19143dddad294286ad2541a75b7a1070efc187afd"],
  ["ASMA-5873 Pathfinder Search with Group 109", "_docs/directory/plans/2026-09-23-13-55-plan-asma-5873-pathfinder-search-group-109.md", "pathfinders", null, "83b0d4a4ad56e22f9e0aaf0044947007f3cbe13352322b786fb5ece16a10dac8"],
  ["2026-05-01-22-53-plan-editor-future-plans.md", "_docs/editor/_shared/plans/2026-05-01-22-53-plan-editor-future-plans.md", null, null, "16739aea80a74a3c931adc5f609b2c9b77a2ce36d0a00858ae455e7154c10a69"],
  ["Introduction", "_docs/editor/_shared/plans/2026-06-21-17-30-plan-end-to-end-arktype-typesafety.md", null, null, "df0ce5ecf9550941d82306f093bbf78475da1d2961cac69afbc61e60561d27fb"],
  ["Introduction", "_docs/editor/_shared/plans/2026-09-10-12-45-plan-onlyoffice-inflight-edit-durability.md", null, "ASMA-7097", "0e9a67ad65d2058dc84400c12fdb7e1f1810722166ee27bcb8158f39fcc431c5"],
  ["Introduction", "_docs/editor/_shared/plans/2026-09-11-19-25-plan-qr-codes-in-document-generation.md", null, "ASMA-7470", "a6a80ef9ad9b598b828d3f349e1fbdce578b981a73add64b7058221f725ae859"],
  ["ASMA-8416 Journal Patient Fields in Document Templates Implementation Plan", "_docs/editor/_shared/plans/2026-10-08-17-09-plan-journal-patient-fields-document-templates.md", null, null, "000f5afab4699e69da85c73dceb30327edff505cb985c1e83d636d93a7ad2321"],
  ["ASMA-8444 Journal Data Bindings in Document Templates Implementation Plan", "_docs/editor/_shared/plans/2026-10-10-18-20-plan-document-template-journal-bindings.md", "files-sharing", null, "d8c5c5c5c68dd4bf79871574609e671d6b287135b44d14855e37660206b2924c"],
  ["ASMA-7470 Phase 2 — QR Guardian Registration in AdOpus and Ad Curis Implementation Plan", "_docs/editor/_shared/plans/2026-10-14-01-00-plan-qr-guardian-registration-both-journals.md", "ad-voca", "ASMA-7470", "d6a40ad00375726105bb5d9728a67ca477a2afeb362b39c881af4956e06f8f87"],
  ["OnlyOffice ASMA Tag Inserter — from one panel to three plugins", "_docs/editor/_shared/plans/2026-10-15-13-05-plan-onlyoffice-tag-inserter-three-plugin-split.md", null, null, "f552c7d1507fb41bd230c31a458dca6e328536bdd63c4120b8a5853e3c464054"],
  ["OnlyOffice proxy trigger routes — authenticate the caller", "_docs/editor/_shared/plans/2026-10-15-18-30-plan-onlyoffice-proxy-trigger-authentication.md", null, null, "4694b1e37c43769e06942d2048696c366bddaf67c8f6f13c15869f91c63e4636"],
  ["PasientMeny port — implementation plan (controlled dynamic execution)", "_docs/editor/adopus-connector-pasientmeny-implementation-plan.md", null, null, "ee944faac9aae07eebbefb8352c85b65ee5b69f2040a012fb36f065f4965cb59"],
  ["Introduction", "_docs/editor/pathways/cross/2026-08-20-10-26-plan-pathway-onlyoffice-document-materialization.md", null, null, "61fb8bd8ae65c017a84f24874a71e77eb8a2c039195eaeffc764de1262299fc0"],
  ["Introduction", "_docs/editor/pathways/cross/2026-09-07-13-45-plan-pathways-production-readiness-remediation.md", null, null, "27c21c419df97e4e5339052983f50710edd7ebc45acd79ccace6473b6c0b886e"],
  ["Pathways catalog writes — who may author templates", "_docs/editor/pathways/cross/2026-10-15-19-00-plan-pathway-catalog-write-roles.md", "ad-curis", null, "6e0e3c153d3ecee78c2f566ac02c42478bef23be9dcc06dc9eeb34ca47b8a2ef"],
  ["AdOpus pathway reference-event automation", "_docs/editor/pathways/cross/2026-10-19-13-10-plan-adopus-pathway-reference-event-automation.md", "adopus-graphql", null, "fba5d52fd712da471db26672423066c0eebbdf6d35b19e84d163f539bd269797"],
  ["2026-05-01-01-41-plan-qnreditor-adopus-prefill.md", "_docs/editor/qnrs/backend/plans/2026-05-01-01-41-plan-qnreditor-adopus-prefill.md", null, null, "ce127aec454db1a458928fe718f0b0a0f22a947e68ab6226356552f4e08b0e6b"],
  ["2026-05-01-14-01-plan-qnreditor-adopus-prefill-manifest-generation.md", "_docs/editor/qnrs/backend/plans/2026-05-01-14-01-plan-qnreditor-adopus-prefill-manifest-generation.md", null, null, "ff29c6a6230f8259836a2aeb48f9e21a75a895cd8f7fda1592ff721b982737b5"],
  ["2026-05-01-14-01-plan-qnreditor-adopus-prefill-mssql-access.md", "_docs/editor/qnrs/backend/plans/2026-05-01-14-01-plan-qnreditor-adopus-prefill-mssql-access.md", null, null, "78e23caf9b274b687b4d5b9065c14910d3023500740a91e92e1da2ae68537d40"],
  ["2026-05-01-14-01-plan-qnreditor-adopus-prefill-parity-harness.md", "_docs/editor/qnrs/backend/plans/2026-05-01-14-01-plan-qnreditor-adopus-prefill-parity-harness.md", null, null, "d9c6b9310dadb27a08280fd4b322737ec29645670c716eca3ea21491a39ea657"],
  ["2026-05-01-14-01-plan-qnreditor-adopus-prefill-route-contract.md", "_docs/editor/qnrs/backend/plans/2026-05-01-14-01-plan-qnreditor-adopus-prefill-route-contract.md", null, null, "0e3917dd52a2372238fb6c7178530c72b40709d8c5303c41298fbfcdd712bf0c"],
  ["2026-05-01-14-01-plan-qnreditor-adopus-prefill-tenant-database-resolution.md", "_docs/editor/qnrs/backend/plans/2026-05-01-14-01-plan-qnreditor-adopus-prefill-tenant-database-resolution.md", null, null, "1ef9f349ebf260dfe7b4b09fbc9bbfadd80093c38da4d9cd3f11b7ed6de464f6"],
  ["Fixture replay (CI-safe, no DB needed)", "_docs/editor/qnrs/backend/plans/2026-05-01-22-53-plan-qnreditor-adopus-prefill-summary.md", null, null, "84432f5dd1d4df3f3cac3c0e5e2d5b9a1f13a31c4faa2d9b7db21eb8384b85b4"],
  ["Introduction", "_docs/editor/qnrs/backend/plans/2026-05-02-01-04-plan-editor-fetch-schema-abstract-port.md", null, null, "0acb7e3ef38af7ab9abb2fbbc7b0f3e1c7b5102c0caacf94fa95a3972ca1dbd0"],
  ["Introduction", "_docs/editor/qnrs/backend/plans/2026-05-02-01-40-plan-editor-adopus-schema-source-of-truth.md", null, null, "423c57560ba40df2181820f04e3019626e9fe3ad44db13b57c2c070d8e1ba5b8"],
  ["Introduction", "_docs/editor/qnrs/backend/plans/2026-05-02-01-40-plan-editor-fetch-alternatives-adopus-rest-port.md", null, null, "d2ad50f460dfa059d9d26d4349c9163c0879237fb6e038de2bb5d871c442bb6d"],
  ["2026-05-06-15-35-plan-qnreditor-adopus-response.md", "_docs/editor/qnrs/backend/plans/2026-05-06-15-35-plan-qnreditor-adopus-response.md", null, null, "8f7c094ab0eab4b6b728e8cbe6e87d23acad2d312f7c9933477e631819368979"],
  ["Introduction", "_docs/editor/qnrs/backend/plans/2026-05-10-09-26-plan-editor-srv-connector-remaining-rest-port.md", null, null, "e109706942fb5699f8fb6d857e9dc8d6fc767b1e2ed78380639a7333ad43849e"],
  ["Introduction", "_docs/editor/qnrs/backend/plans/2026-06-12-00-00-plan-qnreditor-differential-parity-tests-removal.md", null, null, "52f58943497b27a6ff3f0e15998670803df3ac863858f085f5cc0738bf00084b"],
  ["Introduction", "_docs/editor/qnrs/backend/plans/2026-06-22-12-00-plan-adopus-connector-port-gap-remediation.md", null, null, "aa1fe77346286da54c47a8b232307a6f5cc02e9ca3183137e1927dc38e3f5842"],
  ["Introduction", "_docs/editor/qnrs/cross/2026-06-24-15-30-plan-qnr-materialization-backend-implementation.md", null, null, "e3faca05270f0af7ce09de4e361246c5cc40db3951c38501beecc9021d8d2319"],
  ["Introduction", "_docs/editor/qnrs/cross/2026-07-12-19-38-plan-qnr-legacy-db-and-secrets-cleanup-investigation.md", null, null, "f79766c0e115d5d639f305a018e0bc66809901b5876599bcb24de1f80297468b"],
  ["QNR Stage-2 implementation roadmap", "_docs/editor/qnrs/cross/2026-07-12-21-40-plan-qnr-stage2-new-model-editor-and-sync.md", null, null, "c46c3368d1ea9618a0b0ca4311299a5217edde3078afc45caefd46178ce41c2f"],
  ["Introduction", "_docs/editor/qnrs/cross/2026-07-29-20-43-plan-soknad-liveness-inventory-and-port-exclusion.md", null, null, "93208b56117c1949ee3ce6fd0d824528a93ae6701c0ebd66e66da9a1b0c554c3"],
  ["Introduction", "_docs/editor/qnrs/cross/2026-07-29-23-30-plan-legacy-cleanup-surface-census.md", null, null, "78c0cff21cf6146ce35e5cab07de3f38352d0080bb0bdb3770075ac420e41e54"],
  ["Introduction", "_docs/editor/qnrs/cross/2026-08-02-21-58-plan-qnr-v2-coexistence-guardrails.md", null, null, "ad71cef963ab19fa5c1e1e03244ed0917795103d26438cf6a3eed451ddc553c6"],
  ["QNR Stage 1 — post-cleanup", "_docs/editor/qnrs/cross/2026-08-06-11-00-plan-qnr-stage1-post-cleanup.md", null, null, "78bbb3fc5b6106578985cb08ad5040490b16e6b32e90f618511a0a90217b3aae"],
  ["QNR v2 Nonprod Delivery (ASMA-7675)", "_docs/editor/qnrs/cross/2026-08-18-22-34-plan-qnr-v2-nonprod-delivery.md", null, "ASMA-7675", "cf6b46062a877dfd482c23b586b02b95be960ffe07642d2bee3324a3bc3c90c0"],
  ["QNR v2 Production Migration (ASMA-7929)", "_docs/editor/qnrs/cross/2026-08-18-22-34-plan-qnr-v2-production-migration.md", "qnr-v2", "ASMA-7929", "a1c683ffe34ad24dd523beab07f508096a997534a2d44f364fa3ad95e0407ae6"],
  ["QNR v2 — importer-first engineering closure: architecture/implementation handoff", "_docs/editor/qnrs/cross/2026-08-23-00-00-plan-qnr-v2-importer-first-closure-handoff.md", null, null, "543c057efae197f5680ea994bcedd21fe71ec3bef5a6e7c40e406fdf74edf5f3"],
  ["Introduction", "_docs/editor/qnrs/frontend/2026-07-05-19-30-plan-qnreditor-ui-library-consolidation-asma-ui-core.md", null, null, "f05dc78875b80d0a2db2d841e592b6ea616d90fe4bffd62de386f77119913d76"],
  ["Proposal — Replace `import-map-overrides` with the in-house ASMA Overrides UI", "_docs/frontend/plans/2026-06-03-12-18-plan-asma-overrides-ui-migration.md", "frontend-platform", null, "3d93e87a9908de754bebe34fb27df76e76eb418d6de9a01b72f6e8a3a8651cf8"],
  ["Proposal — Native-ESM Widget Loading (import-map dedup + versioning + per-widget chunks)", "_docs/frontend/plans/2026-07-01-14-00-plan-native-esm-widget-migration.md", "frontend-platform", null, "52fb7e5b3ba8d8df01e1d6914a7bb96bad8edfa7d7904e4a2c80064c53b835b9"],
  ["Introduction", "_docs/frontend/plans/2026-07-02-15-40-plan-shell-dual-loader-esm-and-qiankun.md", "frontend-platform", null, "0cfc0ee5e4f7748f1a14281d779de8a0ddf5599a5870dab1a499b132ebedc1b2"],
  ["Introduction", "_docs/frontend/plans/2026-07-04-03-14-plan-static-server-local-version-resolution.md", "frontend-platform", null, "4ed0067eacdb2f8982b2b75de661d2c80c03e0ee1d61d381d7439bda0f7df0b4"],
  ["Introduction", "_docs/frontend/plans/2026-07-04-13-30-plan-kernel-spec-single-source-of-truth.md", "frontend-platform", null, "32f8b1b6fad61a3297344922bddd36cb4eb18232ea8d8fcc3c5d71bbedbacec5"],
  ["Introduction", "_docs/frontend/plans/2026-07-04-16-30-plan-kernel-lib-fleet-expansion.md", "frontend-platform", null, "c5028bf23120e77d52bc01e56ac0f962e2016b11ea892a03886378980286fc41"],
  ["First-hit authentication, metadata injection and auth/static-server consolidation", "_docs/frontend/plans/2026-07-04-17-30-plan-merge-njs-auth-static-server-first-hit-auth.md", "frontend-platform", null, "3a26f22b91000ecb621d4ba11de900ce556fe1ae2568880e886b6fc7620034ce"],
  ["Migrate the fleet from `asma-helpers-react` → `asma-core-react`", "_docs/frontend/plans/2026-07-05-02-25-plan-migrate-helpers-react-to-core-react.md", "frontend-platform", null, "257b002a1b3e07c7d7692b07fc9f18db733f756a26b4aba65e5fbcb51b84f7c2"],
  ["Fleet upgrade — React 19 + TS 6 + latest majors", "_docs/frontend/plans/2026-07-05-14-00-plan-react19-ts6-fleet-upgrade.md", "frontend-platform", null, "fefbd3d3848a32945e92b6cdd201b4f58135a79a6d13772a580dd6af596eb154"],
  ["2026-07-05-15-00-plan-devextreme-upgrade-to-26-1.md", "_docs/frontend/plans/2026-07-05-15-00-plan-devextreme-upgrade-to-26-1.md", "frontend-platform", null, "3d8ab1729499819be13a054e24513184989891c8abe8c744c577faebeab19560"],
  ["Fleet UI-library consolidation onto asma-ui-core", "_docs/frontend/plans/2026-07-06-16-00-plan-fleet-ui-library-consolidation-asma-ui-core.md", "ui-core", null, "f3d327fb79a9efa6832fe61a62379bb91c5f1b9333dd845f1c87b777398564b2"],
  ["Introduction", "_docs/frontend/plans/2026-07-07-00-00-plan-advoca-minimal-login.md", "ad-voca", null, "f25dd87c214ae51d3be6164b82a3c69546b05c18d301624f8d8d451e1ff36ba2"],
  ["GenQL → gql.tada: Types & Enums Migration Guide", "_docs/frontend/plans/2026-07-09-21-50-plan-genql-to-tada-types-enums-migration.md", "frontend-platform", null, "676fe0a03867f3fc24c8224a32250602ea293c8525694fa90d488d3ab3923fd1"],
  ["Introduction", "_docs/frontend/plans/2026-07-10-19-12-plan-asma-ui-core-mui-removal.md", "ui-core", null, "60386600188de26ae9dc6d859c350badaf9634bbbd03d38eaa531a34d42447b1"],
  ["Introduction", "_docs/frontend/plans/2026-07-11-00-45-plan-visual-regression-playwright-pilot.md", "ui-core", null, "06d86149f06a00b019da06017366865fbae847c8f7cc6bbb88f1ca4cb99abdbc"],
  ["Introduction", "_docs/frontend/plans/2026-07-13-13-45-plan-asma-ui-core-mui-removal-visual-parity.md", "ui-core", null, "cc6679786614a722d0235a06da90b47a5ee23f717d27558680e4a2be4fe6ed9c"],
  ["Figma ↔ asma-ui-core Design Parity", "_docs/frontend/plans/2026-07-15-17-15-plan-figma-asma-ui-core-design-parity.md", "ui-core", null, "e46e225a246166ec102253155c50c6cb665f30a69c5da81d8bdbc6b5bbf3dc75"],
  ["Introduction", "_docs/frontend/plans/2026-07-23-16-00-plan-asma-ui-core-accessibility.md", "ui-core", null, "e24129469048e694ed722ef062038111b244c58bfb073986982e4d4bc86bb95a"],
  ["App migration to the MUI-free asma-ui-core — executable per-app runbook", "_docs/frontend/plans/2026-07-24-12-01-plan-app-migration-to-mui-free-asma-ui-core.md", "ui-core", null, "1098eca40d749156c781a399e91b3ba321078600d9ec7b66b9ae2ad1d50400cd"],
  ["Introduction", "_docs/frontend/plans/2026-08-06-20-45-plan-form-validation-unification-rhf.md", "ui-core", null, "62038ee0139367a7f6faca696e1a6dc3bee7b533f461845eeb6102b9a34f76a6"],
  ["Introduction", "_docs/frontend/plans/2026-09-02-16-05-plan-international-phone-number-fields.md", "pathfinders", null, "9dcdddf5373be038a81fc4d91bedf5caeffec94cb97c07726390f18b74d9c7ed"],
  ["Ad Voca Mobile TabBar Navigation — Implementation Plan", "_docs/frontend/plans/2026-09-17-13-49-plan-advoca-mobile-tabbar-navigation.md", "ad-voca", "ASMA-8208", "1a3d1a8e40b330520fb83d87f9b9fbd0d5066b4b3dbbd523386a681d4bc92e75"],
  ["Move the forms adapter into `asma-ui-core` (`asma-ui-core/forms`)", "_docs/frontend/plans/2026-09-28-00-28-plan-forms-adapter-into-asma-ui-core.md", "ui-core", null, "3056779501b8a66582df0413dbfc4c1ad231ba5da482985b546be025acd7c69c"],
  ["ASMA Unified Application and Tauri Delivery", "_docs/mobile-app/plans/2026-09-05-21-39-plan-unified-shell-native-app.md", "mobile-app", "ASMA-8108", "ea42327bf20db6addf4d2d8a4ebbca43dfe788d6bb7372977bc125d1d1558a8a"],
  ["Core product release before scheduled cleanup — supplemental amendment", "_docs/mobile-app/plans/2026-10-03-10-40-plan-core-release-cleanup-deferral-amendment.md", "mobile-app", null, "d5e9adeae9d295fb712a38923ff673a3cb141a96c628c72d0594ba7506da012b"],
  ["Introduction", "plan/architecture-pathway-data-model-evolution-1.md", null, null, "b204b475c314c65b45e1176ecec2457216616173873e325635690750a63403d4"],
  ["Introduction", "plan/feature-debug-chrome-login-1.md", null, null, "17d6b3addedb5d785a32ee7f5059bb2d6d63c230c175e31c11cd5e1e91610ad2"],
  ["Introduction", "plan/feature-fleet-health-dashboard-1.md", null, null, "e140a3498de91caf7b1f6a4812f94e81f7009f787a226f37536225c3a7fd1120"],
  ["Introduction", "plan/feature-kontor-jira-key-identity-1.md", null, null, "fe9f714dc5b95a056143d996e47b64e2b28e8ffaef07c01617b3c2f5c20f9ba4"],
  ["QNR rematerialization carry-over and restore-all", "plan/feature-qnr-rematerialize-restore-all-1.md", null, "ASMA-6045", "f3254fe39735a358d83d79a8047fdff546386e3d4fc4003da892e7123d6b0953"],
  ["Kontor Targetless Handoff and Consultation Recovery — completed plan", "plan/fix-kontor-targetless-handoff-recovery-1.md", null, null, "92946e4013017a0781cf2e0859d1a288bf1e3e025bc74bf2318f8ae73083c078"],
  ["Introduction", "plan/infrastructure-publication-identity-enforcement-1.md", null, null, "7acee60504e3632b12e4392962695fb49f2a3d437cbb9736a228114681ad793e"],
  ["Kontor Paseo Sessions Navigator — First Release", "_docs/ai-orchestration/plans/2026-10-18-15-15-plan-kontor-paseo-session-navigator.md", "kontor", "ASMA-8539", "285ab80dd6551de7c8de19db23ef69d5e6b336b132fde87871d4261c5b999e13"],
  ["Questionnaire document reports — implementation handoff for Eduard", "_docs/editor/qnrs/cross/plans/2026-10-20-09-49-plan-questionnaire-document-reports.md", "reporting", null, "98354f83109ee21b445563c033dbf07f11bc8d075b737747d1edc16c1fbe8a0c"],
];
export const PLAN_WORKSTREAM_READS: SourceRead[] = rows.map(([title, path, workstream, epic, originalSha256], index) => {
  const source = { id: `real-plan-${index}`, title, kind: "plan" as const, path: `/fixture-projections/${path}`, workstream: null, epic, projectId: null, revisionKind: "saved draft" as const };
  const content = `# ${title}\n\nMetadata projection for preview; not the original plan.\n\nOriginal source: ${path}\nOriginal SHA-256: ${originalSha256}\nDeclared workstream: ${workstream ?? "unknown"}\n\nNo approval, lifecycle or live sessions are inferred.`;
  return Object.assign({ source, status: "ready" as const, revision: `metadata-projection-${index}`, readAt: "2026-10-20T23:47:52.661982+00:00", modifiedAt: null, content, graph: null, records: null, diagnostics: ["Metadata-only audit projection; not exact source bytes or live inventory."] }, { planMetadata: { workstream, epic, diagnostics: [] } });
});
export const PLAN_WORKSTREAM_REGISTRY = {
  "path": "config/orchestration/workstreams.json",
  "revision": "a2993375dbe2dd189f7fb9160f98f8d39d9219e9aa0a48db235d7e7778f9c1a2",
  "workstreams": {
    "kontor": {
      "name": "Kontor"
    },
    "roles-chain-models": {
      "name": "Roles & Chain Models"
    },
    "mobile-app": {
      "name": "Mobile App"
    },
    "qnr-v2": {
      "name": "QNR v2"
    },
    "pr-lifecycle": {
      "name": "PR lifecycle autopilot"
    },
    "tokenomics": {
      "name": "Tokenomics"
    },
    "dev-platform": {
      "name": "Developer platform (asma-cli, repo, release pipeline)"
    },
    "frontend-platform": {
      "name": "Frontend platform (shell, loaders, fleet upgrades)"
    },
    "ui-core": {
      "name": "Design system & asma-ui-core"
    },
    "cdc": {
      "name": "CDC"
    },
    "adopus-graphql": {
      "name": "Ad Opus GraphQL"
    },
    "revman": {
      "name": "REVMAN"
    },
    "ad-voca": {
      "name": "Ad Voca"
    },
    "ad-curis": {
      "name": "Ad Curis"
    },
    "pathfinders": {
      "name": "Pathfinders"
    },
    "participants": {
      "name": "Participants & CRM"
    },
    "workspaces": {
      "name": "Workspaces"
    },
    "communication": {
      "name": "Notifications & chat"
    },
    "tasks-automation": {
      "name": "Tasks & automation"
    },
    "files-sharing": {
      "name": "Files & sharing"
    },
    "reporting": {
      "name": "Reporting"
    },
    "time-calendar": {
      "name": "Time & calendar"
    },
    "organization": {
      "name": "Employees & team"
    },
    "access": {
      "name": "Access & login"
    },
    "integrations": {
      "name": "Integrations"
    },
    "quality": {
      "name": "Quality & test automation"
    },
    "pr-desk-rules": {
      "name": "PR desk & seat rules"
    }
  },
  "desks": {
    "pr-review-desk": {
      "name": "PR Review Desk"
    },
    "adam-adhoc": {
      "name": "Adam • Ad-hoc"
    }
  },
  "diagnostics": []
};
