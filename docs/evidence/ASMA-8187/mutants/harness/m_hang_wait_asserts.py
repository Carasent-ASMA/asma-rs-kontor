import pathlib
P = pathlib.Path("crates/kontor-daemon/src/applications.rs")
s = P.read_text()
# MUTANT: wait_entered panics mid-flight instead of reporting a verdict --
# the exact shape that could unwind past a running worker.
old = """            let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now())
            else {
                return false;
            };
            let (next, _) = self
                .entered"""
new = """            // MUTANT M-HANG-WAIT-ASSERTS: panic instead of reporting.
            let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now())
            else {
                panic!("the projection never reached the barrier");
            };
            let (next, _) = self
                .entered"""
assert s.count(old) == 1
P.write_text(s.replace(old, new, 1))
