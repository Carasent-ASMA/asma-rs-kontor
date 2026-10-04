import pathlib
P = pathlib.Path("crates/kontor-daemon/src/applications.rs")
s = P.read_text()
# MUTANT: pause waits without a deadline -- the original defect. Expected to
# TIME OUT, not to be killed: removing a bound produces the hang the bound
# prevents, and a hang is never scored as a kill.
old = """        let deadline = std::time::Instant::now() + self.deadline;
        while !state.1 {"""
new = """        // MUTANT M-HANG-PAUSE-UNBOUNDED: no deadline at all.
        while !state.1 {"""
assert s.count(old) == 1
s = s.replace(old, new, 1)
old2 = """            let Some(remaining) = deadline.checked_duration_since(std::time::Instant::now()) else {
                state.2 = true;
                break;
            };
            let (next, timeout) = self
                .released
                .wait_timeout(state, remaining)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state = next;
            if timeout.timed_out() && !state.1 {
                state.2 = true;
                break;
            }"""
new2 = """            state = self
                .released
                .wait(state)
                .unwrap_or_else(std::sync::PoisonError::into_inner);"""
assert s.count(old2) == 1
P.write_text(s.replace(old2, new2, 1))
