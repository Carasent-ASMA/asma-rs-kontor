import pathlib
P = pathlib.Path("crates/kontor-store/migrations/0122_core_team_route_succession_receipt_identity.sql")
s = P.read_text()
old = "  OR (OLD.receipted_at IS NOT NULL AND OLD.receipted_at IS NOT NEW.receipted_at)\n"
new = "  -- MUTANT M-P22b2: the completion instant may be rewritten.\n"
assert s.count(old) == 1
P.write_text(s.replace(old, new, 1))
