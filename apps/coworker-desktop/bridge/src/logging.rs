use std::{
    path::Path,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU8, Ordering},
    },
};

use tokio::sync::broadcast;
use tracing::{
    Event, Level, Metadata, Subscriber,
    field::{Field, Visit},
};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{
    Layer, fmt,
    layer::{Context, Filter, SubscriberExt},
    util::SubscriberInitExt,
};

use crate::{
    config::BridgeConfig,
    error::{BridgeError, Result},
};

pub const LOG_FILE_NAME: &str = "coworker_desktop";
const LOG_FILE_SUFFIX: &str = "log";
const MAX_LOG_FILES: usize = 7;

static LOG_EVENTS: OnceLock<broadcast::Sender<String>> = OnceLock::new();
static LOG_FILTERS: OnceLock<LogFilters> = OnceLock::new();

pub fn subscribe_log_events() -> broadcast::Receiver<String> {
    log_events().subscribe()
}

pub fn init_logging(config: &BridgeConfig) -> Result<()> {
    if update_log_levels(&config.log_level, &config.file_log_level) {
        return Ok(());
    }

    std::fs::create_dir_all(&config.logs_dir)?;
    let file_appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(LOG_FILE_NAME)
        .filename_suffix(LOG_FILE_SUFFIX)
        .max_log_files(MAX_LOG_FILES)
        .build(Path::new(&config.logs_dir))
        .map_err(|error| BridgeError::startup(format!("unable to initialize log file: {error}")))?;
    let filters = LogFilters::new(&config.log_level, &config.file_log_level);

    tracing_subscriber::registry()
        .with(
            fmt::layer()
                .with_writer(std::io::stderr)
                .with_ansi(true)
                .with_filter(filters.console.clone()),
        )
        .with(
            fmt::layer()
                .with_writer(file_appender)
                .with_ansi(false)
                .with_filter(filters.file.clone()),
        )
        .with(RealtimeLogLayer.with_filter(filters.file.clone()))
        .try_init()
        .map_err(|error| BridgeError::startup(format!("unable to initialize logging: {error}")))?;

    let _ = LOG_FILTERS.set(filters);
    Ok(())
}

pub fn log_file_path(logs_dir: impl AsRef<Path>) -> std::path::PathBuf {
    logs_dir.as_ref().join(format!(
        "{LOG_FILE_NAME}.{}.{}",
        chrono::Utc::now().format("%Y-%m-%d"),
        LOG_FILE_SUFFIX
    ))
}

pub fn update_log_levels(console_level: &str, file_level: &str) -> bool {
    let Some(filters) = LOG_FILTERS.get() else {
        return false;
    };
    filters.console.update(console_level);
    filters.file.update(file_level);
    true
}

/// Records a single line emitted to a child process' stderr, classifying it by
/// content so genuine errors surface at `ERROR` level under the default `INFO`
/// threshold instead of being filtered away.
pub fn log_subprocess_line(target: &str, line: &str) {
    let lower = line.to_ascii_lowercase();
    let is_error = lower.contains("error")
        || lower.contains("panic")
        || lower.contains("traceback")
        || lower.contains("fatal");
    let is_warn = lower.contains("warn");
    // tracing's `target:` requires a compile-time literal, so dispatch by name.
    match target {
        "codex_app_server" if is_error => tracing::error!(target: "codex_app_server", "{line}"),
        "codex_app_server" if is_warn => tracing::warn!(target: "codex_app_server", "{line}"),
        "codex_app_server" => tracing::info!(target: "codex_app_server", "{line}"),
        "claude_code" if is_error => tracing::error!(target: "claude_code", "{line}"),
        "claude_code" if is_warn => tracing::warn!(target: "claude_code", "{line}"),
        "claude_code" => tracing::info!(target: "claude_code", "{line}"),
        _ if is_error => tracing::error!(target: "coworker_desktop_core", "{line}"),
        _ if is_warn => tracing::warn!(target: "coworker_desktop_core", "{line}"),
        _ => tracing::info!(target: "coworker_desktop_core", "{line}"),
    }
}

/// Formats an error together with its full `source()` cause chain so the
/// underlying root cause is not lost when the error is logged or returned.
pub fn error_chain(error: &dyn std::error::Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        message.push_str("\n | caused by: ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    message
}

struct LogFilters {
    console: DynamicLevelFilter,
    file: DynamicLevelFilter,
}

impl LogFilters {
    fn new(console_level: &str, file_level: &str) -> Self {
        Self {
            console: DynamicLevelFilter::new(console_level),
            file: DynamicLevelFilter::new(file_level),
        }
    }
}

#[derive(Clone)]
struct DynamicLevelFilter {
    max_level: Arc<AtomicU8>,
}

impl DynamicLevelFilter {
    fn new(level: &str) -> Self {
        Self {
            max_level: Arc::new(AtomicU8::new(level_rank_from_name(level))),
        }
    }

    fn update(&self, level: &str) {
        self.max_level
            .store(level_rank_from_name(level), Ordering::Release);
    }

    fn allows(&self, level: &Level) -> bool {
        level_rank(level) <= self.max_level.load(Ordering::Acquire)
    }

    fn allows_metadata(&self, metadata: &Metadata<'_>) -> bool {
        if is_application_target(metadata.target()) {
            return self.allows(metadata.level());
        }
        let configured = self.max_level.load(Ordering::Acquire);
        let max_level = max_level_for_target(configured, metadata.target());
        level_rank(metadata.level()) <= max_level
    }
}

impl<S> Filter<S> for DynamicLevelFilter
where
    S: Subscriber,
{
    fn enabled(&self, metadata: &Metadata<'_>, _cx: &Context<'_, S>) -> bool {
        self.allows_metadata(metadata)
    }
}

fn is_application_target(target: &str) -> bool {
    target.starts_with("coworker_desktop_core")
        || target.starts_with("coworker_desktop_app")
        || target.starts_with("codex_app_server")
        || target == "claude_code"
}

fn max_level_for_target(configured: u8, target: &str) -> u8 {
    if is_application_target(target) {
        configured
    } else {
        configured.min(level_rank(&Level::WARN))
    }
}

struct RealtimeLogLayer;

impl<S> Layer<S> for RealtimeLogLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let _ = log_events().send(format_log_event(event));
    }
}

#[derive(Default)]
struct EventFieldVisitor {
    message: Option<String>,
    fields: Vec<String>,
}

impl Visit for EventFieldVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let value = format!("{value:?}");
        if field.name() == "message" {
            self.message = Some(value);
        } else {
            self.fields.push(format!("{}={value}", field.name()));
        }
    }
}

fn format_log_event(event: &Event<'_>) -> String {
    let metadata = event.metadata();
    let mut visitor = EventFieldVisitor::default();
    event.record(&mut visitor);

    let mut line = format!(
        "{} {} {}:",
        chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        metadata.level(),
        metadata.target()
    );
    if let Some(message) = visitor.message {
        line.push(' ');
        line.push_str(&message);
    }
    for field in visitor.fields {
        line.push(' ');
        line.push_str(&field);
    }
    line.push('\n');
    line
}

fn log_events() -> &'static broadcast::Sender<String> {
    LOG_EVENTS.get_or_init(|| {
        let (sender, _) = broadcast::channel(4096);
        sender
    })
}

fn level_rank_from_name(level: &str) -> u8 {
    match level.trim().to_ascii_lowercase().as_str() {
        "trace" => 5,
        "debug" => 4,
        "warn" | "warning" => 2,
        "error" => 1,
        _ => 3,
    }
}

fn level_rank(level: &Level) -> u8 {
    if *level == Level::ERROR {
        1
    } else if *level == Level::WARN {
        2
    } else if *level == Level::INFO {
        3
    } else if *level == Level::DEBUG {
        4
    } else {
        5
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dynamic_level_filter_updates_without_reinitializing_logging() {
        let filter = DynamicLevelFilter::new("INFO");
        assert!(filter.allows(&Level::ERROR));
        assert!(filter.allows(&Level::INFO));
        assert!(!filter.allows(&Level::DEBUG));

        filter.update("TRACE");
        assert!(filter.allows(&Level::TRACE));

        filter.update("ERROR");
        assert!(filter.allows(&Level::ERROR));
        assert!(!filter.allows(&Level::WARN));
    }

    #[test]
    fn dependency_logs_are_capped_at_warn() {
        assert_eq!(max_level_for_target(5, "coworker_desktop_core::runtime"), 5);
        assert_eq!(max_level_for_target(5, "coworker_desktop_app_lib"), 5);
        assert_eq!(max_level_for_target(5, "claude_code"), 5);
        assert_eq!(max_level_for_target(5, "codex_app_server"), 5);
        assert_eq!(max_level_for_target(5, "hyper_util::client"), 2);
        assert_eq!(max_level_for_target(1, "hyper_util::client"), 1);
    }

    #[test]
    fn error_chain_joins_source_causes() {
        use std::fmt;

        #[derive(Debug)]
        struct Root;
        impl fmt::Display for Root {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "root cause")
            }
        }
        impl std::error::Error for Root {}

        #[derive(Debug)]
        struct Middle {
            source: Root,
        }
        impl fmt::Display for Middle {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "middle")
            }
        }
        impl std::error::Error for Middle {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                Some(&self.source)
            }
        }

        #[derive(Debug)]
        struct Top {
            source: Middle,
        }
        impl fmt::Display for Top {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "top")
            }
        }
        impl std::error::Error for Top {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                Some(&self.source)
            }
        }

        let error = Top {
            source: Middle { source: Root },
        };
        assert_eq!(
            error_chain(&error),
            "top\n | caused by: middle\n | caused by: root cause"
        );
    }
}
