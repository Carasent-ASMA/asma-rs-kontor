-- Schema v76. A pending materialization batch may mix already-known link
-- intents with create intents for new tasks. Recovery still adopts the whole
-- immutable batch: create items prove their marker, while link items prove
-- both their marker and their originally requested Jira key.
DROP TRIGGER jira_materialization_recovery_scope_exact;

CREATE TRIGGER jira_materialization_recovery_scope_exact
BEFORE INSERT ON jira_materialization_recoveries
WHEN NOT EXISTS (
    SELECT 1
    FROM jira_materialization_items AS item
    WHERE item.id = NEW.item_id
      AND item.project_id = NEW.project_id
      AND item.batch_id = NEW.batch_id
      AND item.ordinal = NEW.ordinal
      AND item.marker = NEW.marker
      AND (
          item.intent_kind = 'create'
          OR (item.intent_kind = 'link' AND item.requested_key = NEW.requested_key)
      )
)
BEGIN SELECT RAISE(ABORT, 'Jira materialization recovery scope is not exact'); END;

PRAGMA user_version = 76;
