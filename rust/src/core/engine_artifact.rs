//! Contained, immutable storage for local Engine Interface artifacts.

use std::io::{Read, Seek, Write};
use std::path::{Component, Path, PathBuf};

#[cfg(test)]
use std::cell::Cell;

use sha2::{Digest, Sha256};

use crate::core::data_dir;

#[cfg(test)]
thread_local! {
    static TEST_FAIL_BEFORE_PUBLISH: Cell<bool> = const { Cell::new(false) };
}

#[cfg(test)]
pub(super) fn inject_test_pre_publish_failure() {
    TEST_FAIL_BEFORE_PUBLISH.with(|failpoint| failpoint.set(true));
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

struct TempArtifact {
    file: Option<std::fs::File>,
    path: PathBuf,
    published: bool,
}

impl Drop for TempArtifact {
    fn drop(&mut self) {
        // Closing before unlinking is required on Windows. A failed operation
        // may leave only this private, same-directory temporary leaf behind.
        self.file.take();
        if !self.published {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

pub(super) fn persist_content(
    directory: &str,
    digest: &str,
    extension: &str,
    bytes: &[u8],
) -> Result<std::fs::File, String> {
    validate_artifact_name(digest, extension)?;
    if hex_sha256(bytes) != digest {
        return Err(
            "engine_artifact_digest_mismatch: supplied bytes do not match digest".to_owned(),
        );
    }

    let configured_root = data_dir::lean_ctx_data_dir()?;
    std::fs::create_dir_all(&configured_root)
        .map_err(|error| format!("create Engine data directory: {error}"))?;
    let root = crate::core::pathutil::canonicalize_secure(&configured_root)
        .map_err(|error| format!("resolve Engine data directory: {error}"))?;
    let directory = prepare_directory(&root, directory)?;
    let path = directory.join(format!("{digest}.{extension}"));
    let mut temp = create_temp_artifact(&path, &root)
        .map_err(|_| "engine_artifact_temp_create_failed".to_owned())?;
    write_temp_artifact(&mut temp, bytes)?;

    if test_pre_publish_failure() {
        return Err("engine_artifact_test_pre_publish_failure".to_owned());
    }

    match publish_temp_artifact(&mut temp, &path) {
        Ok(()) => open_published_artifact(&path, &root, digest),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            verify_existing_artifact(&path, &root, digest)
        }
        Err(_) => Err("engine_artifact_publish_failed".to_owned()),
    }
}

fn validate_artifact_name(digest: &str, extension: &str) -> Result<(), String> {
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Engine artifact digest must be 64 hexadecimal characters".to_owned());
    }
    if !matches!(extension, "json" | "txt") {
        return Err("Engine artifact extension is not allowed".to_owned());
    }
    Ok(())
}

fn prepare_directory(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let mut current = root.to_path_buf();
    for component in Path::new(relative).components() {
        let Component::Normal(component) = component else {
            return Err("Engine artifact directory must be a relative normal path".to_owned());
        };
        let next = current.join(component);
        match std::fs::symlink_metadata(&next) {
            Ok(metadata) => validate_directory_metadata(&metadata)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match std::fs::create_dir(&next) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => {
                        return Err(format!("create Engine artifact directory: {error}"));
                    }
                }
                let metadata = std::fs::symlink_metadata(&next)
                    .map_err(|error| format!("inspect Engine artifact directory: {error}"))?;
                validate_directory_metadata(&metadata)?;
            }
            Err(error) => {
                return Err(format!("inspect Engine artifact directory: {error}"));
            }
        }
        let resolved = crate::core::pathutil::canonicalize_secure(&next)
            .map_err(|error| format!("resolve Engine artifact directory: {error}"))?;
        if !resolved.starts_with(root) {
            return Err(
                "engine_artifact_boundary_rejected: directory escaped data root".to_owned(),
            );
        }
        data_dir::ensure_dir_permissions(&resolved);
        current = resolved;
    }
    Ok(current)
}

fn validate_directory_metadata(metadata: &std::fs::Metadata) -> Result<(), String> {
    if crate::core::pathutil::is_symlink_or_reparse(metadata) || !metadata.is_dir() {
        return Err(
            "engine_artifact_boundary_rejected: directory must be real and non-symlink".to_owned(),
        );
    }
    Ok(())
}

fn create_temp_artifact(path: &Path, root: &Path) -> Result<TempArtifact, std::io::Error> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("Engine artifact path has no parent"))?;
    let filename = path
        .file_name()
        .ok_or_else(|| std::io::Error::other("Engine artifact path has no filename"))?
        .to_string_lossy();

    // The bounded suffix loop handles concurrent writers and a temp left by a
    // process crash without putting a random name into any persisted record.
    for suffix in 0..128u16 {
        let temp_name = if suffix == 0 {
            format!(".{filename}.tmp")
        } else {
            format!(".{filename}.tmp.{suffix}")
        };
        let temp_path = parent.join(temp_name);
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create_new(true);
        apply_nofollow_flags(&mut options);
        let file = match options.open(&temp_path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        if let Err(error) = verify_open_artifact(&file, &temp_path, root) {
            drop(file);
            let _ = std::fs::remove_file(&temp_path);
            return Err(error);
        }
        return Ok(TempArtifact {
            file: Some(file),
            path: temp_path,
            published: false,
        });
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "Engine artifact temporary namespace is occupied",
    ))
}

fn write_temp_artifact(temp: &mut TempArtifact, bytes: &[u8]) -> Result<(), String> {
    let artifact = temp
        .file
        .as_mut()
        .ok_or_else(|| "engine_artifact_temp_closed".to_owned())?;
    if let Some(permissions) = artifact_permissions() {
        artifact
            .set_permissions(permissions)
            .map_err(|_| "engine_artifact_temp_permissions_failed".to_owned())?;
    }
    artifact
        .write_all(bytes)
        .map_err(|_| "engine_artifact_temp_write_failed".to_owned())?;
    artifact
        .sync_all()
        .map_err(|_| "engine_artifact_temp_sync_failed".to_owned())?;
    Ok(())
}

fn publish_temp_artifact(temp: &mut TempArtifact, path: &Path) -> Result<(), std::io::Error> {
    // The file is fully written and synchronized before it becomes visible at
    // its content address. Closing also makes the publication portable.
    temp.file.take();

    #[cfg(windows)]
    windows_publish_no_replace(&temp.path, path)?;

    #[cfg(not(windows))]
    {
        // hard_link is an atomic create-without-replacement primitive: unlike
        // rename, it cannot replace an existing addressed artifact.
        std::fs::hard_link(&temp.path, path)?;
        #[cfg(unix)]
        sync_directory(path.parent());
        std::fs::remove_file(&temp.path)?;
    }

    temp.published = true;
    Ok(())
}

#[cfg(windows)]
fn windows_publish_no_replace(temp: &Path, path: &Path) -> Result<(), std::io::Error> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW};

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    let temp = wide(temp);
    let path = wide(path);
    // Omitting MOVEFILE_REPLACE_EXISTING makes an existing final path a
    // deterministic AlreadyExists failure; the move stays same-volume because
    // the temp was created in the final directory.
    let ok = unsafe { MoveFileExW(temp.as_ptr(), path.as_ptr(), MOVEFILE_WRITE_THROUGH) };
    if ok == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(unix)]
fn sync_directory(parent: Option<&Path>) {
    if let Some(parent) = parent
        && let Ok(directory) = std::fs::File::open(parent)
    {
        let _ = directory.sync_all();
    }
}

fn open_published_artifact(
    path: &Path,
    root: &Path,
    digest: &str,
) -> Result<std::fs::File, String> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    apply_nofollow_flags(&mut options);
    let mut artifact = options
        .open(path)
        .map_err(|_| "engine_artifact_publish_verification_failed".to_owned())?;
    verify_open_artifact(&artifact, path, root)
        .map_err(|_| "engine_artifact_publish_verification_failed".to_owned())?;
    let mut bytes = Vec::new();
    artifact
        .read_to_end(&mut bytes)
        .map_err(|_| "engine_artifact_publish_verification_failed".to_owned())?;
    if hex_sha256(&bytes) != digest {
        return Err("engine_artifact_publish_verification_failed".to_owned());
    }
    artifact
        .rewind()
        .map_err(|_| "engine_artifact_publish_verification_failed".to_owned())?;
    Ok(artifact)
}

fn verify_existing_artifact(
    path: &Path,
    root: &Path,
    digest: &str,
) -> Result<std::fs::File, String> {
    if std::fs::symlink_metadata(path)
        .is_ok_and(|metadata| crate::core::pathutil::is_symlink_or_reparse(&metadata))
    {
        return Err(
            "engine_artifact_leaf_untrusted: artifact must be regular and non-symlink".to_owned(),
        );
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    apply_nofollow_flags(&mut options);
    let mut artifact = options
        .open(path)
        .map_err(|error| format!("open Engine artifact without following links: {error}"))?;
    verify_open_artifact(&artifact, path, root).map_err(|_| {
        "engine_artifact_boundary_rejected: opened file escaped data root".to_owned()
    })?;
    let mut bytes = Vec::new();
    artifact
        .read_to_end(&mut bytes)
        .map_err(|error| format!("read Engine artifact: {error}"))?;
    if hex_sha256(&bytes) != digest {
        return Err(
            "engine_artifact_digest_mismatch: content differs from addressed digest".to_owned(),
        );
    }
    if let Some(permissions) = artifact_permissions() {
        artifact
            .set_permissions(permissions)
            .map_err(|error| format!("harden Engine artifact permissions: {error}"))?;
    }
    artifact
        .rewind()
        .map_err(|error| format!("rewind Engine artifact: {error}"))?;
    Ok(artifact)
}

fn apply_nofollow_flags(options: &mut std::fs::OpenOptions) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
}

fn verify_open_artifact(
    artifact: &std::fs::File,
    path: &Path,
    root: &Path,
) -> Result<(), std::io::Error> {
    let metadata = artifact.metadata()?;
    if crate::core::pathutil::is_symlink_or_reparse(&metadata) || !metadata.is_file() {
        return Err(std::io::Error::other(
            "Engine artifact must be a regular non-symlink file",
        ));
    }
    verify_handle_path(artifact, path, root, &metadata)
}

#[cfg(unix)]
fn verify_handle_path(
    _artifact: &std::fs::File,
    path: &Path,
    root: &Path,
    metadata: &std::fs::Metadata,
) -> Result<(), std::io::Error> {
    use std::os::unix::fs::MetadataExt;

    let resolved = crate::core::pathutil::canonicalize_secure(path)?;
    let resolved_metadata = std::fs::metadata(&resolved)?;
    if !resolved.starts_with(root)
        || metadata.dev() != resolved_metadata.dev()
        || metadata.ino() != resolved_metadata.ino()
    {
        return Err(std::io::Error::other(
            "opened Engine artifact escaped the data root",
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn verify_handle_path(
    artifact: &std::fs::File,
    _path: &Path,
    root: &Path,
    _metadata: &std::fs::Metadata,
) -> Result<(), std::io::Error> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_NAME_NORMALIZED, GetFinalPathNameByHandleW, VOLUME_NAME_DOS,
    };

    let mut buffer = vec![0u16; 32_768];
    // SAFETY: the handle is live and buffer is writable for its full length.
    let length = unsafe {
        GetFinalPathNameByHandleW(
            artifact.as_raw_handle() as _,
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
        )
    };
    if length == 0 || length as usize >= buffer.len() {
        return Err(std::io::Error::last_os_error());
    }
    buffer.truncate(length as usize);
    let opened = crate::core::pathutil::strip_verbatim(PathBuf::from(OsString::from_wide(&buffer)));
    let opened = opened.to_string_lossy().to_ascii_lowercase();
    let root = root.to_string_lossy().to_ascii_lowercase();
    if opened != root
        && !opened
            .strip_prefix(&root)
            .is_some_and(|suffix| suffix.starts_with('\\') || suffix.starts_with('/'))
    {
        return Err(std::io::Error::other(
            "opened Engine artifact escaped the data root",
        ));
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn verify_handle_path(
    _artifact: &std::fs::File,
    path: &Path,
    root: &Path,
    _metadata: &std::fs::Metadata,
) -> Result<(), std::io::Error> {
    let resolved = crate::core::pathutil::canonicalize_secure(path)?;
    if !resolved.starts_with(root) {
        return Err(std::io::Error::other(
            "opened Engine artifact escaped the data root",
        ));
    }
    Ok(())
}

fn hex_sha256(bytes: &[u8]) -> String {
    crate::core::agent_identity::hex_encode(&Sha256::digest(bytes))
}

fn artifact_permissions() -> Option<std::fs::Permissions> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        Some(std::fs::Permissions::from_mode(0o600))
    }
    #[cfg(not(unix))]
    {
        None
    }
}
