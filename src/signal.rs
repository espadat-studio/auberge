use eyre::Result;
use indicatif::ProgressBar;
use std::sync::Mutex;

static ACTIVE_BAR: Mutex<Option<ProgressBar>> = Mutex::new(None);
static INTERRUPT_NOTE: Mutex<Option<String>> = Mutex::new(None);

pub fn register_progress_bar(pb: &ProgressBar) {
    if let Ok(mut guard) = ACTIVE_BAR.lock() {
        *guard = Some(pb.clone());
    }
}

fn cleanup_progress_bar() {
    if let Ok(guard) = ACTIVE_BAR.lock()
        && let Some(pb) = guard.as_ref()
    {
        pb.finish_and_clear();
    }
}

fn take_interrupt_note() -> Option<String> {
    INTERRUPT_NOTE
        .lock()
        .ok()
        .and_then(|mut guard| guard.take())
}

fn install_handler() -> Result<()> {
    let handler_result = ctrlc::set_handler(|| {
        cleanup_progress_bar();
        if let Some(note) = take_interrupt_note() {
            eprintln!("{note}");
        }
        std::process::exit(130);
    });

    match handler_result {
        Ok(_) | Err(ctrlc::Error::MultipleHandlers) => Ok(()),
        Err(e) => Err(eyre::eyre!("Failed to set Ctrl-C handler: {}", e)),
    }
}

pub fn with_ctrlc<F: FnOnce() -> Result<()>>(f: F) -> Result<()> {
    install_handler()?;
    f()
}

/// Ctrl-C prints `note` after clearing the bar, for a command whose work
/// outlives it on the host.
pub fn on_interrupt_note(note: &str) -> Result<()> {
    if let Ok(mut guard) = INTERRUPT_NOTE.lock() {
        *guard = Some(note.to_string());
    }
    install_handler()
}

#[cfg(test)]
mod tests {
    use super::*;
    use indicatif::{ProgressBar, ProgressDrawTarget};

    #[test]
    fn test_register_and_cleanup_progress_bar() {
        let pb = ProgressBar::with_draw_target(Some(10), ProgressDrawTarget::hidden());
        assert!(!pb.is_finished());

        register_progress_bar(&pb);
        cleanup_progress_bar();

        assert!(pb.is_finished());
    }

    #[test]
    fn test_with_ctrlc_runs_closure() {
        let result = with_ctrlc(|| {
            let x: i32 = 2 + 2;
            assert_eq!(x, 4);
            Ok(())
        });
        assert!(result.is_ok());
    }

    #[test]
    fn test_with_ctrlc_propagates_error() {
        let result = with_ctrlc(|| eyre::bail!("test error"));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("test error"));
    }

    #[test]
    fn interrupt_note_is_handed_out_once() {
        on_interrupt_note("the pass keeps running").unwrap();
        assert_eq!(
            take_interrupt_note().as_deref(),
            Some("the pass keeps running")
        );
        assert_eq!(take_interrupt_note(), None);
    }

    #[test]
    fn test_cleanup_without_registered_bar() {
        *ACTIVE_BAR.lock().unwrap() = None;
        // Should not panic when no bar is registered
        cleanup_progress_bar();
    }
}
