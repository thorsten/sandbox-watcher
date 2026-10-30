//! User settings. Persistence lives in the app shell; this is the model,
//! its defaults and validation, and the legacy environment overrides.

use serde::{Deserialize, Serialize};

use crate::model::RuntimeKind;
use crate::usage::{PriceTable, default_prices};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

/// Which terminal "Open terminal" uses on macOS. Linux honours `$TERMINAL`
/// and Windows uses Windows Terminal, so only macOS needs a choice today.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "kind", content = "command")]
pub enum TerminalChoice {
    /// iTerm2 when installed, Terminal.app otherwise.
    #[default]
    Auto,
    Iterm,
    TerminalApp,
    /// A command to run; `{dir}` is replaced by the project directory.
    Custom(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct NotificationSettings {
    pub enabled: bool,
    pub session_started: bool,
    pub session_ended: bool,
    pub container_started: bool,
    pub container_stopped: bool,
    pub container_died: bool,
    pub blocked_burst: bool,
    pub stale_container: bool,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            session_started: true,
            session_ended: false,
            container_started: true,
            container_stopped: true,
            container_died: true,
            blocked_burst: true,
            stale_container: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct Settings {
    /// Fallback refresh when no runtime events arrive, in seconds.
    pub poll_interval_secs: u64,
    /// Use only this runtime. `None` follows sandbox's config, then detection.
    pub runtime_override: Option<RuntimeKind>,
    pub terminal: TerminalChoice,
    pub confirm_stop: bool,
    pub notifications: NotificationSettings,
    pub launch_at_login: bool,
    /// Closing the window keeps the app in the tray.
    pub close_to_tray: bool,
    /// Start without showing the window.
    pub start_minimized: bool,
    pub theme: Theme,
    /// Blocked requests within `blocked_burst_window_secs` that count as a burst.
    pub blocked_burst_threshold: u64,
    pub blocked_burst_window_secs: u64,
    /// Keep closed history records this long.
    pub history_retention_days: u64,
    pub history_enabled: bool,
    /// USD per million tokens, keyed by model id prefix.
    pub prices: PriceTable,
    /// Look for a new release on start and every few hours.
    pub check_for_updates: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            poll_interval_secs: 10,
            runtime_override: None,
            terminal: TerminalChoice::Auto,
            confirm_stop: true,
            notifications: NotificationSettings::default(),
            launch_at_login: false,
            close_to_tray: true,
            start_minimized: false,
            theme: Theme::System,
            blocked_burst_threshold: 5,
            blocked_burst_window_secs: 60,
            history_retention_days: 30,
            history_enabled: true,
            prices: default_prices(),
            check_for_updates: true,
        }
    }
}

impl Settings {
    /// Clamp values into sane ranges. Applied after loading and before saving.
    pub fn normalised(mut self) -> Self {
        self.poll_interval_secs = self.poll_interval_secs.clamp(2, 300);
        self.blocked_burst_threshold = self.blocked_burst_threshold.clamp(1, 10_000);
        self.blocked_burst_window_secs = self.blocked_burst_window_secs.clamp(10, 3_600);
        self.history_retention_days = self.history_retention_days.clamp(1, 3_650);
        if self.prices.is_empty() {
            self.prices = default_prices();
        }
        if let TerminalChoice::Custom(cmd) = &self.terminal
            && cmd.trim().is_empty()
        {
            self.terminal = TerminalChoice::Auto;
        }
        self
    }

    /// Parse the JSON file contents. Unknown or invalid files yield defaults
    /// so a damaged settings file never blocks the app.
    pub fn from_json(text: &str) -> Self {
        serde_json::from_str::<Settings>(text)
            .unwrap_or_default()
            .normalised()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into())
    }

    /// Settings win over environment variables, which win over nothing.
    /// Keeps `SANDBOX_WATCHER_RUNTIME` and `SANDBOX_WATCHER_TERMINAL` working.
    pub fn with_env_fallbacks(
        mut self,
        runtime_env: Option<&str>,
        terminal_env: Option<&str>,
    ) -> Self {
        if self.runtime_override.is_none() {
            self.runtime_override = runtime_env.and_then(|v| match v.trim() {
                "docker" => Some(RuntimeKind::Docker),
                "podman" => Some(RuntimeKind::Podman),
                "apple-container" | "apple" => Some(RuntimeKind::AppleContainer),
                _ => None,
            });
        }
        if self.terminal == TerminalChoice::Auto {
            self.terminal = match terminal_env
                .map(|v| v.trim().to_ascii_lowercase())
                .as_deref()
            {
                Some("iterm" | "iterm2") => TerminalChoice::Iterm,
                Some("terminal" | "terminal.app") => TerminalChoice::TerminalApp,
                _ => TerminalChoice::Auto,
            };
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip_and_tolerate_garbage() {
        let s = Settings::default();
        assert_eq!(Settings::from_json(&s.to_json()), s);
        assert_eq!(Settings::from_json("not json"), s);
        assert_eq!(Settings::from_json("{}"), s);
        // Partial files keep defaults for missing keys.
        let partial = Settings::from_json(r#"{"poll_interval_secs": 1, "theme": "dark"}"#);
        assert_eq!(partial.poll_interval_secs, 2, "clamped");
        assert_eq!(partial.theme, Theme::Dark);
        assert!(partial.confirm_stop);
    }

    #[test]
    fn terminal_choice_serialises_with_kind_tag() {
        let s = Settings {
            terminal: TerminalChoice::Custom("wezterm start --cwd {dir}".into()),
            ..Default::default()
        };
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["terminal"]["kind"], "custom");
        assert_eq!(json["terminal"]["command"], "wezterm start --cwd {dir}");
        let empty = Settings {
            terminal: TerminalChoice::Custom("  ".into()),
            ..Default::default()
        }
        .normalised();
        assert_eq!(empty.terminal, TerminalChoice::Auto);
    }

    #[test]
    fn env_fallbacks_only_fill_gaps() {
        let s = Settings::default().with_env_fallbacks(Some("podman"), Some("iterm"));
        assert_eq!(s.runtime_override, Some(RuntimeKind::Podman));
        assert_eq!(s.terminal, TerminalChoice::Iterm);
        let pinned = Settings {
            runtime_override: Some(RuntimeKind::Docker),
            ..Default::default()
        }
        .with_env_fallbacks(Some("podman"), None);
        assert_eq!(pinned.runtime_override, Some(RuntimeKind::Docker));
    }
}
