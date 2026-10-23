---
goal: Top-of-screen notices on the Kontor Sessions screen can be hidden, stop holding vertical space once hidden, and stay reachable from one compact status button beside Resume without any notice being resolved, softened or lost.
version: 0.1
date_created: 2026-10-22
last_updated: 2026-10-22
status: Proposed
tags: [paseo-plugin, sessions, ux, accessibility, ASMA-8541]
workstream: kontor
---

# Dismissible top notices on the Sessions screen

> **Date:** 2026-10-22 21:00
> **Status:** 🔵 Proposed, implemented on `feat/ASMA-8541-dismissible-top-status`
> **Category:** plan
> **Scope:** `apps/paseo-plugin` Sessions screen only
> **Jira:** `ASMA-8541`

## When to load

Changing the notices at the top of the Sessions screen, adding a new notice there,
or reusing the pattern on another plugin page.

## Problem

Every Sessions notice is inline chrome above the toolbar and stays there until its
cause goes away. The archive recovery line (`1 archive not confirmed` with
**Check again** and **Details**) can stay for a long time. It pushes **Resume**, the
heading and the list down on every visit, and the operator cannot put it aside.

## Inventory

All of these render in `client/sessions-screen.tsx`, so this is one change in this
repository:

| Notice | Tone | Actions | Layouts |
| --- | --- | --- | --- |
| Archive recovery (`sessions-archive-recovery`) | warning | Check again / Retry, Details | wide full toolbar, wide slim row, compact |
| Source stale or error (`sessions-source-banner`) | warning / danger | Retry | wide full toolbar, compact |
| No native navigation (`sessions-navigation-banner`) | warning | none | wide full toolbar, compact |
| Organization settings unavailable | warning | none | wide full toolbar, compact |
| Settings save failure | warning (alert) | none | wide full toolbar, compact |

The Graph page's source notices and the Plan reader target line are content inside
their pages, not top chrome; they are out of scope.

## Guidance applied

- WAI-ARIA APG **Disclosure** pattern: a button with `aria-expanded` shows and hides
  a region. No modal, no focus trap.
- WCAG 2.2 **4.1.3 Status Messages**: status text stays a polite live region; the
  save failure stays an alert.
- WCAG 2.2 **2.4.3 Focus Order** and **2.1.2 No Keyboard Trap**: focus moves to the
  control that replaces the one that disappeared, and Tab leaves the region freely.
- WCAG 2.2 **2.5.8 Target Size** and the platform 44 pt touch target: every new
  control is 44 pt on touch and on compact layouts.
- GOV.UK notification banner, Carbon notifications, Material banners: a notice that
  reports an unresolved problem is never auto-dismissed and never light-dismissed by
  an outside press; only an explicit close hides it. The Popover API's
  `popover="manual"` model, not `auto`, is the equivalent for this kind of status.
- Nielsen Norman Group, visibility of system status: hiding a status must not
  remove the system's only indication of it.

## Proposal: one notice tray

1. **Placement.** The existing notices stay where they are, grouped as one tray:
   the top of the wide full toolbar, the recovery line under the wide slim row,
   and the compact notice block. No overlay, so nothing covers Resume or the list.
2. **Close affordance.** The tray has one **Hide notices** button at its top right,
   the existing `Action` with the `X` icon (icon-only with a hover/focus tooltip on a
   fine pointer, labelled `X Hide` on touch), the same control that closes Details.
3. **Collapsed trigger.** A hidden tray is replaced by one status button placed
   beside **Resume**, so it adds no row: in the wide toolbar's Resume row, in the
   wide slim row after Resume, and under Resume in the compact toolbar column. The
   compact Filters page, which has no Resume, shows the button where the tray was.
   It keeps the tray's tone (warning or danger), names the first notice (wide) or
   the notice count (compact), and its accessible name lists every hidden notice:
   `Show notices: 1 archive not confirmed`. It is a disclosure button with
   `aria-expanded="false"`.
4. **Actions.** Check again, Retry and Details live in the tray unchanged. Hiding
   never runs, cancels or disables them; showing the tray brings them back with the
   same enabled state.
5. **Persistence.** The hidden notices are remembered per Sessions view in the
   in-memory view store, like the legend and toolbar disclosures: they survive
   re-renders, a lifecycle dialog, a host switch and return, and a remount. They are
   never saved to organization settings, so a new app session shows them again.
6. **Reopen on change.** Each notice has stable keys: the source status and
   message, one key per unconfirmed archive (review and agent ID), the read error
   text, the settings status and the save message. The tray stays hidden only while
   every current key was hidden; any new or changed notice opens it again, so a new
   failure is never born hidden. A count that only falls (an archive confirmed)
   stays hidden. A notice that stays continuously present stays hidden. When a
   completed reading of its source no longer reports a hidden notice, its key is
   forgotten, so the same notice coming back is a new occurrence and opens the
   tray. A reading still in flight is not an absence: the first archive read
   after a mount or host switch, a directory load or refresh, a settings load or
   save, and a Check. A failed archive read reports no list, so it never makes an
   archive absent. Navigation is always read. Only a real change is written, so a
   re-render writes nothing.
7. **Truth.** Hiding is presentation only. It sends no RPC, writes no fence, shows
   no toast and changes no count. The trigger never uses a success tone and keeps
   the unresolved wording (`1 archive not confirmed`).
8. **Keyboard and focus.** Show moves focus to Hide notices; Hide and Escape (while
   focus is in the tray) move focus to the status button. Escape is handled in the
   capture phase only for focus inside the tray, so an open menu, dialog or tooltip
   keeps its own Escape and the compact Filters page's Escape-for-Back still works
   outside the tray. No outside-press dismissal.
9. **Responsive.** 320 and 390 pt use the compact layout: a 44 pt icon button with
   a count caption in the existing 80 pt Resume column. Desktop uses a 30 pt pill on
   a fine pointer, 44 pt on touch, with the summary on one line.
10. **Light and dark.** Only theme tokens: `statusWarning` / `statusDanger` for tone,
    `border`, `surface1` and `foreground`, so both themes follow the host.

## Rejected

- **Anchored popover on reopen.** It would stack under the Details lifecycle
  dialog, which anchors to Resume, and it covers the list on a 320 pt screen.
- **Auto-hide or light dismiss.** Either can lose an unresolved archive or failure
  without an operator decision.
- **Per-notice close buttons.** Five close targets for one decision; one tray and
  one button keep a single place to look.

## Verification

Behaviour tests in `apps/paseo-plugin/test/sessions-notice-tray.test.tsx` cover hide,
show, re-render and remount persistence, reopen on a new archive, continuous
presence through in-flight and failed reads, reopen when the same archive or source
notice returns after a completed reading without it, no RPC or toast
on hide, Check again and Details reachable after show, Escape and focus return, the
compact Filters placement and the 44 pt targets.
