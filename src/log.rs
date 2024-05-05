use log::{Level, LevelFilter};
use std::io::Write;
use termcolor::{Buffer, Color, ColorSpec, WriteColor};

use crate::output;

pub fn setup(max_level: LevelFilter) {
    let logger = Box::new(ErdLogger {
        crate_log_level: max_level,
        colored: true,
    });
    log::set_boxed_logger(logger).expect("Failed to setup logger");
    log::set_max_level(max_level);
}

const CRATE_NAME: &str = env!("CARGO_CRATE_NAME");

struct ErdLogger {
    crate_log_level: LevelFilter,
    colored: bool,
}

impl log::Log for ErdLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        let is_crate_log = metadata.target().starts_with(CRATE_NAME);
        if is_crate_log {
            metadata.level() <= self.crate_log_level
        } else {
            metadata.level() <= LevelFilter::Info
        }
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            if record.level() <= Level::Warn {
                print_error(self.colored, record);
            } else {
                println!("{}", record.args());
            }
        }
    }

    fn flush(&self) {}
}

fn print_error(colored: bool, record: &log::Record) {
    let mut buf = if colored {
        Buffer::ansi()
    } else {
        Buffer::no_color()
    };
    let mut color = ColorSpec::new();
    color.set_fg(Some(Color::Red));
    if buf.set_color(&color).is_err() || write!(buf, "{}", record.args()).is_err() {
        eprintln!("{}", record.args());
        return;
    }
    match output::buf_to_str(buf) {
        Ok(s) => eprintln!("{}", s),
        Err(_) => eprintln!("{}", record.args()),
    }
}
