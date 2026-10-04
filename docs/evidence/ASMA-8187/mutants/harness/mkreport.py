#!/usr/bin/env python3
"""Render the recheck report from the raw receipts. Nothing here is retyped."""
import pathlib, re, subprocess

K = pathlib.Path("/Users/igor/carasent/asma-modules/.worktrees/feat/"
                 "ASMA-8187-repair-admin-preview-apply-stale-native-core-team-succession/"
                 "_tools/asma-rs-kontor")
R = pathlib.Path("/tmp/asma8187-rc")

def block(log, marker):
    # The block is often empty; a non-greedy `.*?` would skip past the empty
    # terminator and swallow the raw output up to the *next* block's end.
    m = re.search(r"--- git status --porcelain=v1 \(whole repo, verbatim\) "
                  + marker + r" ---\n((?:(?!--- \(end).*\n)*)", log)
    return [l for l in m.group(1).splitlines() if l.strip()] if m else None

rows, total_p, total_f, total_b = [], 0, 0, 0
for line in (R / "SUMMARY.tsv").read_text().splitlines():
    rid, status, dur, cmd = line.split("\t", 3)
    log = (R / f"{rid}.log").read_text()
    tallies = re.findall(r"test result: \w+\. (\d+) passed; (\d+) failed", log)
    p = sum(int(a) for a, _ in tallies); f = sum(int(b) for _, b in tallies)
    if tallies:
        witness = f"{p} passed, {f} failed, {len(tallies)} binaries"
        total_p += p; total_f += f; total_b += len(tallies)
    elif "fmt" in rid:
        witness = "no diff"
    else:
        fin = re.findall(r"Finished `\w+` profile.*", log)
        witness = fin[-1] if fin else "see log"
    before, after = block(log, "BEFORE"), block(log, "AFTER")
    clean = "0 / 0" if not before and not after else f"{len(before or [])} / {len(after or [])}"
    ok = status == "0" and f == 0
    rows.append((rid, cmd, dur, witness, clean, "**PASS**" if ok else "**FAIL**"))

print("| Step | Command | Wall clock | Witness | Dirty before / after | Verdict |")
print("|---|---|---|---|---|---|")
for r in rows:
    print(f"| `{r[0]}` | `{r[1]}` | {r[2]} | {r[3]} | {r[4]} | {r[5]} |")
print()
print(f"TOTALS {total_p} passed, {total_f} failed, {total_b} binaries")
