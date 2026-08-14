# Seat watchdog

Perform one read-only observation using the active supervision policy. Gather
active-turn age, last meaningful progress, runtime state and pending
permissions. Report a suspected hang only when every configured stale predicate
holds. Do not mutate or replace a seat.
