import pathlib
P = pathlib.Path("crates/kontor-daemon/src/applications.rs")
s = P.read_text()
# MUTANT: wait_entered panics mid-flight instead of reporting a verdict --
# the exact shape that could unwind past a running worker.
# The `return false;` is what distinguishes this site from the identically
# opened one in pause().
old = """            let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) else {
                return false;
            };"""
new = """            // MUTANT M-HANG-WAIT-ASSERTS: panic instead of reporting.
            let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) else {
                panic!("the projection never reached the barrier");
            };"""
assert s.count(old) == 1, f"expected exactly one site, found {s.count(old)}"
P.write_text(s.replace(old, new, 1))
