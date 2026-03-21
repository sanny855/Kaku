use std::ffi::OsStr;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub enum ShellKind {
    Zsh,
    Fish,
    Unsupported(String),
    Unknown,
}

impl ShellKind {
    pub fn is_managed(&self) -> bool {
        matches!(self, ShellKind::Zsh | ShellKind::Fish)
    }

    pub fn name(&self) -> &str {
        match self {
            ShellKind::Zsh => "zsh",
            ShellKind::Fish => "fish",
            ShellKind::Unsupported(s) => s.as_str(),
            ShellKind::Unknown => "unknown",
        }
    }
}

pub fn detect_shell_kind() -> ShellKind {
    match std::env::var("SHELL") {
        Err(_) => ShellKind::Unknown,
        Ok(s) => match Path::new(&s)
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or("")
        {
            "zsh" => ShellKind::Zsh,
            "fish" => ShellKind::Fish,
            "" => ShellKind::Unknown,
            other => ShellKind::Unsupported(other.to_string()),
        },
    }
}
