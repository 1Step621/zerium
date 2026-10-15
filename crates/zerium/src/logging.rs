use std::{
    backtrace::Backtrace,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    panic,
    path::{Path, PathBuf},
    sync::OnceLock,
    time::{SystemTime, UNIX_EPOCH},
};

use directories::ProjectDirs;

const RETAINED_LOGS: usize = 10;
static LOG_FILE: OnceLock<PathBuf> = OnceLock::new();

pub(crate) fn log_file() -> Option<&'static Path> {
    LOG_FILE.get().map(PathBuf::as_path)
}

pub(crate) fn initialize() {
    let mut logger = env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn,zerium=info"),
    );
    logger
        .format_timestamp_millis()
        .write_style(env_logger::WriteStyle::Never);
    match create_log() {
        Ok((path, file)) => {
            let _ = LOG_FILE.set(path);
            logger.target(env_logger::Target::Pipe(Box::new(LogOutput(file))));
        }
        Err(error) => eprintln!("Could not create a log file: {error}"),
    }
    if let Err(error) = logger.try_init() {
        eprintln!("Could not initialize logging: {error}");
    }

    let previous_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let report = format!(
            "\n=== Rust panic at Unix time {} ===\nZerium {} ({}/{})\nThread: {} ({:?})\n{info}\nBacktrace:\n{}\n",
            timestamp(),
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            thread.name().unwrap_or("unnamed"),
            thread.id(),
            Backtrace::force_capture(),
        );
        // Bypass the logger's filter and lock, including when it is the code
        // that panicked. Sync before unwinding or panic=abort exits.
        if let Some(path) = log_file()
            && let Err(error) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .and_then(|mut file| {
                    file.write_all(report.as_bytes())?;
                    file.sync_data()
                })
        {
            let _ = writeln!(io::stderr().lock(), "Could not save panic details: {error}");
        }
        previous_hook(info);
    }));

    log::info!(
        "Zerium {} started; OS={}, arch={}, PID={}, log={}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::process::id(),
        log_file().map_or_else(|| "stderr only".into(), |path| path.display().to_string()),
    );
}

fn timestamp() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn create_log() -> io::Result<(PathBuf, File)> {
    let directories = ProjectDirs::from("dev", "1Step621", "Zerium").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "User data directory is unavailable",
        )
    })?;
    let directory = directories
        .state_dir()
        .unwrap_or_else(|| directories.data_local_dir())
        .join("logs");
    fs::create_dir_all(&directory)?;
    let path = directory.join(format!("zerium-{}-{}.log", timestamp(), std::process::id()));
    let file = OpenOptions::new()
        .create_new(true)
        .append(true)
        .open(&path)?;
    if let Err(error) = prune_logs(&directory, &path) {
        let _ = writeln!(io::stderr().lock(), "Could not remove old logs: {error}");
    }
    Ok((path, file))
}

fn prune_logs(directory: &Path, current: &Path) -> io::Result<()> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path != current
            && path.is_file()
            && path.extension().is_some_and(|extension| extension == "log")
            && path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("zerium-"))
        {
            paths.push(path);
        }
    }
    paths.sort_unstable();
    let remove = paths.len().saturating_sub(RETAINED_LOGS - 1);
    for path in paths.into_iter().take(remove) {
        fs::remove_file(path)?;
    }
    Ok(())
}

struct LogOutput(File);

impl Write for LogOutput {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let _ = io::stderr().lock().write_all(buffer);
        self.0.write_all(buffer)?;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let _ = io::stderr().flush();
        self.0.flush()
    }
}
