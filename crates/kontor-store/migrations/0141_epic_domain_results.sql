-- Epic creation uses the existing exact local-result ledger atomically.
DROP TRIGGER local_command_results_exact_intent;
CREATE TRIGGER local_command_results_exact_intent BEFORE INSERT ON local_command_results
WHEN NOT EXISTS (
    SELECT 1 FROM command_receipts AS receipt
    WHERE receipt.project_id = NEW.project_id AND receipt.id = NEW.receipt_id
      AND receipt.execution_mode = 'local' AND receipt.state = 'intent_persisted'
      AND receipt.kind IN ('transition_task', 'withdraw_task', 'resume_task', 'record_gate_verdict', 'select_project_team_definition', 'apply_epic_graph')
      AND json_extract(NEW.payload, '$.result.intent_hash') = receipt.intent_hash
      AND json_remove(NEW.payload, '$.result') = json(receipt.intent)
)
BEGIN SELECT RAISE(ABORT, 'local command result requires its exact pending local intent'); END;

PRAGMA user_version = 141;
