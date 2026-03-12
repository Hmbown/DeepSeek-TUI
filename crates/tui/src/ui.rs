//! Terminal UI helpers (progress bars, spinners).

use indicatif::{ProgressBar, ProgressStyle};

/// Create a spinner progress indicator.
#[must_use]
#[allow(dead_code)]
pub fn spinner(message: &str) -> ProgressBar {
    let spinner = ProgressBar::new_spinner();
    spinner.set_message(message.to_string());
    spinner.set_style(
        ProgressStyle::with_template("{spinner} {msg}")
            .unwrap_or_else(|_| ProgressStyle::default_spinner()),
    );
    spinner.enable_steady_tick(std::time::Duration::from_millis(120));
    spinner
}

/// Create a progress bar for byte-based transfers.
#[must_use]
#[allow(dead_code)]
pub fn progress_bar(total: u64, message: &str) -> ProgressBar {
    let bar = ProgressBar::new(total);
    bar.set_message(message.to_string());
    bar.set_style(
        ProgressStyle::with_template("{msg} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
            .unwrap_or_else(|_| ProgressStyle::default_bar()),
    );
    bar
}
