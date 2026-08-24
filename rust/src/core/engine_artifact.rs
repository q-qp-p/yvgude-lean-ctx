//! Contained, immutable storage for local Engine Interface artifacts.

use std::io::{Read, Seek, Write};
use std::path::{Component, Path};

#[cfg(test)]
use std::cell::Cell;

use sha2::{Digest, Sha256};

use crate::core::data_dir;

const ARTIFACT_BOUNDARY_REJECTED: &str = "engine_artifact_boundary_rejected";
const ARTIFACT_BOUNDARY_UNSUPPORTED: &str = "engine_artifact_boundary_unsupported";
const ARTIFACT_DIRECTORY_OPEN_FAILED: &str = "engine_artifact_directory_open_failed";
#[cfg(unix)]
const ARTIFACT_DIRECTORY_CREATE_FAILED: &str = "engine_artifact_directory_create_failed";
const ARTIFACT_TEMP_CREATE_FAILED: &str = "engine_artifact_temp_create_failed";
const ARTIFACT_WRITE_FAILED: &str = "engine_artifact_write_failed";
#[cfg(unix)]
const ARTIFACT_PERMISSIONS_FAILED: &str = "engine_artifact_permissions_failed";
const ARTIFACT_SYNC_FAILED: &str = "engine_artifact_sync_failed";
const ARTIFACT_PUBLISH_FAILED: &str = "engine_artifact_publish_failed";
const ARTIFACT_PUBLISH_UNSUPPORTED: &str = "engine_artifact_publish_unsupported";
const ARTIFACT_LEAF_UNTRUSTED: &str = "engine_artifact_leaf_untrusted";
const ARTIFACT_DIGEST_MISMATCH: &str = "engine_artifact_digest_mismatch";
const ARTIFACT_CLEANUP_FAILED: &str = "engine_artifact_cleanup_failed";
const ARTIFACT_SIZE_LIMIT_EXCEEDED: &str = "engine_artifact_size_limit_exceeded";

#[cfg(test)]
thread_local! {
    static TEST_FAIL_BEFORE_PUBLISH: Cell<bool> = const { Cell::new(false) };
    static TEST_FAIL_CAPABILITY_PREFLIGHT: Cell<bool> = const { Cell::new(false) };
    #[cfg(windows)]
    static TEST_FAIL_TEMP_VALIDATION: Cell<bool> = const { Cell::new(false) };
}

#[cfg(test)]
pub(super) fn inject_test_pre_publish_failure() {
    TEST_FAIL_BEFORE_PUBLISH.with(|failpoint| failpoint.set(true));
}

#[cfg(test)]
pub(super) fn inject_test_capability_preflight_failure() {
    TEST_FAIL_CAPABILITY_PREFLIGHT.with(|failpoint| failpoint.set(true));
}

#[cfg(all(test, windows))]
pub(super) fn inject_test_temp_validation_failure() {
    TEST_FAIL_TEMP_VALIDATION.with(|failpoint| failpoint.set(true));
}

fn test_pre_publish_failure() -> bool {
    #[cfg(test)]
    {
        TEST_FAIL_BEFORE_PUBLISH.with(|failpoint| failpoint.replace(false))
    }
    #[cfg(not(test))]
    {
        false
    }
}

fn test_capability_preflight_failure() -> bool {
    #[cfg(test)]
    {
        TEST_FAIL_CAPABILITY_PREFLIGHT.with(|failpoint| failpoint.replace(false))
    }
    #[cfg(not(test))]
    {
        false
    }
}

#[cfg(windows)]
fn test_temp_validation_failure() -> bool {
    #[cfg(test)]
    {
        TEST_FAIL_TEMP_VALIDATION.with(|failpoint| failpoint.replace(false))
    }
    #[cfg(not(test))]
    {
        false
    }
}

pub(super) fn persist_content(
    directory: &str,
    digest: &str,
    extension: &str,
    bytes: &[u8],
) -> Result<std::fs::File, String> {
    persist_content_checked(directory, digest, extension, bytes, None, None)
}

/// Read one integrity-addressed artifact through descriptor/handle-relative,
/// no-follow traversal of the configured Engine data root.
pub(super) fn read_content(
    directory: &str,
    digest: &str,
    extension: &str,
) -> Result<Vec<u8>, String> {
    validate_artifact_name(digest, extension)?;
    let configured_root =
        data_dir::lean_ctx_data_dir().map_err(|_| ARTIFACT_BOUNDARY_REJECTED.to_owned())?;
    let bytes = read_bounded_content(&configured_root, directory, digest, extension, 1024 * 1024)?;
    if hex_sha256(&bytes) != digest {
        return Err(ARTIFACT_DIGEST_MISMATCH.to_owned());
    }
    Ok(bytes)
}

pub(super) fn read_bounded_content(
    configured_root: &Path,
    directory: &str,
    digest: &str,
    extension: &str,
    max_bytes: usize,
) -> Result<Vec<u8>, String> {
    validate_artifact_name(digest, extension)?;
    if max_bytes == 0 {
        return Err(ARTIFACT_BOUNDARY_REJECTED.to_owned());
    }

    #[cfg(unix)]
    {
        unix::read_bounded_content(configured_root, directory, digest, extension, max_bytes)
    }

    #[cfg(windows)]
    {
        windows::read_bounded_content(configured_root, directory, digest, extension, max_bytes)
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = (configured_root, directory, digest, extension, max_bytes);
        Err(ARTIFACT_BOUNDARY_UNSUPPORTED.to_owned())
    }
}

#[cfg(all(test, any(unix, windows)))]
pub(super) fn persist_content_with_test_barrier(
    directory: &str,
    digest: &str,
    extension: &str,
    bytes: &[u8],
    barrier: Box<dyn FnOnce()>,
) -> Result<std::fs::File, String> {
    persist_content_checked(directory, digest, extension, bytes, Some(barrier), None)
}

#[cfg(all(test, unix))]
pub(super) fn persist_content_with_test_publish_barrier(
    directory: &str,
    digest: &str,
    extension: &str,
    bytes: &[u8],
    barrier: Box<dyn FnOnce()>,
) -> Result<std::fs::File, String> {
    persist_content_checked(directory, digest, extension, bytes, None, Some(barrier))
}

fn persist_content_checked(
    directory: &str,
    digest: &str,
    extension: &str,
    bytes: &[u8],
    bind_barrier: Option<Box<dyn FnOnce()>>,
    publish_barrier: Option<Box<dyn FnOnce()>>,
) -> Result<std::fs::File, String> {
    validate_artifact_name(digest, extension)?;
    if hex_sha256(bytes) != digest {
        return Err(ARTIFACT_DIGEST_MISMATCH.to_owned());
    }

    let configured_root =
        data_dir::lean_ctx_data_dir().map_err(|_| ARTIFACT_BOUNDARY_REJECTED.to_owned())?;

    #[cfg(unix)]
    {
        unix::persist_content(
            &configured_root,
            directory,
            digest,
            extension,
            bytes,
            bind_barrier,
            publish_barrier,
        )
    }

    #[cfg(windows)]
    {
        windows::persist_content(
            &configured_root,
            directory,
            digest,
            extension,
            bytes,
            bind_barrier,
            publish_barrier,
        )
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = (
            configured_root,
            directory,
            digest,
            extension,
            bytes,
            bind_barrier,
            publish_barrier,
        );
        Err(ARTIFACT_BOUNDARY_UNSUPPORTED.to_owned())
    }
}

fn validate_artifact_name(digest: &str, extension: &str) -> Result<(), String> {
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ARTIFACT_BOUNDARY_REJECTED.to_owned());
    }
    if !matches!(extension, "json" | "txt") {
        return Err(ARTIFACT_BOUNDARY_REJECTED.to_owned());
    }
    Ok(())
}

fn hex_sha256(bytes: &[u8]) -> String {
    crate::core::agent_identity::hex_encode(&Sha256::digest(bytes))
}

#[cfg(unix)]
#[allow(clippy::wildcard_imports)]
mod unix {
    use super::*;
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, FromRawFd, RawFd};
    use std::os::unix::ffi::OsStrExt;

    const ROOT_FLAGS: libc::c_int =
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW;
    const DIRECTORY_FLAGS: libc::c_int =
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW;
    const LEAF_READ_FLAGS: libc::c_int =
        libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK;
    const TEMP_FLAGS: libc::c_int =
        libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC | libc::O_NOFOLLOW;

    struct ArtifactDirectory {
        _root: std::fs::File,
        final_dir: std::fs::File,
    }

    struct TempArtifact {
        file: Option<std::fs::File>,
        directory_fd: RawFd,
        name: CString,
        disarmed: bool,
    }

    impl TempArtifact {
        fn cleanup(&mut self) -> Result<(), String> {
            if self.disarmed {
                return Ok(());
            }
            self.file.take();
            // SAFETY: directory_fd remains held; name is NUL-terminated.
            let result = unsafe { libc::unlinkat(self.directory_fd, self.name.as_ptr(), 0) };
            if result == 0 || errno() == libc::ENOENT {
                self.disarmed = true;
                Ok(())
            } else {
                Err(ARTIFACT_CLEANUP_FAILED.to_owned())
            }
        }
    }

    impl Drop for TempArtifact {
        fn drop(&mut self) {
            if !self.disarmed {
                self.file.take();
                // SAFETY: directory_fd remains held; name is NUL-terminated.
                unsafe {
                    let _ = libc::unlinkat(self.directory_fd, self.name.as_ptr(), 0);
                }
            }
        }
    }

    pub(super) fn persist_content(
        configured_root: &Path,
        relative: &str,
        digest: &str,
        extension: &str,
        bytes: &[u8],
        bind_barrier: Option<Box<dyn FnOnce()>>,
        publish_barrier: Option<Box<dyn FnOnce()>>,
    ) -> Result<std::fs::File, String> {
        let root_components = validate_absolute_components(configured_root)?;
        let relative_components = validate_relative_components(relative)?;
        let final_name = cstring(format!("{digest}.{extension}"))?;
        let directories = bind_directories(&root_components, &relative_components, &final_name)?;

        if let Some(barrier) = bind_barrier {
            barrier();
        }

        let mut temp = create_temp_artifact(&directories.final_dir, &final_name)?;
        if let Err(error) = write_temp_artifact(&mut temp, bytes) {
            return Err(temp.cleanup().err().unwrap_or(error));
        }
        if let Some(barrier) = publish_barrier {
            barrier();
        }

        if super::test_pre_publish_failure() {
            temp.cleanup()?;
            return Err("engine_artifact_test_pre_publish_failure".to_owned());
        }

        publish_temp_artifact(
            &mut temp,
            directories.final_dir.as_raw_fd(),
            &final_name,
            digest,
        )
    }

    pub(super) fn read_bounded_content(
        configured_root: &Path,
        relative: &str,
        digest: &str,
        extension: &str,
        max_bytes: usize,
    ) -> Result<Vec<u8>, String> {
        let root_components = validate_absolute_components(configured_root)?;
        let relative_components = validate_relative_components(relative)?;
        let final_name = cstring(format!("{digest}.{extension}"))?;
        let final_dir = bind_existing_directory(&root_components, &relative_components)?;
        read_existing_artifact_bounded(final_dir.as_raw_fd(), &final_name, max_bytes)
    }

    fn validate_absolute_components(path: &Path) -> Result<Vec<CString>, String> {
        if !path.is_absolute() {
            return Err(ARTIFACT_BOUNDARY_REJECTED.to_owned());
        }
        #[cfg(target_os = "macos")]
        let normalized = normalize_macos_root_alias(path);
        #[cfg(target_os = "macos")]
        let path = normalized.as_path();
        let mut names = Vec::new();
        for component in path.components() {
            match component {
                Component::RootDir => {}
                Component::Normal(name) => names.push(
                    CString::new(name.as_bytes())
                        .map_err(|_| ARTIFACT_BOUNDARY_REJECTED.to_owned())?,
                ),
                _ => return Err(ARTIFACT_BOUNDARY_REJECTED.to_owned()),
            }
        }
        if names.is_empty() {
            return Err(ARTIFACT_BOUNDARY_REJECTED.to_owned());
        }
        Ok(names)
    }

    #[cfg(target_os = "macos")]
    fn normalize_macos_root_alias(path: &Path) -> std::path::PathBuf {
        for (alias, target) in [
            (Path::new("/var"), Path::new("/private/var")),
            (Path::new("/tmp"), Path::new("/private/tmp")),
            (Path::new("/etc"), Path::new("/private/etc")),
        ] {
            if let Ok(suffix) = path.strip_prefix(alias) {
                return target.join(suffix);
            }
        }
        path.to_path_buf()
    }

    fn validate_relative_components(path: &str) -> Result<Vec<CString>, String> {
        let mut names = Vec::new();
        for component in Path::new(path).components() {
            let Component::Normal(name) = component else {
                return Err(ARTIFACT_BOUNDARY_REJECTED.to_owned());
            };
            names.push(
                CString::new(name.as_bytes()).map_err(|_| ARTIFACT_BOUNDARY_REJECTED.to_owned())?,
            );
        }
        if names.is_empty() {
            return Err(ARTIFACT_BOUNDARY_REJECTED.to_owned());
        }
        Ok(names)
    }

    fn bind_directories(
        root_components: &[CString],
        relative_components: &[CString],
        final_name: &CString,
    ) -> Result<ArtifactDirectory, String> {
        let slash = CString::new("/").expect("static root path");
        // SAFETY: slash is NUL-terminated; File owns the successful descriptor.
        let anchor_fd = unsafe { libc::open(slash.as_ptr(), ROOT_FLAGS) };
        if anchor_fd < 0 {
            return Err(ARTIFACT_DIRECTORY_OPEN_FAILED.to_owned());
        }
        // SAFETY: anchor_fd is a successful descriptor owned immediately.
        let mut handles = vec![unsafe { std::fs::File::from_raw_fd(anchor_fd) }];
        let names: Vec<&CString> = root_components
            .iter()
            .chain(relative_components.iter())
            .collect();
        let mut opened = 0usize;

        for name in &names {
            let parent = handles
                .last()
                .ok_or_else(|| ARTIFACT_DIRECTORY_OPEN_FAILED.to_owned())?;
            match open_existing_directory_at(parent.as_raw_fd(), name)? {
                Some(child) => {
                    handles.push(child);
                    opened += 1;
                }
                None => break,
            }
        }

        let existing_fd = handles
            .last()
            .ok_or_else(|| ARTIFACT_DIRECTORY_OPEN_FAILED.to_owned())?
            .as_raw_fd();
        validate_component_lengths(existing_fd, &names, final_name)?;
        preflight_publication(existing_fd)?;

        for handle in handles.iter().skip(root_components.len()) {
            chmod_directory(handle.as_raw_fd())?;
        }

        for (index, name) in names.iter().enumerate().skip(opened) {
            let parent_fd = handles
                .last()
                .ok_or_else(|| ARTIFACT_DIRECTORY_OPEN_FAILED.to_owned())?
                .as_raw_fd();
            let child = open_or_create_directory_at(parent_fd, name)?;
            if index + 1 >= root_components.len() {
                chmod_directory(child.as_raw_fd())?;
            }
            handles.push(child);
        }

        let root = handles
            .get(root_components.len())
            .ok_or_else(|| ARTIFACT_DIRECTORY_OPEN_FAILED.to_owned())?
            .try_clone()
            .map_err(|_| ARTIFACT_DIRECTORY_OPEN_FAILED.to_owned())?;
        let final_dir = handles
            .last()
            .ok_or_else(|| ARTIFACT_DIRECTORY_OPEN_FAILED.to_owned())?
            .try_clone()
            .map_err(|_| ARTIFACT_DIRECTORY_OPEN_FAILED.to_owned())?;
        Ok(ArtifactDirectory {
            _root: root,
            final_dir,
        })
    }

    fn bind_existing_directory(
        root_components: &[CString],
        relative_components: &[CString],
    ) -> Result<std::fs::File, String> {
        let slash = CString::new("/").expect("static root path");
        // SAFETY: slash is NUL-terminated; File owns the successful descriptor.
        let anchor_fd = unsafe { libc::open(slash.as_ptr(), ROOT_FLAGS) };
        if anchor_fd < 0 {
            return Err(ARTIFACT_DIRECTORY_OPEN_FAILED.to_owned());
        }
        // SAFETY: anchor_fd is a successful descriptor owned immediately.
        let mut directory = unsafe { std::fs::File::from_raw_fd(anchor_fd) };
        for name in root_components.iter().chain(relative_components) {
            directory = open_existing_directory_at(directory.as_raw_fd(), name)?
                .ok_or_else(|| ARTIFACT_DIRECTORY_OPEN_FAILED.to_owned())?;
        }
        Ok(directory)
    }

    fn validate_component_lengths(
        directory_fd: RawFd,
        names: &[&CString],
        final_name: &CString,
    ) -> Result<(), String> {
        // SAFETY: directory_fd is a live descriptor and _PC_NAME_MAX is a
        // side-effect-free limit query for the target filesystem.
        let name_max = unsafe { libc::fpathconf(directory_fd, libc::_PC_NAME_MAX) };
        let name_max =
            usize::try_from(name_max).map_err(|_| ARTIFACT_BOUNDARY_UNSUPPORTED.to_owned())?;
        let longest_temp = final_name
            .as_bytes()
            .len()
            .checked_add("..tmp.127".len())
            .ok_or_else(|| ARTIFACT_BOUNDARY_REJECTED.to_owned())?;
        if names.iter().any(|name| name.as_bytes().len() > name_max)
            || final_name.as_bytes().len() > name_max
            || longest_temp > name_max
        {
            return Err(ARTIFACT_BOUNDARY_REJECTED.to_owned());
        }
        Ok(())
    }

    fn open_existing_directory_at(
        parent_fd: RawFd,
        name: &CString,
    ) -> Result<Option<std::fs::File>, String> {
        // SAFETY: parent_fd is held and name is NUL-terminated.
        let child_fd = unsafe { libc::openat(parent_fd, name.as_ptr(), DIRECTORY_FLAGS) };
        if child_fd >= 0 {
            // SAFETY: child_fd is a successful descriptor owned immediately.
            return Ok(Some(unsafe { std::fs::File::from_raw_fd(child_fd) }));
        }
        match errno() {
            libc::ENOENT => Ok(None),
            libc::ELOOP | libc::ENOTDIR => Err(ARTIFACT_BOUNDARY_REJECTED.to_owned()),
            _ => Err(ARTIFACT_DIRECTORY_OPEN_FAILED.to_owned()),
        }
    }

    fn open_or_create_directory_at(
        parent_fd: RawFd,
        name: &CString,
    ) -> Result<std::fs::File, String> {
        // SAFETY: parent_fd is held and name is NUL-terminated.
        let mut child_fd = unsafe { libc::openat(parent_fd, name.as_ptr(), DIRECTORY_FLAGS) };
        if child_fd < 0 && errno() == libc::ENOENT {
            // SAFETY: parent_fd is held and name is NUL-terminated.
            let created = unsafe { libc::mkdirat(parent_fd, name.as_ptr(), 0o700) };
            if created != 0 && errno() != libc::EEXIST {
                return Err(ARTIFACT_DIRECTORY_CREATE_FAILED.to_owned());
            }
            // SAFETY: parent_fd is held and name is NUL-terminated.
            child_fd = unsafe { libc::openat(parent_fd, name.as_ptr(), DIRECTORY_FLAGS) };
        }
        if child_fd < 0 {
            return Err(if matches!(errno(), libc::ELOOP | libc::ENOTDIR) {
                ARTIFACT_BOUNDARY_REJECTED.to_owned()
            } else {
                ARTIFACT_DIRECTORY_OPEN_FAILED.to_owned()
            });
        }
        // SAFETY: child_fd is a successful descriptor owned immediately.
        Ok(unsafe { std::fs::File::from_raw_fd(child_fd) })
    }

    fn preflight_publication(directory_fd: RawFd) -> Result<(), String> {
        if super::test_capability_preflight_failure() {
            return Err(ARTIFACT_PUBLISH_UNSUPPORTED.to_owned());
        }
        let mut probe = None;
        for suffix in 0..128u16 {
            let name = cstring(format!(".leanctx-engine-capability-probe-{suffix}"))?;
            // SAFETY: directory_fd is held and name is NUL-terminated.
            let fd = unsafe {
                libc::openat(
                    directory_fd,
                    name.as_ptr(),
                    libc::O_CREAT
                        | libc::O_EXCL
                        | libc::O_WRONLY
                        | libc::O_CLOEXEC
                        | libc::O_NOFOLLOW,
                    0o600,
                )
            };
            if fd >= 0 {
                // SAFETY: fd is a successful descriptor owned immediately.
                probe = Some((unsafe { std::fs::File::from_raw_fd(fd) }, name));
                break;
            }
            if errno() != libc::EEXIST {
                return Err(ARTIFACT_PUBLISH_UNSUPPORTED.to_owned());
            }
        }
        let (probe_file, probe_name) =
            probe.ok_or_else(|| ARTIFACT_PUBLISH_UNSUPPORTED.to_owned())?;

        #[cfg(target_os = "linux")]
        let result = unsafe {
            libc::renameat2(
                directory_fd,
                probe_name.as_ptr(),
                directory_fd,
                probe_name.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        #[cfg(target_os = "macos")]
        // SAFETY: directory_fd is held and both probe names are NUL-terminated.
        let result = unsafe {
            libc::renameatx_np(
                directory_fd,
                probe_name.as_ptr(),
                directory_fd,
                probe_name.as_ptr(),
                libc::RENAME_EXCL,
            )
        };
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        return Err(ARTIFACT_PUBLISH_UNSUPPORTED.to_owned());

        let supported = result == 0 || errno() == libc::EEXIST;
        drop(probe_file);
        let cleanup = unlink_name(directory_fd, &probe_name);
        if !supported || cleanup.is_err() {
            return Err(ARTIFACT_PUBLISH_UNSUPPORTED.to_owned());
        }
        Ok(())
    }

    fn create_temp_artifact(
        directory: &std::fs::File,
        final_name: &CString,
    ) -> Result<TempArtifact, String> {
        for suffix in 0..128u16 {
            let name = if suffix == 0 {
                cstring(format!(".{}.tmp", final_name.to_string_lossy()))?
            } else {
                cstring(format!(".{}.tmp.{suffix}", final_name.to_string_lossy()))?
            };
            // SAFETY: directory fd is held; name is NUL-terminated.
            let fd =
                unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), TEMP_FLAGS, 0o600) };
            if fd < 0 {
                if errno() == libc::EEXIST {
                    continue;
                }
                return Err(ARTIFACT_TEMP_CREATE_FAILED.to_owned());
            }
            // SAFETY: fd is a successful descriptor owned immediately.
            let file = unsafe { std::fs::File::from_raw_fd(fd) };
            return Ok(TempArtifact {
                file: Some(file),
                directory_fd: directory.as_raw_fd(),
                name,
                disarmed: false,
            });
        }
        Err(ARTIFACT_TEMP_CREATE_FAILED.to_owned())
    }

    fn write_temp_artifact(temp: &mut TempArtifact, bytes: &[u8]) -> Result<(), String> {
        let file = temp
            .file
            .as_mut()
            .ok_or_else(|| ARTIFACT_WRITE_FAILED.to_owned())?;
        chmod_file(file.as_raw_fd())?;
        file.write_all(bytes)
            .map_err(|_| ARTIFACT_WRITE_FAILED.to_owned())?;
        file.sync_all()
            .map_err(|_| ARTIFACT_SYNC_FAILED.to_owned())?;
        Ok(())
    }

    fn publish_temp_artifact(
        temp: &mut TempArtifact,
        directory_fd: RawFd,
        final_name: &CString,
        digest: &str,
    ) -> Result<std::fs::File, String> {
        let temp_file = temp
            .file
            .as_ref()
            .ok_or_else(|| ARTIFACT_PUBLISH_FAILED.to_owned())?;
        if !named_entry_matches_file(temp_file, directory_fd, &temp.name) {
            temp.cleanup()?;
            return Err(ARTIFACT_LEAF_UNTRUSTED.to_owned());
        }

        #[cfg(target_os = "linux")]
        let published = unsafe {
            libc::renameat2(
                directory_fd,
                temp.name.as_ptr(),
                directory_fd,
                final_name.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        #[cfg(target_os = "macos")]
        // SAFETY: directory_fd is held and both names are NUL-terminated.
        let published = unsafe {
            libc::renameatx_np(
                directory_fd,
                temp.name.as_ptr(),
                directory_fd,
                final_name.as_ptr(),
                libc::RENAME_EXCL,
            )
        };
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        let published = -1;

        if published == 0 {
            if !named_entry_matches_file(temp_file, directory_fd, final_name) {
                let cleanup = unlink_name(directory_fd, final_name);
                temp.disarmed = true;
                return Err(cleanup
                    .err()
                    .unwrap_or_else(|| ARTIFACT_LEAF_UNTRUSTED.to_owned()));
            }
            temp.disarmed = true;
            return finish_published(temp, directory_fd);
        }
        if errno() == libc::EEXIST {
            return verify_collision(temp, directory_fd, final_name, digest);
        }
        temp.cleanup()?;
        Err(ARTIFACT_PUBLISH_FAILED.to_owned())
    }

    fn named_entry_matches_file(file: &std::fs::File, directory_fd: RawFd, name: &CString) -> bool {
        // SAFETY: zeroed is a valid initial state for libc::stat output buffers.
        let mut held: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: file descriptor is live and held points to writable storage.
        if unsafe { libc::fstat(file.as_raw_fd(), &raw mut held) } != 0 {
            return false;
        }
        // SAFETY: zeroed is a valid initial state for libc::stat output buffers.
        let mut named: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: directory is held, name is NUL-terminated, and named is writable.
        if unsafe {
            libc::fstatat(
                directory_fd,
                name.as_ptr(),
                &raw mut named,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } != 0
        {
            return false;
        }
        held.st_dev == named.st_dev
            && held.st_ino == named.st_ino
            && (named.st_mode & libc::S_IFMT) == libc::S_IFREG
    }

    fn unlink_name(directory_fd: RawFd, name: &CString) -> Result<(), String> {
        // SAFETY: directory is held and name is NUL-terminated.
        let result = unsafe { libc::unlinkat(directory_fd, name.as_ptr(), 0) };
        if result == 0 || errno() == libc::ENOENT {
            Ok(())
        } else {
            Err(ARTIFACT_CLEANUP_FAILED.to_owned())
        }
    }

    fn verify_collision(
        temp: &mut TempArtifact,
        directory_fd: RawFd,
        final_name: &CString,
        digest: &str,
    ) -> Result<std::fs::File, String> {
        temp.cleanup()?;
        sync_directory(directory_fd)?;
        verify_existing_artifact(directory_fd, final_name, digest)
    }

    fn finish_published(
        temp: &mut TempArtifact,
        directory_fd: RawFd,
    ) -> Result<std::fs::File, String> {
        sync_directory(directory_fd)?;
        let mut file = temp
            .file
            .take()
            .ok_or_else(|| ARTIFACT_PUBLISH_FAILED.to_owned())?;
        file.rewind().map_err(|_| ARTIFACT_SYNC_FAILED.to_owned())?;
        Ok(file)
    }

    fn verify_existing_artifact(
        directory_fd: RawFd,
        final_name: &CString,
        digest: &str,
    ) -> Result<std::fs::File, String> {
        // SAFETY: directory_fd is held and final_name is NUL-terminated.
        let fd = unsafe { libc::openat(directory_fd, final_name.as_ptr(), LEAF_READ_FLAGS) };
        if fd < 0 {
            return Err(ARTIFACT_LEAF_UNTRUSTED.to_owned());
        }
        // SAFETY: fd is a successful descriptor owned immediately.
        let mut artifact = unsafe { std::fs::File::from_raw_fd(fd) };
        if !artifact
            .metadata()
            .map_err(|_| ARTIFACT_LEAF_UNTRUSTED.to_owned())?
            .is_file()
        {
            return Err(ARTIFACT_LEAF_UNTRUSTED.to_owned());
        }
        let mut bytes = Vec::new();
        artifact
            .read_to_end(&mut bytes)
            .map_err(|_| ARTIFACT_LEAF_UNTRUSTED.to_owned())?;
        if hex_sha256(&bytes) != digest {
            return Err(ARTIFACT_DIGEST_MISMATCH.to_owned());
        }
        chmod_file(artifact.as_raw_fd())?;
        artifact
            .rewind()
            .map_err(|_| ARTIFACT_LEAF_UNTRUSTED.to_owned())?;
        Ok(artifact)
    }

    fn read_existing_artifact_bounded(
        directory_fd: RawFd,
        final_name: &CString,
        max_bytes: usize,
    ) -> Result<Vec<u8>, String> {
        // SAFETY: directory_fd is held and final_name is NUL-terminated.
        let fd = unsafe { libc::openat(directory_fd, final_name.as_ptr(), LEAF_READ_FLAGS) };
        if fd < 0 {
            return Err(ARTIFACT_LEAF_UNTRUSTED.to_owned());
        }
        // SAFETY: fd is a successful descriptor owned immediately.
        let artifact = unsafe { std::fs::File::from_raw_fd(fd) };
        let metadata = artifact
            .metadata()
            .map_err(|_| ARTIFACT_LEAF_UNTRUSTED.to_owned())?;
        if !metadata.is_file() {
            return Err(ARTIFACT_LEAF_UNTRUSTED.to_owned());
        }
        if metadata.len() > max_bytes as u64 {
            return Err(ARTIFACT_SIZE_LIMIT_EXCEEDED.to_owned());
        }
        let read_limit = max_bytes
            .checked_add(1)
            .ok_or_else(|| ARTIFACT_BOUNDARY_REJECTED.to_owned())? as u64;
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        artifact
            .take(read_limit)
            .read_to_end(&mut bytes)
            .map_err(|_| ARTIFACT_LEAF_UNTRUSTED.to_owned())?;
        if bytes.len() > max_bytes {
            return Err(ARTIFACT_SIZE_LIMIT_EXCEEDED.to_owned());
        }
        Ok(bytes)
    }

    fn chmod_directory(fd: RawFd) -> Result<(), String> {
        // SAFETY: fd is a live directory descriptor owned by this function.
        if unsafe { libc::fchmod(fd, 0o700) } != 0 {
            Err(ARTIFACT_PERMISSIONS_FAILED.to_owned())
        } else {
            Ok(())
        }
    }

    fn chmod_file(fd: RawFd) -> Result<(), String> {
        // SAFETY: fd is a live regular-file descriptor owned by this function.
        if unsafe { libc::fchmod(fd, 0o600) } != 0 {
            Err(ARTIFACT_PERMISSIONS_FAILED.to_owned())
        } else {
            Ok(())
        }
    }

    fn sync_directory(fd: RawFd) -> Result<(), String> {
        // SAFETY: fd is a live descriptor held for the directory lifetime.
        if unsafe { libc::fsync(fd) } != 0 {
            Err(ARTIFACT_SYNC_FAILED.to_owned())
        } else {
            Ok(())
        }
    }

    fn cstring(value: String) -> Result<CString, String> {
        CString::new(value).map_err(|_| ARTIFACT_BOUNDARY_REJECTED.to_owned())
    }

    fn errno() -> i32 {
        std::io::Error::last_os_error().raw_os_error().unwrap_or(-1)
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use std::path::PathBuf;
    use std::ptr::{null, null_mut};
    use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
    use windows_sys::Wdk::Storage::FileSystem::{
        FILE_CREATE, FILE_DELETE_ON_CLOSE, FILE_DIRECTORY_FILE, FILE_DISPOSITION_DELETE,
        FILE_DISPOSITION_INFORMATION_EX, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_IF,
        FILE_OPEN_REPARSE_POINT, FILE_RENAME_INFORMATION, FILE_SYNCHRONOUS_IO_NONALERT,
        FileDispositionInformationEx, FileRenameInformationEx, NtCreateFile, NtSetInformationFile,
    };
    use windows_sys::Win32::Foundation::{
        ERROR_INVALID_FUNCTION, ERROR_NOT_SUPPORTED, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
        OBJ_CASE_INSENSITIVE, OBJ_DONT_REPARSE, STATUS_INVALID_PARAMETER, STATUS_NOT_SUPPORTED,
        STATUS_OBJECT_NAME_COLLISION, STATUS_OBJECT_NAME_EXISTS, STATUS_OBJECT_NAME_NOT_FOUND,
        STATUS_OBJECT_PATH_NOT_FOUND, STATUS_REPARSE_POINT_ENCOUNTERED, STATUS_SUCCESS,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, DELETE, FILE_ADD_FILE, FILE_ADD_SUBDIRECTORY, FILE_ATTRIBUTE_DIRECTORY,
        FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO,
        FILE_DELETE_CHILD, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_LIST_DIRECTORY, FILE_NAME_NORMALIZED, FILE_READ_ATTRIBUTES, FILE_READ_DATA,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_STANDARD_INFO,
        FILE_WRITE_ATTRIBUTES, FILE_WRITE_DATA, FileAttributeTagInfo, FileStandardInfo,
        FlushFileBuffers, GetFileInformationByHandleEx, GetFinalPathNameByHandleW, OPEN_EXISTING,
        SYNCHRONIZE, VOLUME_NAME_DOS,
    };
    use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

    const TRAVERSE_DIRECTORY_ACCESS: u32 = FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE;
    const DIRECTORY_ACCESS: u32 = TRAVERSE_DIRECTORY_ACCESS
        | FILE_ADD_FILE
        | FILE_ADD_SUBDIRECTORY
        | FILE_DELETE_CHILD
        | GENERIC_WRITE;
    const TEMP_ACCESS: u32 = FILE_READ_DATA
        | FILE_WRITE_DATA
        | FILE_READ_ATTRIBUTES
        | FILE_WRITE_ATTRIBUTES
        | DELETE
        | SYNCHRONIZE;
    const LEAF_ACCESS: u32 =
        FILE_READ_DATA | FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES | SYNCHRONIZE;
    const READ_LEAF_ACCESS: u32 = FILE_READ_DATA | FILE_READ_ATTRIBUTES | SYNCHRONIZE;

    struct ArtifactDirectory {
        _root: std::fs::File,
        final_dir: std::fs::File,
    }

    struct TempArtifact {
        file: Option<std::fs::File>,
        disarmed: bool,
    }

    impl TempArtifact {
        fn cleanup(&mut self) -> Result<(), String> {
            if self.disarmed {
                return Ok(());
            }
            if let Some(file) = self.file.as_ref() {
                mark_delete(file)?;
            }
            self.file.take();
            self.disarmed = true;
            Ok(())
        }
    }

    impl Drop for TempArtifact {
        fn drop(&mut self) {
            if !self.disarmed {
                if let Some(file) = self.file.as_ref() {
                    let _ = mark_delete(file);
                }
                self.file.take();
            }
        }
    }

    pub(super) fn persist_content(
        configured_root: &Path,
        relative: &str,
        digest: &str,
        extension: &str,
        bytes: &[u8],
        bind_barrier: Option<Box<dyn FnOnce()>>,
        publish_barrier: Option<Box<dyn FnOnce()>>,
    ) -> Result<std::fs::File, String> {
        let (anchor, root_components) = split_windows_root(configured_root)?;
        let relative_components = validate_relative_components(relative)?;
        let final_name = wide_component(&format!("{digest}.{extension}"))?;
        let root = bind_root(&anchor, &root_components)?;
        let directories = prepare_directory(&root, &relative_components)?;
        if let Some(barrier) = bind_barrier {
            barrier();
        }
        let mut temp = create_temp_artifact(&directories.final_dir, &final_name)?;
        if let Err(error) = write_temp_artifact(&mut temp, bytes) {
            return Err(temp.cleanup().err().unwrap_or(error));
        }
        if let Some(barrier) = publish_barrier {
            barrier();
        }
        if super::test_pre_publish_failure() {
            temp.cleanup()?;
            return Err("engine_artifact_test_pre_publish_failure".to_owned());
        }
        publish_temp_artifact(&mut temp, &directories.final_dir, &final_name, digest)
    }

    pub(super) fn read_bounded_content(
        configured_root: &Path,
        relative: &str,
        digest: &str,
        extension: &str,
        max_bytes: usize,
    ) -> Result<Vec<u8>, String> {
        let (anchor, root_components) = split_windows_root(configured_root)?;
        let relative_components = validate_relative_components(relative)?;
        let final_name = wide_component(&format!("{digest}.{extension}"))?;
        let root = bind_root_existing(&anchor, &root_components)?;
        let directories = prepare_existing_directory(&root, &relative_components)?;
        read_existing_artifact_bounded(&directories.final_dir, &final_name, max_bytes)
    }

    fn bind_root(anchor: &Path, components: &[Vec<u16>]) -> Result<std::fs::File, String> {
        let name = wide_path(anchor);
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                TRAVERSE_DIRECTORY_ACCESS,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE || handle.is_null() {
            return Err(ARTIFACT_DIRECTORY_OPEN_FAILED.to_owned());
        }
        // SAFETY: handle is successful and transferred immediately to File.
        let mut root = unsafe { std::fs::File::from_raw_handle(handle) };
        ensure_directory(&root)?;
        ensure_opened_path(&root, anchor)?;
        let options = FILE_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT;
        let mut missing = None;
        let mut opened = 0usize;
        for (index, name) in components.iter().enumerate() {
            match open_relative(&root, name, DIRECTORY_ACCESS, FILE_OPEN, options) {
                Ok(child) => {
                    ensure_directory(&child)?;
                    root = child;
                    opened += 1;
                }
                Err(ArtifactStatus::Missing) => {
                    missing = Some(index);
                    break;
                }
                Err(status) => return Err(map_directory_status(status)),
            }
        }
        if opened == 0 {
            return Err(ARTIFACT_BOUNDARY_UNSUPPORTED.to_owned());
        }
        preflight_native_operations(&root)?;
        let Some(first_missing) = missing else {
            return Ok(root);
        };
        for name in &components[first_missing..] {
            root = open_relative(&root, name, DIRECTORY_ACCESS, FILE_OPEN_IF, options)
                .map_err(map_directory_status)?;
            ensure_directory(&root)?;
        }
        Ok(root)
    }

    fn bind_root_existing(anchor: &Path, components: &[Vec<u16>]) -> Result<std::fs::File, String> {
        let name = wide_path(anchor);
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                TRAVERSE_DIRECTORY_ACCESS,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE || handle.is_null() {
            return Err(ARTIFACT_DIRECTORY_OPEN_FAILED.to_owned());
        }
        // SAFETY: handle is successful and transferred immediately to File.
        let mut root = unsafe { std::fs::File::from_raw_handle(handle) };
        ensure_directory(&root)?;
        ensure_opened_path(&root, anchor)?;
        let options = FILE_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT;
        for name in components {
            root = open_relative(&root, name, TRAVERSE_DIRECTORY_ACCESS, FILE_OPEN, options)
                .map_err(map_directory_status)?;
            ensure_directory(&root)?;
        }
        Ok(root)
    }

    fn split_windows_root(path: &Path) -> Result<(PathBuf, Vec<Vec<u16>>), String> {
        if !path.is_absolute() {
            return Err(ARTIFACT_BOUNDARY_REJECTED.to_owned());
        }
        let anchor = path
            .ancestors()
            .last()
            .filter(|candidate| candidate.has_root())
            .ok_or_else(|| ARTIFACT_BOUNDARY_REJECTED.to_owned())?
            .to_path_buf();
        let relative = path
            .strip_prefix(&anchor)
            .map_err(|_| ARTIFACT_BOUNDARY_REJECTED.to_owned())?;
        let mut components = Vec::new();
        for component in relative.components() {
            let Component::Normal(component) = component else {
                return Err(ARTIFACT_BOUNDARY_REJECTED.to_owned());
            };
            components.push(wide_os_component(component)?);
        }
        if components.is_empty() {
            return Err(ARTIFACT_BOUNDARY_REJECTED.to_owned());
        }
        Ok((anchor, components))
    }

    fn validate_relative_components(path: &str) -> Result<Vec<Vec<u16>>, String> {
        let mut components = Vec::new();
        for component in Path::new(path).components() {
            let Component::Normal(component) = component else {
                return Err(ARTIFACT_BOUNDARY_REJECTED.to_owned());
            };
            components.push(wide_os_component(component)?);
        }
        if components.is_empty() {
            return Err(ARTIFACT_BOUNDARY_REJECTED.to_owned());
        }
        Ok(components)
    }

    fn ensure_opened_path(file: &std::fs::File, expected: &Path) -> Result<(), String> {
        let actual = final_path_by_handle(file)?;
        let expected = normalize_dos_path(expected.as_os_str().encode_wide().collect());
        if wide_path_eq(&actual, &expected) {
            Ok(())
        } else {
            Err(ARTIFACT_BOUNDARY_REJECTED.to_owned())
        }
    }

    fn final_path_by_handle(file: &std::fs::File) -> Result<Vec<u16>, String> {
        let mut buffer = vec![0u16; 512];
        loop {
            let capacity = u32::try_from(buffer.len())
                .map_err(|_| ARTIFACT_BOUNDARY_UNSUPPORTED.to_owned())?;
            let length = unsafe {
                GetFinalPathNameByHandleW(
                    file.as_raw_handle(),
                    buffer.as_mut_ptr(),
                    capacity,
                    FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
                )
            };
            if length == 0 {
                return Err(ARTIFACT_BOUNDARY_UNSUPPORTED.to_owned());
            }
            let length =
                usize::try_from(length).map_err(|_| ARTIFACT_BOUNDARY_UNSUPPORTED.to_owned())?;
            if length < buffer.len() {
                buffer.truncate(length);
                return Ok(normalize_dos_path(buffer));
            }
            buffer.resize(
                length
                    .checked_add(1)
                    .ok_or_else(|| ARTIFACT_BOUNDARY_UNSUPPORTED.to_owned())?,
                0,
            );
        }
    }

    fn normalize_dos_path(mut path: Vec<u16>) -> Vec<u16> {
        const VERBATIM: &[u16] = &[b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16];
        const VERBATIM_UNC: &[u16] = &[
            b'\\' as u16,
            b'\\' as u16,
            b'?' as u16,
            b'\\' as u16,
            b'U' as u16,
            b'N' as u16,
            b'C' as u16,
            b'\\' as u16,
        ];
        if path.starts_with(VERBATIM_UNC) {
            path.splice(..VERBATIM_UNC.len(), [b'\\' as u16, b'\\' as u16]);
        } else if path.starts_with(VERBATIM) {
            path.drain(..VERBATIM.len());
        }
        for unit in &mut path {
            if *unit == b'/' as u16 {
                *unit = b'\\' as u16;
            }
        }
        path
    }

    fn wide_path_eq(left: &[u16], right: &[u16]) -> bool {
        left.len() == right.len()
            && left.iter().zip(right).all(|(left, right)| {
                if *left <= u8::MAX as u16 && *right <= u8::MAX as u16 {
                    (*left as u8).eq_ignore_ascii_case(&(*right as u8))
                } else {
                    left == right
                }
            })
    }

    fn prepare_directory(
        root: &std::fs::File,
        components: &[Vec<u16>],
    ) -> Result<ArtifactDirectory, String> {
        let mut parent = root
            .try_clone()
            .map_err(|_| ARTIFACT_DIRECTORY_OPEN_FAILED.to_owned())?;
        for name in components {
            let child = open_relative(
                &parent,
                name,
                DIRECTORY_ACCESS,
                FILE_OPEN_IF,
                FILE_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
            )
            .map_err(map_directory_status)?;
            ensure_directory(&child)?;
            parent = child;
        }
        Ok(ArtifactDirectory {
            _root: root
                .try_clone()
                .map_err(|_| ARTIFACT_DIRECTORY_OPEN_FAILED.to_owned())?,
            final_dir: parent,
        })
    }

    fn prepare_existing_directory(
        root: &std::fs::File,
        components: &[Vec<u16>],
    ) -> Result<ArtifactDirectory, String> {
        let mut parent = root
            .try_clone()
            .map_err(|_| ARTIFACT_DIRECTORY_OPEN_FAILED.to_owned())?;
        for name in components {
            let child = open_relative(
                &parent,
                name,
                TRAVERSE_DIRECTORY_ACCESS,
                FILE_OPEN,
                FILE_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
            )
            .map_err(map_directory_status)?;
            ensure_directory(&child)?;
            parent = child;
        }
        Ok(ArtifactDirectory {
            _root: root
                .try_clone()
                .map_err(|_| ARTIFACT_DIRECTORY_OPEN_FAILED.to_owned())?,
            final_dir: parent,
        })
    }

    fn preflight_native_operations(directory: &std::fs::File) -> Result<(), String> {
        if super::test_capability_preflight_failure() {
            return Err(ARTIFACT_PUBLISH_UNSUPPORTED.to_owned());
        }

        let mut probe = None;
        for suffix in 0..128u16 {
            let name = wide_component(&format!(".leanctx-engine-capability-probe-{suffix}"))?;
            match open_relative(
                directory,
                &name,
                TEMP_ACCESS,
                FILE_CREATE,
                FILE_NON_DIRECTORY_FILE
                    | FILE_OPEN_REPARSE_POINT
                    | FILE_SYNCHRONOUS_IO_NONALERT
                    | FILE_DELETE_ON_CLOSE,
            ) {
                Ok(file) => {
                    probe = Some((file, name));
                    break;
                }
                Err(ArtifactStatus::Collision) => continue,
                Err(_) => return Err(ARTIFACT_BOUNDARY_UNSUPPORTED.to_owned()),
            }
        }
        let (probe, probe_name) = probe.ok_or_else(|| ARTIFACT_BOUNDARY_UNSUPPORTED.to_owned())?;
        ensure_regular(&probe).map_err(|_| ARTIFACT_BOUNDARY_UNSUPPORTED.to_owned())?;

        if !matches!(
            rename_relative(&probe, directory.as_raw_handle(), &probe_name),
            Ok(PublishResult::Published | PublishResult::Collision)
        ) {
            return Err(ARTIFACT_BOUNDARY_UNSUPPORTED.to_owned());
        }

        // Exercise the exact disposition request used to clean production
        // temporaries. FILE_DELETE_ON_CLOSE remains only a fail-safe if the
        // information class is rejected.
        mark_delete(&probe).map_err(|_| ARTIFACT_BOUNDARY_UNSUPPORTED.to_owned())?;
        drop(probe);

        match open_relative(
            directory,
            &probe_name,
            TEMP_ACCESS,
            FILE_OPEN,
            FILE_NON_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
        ) {
            Err(ArtifactStatus::Missing) => Ok(()),
            Ok(stale) => {
                let _ = mark_delete(&stale);
                Err(ARTIFACT_BOUNDARY_UNSUPPORTED.to_owned())
            }
            Err(_) => Err(ARTIFACT_BOUNDARY_UNSUPPORTED.to_owned()),
        }
    }

    fn create_temp_artifact(
        directory: &std::fs::File,
        final_name: &[u16],
    ) -> Result<TempArtifact, String> {
        for suffix in 0..128u16 {
            let name = if suffix == 0 {
                wide_component(&format!(".{}.tmp", display_wide(final_name)))?
            } else {
                wide_component(&format!(".{}.tmp.{suffix}", display_wide(final_name)))?
            };
            match open_relative(
                directory,
                &name,
                TEMP_ACCESS,
                FILE_CREATE,
                FILE_NON_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
            ) {
                Ok(file) => {
                    let mut temp = TempArtifact {
                        file: Some(file),
                        disarmed: false,
                    };
                    let validation = if super::test_temp_validation_failure() {
                        Err(ARTIFACT_LEAF_UNTRUSTED.to_owned())
                    } else {
                        ensure_regular(
                            temp.file
                                .as_ref()
                                .ok_or_else(|| ARTIFACT_TEMP_CREATE_FAILED.to_owned())?,
                        )
                    };
                    if let Err(error) = validation {
                        return Err(temp.cleanup().err().unwrap_or(error));
                    }
                    return Ok(temp);
                }
                Err(ArtifactStatus::Collision) => continue,
                Err(ArtifactStatus::Unsupported) => {
                    return Err(ARTIFACT_BOUNDARY_UNSUPPORTED.to_owned());
                }
                Err(ArtifactStatus::Reparse) => {
                    return Err(ARTIFACT_BOUNDARY_REJECTED.to_owned());
                }
                Err(ArtifactStatus::Missing | ArtifactStatus::Failure) => {
                    return Err(ARTIFACT_TEMP_CREATE_FAILED.to_owned());
                }
            }
        }
        Err(ARTIFACT_TEMP_CREATE_FAILED.to_owned())
    }

    fn write_temp_artifact(temp: &mut TempArtifact, bytes: &[u8]) -> Result<(), String> {
        let file = temp
            .file
            .as_mut()
            .ok_or_else(|| ARTIFACT_WRITE_FAILED.to_owned())?;
        file.write_all(bytes)
            .map_err(|_| ARTIFACT_WRITE_FAILED.to_owned())?;
        file.sync_all()
            .map_err(|_| ARTIFACT_SYNC_FAILED.to_owned())?;
        Ok(())
    }

    fn publish_temp_artifact(
        temp: &mut TempArtifact,
        directory: &std::fs::File,
        final_name: &[u16],
        digest: &str,
    ) -> Result<std::fs::File, String> {
        let file = temp
            .file
            .as_ref()
            .ok_or_else(|| ARTIFACT_PUBLISH_FAILED.to_owned())?;
        let publish = match rename_relative(file, directory.as_raw_handle(), final_name) {
            Ok(result) => result,
            Err(error) => return Err(temp.cleanup().err().unwrap_or(error)),
        };
        match publish {
            PublishResult::Published => {
                let published = match verify_existing_artifact(directory, final_name, digest) {
                    Ok(published) => published,
                    Err(error) => return Err(temp.cleanup().err().unwrap_or(error)),
                };
                if let Err(error) = sync_directory(directory.as_raw_handle()) {
                    return Err(temp.cleanup().err().unwrap_or(error));
                }
                temp.disarmed = true;
                temp.file.take();
                Ok(published)
            }
            PublishResult::Collision => {
                temp.cleanup()?;
                verify_existing_artifact(directory, final_name, digest)
            }
        }
    }

    fn sync_directory(directory_handle: HANDLE) -> Result<(), String> {
        if unsafe { FlushFileBuffers(directory_handle) } != 0 {
            return Ok(());
        }
        match std::io::Error::last_os_error().raw_os_error() {
            Some(code) if matches!(code as u32, ERROR_INVALID_FUNCTION | ERROR_NOT_SUPPORTED) => {
                Ok(())
            }
            _ => Err(ARTIFACT_SYNC_FAILED.to_owned()),
        }
    }

    fn verify_existing_artifact(
        directory: &std::fs::File,
        final_name: &[u16],
        digest: &str,
    ) -> Result<std::fs::File, String> {
        let mut artifact = open_relative(
            directory,
            final_name,
            LEAF_ACCESS,
            FILE_OPEN,
            FILE_NON_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
        )
        .map_err(|status| match status {
            ArtifactStatus::Unsupported => ARTIFACT_BOUNDARY_UNSUPPORTED.to_owned(),
            _ => ARTIFACT_LEAF_UNTRUSTED.to_owned(),
        })?;
        ensure_regular(&artifact).map_err(|_| ARTIFACT_LEAF_UNTRUSTED.to_owned())?;
        let mut bytes = Vec::new();
        artifact
            .read_to_end(&mut bytes)
            .map_err(|_| ARTIFACT_LEAF_UNTRUSTED.to_owned())?;
        if hex_sha256(&bytes) != digest {
            return Err(ARTIFACT_DIGEST_MISMATCH.to_owned());
        }
        artifact
            .rewind()
            .map_err(|_| ARTIFACT_LEAF_UNTRUSTED.to_owned())?;
        Ok(artifact)
    }

    fn read_existing_artifact_bounded(
        directory: &std::fs::File,
        final_name: &[u16],
        max_bytes: usize,
    ) -> Result<Vec<u8>, String> {
        let artifact = open_relative(
            directory,
            final_name,
            READ_LEAF_ACCESS,
            FILE_OPEN,
            FILE_NON_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
        )
        .map_err(|_| ARTIFACT_LEAF_UNTRUSTED.to_owned())?;
        ensure_regular(&artifact)?;
        let metadata = artifact
            .metadata()
            .map_err(|_| ARTIFACT_LEAF_UNTRUSTED.to_owned())?;
        if metadata.len() > max_bytes as u64 {
            return Err(ARTIFACT_SIZE_LIMIT_EXCEEDED.to_owned());
        }
        let read_limit = max_bytes
            .checked_add(1)
            .ok_or_else(|| ARTIFACT_BOUNDARY_REJECTED.to_owned())? as u64;
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        artifact
            .take(read_limit)
            .read_to_end(&mut bytes)
            .map_err(|_| ARTIFACT_LEAF_UNTRUSTED.to_owned())?;
        if bytes.len() > max_bytes {
            return Err(ARTIFACT_SIZE_LIMIT_EXCEEDED.to_owned());
        }
        Ok(bytes)
    }

    #[derive(Clone, Copy)]
    enum ArtifactStatus {
        Failure,
        Collision,
        Missing,
        Reparse,
        Unsupported,
    }

    enum PublishResult {
        Published,
        Collision,
    }

    fn open_relative(
        parent: &std::fs::File,
        name: &[u16],
        desired_access: u32,
        disposition: u32,
        options: u32,
    ) -> Result<std::fs::File, ArtifactStatus> {
        let mut unicode = unicode_string(name)?;
        let mut attributes = OBJECT_ATTRIBUTES {
            Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
            RootDirectory: parent.as_raw_handle(),
            ObjectName: &mut unicode,
            Attributes: OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE,
            SecurityDescriptor: null(),
            SecurityQualityOfService: null(),
        };
        let mut handle: HANDLE = null_mut();
        let mut io_status = IO_STATUS_BLOCK::default();
        let status = unsafe {
            NtCreateFile(
                &mut handle,
                desired_access,
                &mut attributes,
                &mut io_status,
                null(),
                FILE_ATTRIBUTE_NORMAL,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                disposition,
                options,
                null(),
                0,
            )
        };
        let opened_existing = disposition == FILE_OPEN_IF && status == STATUS_OBJECT_NAME_EXISTS;
        if (status == STATUS_SUCCESS || opened_existing)
            && !handle.is_null()
            && handle != INVALID_HANDLE_VALUE
        {
            // SAFETY: NtCreateFile returned a successful handle owned immediately.
            return Ok(unsafe { std::fs::File::from_raw_handle(handle) });
        }
        if !handle.is_null() && handle != INVALID_HANDLE_VALUE {
            // SAFETY: an unexpected returned handle is still owned by this call.
            drop(unsafe { std::fs::File::from_raw_handle(handle) });
        }
        Err(
            if status == STATUS_OBJECT_NAME_COLLISION || status == STATUS_OBJECT_NAME_EXISTS {
                ArtifactStatus::Collision
            } else if status == STATUS_REPARSE_POINT_ENCOUNTERED {
                ArtifactStatus::Reparse
            } else if status == STATUS_OBJECT_NAME_NOT_FOUND
                || status == STATUS_OBJECT_PATH_NOT_FOUND
            {
                ArtifactStatus::Missing
            } else if status == STATUS_INVALID_PARAMETER || status == STATUS_NOT_SUPPORTED {
                ArtifactStatus::Unsupported
            } else {
                ArtifactStatus::Failure
            },
        )
    }

    fn rename_relative(
        file: &std::fs::File,
        directory_handle: HANDLE,
        name: &[u16],
    ) -> Result<PublishResult, String> {
        let header_size = size_of::<FILE_RENAME_INFORMATION>() - size_of::<u16>();
        let bytes = header_size
            .checked_add(
                name.len()
                    .checked_mul(size_of::<u16>())
                    .ok_or_else(|| ARTIFACT_BOUNDARY_UNSUPPORTED.to_owned())?,
            )
            .ok_or_else(|| ARTIFACT_BOUNDARY_UNSUPPORTED.to_owned())?;
        let name_bytes = u32::try_from(
            name.len()
                .checked_mul(size_of::<u16>())
                .ok_or_else(|| ARTIFACT_BOUNDARY_UNSUPPORTED.to_owned())?,
        )
        .map_err(|_| ARTIFACT_BOUNDARY_UNSUPPORTED.to_owned())?;
        let buffer_bytes =
            u32::try_from(bytes).map_err(|_| ARTIFACT_BOUNDARY_UNSUPPORTED.to_owned())?;
        let words = bytes.div_ceil(size_of::<u64>());
        let mut storage = vec![0u64; words];
        let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
        unsafe {
            (*info).Anonymous.Flags = 0;
            (*info).RootDirectory = directory_handle;
            (*info).FileNameLength = name_bytes;
            std::ptr::copy_nonoverlapping(name.as_ptr(), (*info).FileName.as_mut_ptr(), name.len());
        }
        let mut io_status = IO_STATUS_BLOCK::default();
        let status = unsafe {
            NtSetInformationFile(
                file.as_raw_handle(),
                &mut io_status,
                info.cast::<c_void>(),
                buffer_bytes,
                FileRenameInformationEx,
            )
        };
        if status == STATUS_SUCCESS {
            Ok(PublishResult::Published)
        } else if status == STATUS_OBJECT_NAME_COLLISION || status == STATUS_OBJECT_NAME_EXISTS {
            Ok(PublishResult::Collision)
        } else if status == STATUS_REPARSE_POINT_ENCOUNTERED {
            Err(ARTIFACT_BOUNDARY_REJECTED.to_owned())
        } else if status == STATUS_INVALID_PARAMETER || status == STATUS_NOT_SUPPORTED {
            Err(ARTIFACT_BOUNDARY_UNSUPPORTED.to_owned())
        } else {
            Err(ARTIFACT_PUBLISH_FAILED.to_owned())
        }
    }

    fn mark_delete(file: &std::fs::File) -> Result<(), String> {
        let mut info = FILE_DISPOSITION_INFORMATION_EX {
            Flags: FILE_DISPOSITION_DELETE,
        };
        let mut io_status = IO_STATUS_BLOCK::default();
        let status = unsafe {
            NtSetInformationFile(
                file.as_raw_handle(),
                &mut io_status,
                (&mut info as *mut FILE_DISPOSITION_INFORMATION_EX).cast::<c_void>(),
                size_of::<FILE_DISPOSITION_INFORMATION_EX>() as u32,
                FileDispositionInformationEx,
            )
        };
        if status == STATUS_SUCCESS {
            Ok(())
        } else {
            Err(ARTIFACT_CLEANUP_FAILED.to_owned())
        }
    }

    fn ensure_directory(file: &std::fs::File) -> Result<(), String> {
        let info = query_tag(file)?;
        if info.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || info.FileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
        {
            return Err(ARTIFACT_BOUNDARY_REJECTED.to_owned());
        }
        Ok(())
    }

    fn ensure_regular(file: &std::fs::File) -> Result<(), String> {
        let tag = query_tag(file)?;
        if tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || tag.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0
        {
            return Err(ARTIFACT_LEAF_UNTRUSTED.to_owned());
        }
        let mut standard = FILE_STANDARD_INFO::default();
        let ok = unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileStandardInfo,
                (&mut standard as *mut FILE_STANDARD_INFO).cast::<c_void>(),
                size_of::<FILE_STANDARD_INFO>() as u32,
            )
        };
        if ok == 0 || standard.Directory {
            return Err(ARTIFACT_LEAF_UNTRUSTED.to_owned());
        }
        Ok(())
    }

    fn query_tag(file: &std::fs::File) -> Result<FILE_ATTRIBUTE_TAG_INFO, String> {
        let mut info = FILE_ATTRIBUTE_TAG_INFO::default();
        let ok = unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileAttributeTagInfo,
                (&mut info as *mut FILE_ATTRIBUTE_TAG_INFO).cast::<c_void>(),
                size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
            )
        };
        if ok == 0 {
            Err(ARTIFACT_BOUNDARY_UNSUPPORTED.to_owned())
        } else {
            Ok(info)
        }
    }

    fn map_directory_status(status: ArtifactStatus) -> String {
        match status {
            ArtifactStatus::Unsupported => ARTIFACT_BOUNDARY_UNSUPPORTED.to_owned(),
            ArtifactStatus::Reparse => ARTIFACT_BOUNDARY_REJECTED.to_owned(),
            ArtifactStatus::Collision | ArtifactStatus::Missing | ArtifactStatus::Failure => {
                ARTIFACT_DIRECTORY_OPEN_FAILED.to_owned()
            }
        }
    }

    fn unicode_string(
        name: &[u16],
    ) -> Result<windows_sys::Win32::Foundation::UNICODE_STRING, ArtifactStatus> {
        let byte_length = name
            .len()
            .checked_mul(size_of::<u16>())
            .and_then(|length| u16::try_from(length).ok())
            .ok_or(ArtifactStatus::Unsupported)?;
        Ok(windows_sys::Win32::Foundation::UNICODE_STRING {
            Length: byte_length,
            MaximumLength: byte_length,
            Buffer: name.as_ptr() as *mut u16,
        })
    }

    fn wide_path(path: &Path) -> Vec<u16> {
        path.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    fn wide_component(value: &str) -> Result<Vec<u16>, String> {
        wide_os_component(std::ffi::OsStr::new(value))
    }

    fn wide_os_component(value: &std::ffi::OsStr) -> Result<Vec<u16>, String> {
        let value: Vec<u16> = value.encode_wide().collect();
        let byte_length = value.len().checked_mul(size_of::<u16>());
        if value.is_empty()
            || value.iter().any(|unit| matches!(*unit, 0 | 47 | 92))
            || byte_length
                .and_then(|length| u16::try_from(length).ok())
                .is_none()
        {
            return Err(ARTIFACT_BOUNDARY_REJECTED.to_owned());
        }
        Ok(value)
    }

    fn display_wide(name: &[u16]) -> String {
        String::from_utf16_lossy(name)
    }
}
