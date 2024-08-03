use log::{Level, LevelFilter};
use std::io::{self, Write};
use termcolor::{Buffer, Color, ColorSpec, WriteColor};

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
    if !colored {
        eprintln!("{}", record.args());
        return;
    }
    match create_error_string(record) {
        Ok(s) => eprintln!("{}", s),
        Err(e) => {
            eprintln!("{}", record.args());
            eprintln!("Failed to write colored output: {}", e);
        }
    }
}

fn create_error_string(record: &log::Record) -> Result<String, io::Error> {
    let mut buf = Buffer::ansi();

    let mut color = ColorSpec::new();
    color.set_fg(Some(Color::Red));

    buf.set_color(&color)?;
    write!(buf, "{}", record.args())?;
    buf.reset()?;

    String::from_utf8(buf.into_inner()).map_err(|_| 
            io::Error::new(io::ErrorKind::InvalidData, "Couldn't convert to UTF-8".to_owned()))
}