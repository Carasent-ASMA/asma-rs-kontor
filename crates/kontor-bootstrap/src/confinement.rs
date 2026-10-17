//! Directory-bound confinement for every filesystem effect.
//!
//! Round one resolved paths by name for each operation, which leaves a
//! check-then-use window: a writer with the same rights can replace a checked
//! directory with a symlink between the check and the write. This module
//! removes the window by making every operation relative to an already-open
//! directory descriptor.
//!
//! # Primitive and platform assumptions
//!
//! The primitive is the POSIX `*at` family (`openat`, `statat`, `renameat`,
//! `unlinkat`, `mkdirat`, `symlinkat`, `readlinkat`) with `O_NOFOLLOW` /
//! `AT_SYMLINK_NOFOLLOW`, reached through `rustix` (an exact workspace pin)
//! with no `unsafe` code in this crate. Every directory component inside an
//! injected root is opened once with `O_DIRECTORY | O_NOFOLLOW`; after that,
//! names are resolved only against held descriptors, so a path component that
//! is swapped for a symlink after validation cannot redirect an effect.
//!
//! The injected root itself is opened exactly once by absolute path; ancestor
//! directories of that root are resolved by the kernel at that single `openat`
//! and are the caller's trust boundary. Refusal is total *inside* the root:
//! a symlink at any component, a non-regular file where a file is required, a
//! name that is not a single component, and a path that would leave the
//! opened directory are all typed refusals.
//!
//! This is a Unix implementation (macOS and Linux are the tested platforms).
//! On other platforms the confined operations return
//! [`ConfinementError::UnsupportedPlatform`] rather than falling back to
//! path-based access.

/// The largest config or journal document any confined read will return.
pub const MAX_DOCUMENT_BYTES: u64 = 4 * 1024 * 1024;

/// Why a confined operation was refused.
///
/// Every variant is path-free: an error can be recorded in a redacted receipt
/// or printed without leaking a host location.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ConfinementError {
    /// This platform has no confined implementation here.
    #[error("confined filesystem access is not supported on this platform")]
    UnsupportedPlatform,
    /// A component that must be a real directory or regular file is a symlink.
    #[error("a symlink was found where a real directory or file is required")]
    Symlink,
    /// A required file is not a regular file.
    #[error("a required file is not a regular file")]
    NotRegularFile,
    /// A name is not a single, safe path component.
    #[error("a path component is not a safe single name")]
    UnsafeName,
    /// A document exceeds the configured read bound.
    #[error("a document exceeds the configured size bound")]
    TooLarge,
    /// The path does not exist.
    #[error("the path does not exist")]
    Missing,
    /// A no-replace creation found the name already taken.
    #[error("the target already exists")]
    Exists,
    /// A filesystem operation failed.
    #[error("a confined filesystem operation failed")]
    Io,
}

/// What one directory entry is, without following it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    /// A regular file.
    File,
    /// A directory.
    Directory,
    /// A symbolic link.
    Symlink,
    /// Anything else.
    Other,
}

/// One held directory descriptor.
///
/// Dropping the value closes the descriptor; the directory it names may have
/// been renamed since, and operations still address the held inode. That is the
/// point: an attacker swapping a path cannot redirect them.
#[derive(Debug)]
pub struct Dir {
    #[cfg(unix)]
    fd: std::os::fd::OwnedFd,
}

/// Whether this build's confined primitives are supported here.
///
/// The selected supported boundary is exactly macOS and Linux; every other
/// target is a typed refusal before any filesystem effect.
#[must_use]
pub const fn supported_target() -> bool {
    cfg!(any(target_os = "macos", target_os = "linux"))
}

/// Whether a name is exactly one safe path component.
#[must_use]
pub fn is_simple_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains('\0')
}

/// Create a missing directory and its parents with owner-only access.
///
/// An existing directory is accepted unchanged; the caller's root validation
/// decides whether its current owner and mode are acceptable.
///
/// # Errors
/// [`ConfinementError::UnsupportedPlatform`] on a target without the Unix
/// permission model, and [`ConfinementError::Io`] when creation fails.
pub fn create_private_dir_all(path: &std::path::Path) -> Result<(), ConfinementError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;

        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)
            .map_err(|_| ConfinementError::Io)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Err(ConfinementError::UnsupportedPlatform)
    }
}

/// The effective user id of this process.
#[must_use]
pub fn effective_user() -> u32 {
    #[cfg(unix)]
    {
        rustix::process::geteuid().as_raw()
    }
    #[cfg(not(unix))]
    {
        0
    }
}

#[cfg(unix)]
mod unix {
    use std::io::Write;

    #[cfg(test)]
    use std::io::Read;

    #[cfg(test)]
    type SeamCallback = std::cell::RefCell<Option<Box<dyn FnMut(&str)>>>;

    // Test-only seam: runs after the name's statat validation and before the
    // descriptor is opened, so a regression can swap in a FIFO or other
    // non-regular node and prove the opened-fd kind/NONBLOCK guard.
    #[cfg(test)]
    thread_local! {
        static PRE_OPEN_SEAM: SeamCallback = const { std::cell::RefCell::new(None) };
        static PRE_READ_SEAM: SeamCallback = const { std::cell::RefCell::new(None) };
        static PRE_FSTAT_SEAM: SeamCallback = const { std::cell::RefCell::new(None) };
        static READ_CONSUMED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    }

    /// Test-only byte-consumption instrumentation: counts the bytes the
    /// bounded read actually took from the descriptor, so removing the
    /// `max_bytes + 1` take is discriminated by consumption rather than by
    /// the post-read bound alone. Production builds carry no counter.
    #[cfg(test)]
    fn note_consumed(count: usize) {
        READ_CONSUMED.with(|cell| cell.set(cell.get() + count as u64));
    }

    /// Test-only: the bytes consumed since the last reset.
    #[cfg(test)]
    pub(crate) fn consumed_bytes() -> u64 {
        READ_CONSUMED.with(std::cell::Cell::get)
    }

    /// Test-only: reset the consumption counter.
    #[cfg(test)]
    pub(crate) fn reset_consumed() {
        READ_CONSUMED.with(|cell| cell.set(0));
    }

    /// Test-only reader wrapper that records consumed bytes.
    #[cfg(test)]
    struct CountingReader<R> {
        inner: R,
    }

    #[cfg(test)]
    impl<R: Read> Read for CountingReader<R> {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let read = self.inner.read(buffer)?;
            note_consumed(read);
            Ok(read)
        }
    }

    #[cfg(test)]
    pub(crate) fn set_pre_fstat_seam(action: impl FnMut(&str) + 'static) {
        PRE_FSTAT_SEAM.with(|seam| *seam.borrow_mut() = Some(Box::new(action)));
    }

    #[cfg(test)]
    fn run_pre_fstat_seam(name: &str) {
        PRE_FSTAT_SEAM.with(|seam| {
            if let Some(action) = seam.borrow_mut().as_mut() {
                action(name);
            }
        });
    }

    #[cfg(test)]
    pub(crate) fn set_pre_open_seam(action: impl FnMut(&str) + 'static) {
        PRE_OPEN_SEAM.with(|seam| *seam.borrow_mut() = Some(Box::new(action)));
    }

    #[cfg(test)]
    fn run_pre_open_seam(name: &str) {
        PRE_OPEN_SEAM.with(|seam| {
            if let Some(action) = seam.borrow_mut().as_mut() {
                action(name);
            }
        });
    }

    #[cfg(test)]
    pub(crate) fn set_pre_read_seam(action: impl FnMut(&str) + 'static) {
        PRE_READ_SEAM.with(|seam| *seam.borrow_mut() = Some(Box::new(action)));
    }

    #[cfg(test)]
    fn run_pre_read_seam(name: &str) {
        PRE_READ_SEAM.with(|seam| {
            if let Some(action) = seam.borrow_mut().as_mut() {
                action(name);
            }
        });
    }

    use rustix::fs::{AtFlags, DirEntry, FileType, Mode, OFlags, Stat};

    use super::{ConfinementError, Dir, NodeKind, is_simple_name};

    fn mode_bits(bits: u32) -> Mode {
        let raw: rustix::fs::RawMode = bits.try_into().unwrap_or_default();
        Mode::from_raw_mode(raw)
    }

    fn flags_nofollow() -> OFlags {
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC
    }

    fn classify(stat: &Stat) -> NodeKind {
        match FileType::from_raw_mode(stat.st_mode) {
            FileType::RegularFile => NodeKind::File,
            FileType::Directory => NodeKind::Directory,
            FileType::Symlink => NodeKind::Symlink,
            _ => NodeKind::Other,
        }
    }

    impl Dir {
        /// Open one injected root directory by absolute path.
        ///
        /// # Errors
        /// [`ConfinementError::Symlink`] when the final component is a symlink,
        /// [`ConfinementError::Missing`] when it does not exist, and
        /// [`ConfinementError::Io`] for anything else.
        pub fn open_root(root: &std::path::Path) -> Result<Self, ConfinementError> {
            if !super::supported_target() {
                return Err(ConfinementError::UnsupportedPlatform);
            }
            if !root.is_absolute() {
                return Err(ConfinementError::UnsafeName);
            }
            let fd = rustix::fs::openat(rustix::fs::CWD, root, flags_nofollow(), Mode::empty())
                .map_err(|error| map_open_at(rustix::fs::CWD, root, error))?;
            Ok(Self { fd })
        }

        /// Duplicate the descriptor so one holder can be used in two roles.
        ///
        /// # Errors
        /// [`ConfinementError::Io`].
        pub fn try_clone(&self) -> Result<Self, ConfinementError> {
            self.fd
                .try_clone()
                .map(|fd| Self { fd })
                .map_err(|_| ConfinementError::Io)
        }

        /// Open one child directory, refusing a symlink or non-directory.
        ///
        /// # Errors
        /// The typed refusals of [`ConfinementError`].
        pub fn open_child(&self, name: &str) -> Result<Self, ConfinementError> {
            if !is_simple_name(name) {
                return Err(ConfinementError::UnsafeName);
            }
            let fd = rustix::fs::openat(&self.fd, name, flags_nofollow(), Mode::empty())
                .map_err(|error| map_open_at(&self.fd, name, error))?;
            Ok(Self { fd })
        }

        /// Open one child directory, creating it with `mode` when absent.
        ///
        /// # Errors
        /// The typed refusals of [`ConfinementError`].
        pub fn open_child_or_create(
            &self,
            name: &str,
            mode: u32,
        ) -> Result<Self, ConfinementError> {
            match self.open_child(name) {
                Ok(dir) => Ok(dir),
                Err(ConfinementError::Missing) => {
                    match rustix::fs::mkdirat(&self.fd, name, mode_bits(mode)) {
                        Ok(()) | Err(rustix::io::Errno::EXIST) => {}
                        Err(error) => return Err(map_errno(error)),
                    }
                    self.open_child(name)
                }
                Err(error) => Err(error),
            }
        }

        /// Create one child directory exclusively.
        ///
        /// `mkdirat` is the atomic primitive: an existing name (of any kind) is
        /// a typed refusal and is never adopted.
        ///
        /// # Errors
        /// [`ConfinementError::Exists`] when the name is taken.
        pub fn create_child_dir_exclusive(
            &self,
            name: &str,
            mode: u32,
        ) -> Result<Self, ConfinementError> {
            if !is_simple_name(name) {
                return Err(ConfinementError::UnsafeName);
            }
            match rustix::fs::mkdirat(&self.fd, name, mode_bits(mode)) {
                Ok(()) => {}
                Err(rustix::io::Errno::EXIST) => return Err(ConfinementError::Exists),
                Err(error) => return Err(map_errno(error)),
            }
            self.open_child(name)
        }

        /// The `(device, inode)` of one child, without following a symlink.
        ///
        /// # Errors
        /// [`ConfinementError`].
        #[allow(clippy::unnecessary_cast)]
        pub fn child_identity(&self, name: &str) -> Result<Option<(u64, u64)>, ConfinementError> {
            if !is_simple_name(name) {
                return Err(ConfinementError::UnsafeName);
            }
            match rustix::fs::statat(&self.fd, name, AtFlags::SYMLINK_NOFOLLOW) {
                Ok(stat) => Ok(Some((stat.st_dev as u64, stat.st_ino as u64))),
                Err(rustix::io::Errno::NOENT) => Ok(None),
                Err(error) => Err(map_errno(error)),
            }
        }

        /// Flush this held directory's metadata and entry names.
        ///
        /// This narrows the process-interruption window at durable
        /// name-transition boundaries. It is not a power-loss durability
        /// qualification: the storage stack may still lose acknowledged
        /// writes.
        ///
        /// # Errors
        /// [`ConfinementError::Io`].
        pub fn sync(&self) -> Result<(), ConfinementError> {
            rustix::fs::fsync(&self.fd).map_err(|_| ConfinementError::Io)
        }

        /// The `(device, inode)` of this held directory.
        ///
        /// # Errors
        /// [`ConfinementError::Io`].
        #[allow(clippy::unnecessary_cast)]
        pub fn dir_identity(&self) -> Result<(u64, u64), ConfinementError> {
            let stat = rustix::fs::fstat(&self.fd).map_err(|_| ConfinementError::Io)?;
            Ok((stat.st_dev as u64, stat.st_ino as u64))
        }

        /// The owner and permission bits of this held directory.
        ///
        /// # Errors
        /// [`ConfinementError::Io`].
        pub fn owner_and_mode(&self) -> Result<(u32, u32), ConfinementError> {
            let stat = rustix::fs::fstat(&self.fd).map_err(|_| ConfinementError::Io)?;
            let raw: u32 = stat.st_mode.into();
            Ok((stat.st_uid, raw & 0o7777))
        }

        /// The `(device, inode)` of one open file.
        ///
        /// # Errors
        /// [`ConfinementError::UnsupportedPlatform`] outside the selected
        /// macOS/Linux boundary; [`ConfinementError::Io`] otherwise.
        #[allow(clippy::unnecessary_cast)]
        pub fn file_identity(file: &std::fs::File) -> Result<(u64, u64), ConfinementError> {
            if !super::supported_target() {
                return Err(ConfinementError::UnsupportedPlatform);
            }
            let stat = rustix::fs::fstat(file).map_err(|_| ConfinementError::Io)?;
            Ok((stat.st_dev as u64, stat.st_ino as u64))
        }

        /// Remove one child only when a fresh observation shows exactly the
        /// identity the caller created.
        ///
        /// The `statat` and the `unlinkat` are two separate by-name effects:
        /// a same-UID writer can replace the name between them, which is the
        /// accepted residual. A name that is absent, changed, symlinked or
        /// otherwise uncertain is left in place and reported as `false`.
        ///
        /// # Errors
        /// [`ConfinementError`].
        pub fn remove_child_if(
            &self,
            name: &str,
            identity: (u64, u64),
        ) -> Result<bool, ConfinementError> {
            if !is_simple_name(name) {
                return Err(ConfinementError::UnsafeName);
            }
            match self.child_identity(name)? {
                None => Ok(false),
                Some(current) if current == identity => {
                    match rustix::fs::unlinkat(&self.fd, name, AtFlags::empty()) {
                        Ok(()) | Err(rustix::io::Errno::NOENT) => Ok(true),
                        Err(error) => Err(map_errno(error)),
                    }
                }
                Some(_) => Ok(false),
            }
        }

        /// What one child is, without following a final symlink.
        ///
        /// # Errors
        /// [`ConfinementError`].
        pub fn kind_child(&self, name: &str) -> Result<Option<NodeKind>, ConfinementError> {
            if !is_simple_name(name) {
                return Err(ConfinementError::UnsafeName);
            }
            match rustix::fs::statat(&self.fd, name, AtFlags::SYMLINK_NOFOLLOW) {
                Ok(stat) => Ok(Some(classify(&stat))),
                Err(rustix::io::Errno::NOENT) => Ok(None),
                Err(error) => Err(map_errno(error)),
            }
        }

        /// Read one regular child file without following a symlink.
        ///
        /// # Errors
        /// [`ConfinementError::Symlink`] for a symlink,
        /// [`ConfinementError::TooLarge`] past `max_bytes`, and the other
        /// typed refusals.
        pub fn read_child(
            &self,
            name: &str,
            max_bytes: u64,
        ) -> Result<Option<Vec<u8>>, ConfinementError> {
            if !is_simple_name(name) {
                return Err(ConfinementError::UnsafeName);
            }
            let stat = match rustix::fs::statat(&self.fd, name, AtFlags::SYMLINK_NOFOLLOW) {
                Ok(stat) => stat,
                Err(rustix::io::Errno::NOENT) => return Ok(None),
                Err(error) => return Err(map_errno(error)),
            };
            match classify(&stat) {
                NodeKind::File => {}
                NodeKind::Symlink => return Err(ConfinementError::Symlink),
                _ => return Err(ConfinementError::NotRegularFile),
            }
            if stat.st_size < 0 || stat.st_size as u64 > max_bytes {
                return Err(ConfinementError::TooLarge);
            }
            // Open non-blocking and validate the opened descriptor: a FIFO or
            // device swapped in after the name check is refused instead of
            // blocking or streaming, and the size bound is bound to the fd
            // that is actually read. Deleting this NONBLOCK flag is a
            // hang-only seed: the pre-open FIFO fixture proves the opened-fd
            // kind guard, not a clean assertion kill for the flag itself.
            #[cfg(test)]
            run_pre_open_seam(name);
            let fd = rustix::fs::openat(
                &self.fd,
                name,
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
                Mode::empty(),
            )
            .map_err(|error| map_open_at(&self.fd, name, error))?;
            // Test-only seam after the descriptor is opened and before its
            // fstat validation: growth here must be caught by the opened-fd
            // size operand (or, at worst, the bounded take).
            #[cfg(test)]
            run_pre_fstat_seam(name);
            let descriptor = rustix::fs::fstat(&fd).map_err(|_| ConfinementError::Io)?;
            match classify(&descriptor) {
                NodeKind::File => {}
                NodeKind::Symlink => return Err(ConfinementError::Symlink),
                _ => return Err(ConfinementError::NotRegularFile),
            }
            if descriptor.st_size < 0 || descriptor.st_size as u64 > max_bytes {
                return Err(ConfinementError::TooLarge);
            }
            // Test-only seam after the descriptor validation: a file grown
            // here is bounded by the max+1 take and refused.
            #[cfg(test)]
            run_pre_read_seam(name);
            let file = std::fs::File::from(fd);
            let mut bytes = Vec::new();
            // One production bounded operand, compiled in normal and test
            // builds alike: at most max_bytes + 1 bytes are taken from the
            // opened descriptor. The test-only counter observes that same
            // operand; it never replaces it with a separate implementation.
            let bounded = std::io::Read::take(file, max_bytes.saturating_add(1));
            #[cfg(test)]
            let mut reader = CountingReader { inner: bounded };
            #[cfg(not(test))]
            let mut reader = bounded;
            std::io::Read::read_to_end(&mut reader, &mut bytes)
                .map_err(|_| ConfinementError::Io)?;
            if bytes.len() as u64 > max_bytes {
                return Err(ConfinementError::TooLarge);
            }
            Ok(Some(bytes))
        }

        /// Create one new empty regular child file, failing if it exists.
        ///
        /// The creation mode is applied at `openat` time, so no window exists
        /// in which the file carries wider permissions than requested.
        ///
        /// # Errors
        /// [`ConfinementError`].
        pub fn create_child_empty(
            &self,
            name: &str,
            mode: u32,
        ) -> Result<std::fs::File, ConfinementError> {
            if !is_simple_name(name) {
                return Err(ConfinementError::UnsafeName);
            }
            let fd = rustix::fs::openat(
                &self.fd,
                name,
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                mode_bits(mode),
            )
            .map_err(|error| map_open_at(&self.fd, name, error))?;
            Ok(std::fs::File::from(fd))
        }

        /// Create one new regular child file with content.
        ///
        /// # Errors
        /// [`ConfinementError`].
        pub fn create_child_file(
            &self,
            name: &str,
            bytes: &[u8],
            mode: u32,
        ) -> Result<std::fs::File, ConfinementError> {
            let mut file = self.create_child_empty(name, mode)?;
            file.write_all(bytes).map_err(|_| ConfinementError::Io)?;
            file.sync_all().map_err(|_| ConfinementError::Io)?;
            Ok(file)
        }

        /// Open one existing regular child file read-write without truncating.
        ///
        /// # Errors
        /// [`ConfinementError`].
        pub fn open_child_file_rw(&self, name: &str) -> Result<std::fs::File, ConfinementError> {
            if !is_simple_name(name) {
                return Err(ConfinementError::UnsafeName);
            }
            match self.kind_child(name)? {
                Some(NodeKind::File) => {}
                Some(NodeKind::Symlink) => return Err(ConfinementError::Symlink),
                Some(_) => return Err(ConfinementError::NotRegularFile),
                None => return Err(ConfinementError::Missing),
            }
            let fd = rustix::fs::openat(
                &self.fd,
                name,
                OFlags::RDWR | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|error| map_open_at(&self.fd, name, error))?;
            Ok(std::fs::File::from(fd))
        }

        /// Open one existing regular child file read-only without following a
        /// symlink.
        ///
        /// A read-only descriptor is sufficient for `fchmod`, so a permission
        /// repair never needs write access to the artifact's bytes.
        ///
        /// # Errors
        /// [`ConfinementError`].
        pub fn open_child_file_ro(&self, name: &str) -> Result<std::fs::File, ConfinementError> {
            if !is_simple_name(name) {
                return Err(ConfinementError::UnsafeName);
            }
            match self.kind_child(name)? {
                Some(NodeKind::File) => {}
                Some(NodeKind::Symlink) => return Err(ConfinementError::Symlink),
                Some(_) => return Err(ConfinementError::NotRegularFile),
                None => return Err(ConfinementError::Missing),
            }
            let fd = rustix::fs::openat(
                &self.fd,
                name,
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|error| map_open_at(&self.fd, name, error))?;
            Ok(std::fs::File::from(fd))
        }

        /// Replace the permissions of one open file.
        ///
        /// # Errors
        /// [`ConfinementError::UnsupportedPlatform`] outside the selected
        /// macOS/Linux boundary; [`ConfinementError::Io`] otherwise.
        pub fn set_file_mode(file: &std::fs::File, mode: u32) -> Result<(), ConfinementError> {
            if !super::supported_target() {
                return Err(ConfinementError::UnsupportedPlatform);
            }
            rustix::fs::fchmod(file, mode_bits(mode)).map_err(|_| ConfinementError::Io)
        }

        /// Rename one child onto another without replacing an existing target.
        ///
        /// Uses `renameat2`/`renameatx_np` with the no-replace flag, which the
        /// kernel applies atomically; a target created after any check is
        /// reported as [`ConfinementError::Exists`] and left untouched.
        ///
        /// # Errors
        /// [`ConfinementError::Exists`] when the target name is taken.
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        pub fn rename_child_noreplace(
            &self,
            from: &str,
            to_dir: &Dir,
            to: &str,
        ) -> Result<(), ConfinementError> {
            if !is_simple_name(from) || !is_simple_name(to) {
                return Err(ConfinementError::UnsafeName);
            }
            match rustix::fs::renameat_with(
                &self.fd,
                from,
                &to_dir.fd,
                to,
                rustix::fs::RenameFlags::NOREPLACE,
            ) {
                Ok(()) => Ok(()),
                Err(rustix::io::Errno::EXIST | rustix::io::Errno::NOTEMPTY) => {
                    Err(ConfinementError::Exists)
                }
                Err(error) => Err(map_errno(error)),
            }
        }

        /// Rename one child onto another without replacing an existing target.
        ///
        /// This platform has no atomic no-replace primitive in this build, so
        /// the operation is a typed refusal rather than a check-then-replace
        /// fallback. No cross-platform commitment is made.
        ///
        /// # Errors
        /// Always [`ConfinementError::UnsupportedPlatform`] on this platform.
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        pub fn rename_child_noreplace(
            &self,
            _from: &str,
            _to_dir: &Dir,
            _to: &str,
        ) -> Result<(), ConfinementError> {
            Err(ConfinementError::UnsupportedPlatform)
        }

        /// Rename one child onto another (possibly in another held directory).
        ///
        /// `rename` never follows a symlink at the destination's final
        /// component; it replaces the link itself. The caller still refuses a
        /// symlinked destination explicitly so a foreign link is reported
        /// rather than silently replaced.
        ///
        /// # Errors
        /// [`ConfinementError::Io`] and name refusals.
        pub fn rename_child(
            &self,
            from: &str,
            to_dir: &Dir,
            to: &str,
        ) -> Result<(), ConfinementError> {
            if !is_simple_name(from) || !is_simple_name(to) {
                return Err(ConfinementError::UnsafeName);
            }
            rustix::fs::renameat(&self.fd, from, &to_dir.fd, to).map_err(|_| ConfinementError::Io)
        }

        /// Hard-link one child file to a new name, refusing an existing
        /// target atomically (`linkat` without replacement).
        ///
        /// # Errors
        /// [`ConfinementError::Exists`] when the target name is taken.
        pub fn link_child(
            &self,
            from: &str,
            to_dir: &Dir,
            to: &str,
        ) -> Result<(), ConfinementError> {
            if !is_simple_name(from) || !is_simple_name(to) {
                return Err(ConfinementError::UnsafeName);
            }
            match rustix::fs::linkat(&self.fd, from, &to_dir.fd, to, AtFlags::empty()) {
                Ok(()) => Ok(()),
                Err(rustix::io::Errno::EXIST) => Err(ConfinementError::Exists),
                Err(error) => Err(map_errno(error)),
            }
        }

        /// Atomically exchange two names.
        ///
        /// This swaps names; it does not compare bytes or inodes and does not
        /// bind later verification. The design on top of it is
        /// exchange-and-verify, never compare-and-swap.
        ///
        /// # Errors
        /// [`ConfinementError`].
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        pub fn exchange_children(
            &self,
            from: &str,
            other: &Dir,
            to: &str,
        ) -> Result<(), ConfinementError> {
            if !is_simple_name(from) || !is_simple_name(to) {
                return Err(ConfinementError::UnsafeName);
            }
            match rustix::fs::renameat_with(
                &self.fd,
                from,
                &other.fd,
                to,
                rustix::fs::RenameFlags::EXCHANGE,
            ) {
                Ok(()) => Ok(()),
                Err(error) => Err(map_errno(error)),
            }
        }

        /// Atomically exchange two names.
        ///
        /// # Errors
        /// Always [`ConfinementError::UnsupportedPlatform`] on this platform.
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        pub fn exchange_children(
            &self,
            _from: &str,
            _other: &Dir,
            _to: &str,
        ) -> Result<(), ConfinementError> {
            Err(ConfinementError::UnsupportedPlatform)
        }

        /// Remove one child file or symlink.
        ///
        /// # Errors
        /// [`ConfinementError`].
        pub fn remove_child(&self, name: &str) -> Result<(), ConfinementError> {
            if !is_simple_name(name) {
                return Err(ConfinementError::UnsafeName);
            }
            match rustix::fs::unlinkat(&self.fd, name, AtFlags::empty()) {
                Ok(()) | Err(rustix::io::Errno::NOENT) => Ok(()),
                Err(error) => Err(map_errno(error)),
            }
        }

        /// Remove one empty child directory.
        ///
        /// # Errors
        /// [`ConfinementError`].
        pub fn remove_child_directory(&self, name: &str) -> Result<(), ConfinementError> {
            if !is_simple_name(name) {
                return Err(ConfinementError::UnsafeName);
            }
            match rustix::fs::unlinkat(&self.fd, name, AtFlags::REMOVEDIR) {
                Ok(()) | Err(rustix::io::Errno::NOENT) => Ok(()),
                Err(error) => Err(map_errno(error)),
            }
        }

        /// Remove a whole child tree, without following any symlink.
        ///
        /// # Errors
        /// [`ConfinementError`].
        pub fn remove_tree_child(&self, name: &str) -> Result<(), ConfinementError> {
            if !is_simple_name(name) {
                return Err(ConfinementError::UnsafeName);
            }
            match self.kind_child(name)? {
                None => return Ok(()),
                Some(NodeKind::Directory) => {}
                Some(NodeKind::Symlink) => return Err(ConfinementError::Symlink),
                Some(_) => {
                    return self.remove_child(name);
                }
            }
            let child = self.open_child(name)?;
            for entry in child.entries()? {
                match child.kind_child(&entry)? {
                    Some(NodeKind::Directory) => child.remove_tree_child(&entry)?,
                    Some(NodeKind::Symlink) | Some(_) => child.remove_child(&entry)?,
                    None => {}
                }
            }
            self.remove_child_directory(name)
        }

        /// Every entry name in this directory, requiring UTF-8.
        ///
        /// # Errors
        /// [`ConfinementError::UnsafeName`] for a non-UTF-8 entry.
        pub fn entries(&self) -> Result<Vec<String>, ConfinementError> {
            let mut dir = rustix::fs::Dir::read_from(&self.fd).map_err(|_| ConfinementError::Io)?;
            let mut names = Vec::new();
            while let Some(entry) = dir.read() {
                let entry: DirEntry = entry.map_err(|_| ConfinementError::Io)?;
                let name = entry
                    .file_name()
                    .to_str()
                    .map_err(|_| ConfinementError::UnsafeName)?;
                if name == "." || name == ".." {
                    continue;
                }
                names.push(name.to_owned());
            }
            Ok(names)
        }

        /// Create one child symlink to a relative target.
        ///
        /// # Errors
        /// [`ConfinementError`].
        pub fn create_child_symlink(
            &self,
            target: &str,
            name: &str,
        ) -> Result<(), ConfinementError> {
            if !is_simple_name(name) || target.is_empty() || target.contains('\0') {
                return Err(ConfinementError::UnsafeName);
            }
            match rustix::fs::symlinkat(target, &self.fd, name) {
                Ok(()) => Ok(()),
                Err(rustix::io::Errno::EXIST) => Err(ConfinementError::Exists),
                Err(error) => Err(map_errno(error)),
            }
        }

        /// Read one child symlink target.
        ///
        /// # Errors
        /// [`ConfinementError`].
        pub fn read_link_child(&self, name: &str) -> Result<Option<String>, ConfinementError> {
            if !is_simple_name(name) {
                return Err(ConfinementError::UnsafeName);
            }
            match rustix::fs::readlinkat(&self.fd, name, Vec::new()) {
                Ok(target) => target
                    .to_str()
                    .map(|text| Some(text.to_owned()))
                    .map_err(|_| ConfinementError::UnsafeName),
                Err(rustix::io::Errno::NOENT) => Ok(None),
                Err(error) => Err(map_errno(error)),
            }
        }

        /// The permission bits of one child, without following a symlink.
        ///
        /// # Errors
        /// [`ConfinementError`].
        pub fn child_mode(&self, name: &str) -> Result<Option<u32>, ConfinementError> {
            if !is_simple_name(name) {
                return Err(ConfinementError::UnsafeName);
            }
            match rustix::fs::statat(&self.fd, name, AtFlags::SYMLINK_NOFOLLOW) {
                Ok(stat) => {
                    let raw: u32 = stat.st_mode.into();
                    Ok(Some(raw & 0o7777))
                }
                Err(rustix::io::Errno::NOENT) => Ok(None),
                Err(error) => Err(map_errno(error)),
            }
        }
    }

    /// Map an `openat` failure, disambiguating platforms (macOS reports a
    /// symlinked directory under `O_DIRECTORY|O_NOFOLLOW` as `ENOTDIR`) by one
    /// no-follow `statat` that cannot itself follow the link.
    fn map_open_at<Fd: std::os::fd::AsFd, P: rustix::path::Arg>(
        dirfd: Fd,
        path: P,
        error: rustix::io::Errno,
    ) -> ConfinementError {
        match error {
            rustix::io::Errno::NOENT => ConfinementError::Missing,
            rustix::io::Errno::EXIST => ConfinementError::Exists,
            rustix::io::Errno::LOOP => ConfinementError::Symlink,
            rustix::io::Errno::NOTDIR => {
                match rustix::fs::statat(&dirfd, path, AtFlags::SYMLINK_NOFOLLOW) {
                    Ok(stat) => match classify(&stat) {
                        NodeKind::Symlink => ConfinementError::Symlink,
                        NodeKind::File => ConfinementError::NotRegularFile,
                        _ => ConfinementError::Io,
                    },
                    Err(rustix::io::Errno::NOENT) => ConfinementError::Missing,
                    Err(_) => ConfinementError::Io,
                }
            }
            _ => ConfinementError::Io,
        }
    }

    fn map_errno(error: rustix::io::Errno) -> ConfinementError {
        match error {
            rustix::io::Errno::NOENT => ConfinementError::Missing,
            rustix::io::Errno::EXIST | rustix::io::Errno::NOTEMPTY => ConfinementError::Exists,
            rustix::io::Errno::LOOP => ConfinementError::Symlink,
            rustix::io::Errno::NOTDIR => ConfinementError::NotRegularFile,
            _ => ConfinementError::Io,
        }
    }
}

#[cfg(not(unix))]
mod unix {
    use std::path::Path;

    use super::{ConfinementError, Dir, NodeKind};

    /// Every confined operation is a typed refusal on a platform without the
    /// Unix `*at` primitives. Nothing here falls back to path-based access and
    /// no cross-platform commitment is made.
    #[allow(unused_variables)]
    impl Dir {
        pub fn open_root(root: &Path) -> Result<Self, ConfinementError> {
            Err(ConfinementError::UnsupportedPlatform)
        }

        pub fn try_clone(&self) -> Result<Self, ConfinementError> {
            Err(ConfinementError::UnsupportedPlatform)
        }

        pub fn open_child(&self, name: &str) -> Result<Self, ConfinementError> {
            Err(ConfinementError::UnsupportedPlatform)
        }

        pub fn open_child_or_create(
            &self,
            name: &str,
            mode: u32,
        ) -> Result<Self, ConfinementError> {
            Err(ConfinementError::UnsupportedPlatform)
        }

        pub fn create_child_dir_exclusive(
            &self,
            name: &str,
            mode: u32,
        ) -> Result<Self, ConfinementError> {
            Err(ConfinementError::UnsupportedPlatform)
        }

        pub fn kind_child(&self, name: &str) -> Result<Option<NodeKind>, ConfinementError> {
            Err(ConfinementError::UnsupportedPlatform)
        }

        pub fn child_identity(&self, name: &str) -> Result<Option<(u64, u64)>, ConfinementError> {
            Err(ConfinementError::UnsupportedPlatform)
        }

        pub fn dir_identity(&self) -> Result<(u64, u64), ConfinementError> {
            Err(ConfinementError::UnsupportedPlatform)
        }

        pub fn owner_and_mode(&self) -> Result<(u32, u32), ConfinementError> {
            Err(ConfinementError::UnsupportedPlatform)
        }

        pub fn sync(&self) -> Result<(), ConfinementError> {
            Err(ConfinementError::UnsupportedPlatform)
        }

        pub fn file_identity(file: &std::fs::File) -> Result<(u64, u64), ConfinementError> {
            Err(ConfinementError::UnsupportedPlatform)
        }

        pub fn remove_child_if(
            &self,
            name: &str,
            identity: (u64, u64),
        ) -> Result<bool, ConfinementError> {
            Err(ConfinementError::UnsupportedPlatform)
        }

        pub fn read_child(
            &self,
            name: &str,
            max_bytes: u64,
        ) -> Result<Option<Vec<u8>>, ConfinementError> {
            Err(ConfinementError::UnsupportedPlatform)
        }

        pub fn create_child_empty(
            &self,
            name: &str,
            mode: u32,
        ) -> Result<std::fs::File, ConfinementError> {
            Err(ConfinementError::UnsupportedPlatform)
        }

        pub fn create_child_file(
            &self,
            name: &str,
            bytes: &[u8],
            mode: u32,
        ) -> Result<std::fs::File, ConfinementError> {
            Err(ConfinementError::UnsupportedPlatform)
        }

        pub fn open_child_file_rw(&self, name: &str) -> Result<std::fs::File, ConfinementError> {
            Err(ConfinementError::UnsupportedPlatform)
        }

        pub fn open_child_file_ro(&self, name: &str) -> Result<std::fs::File, ConfinementError> {
            Err(ConfinementError::UnsupportedPlatform)
        }

        pub fn set_file_mode(file: &std::fs::File, mode: u32) -> Result<(), ConfinementError> {
            Err(ConfinementError::UnsupportedPlatform)
        }

        pub fn rename_child_noreplace(
            &self,
            from: &str,
            to_dir: &Dir,
            to: &str,
        ) -> Result<(), ConfinementError> {
            Err(ConfinementError::UnsupportedPlatform)
        }

        pub fn exchange_children(
            &self,
            from: &str,
            other: &Dir,
            to: &str,
        ) -> Result<(), ConfinementError> {
            Err(ConfinementError::UnsupportedPlatform)
        }

        pub fn link_child(
            &self,
            from: &str,
            to_dir: &Dir,
            to: &str,
        ) -> Result<(), ConfinementError> {
            Err(ConfinementError::UnsupportedPlatform)
        }

        pub fn rename_child(
            &self,
            from: &str,
            to_dir: &Dir,
            to: &str,
        ) -> Result<(), ConfinementError> {
            Err(ConfinementError::UnsupportedPlatform)
        }

        pub fn remove_child(&self, name: &str) -> Result<(), ConfinementError> {
            Err(ConfinementError::UnsupportedPlatform)
        }

        pub fn remove_child_directory(&self, name: &str) -> Result<(), ConfinementError> {
            Err(ConfinementError::UnsupportedPlatform)
        }

        pub fn remove_tree_child(&self, name: &str) -> Result<(), ConfinementError> {
            Err(ConfinementError::UnsupportedPlatform)
        }

        pub fn entries(&self) -> Result<Vec<String>, ConfinementError> {
            Err(ConfinementError::UnsupportedPlatform)
        }

        pub fn create_child_symlink(
            &self,
            target: &str,
            name: &str,
        ) -> Result<(), ConfinementError> {
            Err(ConfinementError::UnsupportedPlatform)
        }

        pub fn read_link_child(&self, name: &str) -> Result<Option<String>, ConfinementError> {
            Err(ConfinementError::UnsupportedPlatform)
        }

        pub fn child_mode(&self, name: &str) -> Result<Option<u32>, ConfinementError> {
            Err(ConfinementError::UnsupportedPlatform)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn the_supported_target_boundary_is_exactly_macos_and_linux() {
        assert_eq!(
            super::supported_target(),
            cfg!(any(target_os = "macos", target_os = "linux"))
        );
    }

    #[test]
    fn simple_names_are_enforced() {
        assert!(is_simple_name("config.toml"));
        assert!(is_simple_name("staging-123-456"));
        for bad in ["", ".", "..", "a/b", "a\\b", "a\0b", &"x".repeat(256)] {
            assert!(!is_simple_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_confined_root_reads_and_writes_only_its_own_children() {
        let root = tempfile::tempdir().expect("root");
        let dir = Dir::open_root(root.path()).expect("open");
        dir.open_child_or_create(".kontor-bootstrap", 0o700)
            .expect("internal");
        let internal = dir.open_child(".kontor-bootstrap").expect("internal");
        internal
            .create_child_file("journal.json", b"{}", 0o600)
            .expect("journal");
        assert_eq!(
            internal.read_child("journal.json", 1024).expect("read"),
            Some(b"{}".to_vec())
        );
        assert_eq!(
            internal.child_mode("journal.json").expect("mode"),
            Some(0o600)
        );
        for bad in ["../escape", "/etc/passwd", "a/b"] {
            assert_eq!(
                internal.read_child(bad, 1024).unwrap_err(),
                ConfinementError::UnsafeName
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_children_are_refused_at_every_step() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().expect("root");
        let outside = tempfile::tempdir().expect("outside");
        std::fs::write(outside.path().join("target"), "outside").expect("write");
        symlink(outside.path(), root.path().join("dir-link")).expect("dir link");
        symlink(outside.path().join("target"), root.path().join("file-link")).expect("file link");
        let dir = Dir::open_root(root.path()).expect("open");
        assert_eq!(
            dir.open_child("dir-link").unwrap_err(),
            ConfinementError::Symlink
        );
        assert_eq!(
            dir.read_child("file-link", 1024).unwrap_err(),
            ConfinementError::Symlink
        );
        assert_eq!(
            dir.kind_child("file-link").expect("kind"),
            Some(NodeKind::Symlink)
        );
        assert_eq!(
            std::fs::read_to_string(outside.path().join("target")).expect("read"),
            "outside"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_swapped_directory_cannot_redirect_a_held_operation() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().expect("root");
        let outside = tempfile::tempdir().expect("outside");
        let real = root.path().join("client");
        std::fs::create_dir(&real).expect("client");
        let moved = root.path().join("client-moved");

        let dir = Dir::open_root(root.path()).expect("open");
        let client = dir.open_child("client").expect("client dir");
        // The attacker swaps the checked path for a symlink after validation.
        std::fs::rename(&real, &moved).expect("move");
        symlink(&outside, &real).expect("swap symlink");

        // The held descriptor still addresses the original directory.
        client
            .create_child_file("config.json", b"{}", 0o600)
            .expect("write");
        assert!(moved.join("config.json").exists());
        assert!(
            std::fs::read_dir(outside.path())
                .expect("outside")
                .next()
                .is_none(),
            "nothing may land outside the held directory"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_root_is_refused() {
        use std::os::unix::fs::symlink;
        let holder = tempfile::tempdir().expect("holder");
        let outside = tempfile::tempdir().expect("outside");
        let link = holder.path().join("root-link");
        symlink(outside.path(), &link).expect("link");
        assert_eq!(
            Dir::open_root(&link).unwrap_err(),
            ConfinementError::Symlink
        );
    }

    #[test]
    fn a_relative_root_is_refused() {
        assert_eq!(
            Dir::open_root(Path::new("relative/root")).unwrap_err(),
            ConfinementError::UnsafeName
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_regular_name_replaced_by_a_fifo_before_open_is_refused() {
        let root = tempfile::tempdir().expect("root");
        let path = root.path().join("swappable");
        std::fs::write(&path, b"regular bytes").expect("regular");
        let seam_path = path.clone();
        super::unix::set_pre_open_seam(move |_| {
            std::fs::remove_file(&seam_path).expect("remove");
            let status = std::process::Command::new("mkfifo")
                .arg(&seam_path)
                .status()
                .expect("mkfifo");
            assert!(status.success());
        });
        let dir = Dir::open_root(root.path()).expect("open");
        assert_eq!(
            dir.read_child("swappable", 1024).unwrap_err(),
            ConfinementError::NotRegularFile
        );
        super::unix::set_pre_open_seam(|_| {});
    }

    #[cfg(unix)]
    #[test]
    fn a_file_grown_between_open_and_fstat_is_refused() {
        let root = tempfile::tempdir().expect("root");
        let path = root.path().join("growing-before-fstat");
        std::fs::write(&path, vec![b'x'; 16]).expect("write");
        let seam_path = path.clone();
        super::unix::set_pre_fstat_seam(move |_| {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(&seam_path)
                .expect("open");
            file.write_all(&vec![b'y'; 4096]).expect("grow");
        });
        let dir = Dir::open_root(root.path()).expect("open");
        super::unix::reset_consumed();
        assert_eq!(
            dir.read_child("growing-before-fstat", 64).unwrap_err(),
            ConfinementError::TooLarge
        );
        // The opened-fd size guard refuses before the bounded read consumes
        // anything: zero bytes cross the read seam. Deleting the size check
        // would let the max+1 take deliver bytes (65) before the post-read
        // bound, so this is a no-read discriminator for that single guard.
        // The recorded F7 fd-size status stays STATIC_MASKED/no-kill; this is
        // a strengthened candidate oracle, unexecuted until admission.
        assert_eq!(
            super::unix::consumed_bytes(),
            0,
            "the fd-size guard refuses before any read"
        );
        super::unix::set_pre_fstat_seam(|_| {});
        super::unix::reset_consumed();
    }

    #[cfg(unix)]
    #[test]
    fn a_file_grown_after_fstat_is_bounded_and_refused() {
        let root = tempfile::tempdir().expect("root");
        let path = root.path().join("growing");
        std::fs::write(&path, vec![b'x'; 64]).expect("write");
        let seam_path = path.clone();
        super::unix::set_pre_read_seam(move |_| {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(&seam_path)
                .expect("open");
            file.write_all(b"growth").expect("grow");
        });
        let dir = Dir::open_root(root.path()).expect("open");
        assert_eq!(
            dir.read_child("growing", 64).unwrap_err(),
            ConfinementError::TooLarge
        );
        super::unix::set_pre_read_seam(|_| {});
    }

    #[cfg(unix)]
    #[test]
    fn the_bounded_take_consumes_at_most_max_plus_one_bytes() {
        // F7-fd-bound sub-seed: deleting the bounded take leaves the post-read
        // bound intact but reads the whole grown file. The production operand
        // is the single `Read::take(max+1)` compiled in every build; the
        // test-only CountingReader wraps exactly that operand, so the counter
        // observes the same take the production path uses. It records bytes
        // delivered by the reader, not kernel/device I/O.
        let root = tempfile::tempdir().expect("root");
        let path = root.path().join("take-bound");
        std::fs::write(&path, vec![b'x'; 32]).expect("write");
        let seam_path = path.clone();
        super::unix::set_pre_read_seam(move |_| {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(&seam_path)
                .expect("open");
            file.write_all(&[b'y'; 96]).expect("grow");
        });
        let dir = Dir::open_root(root.path()).expect("open");
        super::unix::reset_consumed();
        assert_eq!(
            dir.read_child("take-bound", 64).unwrap_err(),
            ConfinementError::TooLarge
        );
        assert_eq!(
            super::unix::consumed_bytes(),
            65,
            "the one production max+1 take caps descriptor consumption"
        );
        super::unix::set_pre_read_seam(|_| {});
        super::unix::reset_consumed();
    }

    #[cfg(unix)]
    #[test]
    fn a_fifo_is_refused_without_blocking() {
        let root = tempfile::tempdir().expect("root");
        let status = std::process::Command::new("mkfifo")
            .arg(root.path().join("pipe"))
            .status()
            .expect("mkfifo");
        assert!(status.success());
        let dir = Dir::open_root(root.path()).expect("open");
        assert_eq!(
            dir.read_child("pipe", 1024).unwrap_err(),
            ConfinementError::NotRegularFile
        );
    }

    #[test]
    fn oversized_reads_are_refused() {
        let root = tempfile::tempdir().expect("root");
        let dir = Dir::open_root(root.path()).expect("open");
        dir.create_child_file("big", &[b'x'; 64], 0o600)
            .expect("write");
        assert_eq!(
            dir.read_child("big", 8).unwrap_err(),
            ConfinementError::TooLarge
        );
    }

    #[test]
    fn trees_are_removed_without_following_symlinks() {
        let root = tempfile::tempdir().expect("root");
        let dir = Dir::open_root(root.path()).expect("open");
        let tree = dir.open_child_or_create("tree", 0o700).expect("tree");
        tree.open_child_or_create("nested", 0o700).expect("nested");
        tree.create_child_file("file", b"x", 0o600).expect("file");
        drop(tree);
        dir.remove_tree_child("tree").expect("remove");
        assert_eq!(dir.kind_child("tree").expect("kind"), None);
    }

    #[test]
    fn document_bound_is_declared() {
        assert_eq!(MAX_DOCUMENT_BYTES, 4 * 1024 * 1024);
    }
}
