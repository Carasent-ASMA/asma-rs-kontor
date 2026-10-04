import pathlib
P = pathlib.Path("crates/kontor-store/migrations/0122_core_team_route_succession_receipt_identity.sql")
s = P.read_text()
a = s.index("CREATE TEMP TABLE core_team_route_succession_bindings_are_coherent (")
m = "DROP TABLE core_team_route_succession_bindings_are_coherent;"
z = s.index(m) + len(m)
P.write_text(s[:a] + "-- MUTANT M-P23: historical bindings are assumed coherent." + s[z:])
