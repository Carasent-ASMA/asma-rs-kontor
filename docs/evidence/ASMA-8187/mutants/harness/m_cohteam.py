import pathlib
P = pathlib.Path("crates/kontor-daemon/src/applications.rs")
s = P.read_text()
# Defer the persona read to a second acquisition AND move the barrier into the
# gap, so the split is the thing under test.
old = """                    projection_barrier_pause();
                    let mut captured = Vec::with_capacity(role_codes.len());"""
new = """                    let mut captured = Vec::with_capacity(role_codes.len());"""
assert s.count(old) == 1, "pause removal"
s = s.replace(old, new, 1)
old = """                                let persona = match generation {
                                    Some(generation) => store.get_hosted_seat_role_persona(
                                        project_id,
                                        seat_binding_id,
                                        generation,
                                    )?,
                                    None => None,
                                };"""
new = """                                // MUTANT M-COH-TEAM: deferred to a second
                                // acquisition.
                                let persona = None;"""
assert s.count(old) == 1, "persona defer"
s = s.replace(old, new, 1)
old = """        // Pure mapping, after the release.
        for (seat, captured) in seats.iter_mut().zip(captured) {"""
new = """        // MUTANT M-COH-TEAM: the gap, and the deferred read inside it.
        projection_barrier_pause();
        let mut captured = captured;
        for entry in &mut captured {
            if let (Some(seat_binding_id), Some(generation)) =
                (entry.seat_binding_id, entry.occupancy_generation)
            {
                entry.persona = state
                    .with_store(|store| {
                        store.get_hosted_seat_role_persona(project_id, seat_binding_id, generation)
                    })
                    .map_err(|error| self.refuse(&error))?;
            }
        }
        for (seat, captured) in seats.iter_mut().zip(captured) {"""
assert s.count(old) == 1, "deferred loop"
P.write_text(s.replace(old, new, 1))
