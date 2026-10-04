//! Color theme and semantic color formatting.

use console::{style, Color, Term};
use std::sync::OnceLock;

/// Color theme for the CLI
#[derive(Debug, Clone)]
pub struct ColorTheme {
    pub success: Color,
    pub error: Color,
    pub warning: Color,
    pub info: Color,
    pub primary: Color,
    pub secondary: Color,
    pub dim: Color,
}

impl Default for ColorTheme {
    fn default() -> Self {
        Self {
            success: Color::Green,
            error: Color::Red,
            warning: Color::Yellow,
            info: Color::Blue,
            primary: Color::Cyan,
            secondary: Color::Magenta,
            dim: Color::Color256(8), // Dark gray
        }
    }
}

/// Color formatter with theme support
pub struct ColorFormatter {
    theme: ColorTheme,
    colors_enabled: bool,
}

impl ColorFormatter {
    /// Create a new color formatter
    pub fn new(colors_enabled: bool) -> Self {
        Self {
            theme: ColorTheme::default(),
            colors_enabled: colors_enabled && Term::stdout().features().colors_supported(),
        }
    }

    /// Create formatter with custom theme
    pub fn with_theme(theme: ColorTheme, colors_enabled: bool) -> Self {
        Self {
            theme,
            colors_enabled: colors_enabled && Term::stdout().features().colors_supported(),
        }
    }

    /// Format success message
    pub fn success(&self, text: &str) -> String {
        if self.colors_enabled {
            style(text).fg(self.theme.success).bold().to_string()
        } else {
            text.to_string()
        }
    }

    /// Format error message
    pub fn error(&self, text: &str) -> String {
        if self.colors_enabled {
            style(text).fg(self.theme.error).bold().to_string()
        } else {
            text.to_string()
        }
    }

    /// Format warning message
    pub fn warning(&self, text: &str) -> String {
        if self.colors_enabled {
            style(text).fg(self.theme.warning).bold().to_string()
        } else {
            text.to_string()
        }
    }

    /// Format info message
    pub fn info(&self, text: &str) -> String {
        if self.colors_enabled {
            style(text).fg(self.theme.info).to_string()
        } else {
            text.to_string()
        }
    }

    /// Format primary text
    pub fn primary(&self, text: &str) -> String {
        if self.colors_enabled {
            style(text).fg(self.theme.primary).bold().to_string()
        } else {
            text.to_string()
        }
    }

    /// Format secondary text
    pub fn secondary(&self, text: &str) -> String {
        if self.colors_enabled {
            style(text).fg(self.theme.secondary).to_string()
        } else {
            text.to_string()
        }
    }

    /// Format dimmed text
    pub fn dim(&self, text: &str) -> String {
        if self.colors_enabled {
            style(text).fg(self.theme.dim).to_string()
        } else {
            text.to_string()
        }
    }

    /// Format bold text
    pub fn bold(&self, text: &str) -> String {
        if self.colors_enabled {
            style(text).bold().to_string()
        } else {
            text.to_string()
        }
    }

    /// Format italic text
    pub fn italic(&self, text: &str) -> String {
        if self.colors_enabled {
            style(text).italic().to_string()
        } else {
            text.to_string()
        }
    }

    /// Format underlined text
    pub fn underline(&self, text: &str) -> String {
        if self.colors_enabled {
            style(text).underlined().to_string()
        } else {
            text.to_string()
        }
    }

    /// Format strikethrough text
    pub fn strikethrough(&self, text: &str) -> String {
        if self.colors_enabled {
            style(text).strikethrough().to_string()
        } else {
            text.to_string()
        }
    }

    /// Format resource status with appropriate colors
    pub fn status(&self, status: &str) -> String {
        let color = match status.to_lowercase().as_str() {
            "running" | "active" | "ready" => self.theme.success,
            "completed" | "success" | "succeeded" => Color::Green,
            "failed" | "error" | "failure" => self.theme.error,
            "pending" | "waiting" | "starting" => self.theme.warning,
            "stopped" | "terminated" | "finished" => self.theme.dim,
            _ => self.theme.info,
        };

        if self.colors_enabled {
            style(status).fg(color).to_string()
        } else {
            status.to_string()
        }
    }

    /// Format percentage with color gradient
    pub fn percentage(&self, percentage: f64) -> String {
        let color = if percentage >= 90.0 {
            Color::Green
        } else if percentage >= 70.0 {
            self.theme.success
        } else if percentage >= 50.0 {
            self.theme.warning
        } else {
            Color::Red
        };

        let text = format!("{:.1}%", percentage);
        if self.colors_enabled {
            style(text).fg(color).to_string()
        } else {
            text
        }
    }

    /// Format duration with appropriate colors
    pub fn duration(&self, duration: &str) -> String {
        // Parse duration to determine color
        let color = if duration.contains('d') || duration.contains("day") {
            self.theme.error // Very long duration
        } else if duration.contains('h') || duration.contains("hour") {
            self.theme.warning // Long duration
        } else if duration.contains('m') || duration.contains("min") {
            self.theme.info // Medium duration
        } else {
            self.theme.success // Short duration
        };

        if self.colors_enabled {
            style(duration).fg(color).to_string()
        } else {
            duration.to_string()
        }
    }

    /// Create a colored progress bar
    pub fn progress_bar(&self, current: usize, total: usize, width: usize) -> String {
        if !self.colors_enabled {
            return format!("[{}/{}]", current, total);
        }

        let percentage = if total > 0 {
            current as f64 / total as f64
        } else {
            0.0
        };

        let filled = (width as f64 * percentage) as usize;
        let empty = width.saturating_sub(filled);

        let filled_bar = "█".repeat(filled);
        let empty_bar = "░".repeat(empty);

        let color = if percentage >= 0.9 {
            self.theme.success
        } else if percentage >= 0.5 {
            self.theme.warning
        } else {
            self.theme.error
        };

        format!(
            "[{}{}] {}/{}",
            style(filled_bar).fg(color),
            style(empty_bar).fg(self.theme.dim),
            current,
            total
        )
    }

    /// Format table header
    pub fn table_header(&self, text: &str) -> String {
        if self.colors_enabled {
            style(text).bold().underlined().to_string()
        } else {
            text.to_string()
        }
    }

    /// Format code or technical text
    pub fn code(&self, text: &str) -> String {
        if self.colors_enabled {
            style(text).fg(Color::Color256(117)).to_string() // Light blue
        } else {
            format!("`{}`", text)
        }
    }

    /// Format file paths
    pub fn path(&self, path: &str) -> String {
        if self.colors_enabled {
            style(path).fg(Color::Color256(214)).to_string() // Orange
        } else {
            path.to_string()
        }
    }

    /// Format URLs
    pub fn url(&self, url: &str) -> String {
        if self.colors_enabled {
            style(url).fg(Color::Blue).underlined().to_string()
        } else {
            url.to_string()
        }
    }

    /// Format timestamps
    pub fn timestamp(&self, timestamp: &str) -> String {
        if self.colors_enabled {
            style(timestamp).fg(self.theme.dim).to_string()
        } else {
            timestamp.to_string()
        }
    }

    /// Check if colors are enabled
    pub fn colors_enabled(&self) -> bool {
        self.colors_enabled
    }
}

/// Global color formatter instance
static GLOBAL_FORMATTER: OnceLock<ColorFormatter> = OnceLock::new();

/// Initialize global color formatter
pub fn init_colors(colors_enabled: bool) {
    GLOBAL_FORMATTER
        .set(ColorFormatter::new(colors_enabled))
        .unwrap_or_else(|_| panic!("Color formatter already initialized"));
}

/// Get global color formatter
pub fn colors() -> &'static ColorFormatter {
    GLOBAL_FORMATTER
        .get()
        .unwrap_or_else(|| panic!("Color formatter not initialized"))
}

/// Convenience functions for common formatting
pub fn success(text: &str) -> String {
    colors().success(text)
}

pub fn error(text: &str) -> String {
    colors().error(text)
}

pub fn warning(text: &str) -> String {
    colors().warning(text)
}

pub fn info(text: &str) -> String {
    colors().info(text)
}

pub fn primary(text: &str) -> String {
    colors().primary(text)
}

pub fn secondary(text: &str) -> String {
    colors().secondary(text)
}

pub fn dim(text: &str) -> String {
    colors().dim(text)
}

pub fn bold(text: &str) -> String {
    colors().bold(text)
}

pub fn code(text: &str) -> String {
    colors().code(text)
}

pub fn path(text: &str) -> String {
    colors().path(text)
}

pub fn status(text: &str) -> String {
    colors().status(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_formatter_creation() {
        // When colors_enabled is false, it should always be false
        let formatter = ColorFormatter::new(false);
        assert!(!formatter.colors_enabled);

        // When colors_enabled is true, it depends on terminal capabilities
        // In test environment (no TTY), colors are typically not supported
        // so we just verify the formatter is created successfully
        let formatter = ColorFormatter::new(true);
        // colors_enabled will be true only if terminal supports colors
        // In CI/test environments without TTY, this is typically false
        let _ = formatter.colors_enabled; // Just verify it's accessible
    }

    #[test]
    fn test_status_formatting() {
        let formatter = ColorFormatter::new(false); // No colors for testing

        assert_eq!(formatter.status("running"), "running");
        assert_eq!(formatter.status("failed"), "failed");
        assert_eq!(formatter.status("pending"), "pending");
    }

    #[test]
    fn test_percentage_formatting() {
        let formatter = ColorFormatter::new(false);

        assert_eq!(formatter.percentage(95.5), "95.5%");
        assert_eq!(formatter.percentage(50.0), "50.0%");
        assert_eq!(formatter.percentage(25.0), "25.0%");
    }

    #[test]
    fn test_progress_bar() {
        let formatter = ColorFormatter::new(false);

        let bar = formatter.progress_bar(5, 10, 10);
        assert!(bar.contains("5/10"));

        let bar = formatter.progress_bar(0, 0, 10);
        assert!(bar.contains("0/0"));
    }

    #[test]
    fn test_basic_formatting() {
        let formatter = ColorFormatter::new(false);

        assert_eq!(formatter.success("test"), "test");
        assert_eq!(formatter.error("test"), "test");
        assert_eq!(formatter.warning("test"), "test");
        assert_eq!(formatter.info("test"), "test");
        assert_eq!(formatter.bold("test"), "test");
        assert_eq!(formatter.dim("test"), "test");
    }

    #[test]
    fn test_custom_theme() {
        let theme = ColorTheme {
            success: Color::Blue,
            error: Color::Yellow,
            warning: Color::Green,
            info: Color::Red,
            primary: Color::Magenta,
            secondary: Color::Cyan,
            dim: Color::White,
        };

        let formatter = ColorFormatter::with_theme(theme, false);
        // Test that formatter was created with custom theme
        assert!(!formatter.colors_enabled);
    }
}
