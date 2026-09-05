//! Progress bar factory — [`make_progress_bar`] wraps `indicatif` with a consistent style.

use indicatif::{ProgressBar, ProgressStyle};

/// Create a progress bar with the shared style and throughput display.
pub fn make_progress_bar(total: u64, label: &str) -> ProgressBar {
    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::with_template(
            "[{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} {msg} \u{2022} {per_sec} \u{2022} ETA {eta}"
        )
        .unwrap()
        .progress_chars("\u{2588}\u{2589}\u{258a}\u{258b}\u{258c}\u{258d}\u{258e}\u{258f}  "),
    );
    pb.set_message(label.to_string());
    pb
}
