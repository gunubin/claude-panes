use ratatui::style::Color;
use serde::Deserialize;
use std::fs;

#[derive(Deserialize)]
struct RawConfig {
    border_color: Option<String>,
    notify_color: Option<String>,
    strip_status: Option<bool>,
    layout: Option<RawLayout>,
}

#[derive(Deserialize)]
struct RawLayout {
    list_percentage: Option<u16>,
    preview_percentage: Option<u16>,
}

pub struct Config {
    pub border_color: Color,
    pub notify_color: Color,
    pub strip_status: bool,
    pub list_percentage: u16,
    pub preview_percentage: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            border_color: Color::Cyan,
            notify_color: Color::Rgb(250, 179, 135),
            strip_status: true,
            list_percentage: 30,
            preview_percentage: 70,
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let path = match dirs::config_dir() {
            Some(d) => d.join("claude-panes").join("config.toml"),
            None => return Self::default(),
        };

        let content = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Self::default(),
            Err(e) => {
                eprintln!("Warning: could not read config {}: {}", path.display(), e);
                return Self::default();
            }
        };

        Self::parse_toml(&content)
    }

    fn parse_toml(content: &str) -> Self {
        let raw: RawConfig = match toml::from_str(content) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Warning: invalid config: {}", e);
                return Self::default();
            }
        };

        let list_pct = raw
            .layout
            .as_ref()
            .and_then(|l| l.list_percentage)
            .unwrap_or(30);
        let preview_pct = raw
            .layout
            .as_ref()
            .and_then(|l| l.preview_percentage)
            .unwrap_or(70);

        let (list_percentage, preview_percentage) =
            if list_pct + preview_pct <= 100 && list_pct > 0 && preview_pct > 0 {
                (list_pct, preview_pct)
            } else {
                eprintln!(
                "Warning: layout percentages must sum to <=100 and be >0, using defaults (30/70)"
            );
                (30, 70)
            };

        Self {
            border_color: parse_color(raw.border_color.as_deref().unwrap_or("cyan")),
            notify_color: raw
                .notify_color
                .as_deref()
                .map(parse_color)
                .unwrap_or(Color::Rgb(250, 179, 135)),
            strip_status: raw.strip_status.unwrap_or(true),
            list_percentage,
            preview_percentage,
        }
    }
}

fn parse_color(s: &str) -> Color {
    match s.to_lowercase().as_str() {
        "red" => Color::Red,
        "green" => Color::Green,
        "blue" => Color::Blue,
        "cyan" => Color::Cyan,
        "gray" | "grey" => Color::Gray,
        "white" => Color::White,
        "yellow" => Color::Yellow,
        "magenta" => Color::Magenta,
        hex if hex.starts_with('#') && hex.len() == 7 => {
            match (
                u8::from_str_radix(&hex[1..3], 16),
                u8::from_str_radix(&hex[3..5], 16),
                u8::from_str_radix(&hex[5..7], 16),
            ) {
                (Ok(r), Ok(g), Ok(b)) => Color::Rgb(r, g, b),
                _ => {
                    eprintln!("Warning: invalid hex color '{}', using cyan", s);
                    Color::Cyan
                }
            }
        }
        other => {
            eprintln!(
                "Warning: unknown color '{}', supported: red, green, blue, cyan, gray, white, yellow, magenta, or #RRGGBB",
                other
            );
            Color::Cyan
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_named() {
        assert_eq!(parse_color("cyan"), Color::Cyan);
        assert_eq!(parse_color("red"), Color::Red);
        assert_eq!(parse_color("green"), Color::Green);
        assert_eq!(parse_color("blue"), Color::Blue);
        assert_eq!(parse_color("white"), Color::White);
        assert_eq!(parse_color("yellow"), Color::Yellow);
        assert_eq!(parse_color("magenta"), Color::Magenta);
    }

    #[test]
    fn color_case_insensitive() {
        assert_eq!(parse_color("Cyan"), Color::Cyan);
        assert_eq!(parse_color("RED"), Color::Red);
    }

    #[test]
    fn color_gray_alias() {
        assert_eq!(parse_color("gray"), Color::Gray);
        assert_eq!(parse_color("grey"), Color::Gray);
    }

    #[test]
    fn color_hex_valid() {
        assert_eq!(parse_color("#ff0000"), Color::Rgb(255, 0, 0));
        assert_eq!(parse_color("#000000"), Color::Rgb(0, 0, 0));
        assert_eq!(parse_color("#ffffff"), Color::Rgb(255, 255, 255));
    }

    #[test]
    fn color_hex_invalid() {
        assert_eq!(parse_color("#gggggg"), Color::Cyan);
        assert_eq!(parse_color("#fff"), Color::Cyan); // too short
        assert_eq!(parse_color("#ff00ff00"), Color::Cyan); // too long
    }

    #[test]
    fn color_unknown_fallback() {
        assert_eq!(parse_color("orange"), Color::Cyan);
        assert_eq!(parse_color(""), Color::Cyan);
    }

    // --- Config parse_toml tests ---

    #[test]
    fn parse_full_config() {
        let toml = r##"
border_color = "green"
notify_color = "#ff8800"
strip_status = false

[layout]
list_percentage = 40
preview_percentage = 60
"##;
        let cfg = Config::parse_toml(toml);
        assert_eq!(cfg.border_color, Color::Green);
        assert_eq!(cfg.notify_color, Color::Rgb(255, 136, 0));
        assert!(!cfg.strip_status);
        assert_eq!(cfg.list_percentage, 40);
        assert_eq!(cfg.preview_percentage, 60);
    }

    #[test]
    fn parse_empty_string() {
        let cfg = Config::parse_toml("");
        assert_eq!(cfg.border_color, Color::Cyan);
        assert_eq!(cfg.notify_color, Color::Rgb(250, 179, 135));
        assert!(cfg.strip_status);
        assert_eq!(cfg.list_percentage, 30);
        assert_eq!(cfg.preview_percentage, 70);
    }

    #[test]
    fn parse_partial_config() {
        let toml = r#"border_color = "red""#;
        let cfg = Config::parse_toml(toml);
        assert_eq!(cfg.border_color, Color::Red);
        assert!(cfg.strip_status);
        assert_eq!(cfg.list_percentage, 30);
        assert_eq!(cfg.preview_percentage, 70);
    }

    #[test]
    fn parse_invalid_layout_sum() {
        let toml = r#"
[layout]
list_percentage = 60
preview_percentage = 60
"#;
        let cfg = Config::parse_toml(toml);
        assert_eq!(cfg.list_percentage, 30);
        assert_eq!(cfg.preview_percentage, 70);
    }

    #[test]
    fn parse_layout_zero() {
        let toml = r#"
[layout]
list_percentage = 0
preview_percentage = 70
"#;
        let cfg = Config::parse_toml(toml);
        assert_eq!(cfg.list_percentage, 30);
        assert_eq!(cfg.preview_percentage, 70);
    }
}
