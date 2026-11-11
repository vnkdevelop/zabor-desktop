use std::path::Path;

#[derive(Debug, PartialEq, Eq)]
pub enum AutostartError {
    InvalidPath,
    CommandFailed(String),
}

impl std::fmt::Display for AutostartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPath => write!(f, "invalid executable path for autostart"),
            Self::CommandFailed(msg) => write!(f, "autostart command failed: {msg}"),
        }
    }
}

impl std::error::Error for AutostartError {}

pub const REG_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
pub const VALUE_NAME: &str = "zabor";

pub fn format_command(exe_path: &Path) -> Result<String, AutostartError> {
    let path_str = exe_path.to_str().ok_or(AutostartError::InvalidPath)?;
    if path_str.trim().is_empty() {
        return Err(AutostartError::InvalidPath);
    }
    let cleaned = path_str.trim_matches('"');
    Ok(format!("\"{}\" --autostart", cleaned))
}

pub fn configure_autostart(exe_path: &Path, enabled: bool) -> Result<(), AutostartError> {
    if enabled {
        let cmd = format_command(exe_path)?;
        let status = std::process::Command::new("reg")
            .args([
                "add", REG_KEY, "/v", VALUE_NAME, "/t", "REG_SZ", "/d", &cmd, "/f",
            ])
            .status()
            .map_err(|e| AutostartError::CommandFailed(e.to_string()))?;
        if !status.success() {
            return Err(AutostartError::CommandFailed(
                "reg add exited with non-zero status".into(),
            ));
        }
    } else {
        let _ = std::process::Command::new("reg")
            .args(["delete", REG_KEY, "/v", VALUE_NAME, "/f"])
            .status();
    }
    Ok(())
}

pub fn is_autostart_enabled() -> Result<bool, AutostartError> {
    let output = std::process::Command::new("reg")
        .args(["query", REG_KEY, "/v", VALUE_NAME])
        .output()
        .map_err(|e| AutostartError::CommandFailed(e.to_string()))?;
    Ok(output.status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_command_strictly_quoted() {
        let p1 = Path::new(r"C:\Program Files\Zabor\zabor-native.exe");
        let cmd1 = format_command(p1).unwrap();
        assert_eq!(
            cmd1,
            r#""C:\Program Files\Zabor\zabor-native.exe" --autostart"#
        );
        assert!(cmd1.starts_with('"'));
        assert!(cmd1.contains(r#"zabor-native.exe" --autostart"#));

        let p2 = Path::new(r#""C:\Program Files\Zabor\zabor-native.exe""#);
        let cmd2 = format_command(p2).unwrap();
        assert_eq!(
            cmd2,
            r#""C:\Program Files\Zabor\zabor-native.exe" --autostart"#
        );

        let p3 = Path::new(r"C:\zabor\app.exe");
        let cmd3 = format_command(p3).unwrap();
        assert_eq!(cmd3, r#""C:\zabor\app.exe" --autostart"#);
    }

    #[test]
    fn test_format_command_rejects_empty() {
        assert!(format_command(Path::new("")).is_err());
        assert!(format_command(Path::new("   ")).is_err());
    }
}
