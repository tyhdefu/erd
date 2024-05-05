use std::fmt::{self, Display};
use std::io::{self, Write};

use log::error;
use termcolor::{Buffer, Color, ColorSpec, WriteColor};

use crate::gitlab;

/// Describes how output should be formatted
#[derive(Debug, Clone)]
pub struct OutputOptions {
    /// Whether to use colored output
    pub color: bool,
    /// Whether to provide a shortened output
    pub short: bool,
}

pub trait FormatOutput<T: Display> {
    fn format_output(self, options: &OutputOptions) -> T;
}

#[derive(Debug)]
pub struct JobHistoryOutput {
    id: String,
    job_ref: String,
    timestamp: String,
    status: String,
    has_artifacts: bool,
    web_url: String,
    commit_short_id: String,
    commit_title: String,
    commit_author: String,
    options: OutputOptions,
}

const COMMIT_HASH_COLOR: Color = Color::Yellow;
impl JobHistoryOutput {
    fn get_status_color(&self) -> Color {
        // TODO: Convert status to enum in display
        match (&*self.status, self.has_artifacts) {
            ("success", true) => Color::Green,
            ("success", false) => Color::Yellow,
            ("failed", false) => Color::Red,
            _ => Color::Yellow,
        }
    }

    fn fmt_short(&self, buf: &mut Buffer) -> Result<(), io::Error> {
        write!(buf, "{} - {} - ", self.id, self.timestamp)?;
        write!(buf, "(")?;
        buf.set_color(ColorSpec::new().set_fg(Some(COMMIT_HASH_COLOR)))?;
        write!(buf, "{}", self.commit_short_id)?;
        buf.reset()?;
        write!(buf, ")")?;
        let color = self.get_status_color();
        buf.set_color(ColorSpec::new().set_fg(Some(color)))?;
        write!(buf, "{}", self.status)?;
        if !self.has_artifacts {
            write!(buf, " (no artifacts)")?;
        }
        buf.reset()
    }

    fn fmt_long(&self, buf: &mut Buffer) -> Result<(), io::Error> {
        buf.set_color(ColorSpec::new().set_fg(Some(COMMIT_HASH_COLOR)))?;
        write!(buf, "{}", self.commit_short_id)?;
        buf.reset()?;
        write!(buf, " (")?;
        buf.set_color(ColorSpec::new().set_fg(Some(COMMIT_HASH_COLOR)))?;
        write!(buf, "{}", self.job_ref)?;
        buf.reset()?;
        write!(buf, ")")?;
        writeln!(buf, " - {}", self.commit_title)?;
        writeln!(buf, "\tBuild id: {}", self.id)?;
        writeln!(buf, "\tTimestamp: {}", self.timestamp)?;

        buf.set_color(ColorSpec::new().set_fg(Some(self.get_status_color())))?;
        writeln!(
            buf,
            "\tStatus: {}{}",
            self.status,
            if !self.has_artifacts {
                " (no artifacts)"
            } else {
                ""
            }
        )?;
        buf.reset()?;
        writeln!(buf, "\tURL: {}", self.web_url)?;
        writeln!(buf, "\tAuthor: {}", self.commit_author)
    }
}

impl Display for JobHistoryOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut buf = Buffer::ansi();

        match self.options.short {
            true => self.fmt_short(&mut buf),
            false => self.fmt_long(&mut buf),
        }
        .map_err(|e| {
            error!("Failed to format JobHistoryOutput: {}", e);
            fmt::Error
        })?;

        let s = String::from_utf8(buf.into_inner()).map_err(|e| {
            error!("Failed to convert buffer to UTF-8 string: {}", e);
            fmt::Error
        })?;
        write!(f, "{}", s)
    }
}

impl FormatOutput<JobHistoryOutput> for gitlab::JobHistory {
    fn format_output(self, options: &OutputOptions) -> JobHistoryOutput {
        JobHistoryOutput {
            id: self.id.to_string(),
            job_ref: self.job_ref,
            timestamp: self.created_at,
            status: self.status,
            has_artifacts: self.artifacts_file.is_some(),
            web_url: self.web_url,
            commit_short_id: self.commit.short_id,
            commit_title: self.commit.title,
            commit_author: self.commit.author_email,
            options: options.clone(),
        }
    }
}
