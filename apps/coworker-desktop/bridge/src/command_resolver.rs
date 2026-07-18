use std::{
    ffi::OsStr,
    io,
    path::{Path, PathBuf},
};
use tokio::process::Command;

#[cfg(windows)]
use std::ffi::OsString;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Debug, Clone)]
pub struct ResolvedCommand {
    executable: PathBuf,
    display_path: PathBuf,
}

impl ResolvedCommand {
    pub fn command(&self) -> Command {
        let mut command = Command::new(self.executable());
        suppress_console_window(&mut command);
        command
    }

    pub fn executable(&self) -> &Path {
        &self.executable
    }

    pub fn display_path(&self) -> &Path {
        &self.display_path
    }

    fn direct(path: PathBuf) -> Self {
        Self {
            executable: path.clone(),
            display_path: path,
        }
    }
}

#[cfg(windows)]
fn suppress_console_window(command: &mut Command) {
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn suppress_console_window(_command: &mut Command) {}

pub fn resolve_command(command: &str) -> io::Result<ResolvedCommand> {
    let command = command.trim();
    if command.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "command is empty",
        ));
    }

    #[cfg(windows)]
    {
        resolve_windows_command(command)
    }

    #[cfg(not(windows))]
    {
        Ok(resolve_unix_command(command))
    }
}

#[cfg(not(windows))]
fn resolve_unix_command(command: &str) -> ResolvedCommand {
    let path = Path::new(command);
    if is_path_like(command, path) {
        return ResolvedCommand::direct(PathBuf::from(command));
    }

    resolve_unix_name(
        command,
        std::env::var_os("PATH").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )
    .map(ResolvedCommand::direct)
    .unwrap_or_else(|| ResolvedCommand::direct(PathBuf::from(command)))
}

#[cfg(not(windows))]
fn resolve_unix_name(
    command: &str,
    path_value: Option<&OsStr>,
    home_value: Option<&OsStr>,
) -> Option<PathBuf> {
    std::env::split_paths(path_value.unwrap_or_else(|| OsStr::new("")))
        .map(|dir| dir.join(command))
        .find(|candidate| candidate.is_file())
        .or_else(|| {
            default_unix_codex_path(command, home_value).filter(|candidate| candidate.is_file())
        })
}

#[cfg(not(windows))]
fn default_unix_codex_path(command: &str, home_value: Option<&OsStr>) -> Option<PathBuf> {
    if command != "codex" {
        return None;
    }
    home_value.map(|home| PathBuf::from(home).join(".local").join("bin").join("codex"))
}

#[cfg(windows)]
fn resolve_windows_command(command: &str) -> io::Result<ResolvedCommand> {
    let path = Path::new(command);
    let resolved = if is_path_like(command, path) {
        resolve_windows_path(path)
    } else {
        resolve_windows_name(command)
    };

    resolved.map(ResolvedCommand::direct).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("command {command:?} was not found on PATH"),
        )
    })
}

#[cfg(windows)]
fn resolve_windows_path(path: &Path) -> Option<PathBuf> {
    if path.is_file() {
        return Some(path.to_path_buf());
    }
    if path.extension().is_some() {
        return None;
    }
    windows_extensions()
        .into_iter()
        .map(|extension| path.with_extension(extension.trim_start_matches('.')))
        .find(|candidate| candidate.is_file())
}

#[cfg(windows)]
fn resolve_windows_name(command: &str) -> Option<PathBuf> {
    let path_value = std::env::var_os("PATH");
    resolve_windows_name_in_path(command, path_value.as_deref())
}

#[cfg(windows)]
fn resolve_windows_name_in_path(command: &str, path_value: Option<&OsStr>) -> Option<PathBuf> {
    let names = windows_candidate_names(command);
    std::env::split_paths(path_value.unwrap_or_else(|| OsStr::new("")))
        .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
        .find(|candidate| candidate.is_file())
}

#[cfg(windows)]
fn windows_candidate_names(command: &str) -> Vec<OsString> {
    let path = Path::new(command);
    if path.extension().is_some() {
        return vec![OsString::from(command)];
    }
    windows_extensions()
        .into_iter()
        .map(|extension| OsString::from(format!("{command}{extension}")))
        .collect()
}

#[cfg(windows)]
fn windows_extensions() -> Vec<String> {
    let mut extensions: Vec<String> = std::env::var_os("PATHEXT")
        .map(|value| {
            value
                .to_string_lossy()
                .split(';')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(normalize_extension)
                .collect()
        })
        .unwrap_or_else(|| {
            [".COM", ".EXE", ".BAT", ".CMD"]
                .into_iter()
                .map(String::from)
                .collect()
        });

    for extension in [".EXE", ".BAT", ".CMD"] {
        if !extensions
            .iter()
            .any(|current| current.eq_ignore_ascii_case(extension))
        {
            extensions.push(extension.into());
        }
    }
    extensions
}

#[cfg(windows)]
fn normalize_extension(extension: &str) -> String {
    if extension.starts_with('.') {
        extension.to_owned()
    } else {
        format!(".{extension}")
    }
}

#[cfg(windows)]
fn is_path_like(command: &str, path: &Path) -> bool {
    path.is_absolute() || command.contains('\\') || command.contains('/')
}

#[cfg(not(windows))]
fn is_path_like(command: &str, path: &Path) -> bool {
    path.is_absolute() || command.contains('/')
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn resolves_bat_from_path_for_extensionless_command() {
        let dir = temp_dir("bat");
        let script = dir.join("codex.bat");
        fs::write(&script, "@echo off\r\n").unwrap();
        let path_value = std::env::join_paths([dir.as_path()]).unwrap();

        let resolved = resolve_windows_name_in_path("codex", Some(&path_value)).unwrap();

        assert_path_eq_ignore_case(&resolved, &script);
        let _ = fs::remove_dir_all(dir);
    }

    fn temp_dir(name: &str) -> PathBuf {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let dir = std::env::temp_dir().join(format!(
            "coworker_desktop_{name}_{}_{}",
            std::process::id(),
            millis
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn assert_path_eq_ignore_case(left: &Path, right: &Path) {
        assert_eq!(
            left.to_string_lossy().to_ascii_lowercase(),
            right.to_string_lossy().to_ascii_lowercase()
        );
    }
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn resolves_unix_command_from_path() {
        let dir = temp_dir("path");
        let script = dir.join("codex");
        fs::write(&script, "#!/bin/sh\n").unwrap();
        let path_value = std::env::join_paths([dir.as_path()]).unwrap();

        let resolved = resolve_unix_name("codex", Some(path_value.as_os_str()), None).unwrap();

        assert_eq!(resolved, script);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn resolves_codex_from_home_local_bin_when_missing_from_path() {
        let home = temp_dir("home");
        let local_bin = home.join(".local").join("bin");
        fs::create_dir_all(&local_bin).unwrap();
        let script = local_bin.join("codex");
        fs::write(&script, "#!/bin/sh\n").unwrap();

        let resolved = resolve_unix_name("codex", None, Some(home.as_os_str())).unwrap();

        assert_eq!(resolved, script);
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn does_not_use_home_local_bin_for_other_commands() {
        let home = temp_dir("home-other");
        let local_bin = home.join(".local").join("bin");
        fs::create_dir_all(&local_bin).unwrap();
        fs::write(local_bin.join("node"), "#!/bin/sh\n").unwrap();

        let resolved = resolve_unix_name("node", None, Some(home.as_os_str()));

        assert!(resolved.is_none());
        let _ = fs::remove_dir_all(home);
    }

    fn temp_dir(name: &str) -> PathBuf {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let dir = std::env::temp_dir().join(format!(
            "coworker_desktop_unix_{name}_{}_{}",
            std::process::id(),
            millis
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
