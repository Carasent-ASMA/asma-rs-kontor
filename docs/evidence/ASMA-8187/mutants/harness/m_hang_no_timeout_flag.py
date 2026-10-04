import pathlib
P = pathlib.Path("crates/kontor-daemon/src/applications.rs")
s = P.read_text()
# MUTANT: pause stops recording that it hit its deadline, so a run that proved
# nothing would be reported as if it had proved something.
old = """            let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) else {
                state.2 = true;
                break;
            };"""
new = """            // MUTANT M-HANG-NO-TIMEOUT-FLAG: the deadline is hit silently.
            let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) else {
                break;
            };"""
assert s.count(old) == 1
s = s.replace(old, new, 1)
old2 = """            if timeout.timed_out() && !state.1 {
                state.2 = true;
                break;
            }"""
new2 = """            if timeout.timed_out() && !state.1 {
                break;
            }"""
assert s.count(old2) == 1
P.write_text(s.replace(old2, new2, 1))
