import pathlib
P = pathlib.Path("crates/kontor-daemon/src/applications.rs")
s = P.read_text()
# Restore the split-lock boundary: release after history/current, reacquire for
# the personas and cursor.
old = """                // Still holding the lock. A writer that could commit here would
                // split this answer across two database states.
                projection_barrier_pause();
                // One persona per occupancy, by that occupancy's own exact
                // generation, read in the same breath as the rows they describe."""
new = """                // MUTANT M-COH: split the acquisition here.
                Ok(CapturedOccupancyChain::Split {
                    binding,
                    history,
                    current,
                })
            })
            .map_err(|error| self.refuse(&error))?;
        let captured = if let CapturedOccupancyChain::Split {
            binding,
            history,
            current,
        } = captured
        {
            projection_barrier_pause();
            state
                .with_store(|store| -> Result<CapturedOccupancyChain, RepositoryError> {
                // One persona per occupancy, by that occupancy's own exact
                // generation, read in the same breath as the rows they describe."""
assert s.count(old) == 1
s = s.replace(old, new, 1)
old2 = """                Ok(CapturedOccupancyChain::Found {
                    binding,
                    history,
                    current,
                    personas,
                    cursor,
                })
            })
            .map_err(|error| self.refuse(&error))?;"""
new2 = """                Ok(CapturedOccupancyChain::Found {
                    binding,
                    history,
                    current,
                    personas,
                    cursor,
                })
                })
                .map_err(|error| self.refuse(&error))?
        } else {
            captured
        };"""
assert s.count(old2) == 1
s = s.replace(old2, new2, 1)
old3 = """enum CapturedOccupancyChain {
    /// The epic has no control plane.
    NoControlPlane,"""
new3 = """enum CapturedOccupancyChain {
    /// MUTANT M-COH: the intermediate split state.
    Split {
        binding: SeatBinding,
        history: Vec<StoredHostedTopologySeat>,
        current: Option<StoredHostedTopologySeat>,
    },
    /// The epic has no control plane.
    NoControlPlane,"""
assert s.count(old3) == 1
s = s.replace(old3, new3, 1)
old4 = """            CapturedOccupancyChain::Found {
                binding,
                history,
                current,
                personas,
                cursor,
            } => (binding, history, current, personas, cursor),"""
new4 = """            CapturedOccupancyChain::Split { .. } => unreachable!("resolved above"),
            CapturedOccupancyChain::Found {
                binding,
                history,
                current,
                personas,
                cursor,
            } => (binding, history, current, personas, cursor),"""
assert s.count(old4) == 1
P.write_text(s.replace(old4, new4, 1))
print("M-COH applied")
