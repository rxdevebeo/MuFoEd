//! The command line under the panels: commands run in the active panel's
//! folder through the system shell, their output streamed into a log.
//!
//! `cd` is handled here (it changes the panel's folder); everything else
//! goes to `cmd /C` or `PowerShell` on Windows and `sh -c` elsewhere.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};

/// The shell commands run through.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Shell {
    /// `cmd.exe /C` on Windows, `sh -c` elsewhere.
    #[default]
    System,
    /// Windows `PowerShell` (`powershell -NoProfile -Command`).
    PowerShell,
}

/// One line of the log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line {
    /// A command as typed, with its folder.
    Command(String),
    /// Standard output.
    Out(String),
    /// Standard error.
    Error(String),
    /// A note from the manager (exit codes, failures).
    Note(String),
}

/// What a typed line asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// Change the panel's folder.
    ChangeFolder(PathBuf),
    /// Clear the log.
    Clear,
    /// Run through the shell.
    Run(String),
    /// Nothing (an empty line).
    Nothing,
}

/// Interprets a typed line relative to `folder`.
pub fn interpret(line: &str, folder: &Path) -> Request {
    let line = line.trim();
    if line.is_empty() {
        return Request::Nothing;
    }
    if matches!(line, "cls" | "clear") {
        return Request::Clear;
    }
    let starts = |prefix: &str| {
        line.get(..prefix.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
    };
    let target = if line.eq_ignore_ascii_case("cd") {
        Some(
            std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map_or_else(|| folder.to_path_buf(), PathBuf::from),
        )
    } else if starts("cd ") || starts("cd\\") || starts("cd/") {
        // `cd\` and `cd/` keep their slash: it names the drive's root.
        let argument = if starts("cd ") {
            line[3..].trim()
        } else {
            &line[2..]
        };
        let argument = argument
            .strip_prefix("/d ")
            .or_else(|| argument.strip_prefix("/D "))
            .unwrap_or(argument);
        let path = PathBuf::from(argument.trim().trim_matches('"'));
        Some(if path.is_absolute() {
            path
        } else {
            folder.join(path)
        })
    } else if line.len() == 2 && line.ends_with(':') && cfg!(windows) {
        // `D:` switches drives, as in cmd.
        Some(PathBuf::from(format!("{line}\\")))
    } else {
        None
    };
    match target {
        Some(path) => Request::ChangeFolder(path),
        None => Request::Run(line.to_owned()),
    }
}

/// A running command.
pub struct Running {
    child: Child,
    lines: Receiver<Line>,
}

impl std::fmt::Debug for Running {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Running").finish_non_exhaustive()
    }
}

impl Running {
    /// Starts `command` in `folder`; `wake` is called on output.
    pub fn start(
        command: &str,
        folder: &Path,
        shell: Shell,
        wake: impl Fn() + Send + Sync + Clone + 'static,
    ) -> std::io::Result<Self> {
        let mut process = match (shell, cfg!(windows)) {
            (Shell::PowerShell, _) => {
                let mut process = Command::new(if cfg!(windows) { "powershell" } else { "pwsh" });
                process.args(["-NoProfile", "-NonInteractive", "-Command", command]);
                process
            }
            (Shell::System, true) => {
                let mut process = Command::new("cmd");
                process.arg("/C").arg(command);
                process
            }
            (Shell::System, false) => {
                let mut process = Command::new("sh");
                process.arg("-c").arg(command);
                process
            }
        };
        process
            .current_dir(folder)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW: the output comes here, not to a console.
            process.creation_flags(0x0800_0000);
        }
        let mut child = process.spawn()?;
        let (sender, lines) = channel();
        if let Some(stdout) = child.stdout.take() {
            pump(stdout, sender.clone(), false, wake.clone());
        }
        if let Some(stderr) = child.stderr.take() {
            pump(stderr, sender, true, wake);
        }
        Ok(Self { child, lines })
    }

    /// The lines that arrived since the last call.
    pub fn take_lines(&self) -> Vec<Line> {
        self.lines.try_iter().collect()
    }

    /// The exit code once the command has ended.
    pub fn finished(&mut self) -> Option<Option<i32>> {
        match self.child.try_wait() {
            Ok(Some(status)) => Some(status.code()),
            Ok(None) => None,
            Err(_) => Some(None),
        }
    }

    /// Stops the command.
    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            self.kill();
        }
    }
}

/// Reads a stream line by line on its own thread. Console programs on
/// Windows write the OEM code page; text that is not UTF-8 is decoded as
/// IBM 866 there (the usual Russian OEM page), as Windows-1252 elsewhere.
fn pump(
    stream: impl Read + Send + 'static,
    sender: Sender<Line>,
    error: bool,
    wake: impl Fn() + Send + 'static,
) {
    let _ = std::thread::Builder::new()
        .name("file-manager-terminal".into())
        .spawn(move || {
            let mut reader = BufReader::new(stream);
            let mut buffer = Vec::new();
            loop {
                buffer.clear();
                match reader.read_until(b'\n', &mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let text = decode(&buffer);
                        let text = text.trim_end_matches(['\r', '\n']).to_owned();
                        let line = if error {
                            Line::Error(text)
                        } else {
                            Line::Out(text)
                        };
                        if sender.send(line).is_err() {
                            break;
                        }
                        wake();
                    }
                }
            }
        });
}

/// Console bytes as text.
pub fn decode(bytes: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_owned();
    }
    let encoding = if cfg!(windows) {
        encoding_rs::IBM866
    } else {
        encoding_rs::WINDOWS_1252
    };
    encoding.decode(bytes).0.into_owned()
}

/// The typed commands, newest last, with a cursor for Up and Down.
#[derive(Clone, Debug, Default)]
pub struct History {
    entries: Vec<String>,
    cursor: Option<usize>,
}

impl History {
    /// Remembers a command (not repeating the last one).
    pub fn push(&mut self, command: &str) {
        if self.entries.last().map(String::as_str) != Some(command) && !command.trim().is_empty() {
            self.entries.push(command.to_owned());
            if self.entries.len() > 500 {
                self.entries.remove(0);
            }
        }
        self.cursor = None;
    }

    /// The previous command (Up).
    pub fn previous(&mut self) -> Option<&str> {
        let next = match self.cursor {
            None => self.entries.len().checked_sub(1)?,
            Some(0) => 0,
            Some(at) => at - 1,
        };
        self.cursor = Some(next);
        self.entries.get(next).map(String::as_str)
    }

    /// The next command (Down); `None` past the newest.
    pub fn newer(&mut self) -> Option<&str> {
        let at = self.cursor? + 1;
        if at >= self.entries.len() {
            self.cursor = None;
            return None;
        }
        self.cursor = Some(at);
        self.entries.get(at).map(String::as_str)
    }

    /// All commands, oldest first.
    pub fn entries(&self) -> &[String] {
        &self.entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cd_is_handled_here() {
        let folder = std::env::temp_dir();
        assert_eq!(
            interpret("cd sub", &folder),
            Request::ChangeFolder(folder.join("sub"))
        );
        assert_eq!(interpret("  ", &folder), Request::Nothing);
        assert_eq!(interpret("cls", &folder), Request::Clear);
        assert_eq!(
            interpret("echo hi", &folder),
            Request::Run("echo hi".into())
        );
        let absolute = if cfg!(windows) { "C:\\Windows" } else { "/usr" };
        assert_eq!(
            interpret(&format!("cd {absolute}"), &folder),
            Request::ChangeFolder(PathBuf::from(absolute))
        );
    }

    #[test]
    fn runs_a_command_and_streams_its_output() {
        let mut running = Running::start("echo hello", &std::env::temp_dir(), Shell::System, || {})
            .expect("starts");
        let mut lines = Vec::new();
        for _ in 0..400 {
            lines.extend(running.take_lines());
            if running.finished().is_some() && lines.iter().any(|line| matches!(line, Line::Out(_)))
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            lines
                .iter()
                .any(|line| matches!(line, Line::Out(text) if text.contains("hello"))),
            "{lines:?}"
        );
    }

    #[test]
    fn history_walks_both_ways() {
        let mut history = History::default();
        history.push("a");
        history.push("b");
        history.push("b");
        assert_eq!(history.entries().len(), 2);
        assert_eq!(history.previous(), Some("b"));
        assert_eq!(history.previous(), Some("a"));
        assert_eq!(history.previous(), Some("a"));
        assert_eq!(history.newer(), Some("b"));
        assert_eq!(history.newer(), None);
        assert_eq!(decode(&[0x8F, 0xE0, 0xA8]).chars().count(), 3);
    }
}
