# Paseo plugin header overlap

> **Date:** 2026-10-21 00:02 Europe/Oslo
> **Status:** Source verified; host delivery pending
> **Scope:** ASMA-8541, under ASMA-8539
> **Summary:** A two-line Paseo host correction clears the collapsed-sidebar title. The installed app is unchanged.

The prepared [source patch and regression test](../../docs/upstream/paseo-v0.12.0-header.patch) target Paseo `v0.12.0`, commit `cafe4b01c8522ff39560241c5deb01427dc8dc5b`. Applying this artifact to Kontor does not change the installed Paseo renderer. A host build containing the correction and packaged macOS verification remain necessary.

## Ownership and correction

[PluginSurfaceScreen](https://github.com/getpaseo/paseo/blob/cafe4b01c8522ff39560241c5deb01427dc8dc5b/packages/app/src/plugins/surface-screen.tsx) creates the title above the plugin body. Its `headerLeft` omits `SidebarMenuToggle`. On macOS, the existing toggle component reserves a placeholder when window chrome owns the real button. `WindowChromeSafeArea` already reserves the traffic lights; the missing placeholder leaves the title under the adjacent sidebar toggle.

The installed 0.12.0 renderer probe placed the collapsed title at x=90 and the toggle at x=82–108. The native Schedules screen placed its title at x=116. The source correction imports and renders the existing `SidebarMenuToggle` before the title, following `MenuHeader`. It adds no SDK contract or layout constant.

The 0.12.0 `PluginScreenContribution` supplies `id`, `title` and a body `Component`; it has no screen-header inset or picker slot. The independent Kontor toolbar/picker work therefore keeps its own plugin scope. No plugin runtime change is included here.

## Verification

The focused regression runs the real 0.12.0 app source, a real isolated daemon and a fictional plugin in headless Chromium. A declared macOS desktop bridge selects the window-chrome layout. This is renderer evidence; it does not establish packaged Electron behavior or physical window dragging.

| Check | Result |
| --- | --- |
| Original source, final regression | Fails at the collapsed title: required gap ≥8 px, observed −18 px |
| Two-line source correction, same regression | 1 test passed, 1.3 minutes |
| Sidebar and viewport changes | Expanded and collapsed; 1440×900, 1024×768, 800×600 and 1728×849; compact 390×844 capture |
| Keyboard and focus | Toggle receives focus; Enter expands; Space collapses; focus remains on the toggle |
| Drag contract | Header drag region remains; toggle computes `no-drag`; native movement untested |
| App typecheck; targeted lint and format | Passed; lint reported 0 warnings and 0 errors |

The first cold Metro warmup timed out before test execution. It has no regression verdict. The final red and green runs use the same test source. Installed renderer and `app.asar` hashes matched before and after; no host restart or DOM-based production workaround occurred.

## Upstream and evidence

Existing [Paseo PR #6319](https://github.com/getpaseo/paseo/pull/6319) contains the identical production correction. It was open and conflicting at this readback. Reconcile that contribution with the current upstream source; no duplicate PR or outward comment was posted.

Raw commands, logs, source snapshots, screenshots, host hashes and the prepared upstream note stay in the producing realm at `<state-root>/paseo-direct/ASMA-8539/evidence/2026-10-20-asma-8541-header-overlap/sol-source-verification/`. Its `SHA256SUMS` digest is `01a5a2ce32e202b2793dbab119e446fab1bc4844cd0038c37daa1a0fc43599d9`; `result.json` digest is `5cfb8fdd693c50418ad310463d613e11f95da7110e222084eff313d092e22ade`. The source patch digest is `bd9a42d37e20b9aa6abd165434465ce8ab1a0c83ecf5eb8e7f47260c589e97f9`.

QA/AUD review is for this prepared source correction and its evidence. It cannot establish live delivery or close ASMA-8541. A Kontor plugin reload cannot deploy this host correction.
