use std::sync::atomic::AtomicBool;

use color_eyre::Report;

pub static RUNNING: AtomicBool = AtomicBool::new(true);

pub fn fuck_error(report: &Report) -> &(dyn std::error::Error + 'static) {
    report.as_ref()
}

pub fn user_has_quit() -> bool {
    !RUNNING.load(std::sync::atomic::Ordering::Relaxed)
}
