//! Spinners and progress bars for long-running operations.

use crate::utils::colors::ColorFormatter;
use console::Term;
use indicatif::{ProgressBar, ProgressState, ProgressStyle};
use std::fmt::Write;
use std::time::{Duration, Instant};

/// Spinner types available for progress indication
#[derive(Debug, Clone)]
pub enum SpinnerType {
    /// Simple dots spinner
    Dots,
    /// Braille pattern spinner
    Braille,
    /// Line spinner
    Line,
    /// Arrow spinner
    Arrow,
    /// Clock spinner
    Clock,
    /// Bounce spinner
    Bounce,
    /// Custom spinner with provided frames
    Custom(Vec<String>),
}

impl SpinnerType {
    /// Get the frames for the spinner type
    pub fn frames(&self) -> Vec<&str> {
        match self {
            SpinnerType::Dots => vec!["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
            SpinnerType::Braille => vec!["⣾", "⣽", "⣻", "⢿", "⡿", "⣟", "⣯", "⣷"],
            SpinnerType::Line => vec!["|", "/", "-", "\\"],
            SpinnerType::Arrow => vec!["←", "↖", "↑", "↗", "→", "↘", "↓", "↙"],
            SpinnerType::Clock => vec![
                "🕐", "🕑", "🕒", "🕓", "🕔", "🕕", "🕖", "🕗", "🕘", "🕙", "🕚", "🕛",
            ],
            SpinnerType::Bounce => vec!["⠁", "⠂", "⠄", "⠂"],
            SpinnerType::Custom(frames) => frames.iter().map(|s| s.as_str()).collect(),
        }
    }

    /// Get the tick interval for the spinner
    pub fn tick_interval(&self) -> Duration {
        match self {
            SpinnerType::Dots => Duration::from_millis(80),
            SpinnerType::Braille => Duration::from_millis(80),
            SpinnerType::Line => Duration::from_millis(200),
            SpinnerType::Arrow => Duration::from_millis(120),
            SpinnerType::Clock => Duration::from_millis(100),
            SpinnerType::Bounce => Duration::from_millis(300),
            SpinnerType::Custom(_) => Duration::from_millis(100),
        }
    }
}

/// Spinner for indicating progress on long-running operations
pub struct Spinner {
    progress_bar: ProgressBar,
    message: String,
    colors_enabled: bool,
    started_at: Instant,
}

impl Spinner {
    /// Create a new spinner with the default type
    pub fn new(message: &str, colors_enabled: bool) -> Self {
        Self::with_type(message, SpinnerType::Braille, colors_enabled)
    }

    /// Create a new spinner with a specific type
    pub fn with_type(message: &str, spinner_type: SpinnerType, colors_enabled: bool) -> Self {
        let progress_bar = ProgressBar::new_spinner();

        // Build the template
        let template = if colors_enabled {
            "{spinner:.blue} {msg} [{elapsed_precise}]"
        } else {
            "{spinner} {msg} [{elapsed_precise}]"
        };

        // Create the spinner style
        let frames = spinner_type.frames();
        let tick_chars = frames.join("");

        let style = ProgressStyle::with_template(template)
            .unwrap()
            .tick_chars(&tick_chars);

        progress_bar.set_style(style);
        progress_bar.set_message(message.to_string());
        progress_bar.enable_steady_tick(spinner_type.tick_interval());

        Self {
            progress_bar,
            message: message.to_string(),
            colors_enabled,
            started_at: Instant::now(),
        }
    }

    /// Update the spinner message
    pub fn set_message(&mut self, message: &str) {
        self.message = message.to_string();
        self.progress_bar.set_message(message.to_string());
    }

    /// Append to the current message
    pub fn append_message(&mut self, suffix: &str) {
        let new_message = format!("{} {}", self.message, suffix);
        self.set_message(&new_message);
    }

    /// Finish the spinner with a success message
    pub fn finish_with_message(&self, message: &str) {
        let elapsed = self.started_at.elapsed();
        let elapsed_str = format_duration(elapsed);

        if self.colors_enabled {
            let formatter = ColorFormatter::new(true);
            self.progress_bar.finish_with_message(format!(
                "{} {} [{}]",
                formatter.success("✓"),
                message,
                elapsed_str
            ));
        } else {
            self.progress_bar
                .finish_with_message(format!("✓ {} [{}]", message, elapsed_str));
        }
    }

    /// Finish the spinner with an error message
    pub fn finish_with_error(&self, message: &str) {
        let elapsed = self.started_at.elapsed();
        let elapsed_str = format_duration(elapsed);

        if self.colors_enabled {
            let formatter = ColorFormatter::new(true);
            self.progress_bar.finish_with_message(format!(
                "{} {} [{}]",
                formatter.error("✗"),
                message,
                elapsed_str
            ));
        } else {
            self.progress_bar
                .finish_with_message(format!("✗ {} [{}]", message, elapsed_str));
        }
    }

    /// Finish the spinner with a warning message
    pub fn finish_with_warning(&self, message: &str) {
        let elapsed = self.started_at.elapsed();
        let elapsed_str = format_duration(elapsed);

        if self.colors_enabled {
            let formatter = ColorFormatter::new(true);
            self.progress_bar.finish_with_message(format!(
                "{} {} [{}]",
                formatter.warning("⚠"),
                message,
                elapsed_str
            ));
        } else {
            self.progress_bar
                .finish_with_message(format!("⚠ {} [{}]", message, elapsed_str));
        }
    }

    /// Clear the spinner without a message
    pub fn clear(&self) {
        self.progress_bar.finish_and_clear();
    }

    /// Suspend the spinner temporarily
    pub fn suspend<F, R>(&self, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        self.progress_bar.suspend(f)
    }

    /// Get the elapsed time since the spinner started
    pub fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        self.progress_bar.finish_and_clear();
    }
}

/// Progress bar for showing determinate progress
pub struct ProgressBarIndicator {
    progress_bar: ProgressBar,
    colors_enabled: bool,
}

impl ProgressBarIndicator {
    /// Create a new progress bar with a known total
    pub fn new(total: u64, colors_enabled: bool) -> Self {
        let progress_bar = ProgressBar::new(total);

        let template = if colors_enabled {
            "{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos}/{len} ({eta})"
        } else {
            "{spinner} [{elapsed_precise}] [{wide_bar}] {pos}/{len} ({eta})"
        };

        let style = ProgressStyle::with_template(template)
            .unwrap()
            .with_key("eta", |state: &ProgressState, w: &mut dyn Write| {
                write!(w, "{:.1}s", state.eta().as_secs_f64()).unwrap()
            })
            .progress_chars("#>-");

        progress_bar.set_style(style);

        Self {
            progress_bar,
            colors_enabled,
        }
    }

    /// Increment the progress by 1
    pub fn inc(&self) {
        self.progress_bar.inc(1);
    }

    /// Increment the progress by a specific amount
    pub fn inc_by(&self, delta: u64) {
        self.progress_bar.inc(delta);
    }

    /// Set the current position
    pub fn set_position(&self, position: u64) {
        self.progress_bar.set_position(position);
    }

    /// Set a message for the progress bar
    pub fn set_message(&self, message: &str) {
        self.progress_bar.set_message(message.to_string());
    }

    /// Finish the progress bar
    pub fn finish(&self) {
        self.progress_bar.finish();
    }

    /// Finish the progress bar with a message
    pub fn finish_with_message(&self, message: &str) {
        if self.colors_enabled {
            let formatter = ColorFormatter::new(true);
            self.progress_bar
                .finish_with_message(formatter.success(message));
        } else {
            self.progress_bar.finish_with_message(message.to_string());
        }
    }

    /// Clear the progress bar
    pub fn clear(&self) {
        self.progress_bar.finish_and_clear();
    }

    /// Wrap an iterator to show progress
    pub fn wrap_iter<T: ExactSizeIterator>(self, iter: T) -> impl Iterator<Item = T::Item> {
        self.progress_bar.wrap_iter(iter)
    }
}

/// Multi-progress manager for handling multiple concurrent operations
pub struct MultiProgress {
    multi_progress: indicatif::MultiProgress,
    colors_enabled: bool,
}

impl MultiProgress {
    /// Create a new multi-progress manager
    pub fn new(colors_enabled: bool) -> Self {
        Self {
            multi_progress: indicatif::MultiProgress::new(),
            colors_enabled,
        }
    }

    /// Add a spinner to the multi-progress
    pub fn add_spinner(&self, message: &str) -> Spinner {
        let spinner = Spinner::new(message, self.colors_enabled);
        self.multi_progress.add(spinner.progress_bar.clone());
        spinner
    }

    /// Add a progress bar to the multi-progress
    pub fn add_progress_bar(&self, total: u64) -> ProgressBarIndicator {
        let progress_bar = ProgressBarIndicator::new(total, self.colors_enabled);
        self.multi_progress.add(progress_bar.progress_bar.clone());
        progress_bar
    }

    /// Clear all progress indicators
    pub fn clear(&self) {
        self.multi_progress.clear().ok();
    }
}

/// Simple status indicator for operations
pub struct StatusIndicator {
    message: String,
    colors_enabled: bool,
    term: Term,
}

impl StatusIndicator {
    /// Create a new status indicator
    pub fn new(message: &str, colors_enabled: bool) -> Self {
        let term = Term::stdout();
        let indicator = Self {
            message: message.to_string(),
            colors_enabled,
            term,
        };

        indicator.show();
        indicator
    }

    /// Update the status message
    pub fn update(&mut self, message: &str) {
        self.message = message.to_string();
        self.show();
    }

    /// Show the current status
    fn show(&self) {
        if self.term.features().colors_supported() && self.colors_enabled {
            let formatter = ColorFormatter::new(true);
            let _ = self
                .term
                .write_line(&formatter.info(&format!("→ {}", self.message)));
        } else {
            let _ = self.term.write_line(&format!("→ {}", self.message));
        }
    }

    /// Finish with success
    pub fn success(self, message: &str) {
        if self.colors_enabled {
            let formatter = ColorFormatter::new(true);
            let _ = self
                .term
                .write_line(&formatter.success(&format!("✓ {}", message)));
        } else {
            let _ = self.term.write_line(&format!("✓ {}", message));
        }
    }

    /// Finish with error
    pub fn error(self, message: &str) {
        if self.colors_enabled {
            let formatter = ColorFormatter::new(true);
            let _ = self
                .term
                .write_line(&formatter.error(&format!("✗ {}", message)));
        } else {
            let _ = self.term.write_line(&format!("✗ {}", message));
        }
    }

    /// Finish with warning
    pub fn warning(self, message: &str) {
        if self.colors_enabled {
            let formatter = ColorFormatter::new(true);
            let _ = self
                .term
                .write_line(&formatter.warning(&format!("⚠ {}", message)));
        } else {
            let _ = self.term.write_line(&format!("⚠ {}", message));
        }
    }
}

/// Format duration in a human-readable way
fn format_duration(duration: Duration) -> String {
    let total_seconds = duration.as_secs();
    let millis = duration.subsec_millis();

    if total_seconds == 0 {
        format!("{}ms", millis)
    } else if total_seconds < 60 {
        format!("{}.{:03}s", total_seconds, millis)
    } else if total_seconds < 3600 {
        let minutes = total_seconds / 60;
        let seconds = total_seconds % 60;
        format!("{}m{:02}s", minutes, seconds)
    } else {
        let hours = total_seconds / 3600;
        let minutes = (total_seconds % 3600) / 60;
        let seconds = total_seconds % 60;
        format!("{}h{:02}m{:02}s", hours, minutes, seconds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spinner_types() {
        let dots = SpinnerType::Dots;
        assert!(!dots.frames().is_empty());
        assert!(dots.tick_interval() > Duration::from_millis(0));

        let braille = SpinnerType::Braille;
        assert!(!braille.frames().is_empty());

        let custom = SpinnerType::Custom(vec!["a".to_string(), "b".to_string()]);
        assert_eq!(custom.frames(), vec!["a", "b"]);
    }

    #[test]
    fn test_spinner_creation() {
        let spinner = Spinner::new("Testing...", false);
        assert_eq!(spinner.message, "Testing...");
        assert!(!spinner.colors_enabled);
    }

    #[test]
    fn test_progress_bar_creation() {
        let progress_bar = ProgressBarIndicator::new(100, false);
        assert!(!progress_bar.colors_enabled);
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(Duration::from_millis(500)), "500ms");
        assert_eq!(format_duration(Duration::from_secs(1)), "1.000s");
        assert_eq!(format_duration(Duration::from_secs(65)), "1m05s");
        assert_eq!(format_duration(Duration::from_secs(3661)), "1h01m01s");
    }

    #[test]
    fn test_status_indicator_creation() {
        let indicator = StatusIndicator::new("Testing status", false);
        assert_eq!(indicator.message, "Testing status");
        assert!(!indicator.colors_enabled);
    }

    #[test]
    fn test_multi_progress_creation() {
        let multi = MultiProgress::new(false);
        assert!(!multi.colors_enabled);
    }
}
