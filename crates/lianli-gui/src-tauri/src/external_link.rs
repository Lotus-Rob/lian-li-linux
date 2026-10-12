use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use lianli_control::services::host_spawn_bridge;
use lianli_shared::installation::InstallationContext;

const EXIT_WAIT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(50);

pub fn open(url: &str) -> Result<(), String> {
    validate(url)?;
    let mut command = launcher(&InstallationContext::detect(), url, host_spawn_bridge)?;
    let child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("Cannot start xdg-open: {error}"))?;
    wait_for_launcher(child)
}

fn validate(url: &str) -> Result<(), String> {
    if !url.starts_with("https://") || url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("Only https links can be opened".into());
    }
    Ok(())
}

fn launcher(
    context: &InstallationContext,
    url: &str,
    bridge: impl FnOnce() -> Option<PathBuf>,
) -> Result<Command, String> {
    let mut command = match context {
        InstallationContext::Distrobox { .. } => {
            let bridge = bridge().ok_or(
                "host-spawn is not installed in this box. Run distrobox-host-exec true once inside the box, then try again",
            )?;
            let mut command = Command::new(bridge);
            command.arg("xdg-open");
            command
        }
        InstallationContext::Native | InstallationContext::UnsupportedContainer => {
            Command::new("xdg-open")
        }
    };
    command.arg(url);
    Ok(command)
}

fn wait_for_launcher(mut child: Child) -> Result<(), String> {
    let deadline = Instant::now() + EXIT_WAIT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(format!("xdg-open failed ({status})")),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(POLL_INTERVAL),
            Ok(None) => {
                std::thread::spawn(move || child.wait());
                return Ok(());
            }
            Err(error) => return Err(format!("Cannot check xdg-open: {error}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://example.com";

    #[test]
    fn only_plain_https_links_are_opened() {
        assert!(validate("https://github.com/sgtaziz/lian-li-linux").is_ok());
        assert!(validate("http://example.com").is_err());
        assert!(validate("file:///etc/passwd").is_err());
        assert!(validate("https://example.com/a b").is_err());
        assert!(validate("https://example.com/\n--help").is_err());
    }

    #[test]
    fn native_runs_xdg_open_directly() {
        let command = launcher(&InstallationContext::Native, URL, || None).unwrap();
        assert_eq!(command.get_program(), "xdg-open");
        assert_eq!(command.get_args().collect::<Vec<_>>(), [URL]);
    }

    #[test]
    fn distrobox_routes_xdg_open_through_the_host_bridge() {
        let boxed = InstallationContext::Distrobox {
            name: "lianli".into(),
        };
        let command = launcher(&boxed, URL, || Some("/usr/bin/host-spawn".into())).unwrap();
        assert_eq!(command.get_program(), "/usr/bin/host-spawn");
        assert_eq!(command.get_args().collect::<Vec<_>>(), ["xdg-open", URL]);
        assert!(launcher(&boxed, URL, || None).is_err());
    }
}
