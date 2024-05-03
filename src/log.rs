use log::{Level, LevelFilter};

pub fn setup(max_level: LevelFilter) {
    let logger = Box::new(ErdLogger {
        level: max_level,
        colored: true,
    });
    log::set_boxed_logger(logger).expect("Failed to setup logger");
    log::set_max_level(max_level);
}

struct ErdLogger {
    level: LevelFilter,
    colored: bool,
}

impl log::Log for ErdLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            if record.level() <= Level::Warn {
                eprintln!("{}", record.args());
            } else {
                println!("{}", record.args());
            }
        }
    }

    fn flush(&self) {}
}
