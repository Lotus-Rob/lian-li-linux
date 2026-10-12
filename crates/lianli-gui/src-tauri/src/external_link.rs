use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use lianli_shared::installation::InstallationContext;

const HOST_SPAWN: [&str; 2] = ["/usr/bin/host-spawn", "/usr/local/bin/host-spawn"];
const EXIT_WAIT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(50);

pub fn open(url: &str) -> Result<(), String> {
    validate(url)?;
    let mut command = launcher(&InstallationContext::detect(), url)?;
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

fn launcher(context: &InstallationContext, url: &str) -> Result<Command, String> {
    match context {
        InstallationContext::Distrobox { .. } => {
            let bridge = HOST_SPAWN
                .into_iter()
                .find(|path| Path::new(path).is_file())
                .ok_or("host-spawn is not installed in this box. Run distrobox-host-exec true once inside the box, then try again")?;
            let mut command = Command::new(bridge);
            command.args(["xdg-open", url]);
            Ok(command)
        }
        InstallationContext::Native | InstallationContext::UnsupportedContainer => {
            let mut command = Command::new("xdg-open");
            command.arg(url);
            Ok(command)
        }
    }
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

    #[test]
    fn only_plain_https_links_are_opened() {
        assert!(validate("https://github.com/sgtaziz/lian-li-linux").is_ok());
        assert!(validate("http://example.com").is_err());
        assert!(validate("file:///etc/passwd").is_err());
        assert!(validate("https://example.com/a b").is_err());
        assert!(validate("https://example.com/\n--help").is_err());
    }

    #[test]
    fn distrobox_routes_xdg_open_through_the_host_bridge() {
        let native = launcher(&InstallationContext::Native, "https://example.com").unwrap();
        assert_eq!(native.get_program(), "xdg-open");
        let boxed = launcher(
            &InstallationContext::Distrobox {
                name: "lianli".into(),
            },
            "https://example.com",
        );
        if let Ok(command) = boxed {
            assert!(HOST_SPAWN.iter().any(|path| command.get_program() == *path));
            assert_eq!(
                command.get_args().collect::<Vec<_>>(),
                ["xdg-open", "https://example.com"]
            );
        }
    }
}
