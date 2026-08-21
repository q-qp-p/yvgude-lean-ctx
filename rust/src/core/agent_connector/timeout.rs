use anyhow::Result;
use std::io::Read;
use std::process::{Command, Output, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub(crate) struct TimedOutput {
    pub(crate) output: Output,
    pub(crate) timed_out: bool,
}

pub(crate) fn run_with_timeout(command: &mut Command, timeout_ms: u64) -> Result<TimedOutput> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = read_child_output(child.stdout.take().expect("stdout is piped"));
    let stderr = read_child_output(child.stderr.take().expect("stderr is piped"));
    let timeout = Duration::from_millis(timeout_ms);
    let start = Instant::now();

    let (status, timed_out) = loop {
        if let Some(status) = child.try_wait()? {
            break (status, false);
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            break (child.wait()?, true);
        }
        let remaining = timeout.saturating_sub(start.elapsed());
        std::thread::sleep(remaining.min(Duration::from_millis(10)));
    };

    Ok(TimedOutput {
        output: Output {
            status,
            stdout: collect_output(stdout)?,
            stderr: collect_output(stderr)?,
        },
        timed_out,
    })
}

fn read_child_output(
    mut output: impl Read + Send + 'static,
) -> JoinHandle<std::io::Result<Vec<u8>>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        output.read_to_end(&mut bytes)?;
        Ok(bytes)
    })
}

fn collect_output(handle: JoinHandle<std::io::Result<Vec<u8>>>) -> Result<Vec<u8>> {
    handle
        .join()
        .map_err(|_| anyhow::anyhow!("child output reader panicked"))?
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    #[allow(unused_imports)]
    use super::*;

    #[cfg(unix)]
    #[test]
    fn kills_child_when_timeout_expires() {
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 1"]);

        let output = run_with_timeout(&mut command, 20).unwrap();

        assert!(output.timed_out);
    }

    #[cfg(unix)]
    #[test]
    fn collects_output_when_child_completes() {
        let mut command = Command::new("sh");
        command.args(["-c", "printf done"]);

        let output = run_with_timeout(&mut command, 1_000).unwrap();

        assert!(!output.timed_out);
        assert_eq!(output.output.stdout, b"done");
    }
}
