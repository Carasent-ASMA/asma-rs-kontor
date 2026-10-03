import pathlib
P = pathlib.Path("crates/kontor-store/src/repository.rs")
s = P.read_text()
old = """            typed
                .check_describes_transition("""
assert s.count(old) == 1
a = s.index(old)
z = s.index("?;", s.index("rule,\n                })", a)) + 2
P.write_text(s[:a] + "            // MUTANT M-P24: the readback need not describe this transition.\n            let _ = &claim;" + s[z:])
