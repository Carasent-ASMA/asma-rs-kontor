import pathlib
P = pathlib.Path("crates/kontor-daemon/src/applications.rs")
s = P.read_text()
# MUTANT: settled_within forgets it has a deadline and always claims success.
old = """    let deadline = std::time::Instant::now() + timeout;
    loop {
        if all_finished() {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }"""
new = """    // MUTANT M-HANG-SETTLE-ALWAYS: an unfinished worker reports as settled.
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if all_finished() {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return true;
        }"""
assert s.count(old) == 1
P.write_text(s.replace(old, new, 1))
