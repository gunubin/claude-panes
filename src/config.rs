use ratatui::style::Color;
use serde::Deserialize;
use std::fs;

#[derive(Deserialize)]
struct RawConfig {
    border_color: Option<String>,
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
    pub strip_status: bool,
    pub list_percentage: u16,
    pub preview_percentage: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            border_color: Color::Cyan,
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
                eprintln!(
                    "Warning: could not read config {}: {}",
                    path.display(),
                    e
                );
                return Self::default();
            }
        };

        let raw: RawConfig = match toml::from_str(&content) {
            Ok(r) => r,
            Err(e) => {
                eprintln!(
                    "Warning: invalid config {}: {}",
                    path.display(),
                    e
                );
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

        let (list_percentage, preview_percentage) = if list_pct + preview_pct <= 100
            && list_pct > 0
            && preview_pct > 0
        {
            (list_pct, preview_pct)
        } else {
            eprintln!(
                "Warning: layout percentages must sum to <=100 and be >0, using defaults (30/70)"
            );
            (30, 70)
        };

        Self {
            border_color: parse_color(raw.border_color.as_deref().unwrap_or("cyan")),
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
