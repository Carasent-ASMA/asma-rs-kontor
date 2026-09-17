-- Schema v104. A hosted-seat launch intent cannot be deleted.
--
-- v103 protected the decision with a `BEFORE UPDATE` trigger, which refuses any
-- statement that restates the authority, the route or the prepared instant. It
-- left the other half of immutability open: nothing refused a DELETE. Because
-- the row's identity is exactly (project_id, seat_binding_id,
-- occupancy_generation), deleting one and inserting it again is a legal pair of
-- statements that lands a *different* autonomy on the same occupancy generation
-- without ever performing an update.
--
-- That defeats the whole point of the row. The repository's own `prepare` path
-- refuses a second authority for a generation that already has one, but it can
-- only refuse what it can see; a caller that deletes first presents an empty
-- slot, and any later reader -- the replay that resolves a lost acknowledgement
-- included -- would then treat the substituted authority as what the live
-- native was created under. Evidence that can be removed and rewritten is not
-- evidence.
--
-- Forward-only: v103 is already deployed, so the fix adds the missing trigger
-- rather than restating the table. A clean install runs every migration in
-- order and therefore ends with both halves present, so upgraded and fresh
-- realms hold the identical protection.
--
-- Nothing legitimate is lost. The row is consumed by *reconciliation*, which is
-- an UPDATE to `installed` that v103 already permits; no supported path has ever
-- needed to remove one. A generation whose native truly never existed is
-- superseded by the audited retire/replace path, which creates the next
-- generation and leaves this one standing as the record of what was attempted.
CREATE TRIGGER hosted_seat_launch_intent_is_append_only
BEFORE DELETE ON hosted_topology_seat_launch_intents
BEGIN
    SELECT RAISE(ABORT,
        'a hosted-seat launch intent is append-only; it records what was resolved before the native call and is never removed');
END;

PRAGMA user_version = 104;
