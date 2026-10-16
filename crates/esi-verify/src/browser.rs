//! Opening the real EVE login for a person.

use std::process::{Command, Stdio};

use url::Url;

pub trait Browser {
    /// Fails, without side effects, where no browser and person can be reached.
    fn ensure_available(&self) -> Result<(), String>;
    fn open(&self, url: &Url) -> Result<(), String>;
}

/// The desktop's browser, through `xdg-open` (`open` on macOS).
pub struct SystemBrowser;

const NEEDS_A_PERSON: &str =
    "esi-verify needs a browser and a person to log in with the verification character";

/// Linux needs a display server to show a browser on.
pub fn display_available(os: &str, env: impl Fn(&str) -> Option<String>) -> Result<(), String> {
    if os != "linux" {
        return Ok(());
    }
    let set = |name: &str| env(name).is_some_and(|v| !v.is_empty());
    if set("DISPLAY") || set("WAYLAND_DISPLAY") {
        Ok(())
    } else {
        Err(format!(
            "{NEEDS_A_PERSON}, but neither DISPLAY nor WAYLAND_DISPLAY is set"
        ))
    }
}

impl Browser for SystemBrowser {
    fn ensure_available(&self) -> Result<(), String> {
        display_available(std::env::consts::OS, |name| std::env::var(name).ok())
    }

    fn open(&self, url: &Url) -> Result<(), String> {
        let program = if std::env::consts::OS == "macos" {
            "open"
        } else {
            "xdg-open"
        };
        Command::new(program)
            .arg(url.as_str())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("{NEEDS_A_PERSON}: cannot run {program}: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_without_a_display_fails_and_says_a_browser_and_a_person_are_needed() {
        let message = display_available("linux", |_| None).unwrap_err();
        assert!(message.contains("browser and a person"), "{message}");
        assert!(display_available("linux", |n| (n == "DISPLAY").then(|| ":0".to_owned())).is_ok());
        assert!(
            display_available("linux", |n| (n == "WAYLAND_DISPLAY")
                .then(|| "wayland-0".to_owned()))
            .is_ok()
        );
        assert!(display_available("linux", |_| Some(String::new())).is_err());
    }

    #[test]
    fn other_systems_are_not_asked_for_a_display() {
        assert!(display_available("macos", |_| None).is_ok());
    }
}
