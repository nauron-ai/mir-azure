use std::process::{Output, Stdio};
use std::time::Duration;

use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::task::JoinHandle;

const OUTPUT_CAPTURE_LIMIT_BYTES: usize = 128 * 1024;
const OUTPUT_TRUNCATED_MARKER: &[u8] = b"\n...[truncated]";
const POST_KILL_WAIT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Error)]
pub enum ProcessError {
    #[error("command execution failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("{command} timed out after {timeout}")]
    TimedOut {
        command: &'static str,
        timeout: String,
    },
}

pub async fn run_command(
    mut command: Command,
    command_name: &'static str,
    timeout: Duration,
) -> Result<Output, ProcessError> {
    command.kill_on_drop(true);
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let mut child = command.spawn()?;
    let stdout_task = spawn_output_task(child.stdout.take());
    let stderr_task = spawn_output_task(child.stderr.take());
    let status = match wait_for_child(&mut child, command_name, timeout).await {
        Ok(status) => status,
        Err(error) => {
            abort_output_tasks(stdout_task, stderr_task);
            return Err(error);
        }
    };
    let stdout = join_output_task(stdout_task).await?;
    let stderr = join_output_task(stderr_task).await?;

    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

async fn wait_for_child(
    child: &mut tokio::process::Child,
    command_name: &'static str,
    timeout: Duration,
) -> Result<std::process::ExitStatus, ProcessError> {
    match tokio::time::timeout(timeout, child.wait()).await {
        Ok(status) => Ok(status?),
        Err(_) => {
            let _ = child.start_kill();
            let _ = tokio::time::timeout(POST_KILL_WAIT_TIMEOUT, child.wait()).await;
            Err(ProcessError::TimedOut {
                command: command_name,
                timeout: format_timeout(timeout),
            })
        }
    }
}

fn spawn_output_task<R>(reader: Option<R>) -> JoinHandle<Result<Vec<u8>, std::io::Error>>
where
    R: AsyncRead + Send + Unpin + 'static,
{
    tokio::spawn(async move {
        let Some(reader) = reader else {
            return Ok(Vec::new());
        };

        capture_output(reader).await
    })
}

async fn join_output_task(
    task: JoinHandle<Result<Vec<u8>, std::io::Error>>,
) -> Result<Vec<u8>, ProcessError> {
    match task.await {
        Ok(output) => Ok(output?),
        Err(error) => Err(ProcessError::Io(std::io::Error::other(error))),
    }
}

fn abort_output_tasks(
    stdout_task: JoinHandle<Result<Vec<u8>, std::io::Error>>,
    stderr_task: JoinHandle<Result<Vec<u8>, std::io::Error>>,
) {
    stdout_task.abort();
    stderr_task.abort();
}

async fn capture_output<R>(reader: R) -> Result<Vec<u8>, std::io::Error>
where
    R: AsyncRead + Unpin,
{
    let mut reader = reader;
    let mut output = Vec::new();
    let mut buffer = [0_u8; 8 * 1024];
    let mut truncated = false;

    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }

        let remaining = OUTPUT_CAPTURE_LIMIT_BYTES.saturating_sub(output.len());
        let keep = read.min(remaining);
        if keep > 0 {
            output.extend_from_slice(&buffer[..keep]);
        }

        if keep < read && !truncated {
            output.extend_from_slice(OUTPUT_TRUNCATED_MARKER);
            truncated = true;
        }
    }

    Ok(output)
}

fn format_timeout(timeout: Duration) -> String {
    let millis = timeout.as_millis();
    if millis.is_multiple_of(1_000) {
        return format!("{}s", millis / 1_000);
    }

    format!("{millis}ms")
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::process::Command;

    use super::{run_command, ProcessError, OUTPUT_CAPTURE_LIMIT_BYTES, OUTPUT_TRUNCATED_MARKER};

    #[tokio::test]
    async fn times_out_long_running_command() {
        let mut command = Command::new("sh");
        command.arg("-c").arg("sleep 1");

        let result = run_command(command, "sh", Duration::from_millis(10)).await;

        assert!(matches!(result, Err(ProcessError::TimedOut { .. })));
    }

    #[tokio::test]
    async fn truncates_output_without_blocking_child_process() {
        let mut command = Command::new("python3");
        command.arg("-c").arg(format!(
            "import sys; sys.stdout.write('a' * {})",
            OUTPUT_CAPTURE_LIMIT_BYTES + 4096
        ));

        let output = run_command(command, "python3", Duration::from_secs(5))
            .await
            .unwrap();

        assert!(output.status.success());
        assert_eq!(
            output.stdout.len(),
            OUTPUT_CAPTURE_LIMIT_BYTES + OUTPUT_TRUNCATED_MARKER.len()
        );
        assert!(output.stdout.ends_with(OUTPUT_TRUNCATED_MARKER));
    }
}
