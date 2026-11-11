//! Port of `src/Ricochet.Core/ExternalSecrets.cs` (ADR 0025). A Variable's value can be a reference, `secret-ref:env:NAME`
//! or `secret-ref:command:LINE`, instead of the Secret itself. A reference holds no secret, so it can be saved in a
//! Workspace file. The resolver reads the value on demand and never saves it. Callers must mask the value wherever it
//! is shown.
//!
//! The port is synchronous, because the app and the CLI call it from background threads. The C# cancellation token is
//! not ported: a command is stopped by its time limit instead.

use std::collections::HashMap;
use std::fmt;
use std::io::Read;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::secrets::SecretValue;

/// The prefix that marks a Variable value as a reference.
pub const PREFIX: &str = "secret-ref:";
const ENVIRONMENT_KIND: &str = "env:";
const COMMAND_KIND: &str = "command:";

/// An external Secret could not be read. The message never contains the value, so it is safe to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalSecretError {
    message: String,
}

impl ExternalSecretError {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ExternalSecretError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ExternalSecretError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalSecretKind {
    EnvironmentVariable,
    Command,
}

/// Where an external Secret's value comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalSecretSource {
    pub kind: ExternalSecretKind,
    pub target: String,
}

/// Reference helpers, the same names as the C# `ExternalSecretReference` class.
pub mod reference {
    use super::{COMMAND_KIND, ENVIRONMENT_KIND, ExternalSecretError, ExternalSecretKind, ExternalSecretSource, PREFIX};

    pub fn is_reference(value: &str) -> bool {
        value.starts_with(PREFIX)
    }

    /// Reads a reference. Fails when the value is not a reference, or the reference is not valid.
    pub fn parse(value: &str) -> Result<ExternalSecretSource, ExternalSecretError> {
        let Some(body) = value.strip_prefix(PREFIX) else {
            return Err(ExternalSecretError::new("This is not a secret reference."));
        };

        if let Some(rest) = body.strip_prefix(ENVIRONMENT_KIND) {
            let name = rest.trim();
            if name.is_empty() || name.contains('=') || name.chars().any(char::is_whitespace) {
                return Err(ExternalSecretError::new("The environment variable name in a secret reference is not valid."));
            }
            return Ok(ExternalSecretSource { kind: ExternalSecretKind::EnvironmentVariable, target: name.to_string() });
        }

        if let Some(rest) = body.strip_prefix(COMMAND_KIND) {
            let line = rest.trim();
            if line.is_empty() {
                return Err(ExternalSecretError::new("The command in a secret reference is empty."));
            }
            return Ok(ExternalSecretSource { kind: ExternalSecretKind::Command, target: line.to_string() });
        }

        Err(ExternalSecretError::new(format!(
            "A secret reference must start with {PREFIX}{ENVIRONMENT_KIND} or {PREFIX}{COMMAND_KIND}."
        )))
    }

    pub fn for_environment_variable(name: &str) -> String {
        format!("{PREFIX}{ENVIRONMENT_KIND}{name}")
    }

    /// Commands must not contain secrets themselves, because the line is saved in the Workspace.
    pub fn for_command(command_line: &str) -> String {
        format!("{PREFIX}{COMMAND_KIND}{command_line}")
    }
}

/// What a command printed, and how it ended. The output may hold a secret, so it is a [`SecretValue`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub exit_code: i32,
    pub standard_output: SecretValue,
}

/// Runs a command line for a secret reference. Replaced in tests, so no real secret store is needed.
pub trait CommandRunner: Send + Sync {
    fn run(&self, command_line: &str) -> Result<CommandOutput, ExternalSecretError>;
}

/// Runs the command through `cmd.exe` on Windows and `/bin/sh` elsewhere, with no input and a time limit.
#[derive(Debug, Clone, Copy)]
pub struct ProcessCommandRunner {
    timeout: Duration,
}

impl ProcessCommandRunner {
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

    pub fn new(timeout: Duration) -> Self {
        Self { timeout }
    }
}

impl Default for ProcessCommandRunner {
    fn default() -> Self {
        Self::new(Self::DEFAULT_TIMEOUT)
    }
}

const COULD_NOT_START: &str = "The command for a secret reference could not be started.";

fn read_all(mut stream: impl Read) -> Vec<u8> {
    let mut bytes = Vec::new();
    let _ = stream.read_to_end(&mut bytes);
    bytes
}

#[cfg(windows)]
fn shell_command(command_line: &str) -> Command {
    use std::os::windows::process::CommandExt;
    let mut command = Command::new("cmd.exe");
    // The line goes to cmd.exe exactly as written, so its quotes are not re-escaped.
    command.raw_arg(format!("/d /s /c \"{command_line}\""));
    command
}

#[cfg(not(windows))]
fn shell_command(command_line: &str) -> Command {
    let mut command = Command::new("/bin/sh");
    command.arg("-c").arg(command_line);
    command
}

/// Stops the command and anything it started. Its pipes may stay open after the kill, so the readers are not joined.
fn kill_tree(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill").args(["/T", "/F", "/PID", &child.id().to_string()]).output();
    }
    let _ = child.kill();
    let _ = child.wait();
}

impl CommandRunner for ProcessCommandRunner {
    fn run(&self, command_line: &str) -> Result<CommandOutput, ExternalSecretError> {
        let mut command = shell_command(command_line);
        command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|_| ExternalSecretError::new(COULD_NOT_START))?;

        let stdout = child.stdout.take().ok_or_else(|| ExternalSecretError::new(COULD_NOT_START))?;
        let stderr = child.stderr.take().ok_or_else(|| ExternalSecretError::new(COULD_NOT_START))?;
        let output_reader = thread::spawn(move || read_all(stdout));
        // Error output is drained so the process cannot block, and is never shown: it may echo a value.
        let _error_drain = thread::spawn(move || read_all(stderr));

        let deadline = Instant::now() + self.timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {
                    if Instant::now() >= deadline {
                        kill_tree(&mut child);
                        return Err(ExternalSecretError::new("The command for a secret reference did not finish in time."));
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => return Err(ExternalSecretError::new("The command for a secret reference could not be run.")),
            }
        };

        let bytes = output_reader.join().unwrap_or_default();
        let text = String::from_utf8_lossy(&bytes).into_owned();
        Ok(CommandOutput { exit_code: status.code().unwrap_or(-1), standard_output: SecretValue::new(text) })
    }
}

type EnvironmentLookup = Box<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// Reads a Secret's value from an external source, on demand. It only reads: nothing is saved.
pub struct ExternalSecretResolver {
    environment: EnvironmentLookup,
    commands: Box<dyn CommandRunner>,
}

impl ExternalSecretResolver {
    pub fn new(environment: impl Fn(&str) -> Option<String> + Send + Sync + 'static, commands: impl CommandRunner + 'static) -> Self {
        Self { environment: Box::new(environment), commands: Box::new(commands) }
    }

    /// The resolver the app and the CLI use: the process environment and the system shell.
    pub fn system() -> Self {
        Self::new(|name| std::env::var(name).ok(), ProcessCommandRunner::default())
    }

    /// Resolves a reference to its value. Fails when the reference is invalid, or the source gives no value.
    pub fn resolve(&self, reference_text: &str) -> Result<SecretValue, ExternalSecretError> {
        let source = reference::parse(reference_text)?;
        match source.kind {
            ExternalSecretKind::EnvironmentVariable => self.from_environment(&source.target),
            ExternalSecretKind::Command => self.from_command(&source.target),
        }
    }

    fn from_environment(&self, name: &str) -> Result<SecretValue, ExternalSecretError> {
        match (self.environment)(name) {
            Some(value) if !value.is_empty() => Ok(SecretValue::new(value)),
            _ => Err(ExternalSecretError::new(format!(
                "Environment variable {name} is not set, so its Secret has no value on this machine."
            ))),
        }
    }

    fn from_command(&self, command_line: &str) -> Result<SecretValue, ExternalSecretError> {
        let result = self.commands.run(command_line)?;
        if result.exit_code != 0 {
            return Err(ExternalSecretError::new(format!(
                "The command for a secret reference exited with code {}. Run it yourself to see its error.",
                result.exit_code
            )));
        }

        let value = result.standard_output.expose().trim_end_matches(&['\r', '\n'][..]);
        if value.is_empty() {
            return Err(ExternalSecretError::new("The command for a secret reference printed no value."));
        }
        Ok(SecretValue::new(value))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    /// Records the command it was asked to run, and gives back a fixed output.
    struct FakeRunner {
        exit_code: i32,
        output: String,
        last: Arc<Mutex<Option<String>>>,
    }

    impl CommandRunner for FakeRunner {
        fn run(&self, command_line: &str) -> Result<CommandOutput, ExternalSecretError> {
            *self.last.lock().unwrap() = Some(command_line.to_string());
            Ok(CommandOutput { exit_code: self.exit_code, standard_output: SecretValue::new(self.output.clone()) })
        }
    }

    fn fake(exit_code: i32, output: &str) -> (FakeRunner, Arc<Mutex<Option<String>>>) {
        let last = Arc::new(Mutex::new(None));
        (FakeRunner { exit_code, output: output.to_string(), last: last.clone() }, last)
    }

    fn environment(values: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + Send + Sync + 'static {
        let map: HashMap<String, String> = values.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |name| map.get(name).cloned()
    }

    #[test]
    fn environment_reference_round_trips() {
        let source = reference::parse(&reference::for_environment_variable("API_TOKEN")).unwrap();

        assert_eq!(source, ExternalSecretSource { kind: ExternalSecretKind::EnvironmentVariable, target: "API_TOKEN".into() });
    }

    #[test]
    fn command_keeps_colons_and_spaces_in_the_line() {
        let line = "vault kv get -field=token secret/app:prod";

        let source = reference::parse(&reference::for_command(line)).unwrap();

        assert_eq!(source, ExternalSecretSource { kind: ExternalSecretKind::Command, target: line.into() });
    }

    #[test]
    fn invalid_references_are_rejected() {
        for value in [
            "secret-ref:env:",
            "secret-ref:env:has space",
            "secret-ref:env:A=B",
            "secret-ref:command:   ",
            "secret-ref:file:/etc/passwd",
            "secret-ref:",
        ] {
            assert!(reference::parse(value).is_err(), "{value} should be rejected");
        }
    }

    #[test]
    fn only_values_with_the_prefix_are_references() {
        assert!(!reference::is_reference("plain value"));
        assert!(!reference::is_reference("{{token}}"));
        assert!(reference::is_reference("secret-ref:env:X"));
    }

    #[test]
    fn reads_an_environment_variable() {
        let resolver = ExternalSecretResolver::new(environment(&[("API_TOKEN", "t-123")]), fake(0, "").0);

        let value = resolver.resolve(&reference::for_environment_variable("API_TOKEN")).unwrap();

        assert_eq!(value.expose(), "t-123");
    }

    #[test]
    fn missing_environment_variable_names_the_variable_not_a_value() {
        let resolver = ExternalSecretResolver::new(environment(&[]), fake(0, "").0);

        let error = resolver.resolve(&reference::for_environment_variable("API_TOKEN")).unwrap_err();

        assert!(error.message().contains("API_TOKEN"));
    }

    #[test]
    fn empty_environment_variable_counts_as_no_value() {
        let resolver = ExternalSecretResolver::new(environment(&[("API_TOKEN", "")]), fake(0, "").0);

        assert!(resolver.resolve(&reference::for_environment_variable("API_TOKEN")).is_err());
    }

    #[test]
    fn command_output_is_used_without_its_final_line_break() {
        let (runner, last) = fake(0, "s3cret\r\n");
        let resolver = ExternalSecretResolver::new(environment(&[]), runner);

        let value = resolver.resolve(&reference::for_command("vault read token")).unwrap();

        assert_eq!(value.expose(), "s3cret");
        assert_eq!(last.lock().unwrap().as_deref(), Some("vault read token"));
    }

    #[test]
    fn failed_command_throws_without_its_output() {
        let resolver = ExternalSecretResolver::new(environment(&[]), fake(3, "s3cret").0);

        let error = resolver.resolve(&reference::for_command("vault read token")).unwrap_err();

        assert!(error.message().contains('3'));
        assert!(!error.message().contains("s3cret"));
    }

    #[test]
    fn command_printing_nothing_throws() {
        let resolver = ExternalSecretResolver::new(environment(&[]), fake(0, "\n").0);

        assert!(resolver.resolve(&reference::for_command("vault read token")).is_err());
    }

    #[test]
    fn invalid_reference_is_rejected_before_anything_runs() {
        let (runner, last) = fake(0, "x");
        let resolver = ExternalSecretResolver::new(environment(&[]), runner);

        assert!(resolver.resolve("secret-ref:bogus:x").is_err());

        assert_eq!(*last.lock().unwrap(), None);
    }

    #[test]
    fn the_process_runner_reads_real_output() {
        // Not in the C# suite: it checks the real shell path with a command both cmd.exe and /bin/sh understand.
        let resolver = ExternalSecretResolver::new(environment(&[]), ProcessCommandRunner::default());

        let value = resolver.resolve(&reference::for_command("echo hello")).unwrap();

        assert_eq!(value.expose(), "hello");
    }

    #[test]
    fn the_process_runner_stops_a_command_that_runs_too_long() {
        // Not in the C# suite: a command that outlives its time limit is an error, not a hang.
        let runner = ProcessCommandRunner::new(Duration::from_millis(200));
        let line = if cfg!(windows) { "ping -n 10 127.0.0.1 >nul" } else { "sleep 10" };

        let error = runner.run(line).unwrap_err();

        assert!(error.message().contains("did not finish in time"));
    }
}
