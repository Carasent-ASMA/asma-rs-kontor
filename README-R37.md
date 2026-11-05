# R37 — rainbow running mark: replaced running-icon sites

Igor's order (2026-11-04): a pulsating, colour-changing running icon that replaces every place the
running icons are used, same proportions as now. This is the complete list of replaced sites; it mirrors
the PR body and review-b's independent grep.

## Animated (live running, motion gates apply)

1. `apps/paseo-plugin/client/composer-pulse.tsx` — `ComposerPulse`, the engine used by the composer
   (Building2) and by live session rows (Play). Six pre-rendered gradient frames crossfade on the shared
   clock; breathing scale capped at 1.12.
2. `apps/paseo-plugin/client/composer-session-state.tsx` — the composer's Building2 running pulse, via
   `ComposerPulse`.
3. `apps/paseo-plugin/client/session-state-marker.tsx` — `RunningSessionMark` (live rows: dense-session-row
   22 px, start/switch rows 16 px) via `ComposerPulse`; not-live/last-known keeps the muted ink.

## Static rainbow (controls, markers, reduced motion — no motion carrier)

4. `apps/paseo-plugin/client/session-state-marker.tsx` — `SessionStatusIcon` static running: desktop filter
   chips and the organization group marker.
5. `apps/paseo-plugin/client/mobile-quick-toolbar.tsx` — the StateToolbar running tile and the Filters
   trigger when running is selected.
6. `apps/paseo-plugin/client/mobile-quick-chooser.tsx` — the running chooser row (20 px).
7. `apps/paseo-plugin/client/start-switch-sheet.tsx` — the More panel running item.

## Saved / last-visited (round-1 F2)

8. `apps/paseo-plugin/client/recent-visits.tsx` — `RecentFooter` saved entries (and the Recent visit rows
   through sessions-screen) carry the directory's live state, so a live running session under Saved/Last
   visited draws the same animated rainbow as any live row; muted last-known ink only when the directory
   really is last known.

## Host residuals (intentionally not replaced)

- Any Paseo host-owned running indicator outside the Kontor plugin surfaces.
- The desktop `ActivityIndicator` spinners (page-controls, sessions-screen loading) — loading affordances,
  not running-session marks.

## Paint and motion notes

- Geometry: the host's exact Lucide shapes (Play, Building2), viewBox 24, stroke-width 2, round caps and
  joins; identical `size` props everywhere.
- Paint: one wrapped linear gradient (violet → … → violet), span 4..20 along the stroke so the violet ends
  are visible (review-b S1), pre-rendered K=6 frames rotated 360/6 about the centre; per-instance gradient
  ids via React `useId` (review-b N1).
- Every stop keeps ≥3:1 (WCAG 1.4.11) on surface0/1/2 of the host tokens AND the Kontor appearance
  overrides, in both themes; the composited crossfade keeps ≥3:1 too (review-b S2).
- Motion: one shared Animated clock for N marks (native driver, opacity/transform only, no SMIL, no timers
  per mark); viewport/viewability, reduced motion, app background and last-known gates unchanged.
