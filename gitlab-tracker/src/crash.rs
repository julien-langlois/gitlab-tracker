//! Crash reports. A panic writes `crash-<time>.txt` next to the logs (version,
//! platform, thread, message, location, backtrace) and logs it, so a crash of the
//! window launched from the desktop — whose stderr goes nowhere — leaves a trace.
//! The next launch says where the report is, once.

use std::path::{Path, PathBuf};

const PREFIX: &str = "crash-";
const SUFFIX: &str = ".txt";
/// The newest report already announced, so each one is announced once.
const SEEN: &str = "crash-seen";
const ISSUES: &str = "https://github.com/julien-langlois/gitlab-tracker/issues/new";

/// Installs the panic hook: restore the terminal, write the report, log it, then
/// the default hook (message on stderr).
pub fn install() {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        crate::utils::restore_terminal();
        let thread = std::thread::current();
        let report = report(
            thread.name().unwrap_or("unnamed"),
            &message(info.payload()),
            info.location().map(ToString::to_string).as_deref(),
            &std::backtrace::Backtrace::force_capture().to_string(),
        );
        let written = crate::storage::get_save_dir().and_then(|dir| write(&dir, &report));
        tracing::error!(report = ?written, "panic: {}", message(info.payload()));
        default_panic(info);
        if let Some(path) = written {
            eprintln!(
                "\ngitlab-tracker hit an internal error. Report: {}\n\
                 Please attach it to an issue ({ISSUES}) after checking it holds nothing private.",
                path.display()
            );
        }
    }));
}

fn message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(no message)".into())
}

fn report(thread: &str, message: &str, location: Option<&str>, backtrace: &str) -> String {
    format!(
        "gitlab-tracker {version} ({os} {arch})\n\
         time: {time}\n\
         thread: {thread}\n\
         location: {location}\n\
         message: {message}\n\n\
         backtrace:\n{backtrace}\n",
        version = env!("CARGO_PKG_VERSION"),
        os = std::env::consts::OS,
        arch = std::env::consts::ARCH,
        time = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        location = location.unwrap_or("unknown"),
    )
}

/// Writes `report` in `dir`; `None` when it cannot (the panic still goes to stderr).
fn write(dir: &Path, report: &str) -> Option<PathBuf> {
    std::fs::create_dir_all(dir).ok()?;
    // Milliseconds: two panics in the same second (two threads) keep both reports.
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S%3f");
    let path = dir.join(format!("{PREFIX}{stamp}{SUFFIX}"));
    std::fs::write(&path, report).ok()?;
    Some(path)
}

/// The newest crash report not announced yet, marked as announced: a launch
/// shows it once, the next one no more.
pub fn take_unseen(dir: &Path) -> Option<PathBuf> {
    let newest = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(PREFIX) && n.ends_with(SUFFIX))
        .max()?;
    let seen = std::fs::read_to_string(dir.join(SEEN)).unwrap_or_default();
    if seen.trim() >= newest.as_str() {
        return None;
    }
    let _ = std::fs::write(dir.join(SEEN), &newest);
    Some(dir.join(newest))
}

/// The status bar's message for an unannounced crash report, once per process.
pub fn notice() -> Option<String> {
    use std::sync::atomic::{AtomicBool, Ordering};
    static ASKED: AtomicBool = AtomicBool::new(false);
    if ASKED.swap(true, Ordering::Relaxed) {
        return None;
    }
    let path = take_unseen(&crate::storage::get_save_dir()?)?;
    Some(format!(
        "The last session stopped on an internal error — report: {}",
        path.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gt-crash-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_report_says_what_where_and_which_version() {
        let text = report("main", "boom", Some("src/app.rs:12:5"), "0: frame");
        assert!(text.starts_with(&format!("gitlab-tracker {}", env!("CARGO_PKG_VERSION"))));
        for part in [
            "thread: main",
            "location: src/app.rs:12:5",
            "message: boom",
            "0: frame",
        ] {
            assert!(text.contains(part), "{part} in {text}");
        }
    }

    #[test]
    fn each_report_is_announced_once_the_newest_first() {
        let dir = dir("seen");
        assert_eq!(take_unseen(&dir), None);
        let first = write(&dir, "a").unwrap();
        assert_eq!(take_unseen(&dir), Some(first));
        assert_eq!(take_unseen(&dir), None);
        std::thread::sleep(std::time::Duration::from_millis(5));
        let second = write(&dir, "b").unwrap();
        assert_eq!(take_unseen(&dir), Some(second));
        assert_eq!(take_unseen(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn payloads_of_both_kinds_give_their_message() {
        let s: Box<dyn std::any::Any + Send> = Box::new("static");
        let o: Box<dyn std::any::Any + Send> = Box::new(String::from("owned"));
        assert_eq!(message(s.as_ref()), "static");
        assert_eq!(message(o.as_ref()), "owned");
    }
}
