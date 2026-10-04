import pathlib
P = pathlib.Path("crates/kontor-daemon/src/applications.rs")
s = P.read_text()
# Faithful restoration of the pre-correction split: native under the first
# acquisition, generation AND persona under a later one, with the barrier in
# the gap. The escape guard is silenced so the persona/native agreement
# assertions must be what catches it.
old = """                    projection_barrier_pause();
                    let mut captured = Vec::with_capacity(role_codes.len());"""
new = """                    let mut captured = Vec::with_capacity(role_codes.len());"""
assert s.count(old) == 1, "pause"
s = s.replace(old, new, 1)
old = """                                // The store's own definition of the current
                                // occupancy -- `1 + count(history)` -- which is
                                // what the occupancy chain derives positionally.
                                // One rule, read twice, never two.
                                let generation = store.hosted_topology_seat_occupancy_generation(
                                    project_id,
                                    seat_binding_id,
                                )?;"""
new = """                                // MUTANT: generation deferred to a later
                                // acquisition, as the pre-correction code did.
                                let generation: Option<u64> = None;"""
assert s.count(old) == 1, "generation defer"
s = s.replace(old, new, 1)
old = """                                let persona = match generation {
                                    Some(generation) => store.get_hosted_seat_role_persona(
                                        project_id,
                                        seat_binding_id,
                                        generation,
                                    )?,
                                    None => None,
                                };"""
new = """                                let persona = None;"""
assert s.count(old) == 1, "persona defer"
s = s.replace(old, new, 1)
old = """        // Pure mapping, after the release.
        for (seat, captured) in seats.iter_mut().zip(captured) {"""
new = """        // MUTANT: the gap, with the generation and persona read inside it.
        projection_barrier_pause();
        let mut captured = captured;
        for entry in &mut captured {
            if let Some(seat_binding_id) = entry.seat_binding_id {
                entry.occupancy_generation = state
                    .with_store(|store| {
                        store.hosted_topology_seat_occupancy_generation(project_id, seat_binding_id)
                    })
                    .map_err(|error| self.refuse(&error))?;
                if let Some(generation) = entry.occupancy_generation {
                    entry.persona = state
                        .with_store(|store| {
                            store.get_hosted_seat_role_persona(
                                project_id,
                                seat_binding_id,
                                generation,
                            )
                        })
                        .map_err(|error| self.refuse(&error))?;
                }
            }
        }
        for (seat, captured) in seats.iter_mut().zip(captured) {"""
assert s.count(old) == 1, "deferred loop"
s = s.replace(old, new, 1)
old = """        assert!(
            !escaped,
            "a succession committed while the projection held the store lock"
        );"""
new = """        // MUTANT: escape guard silenced, so the persona/native agreement
        // assertions must be what catches the hybrid.
        let _ = escaped;"""
assert s.count(old) == 1, "escape guard"
P.write_text(s.replace(old, new, 1))
