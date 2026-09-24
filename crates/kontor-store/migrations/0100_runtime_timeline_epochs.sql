-- Schema v100. Durable Kontor<->raw timeline epoch continuity.
--
-- A Paseo timeline epoch is a raw native string. Kontor addresses positions by
-- a small u64 instead, allocated on first sight by `EpochRegistry`. That
-- registry lived only in the adapter process, and the daemon builds its adapter
-- with `PaseoCheckpoint::fresh`, so every restart began allocating from 1 in
-- whatever order sessions happened to be read.
--
-- The same seat therefore reported epoch 5, then 11, then 1 for one unchanged
-- transcript, and a tuple observed in one process named different content in
-- the next. Settlement validates `message_position.epoch` against the current
-- mapping, so an observation could stop being settleable purely because the
-- daemon restarted.
--
-- The mapping is scoped to the runtime that allocated it. Two hosts may hold
-- unrelated numbering, and an epoch is only meaningful against the plane it was
-- read from. `kontor_epoch` is unique per scope for the same reason the raw
-- string is: the mapping is a bijection, and a duplicate on either side is a
-- renumbering rather than a record.
--
-- Nothing here renumbers history. Rows already written to `role_turns` keep the
-- numbers they were settled under; this table only stops future allocations
-- from drifting.

CREATE TABLE runtime_timeline_epochs (
    runtime_kind TEXT    NOT NULL CHECK (length(runtime_kind) BETWEEN 1 AND 128),
    host         TEXT    NOT NULL CHECK (length(host) BETWEEN 1 AND 512),
    raw_epoch    TEXT    NOT NULL CHECK (length(raw_epoch) BETWEEN 1 AND 256),
    kontor_epoch INTEGER NOT NULL CHECK (kontor_epoch >= 1),
    recorded_at  TEXT    NOT NULL
                         CHECK (recorded_at GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T*Z'),
    PRIMARY KEY (runtime_kind, host, raw_epoch),
    UNIQUE (runtime_kind, host, kontor_epoch)
) STRICT;
PRAGMA user_version = 100;
