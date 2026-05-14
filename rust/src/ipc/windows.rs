use anyhow::{Context, Result};
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeServer, ServerOptions};

pub(super) fn default_pipe_name() -> String {
    let username = std::env::var("USERNAME").unwrap_or_else(|_| "default".to_string());
    let data_dir = dirs::data_local_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join("AppData/Local"))
        .join("lean-ctx");
    let seed = format!("{username}:{}", data_dir.display());
    let hash = blake3::hash(seed.as_bytes());
    let short = &hash.to_hex()[..16];
    format!(r"\\.\pipe\lean-ctx-{short}")
}

pub(super) fn pipe_exists(name: &str) -> bool {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::Pipes::WaitNamedPipeW;

    let wide: Vec<u16> = OsStr::new(name)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe { WaitNamedPipeW(wide.as_ptr(), 1) != 0 }
}

pub(super) async fn connect(
    pipe_name: &str,
) -> Result<tokio::net::windows::named_pipe::NamedPipeClient> {
    use std::time::Duration;
    use windows_sys::Win32::Foundation::ERROR_PIPE_BUSY;

    loop {
        match ClientOptions::new().open(pipe_name) {
            Ok(client) => return Ok(client),
            Err(e)
                if e.kind() == std::io::ErrorKind::NotFound
                    || e.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) =>
            {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            Err(e) => {
                anyhow::bail!("connect to daemon pipe {pipe_name}: {e}");
            }
        }
    }
}

/// Server-side named-pipe listener, analogous to `UnixListener`.
///
/// Each call to [`accept_pipe`] waits for a client to connect, hands back
/// the connected pipe, and creates a fresh instance for the next client.
pub struct NamedPipeListener {
    current: NamedPipeServer,
    name: String,
}

impl NamedPipeListener {
    pub fn bind(name: &str) -> Result<Self> {
        let server = ServerOptions::new()
            .first_pipe_instance(true)
            .create(name)
            .with_context(|| format!("bind named pipe {name}"))?;
        Ok(Self {
            current: server,
            name: name.to_string(),
        })
    }

    /// Wait for a client, return the connected pipe, prepare the next instance.
    pub async fn accept_pipe(&mut self) -> std::io::Result<NamedPipeServer> {
        self.current.connect().await?;
        let next = ServerOptions::new()
            .first_pipe_instance(false)
            .create(&self.name)?;
        Ok(std::mem::replace(&mut self.current, next))
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}
