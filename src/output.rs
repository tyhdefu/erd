use std::fmt::{self, Display};
use std::io::{self, Write};

use log::error;
use termcolor::{Buffer, Color, ColorSpec, WriteColor};

use crate::config::artifacts::SourceConfig;
use crate::commands::fetch::GetArtifactAnswer;

/// Describes how output should be formatted
#[derive(Debug, Clone)]
pub struct OutputOptions {
    /// Whether to use colored output
    pub color: bool,
    /// Whether to provide a shortened output
    pub short: bool,
}

/// Something can be formatted to the terminal output,
/// given certain options
pub trait FormatOutput<T: Display> {
    fn format_output(self, options: &OutputOptions) -> T;
}

const COMMIT_HASH_COLOR: Color = Color::Yellow;
const SOURCE_ID_COLOR: Color = Color::Magenta;
const ARTIFACT_ID_COLOR: Color = Color::Green;
const BRANCH_COLOR: Color = COMMIT_HASH_COLOR;

#[derive(Debug)]
pub struct JobHistoryOutput {
    pub id: String,
    pub job_ref: String,
    pub timestamp: String,
    pub status: String,
    pub has_artifacts: bool,
    pub web_url: String,
    pub commit_short_id: String,
    pub commit_title: String,
    pub commit_author: String,
    pub options: OutputOptions,
}

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
        write!(buf, ") ")?;
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
        let mut buf = create_buf(&self.options);

        match self.options.short {
            true => self.fmt_short(&mut buf),
            false => self.fmt_long(&mut buf),
        }
        .map_err(|e| {
            error!("Failed to format JobHistoryOutput: {}", e);
            fmt::Error
        })?;

        let s = buf_to_str(buf)?;
        write!(f, "{}", s)
    }
}

pub struct ArtifactListOutput<'a> {
    source: &'a SourceConfig,
    options: OutputOptions,
}

impl<'a> FormatOutput<ArtifactListOutput<'a>> for &'a SourceConfig {
    fn format_output(self, options: &OutputOptions) -> ArtifactListOutput<'a> {
        ArtifactListOutput {
            source: self,
            options: options.clone(),
        }
    }
}

impl<'a> ArtifactListOutput<'a> {
    fn fmt_default(&self, buf: &mut Buffer) -> Result<(), io::Error> {
        write!(buf, "== Artifacts from ")?;
        buf.set_color(ColorSpec::new().set_fg(Some(SOURCE_ID_COLOR)))?;
        write!(buf, "{}", self.source.id)?;
        buf.reset()?;
        writeln!(buf, " ==")?;
        for artifact in &self.source.artifacts {
            write!(buf, "- ")?;
            buf.set_color(ColorSpec::new().set_fg(Some(ARTIFACT_ID_COLOR)))?;
            write!(buf, "{}", artifact.id)?;
            buf.reset()?;
            write!(buf, " (")?;
            buf.set_color(ColorSpec::new().set_fg(Some(BRANCH_COLOR)))?;
            write!(buf, "{}", artifact.branch)?;
            buf.reset()?;
            writeln!(buf, ")")?;
        }
        Ok(())
    }
}

impl<'a> Display for ArtifactListOutput<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut buf = create_buf(&self.options);

        self.fmt_default(&mut buf).map_err(|e| {
            error!("Failed to format ArtifactListOutput: {}", e);
            fmt::Error
        })?;

        let s = buf_to_str(buf)?;
        write!(f, "{}", s)
    }
}

pub struct ScannedProject {
    pub path: String,
    pub id: String,
    pub url: String,
}

pub struct ScanProjectsOutput {
    pub projects: Vec<ScannedProject>,
    pub options: OutputOptions,
}

impl ScanProjectsOutput {
    fn fmt_default(&self, buf: &mut Buffer) -> Result<(), io::Error> {
        const PATH_COLOR: Color = Color::Green;
        buf.set_color(ColorSpec::new().set_fg(Some(PATH_COLOR)))?;
        write!(buf, "Path")?;
        buf.reset()?;
        writeln!(buf, " (ID) - URL")?;

        let longest_name: usize = self
            .projects
            .iter()
            .map(|p| p.path.len())
            .max()
            .unwrap_or(0);
        for project in &self.projects {
            buf.set_color(ColorSpec::new().set_fg(Some(PATH_COLOR)))?;
            write!(buf, "{:longest_name$}", project.path)?;
            buf.reset()?;
            writeln!(buf, " ({}) - {}", project.id, project.url)?;
        }
        Ok(())
    }
}

impl Display for ScanProjectsOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut buf = create_buf(&self.options);
        self.fmt_default(&mut buf).map_err(|e| {
            error!("Failed to format ScanProjectsOutput: {}", e);
            fmt::Error
        })?;
        let s = buf_to_str(buf)?;
        write!(f, "{}", s)
    }
}

pub struct GetArtifactAnswerOutput {
    answer: GetArtifactAnswer,
    options: OutputOptions,
}

impl GetArtifactAnswerOutput {
    fn fmt_default(&self, buf: &mut Buffer) -> Result<(), io::Error> {
        let color = match self.answer {
            GetArtifactAnswer::NotFound => Color::Red,
            GetArtifactAnswer::NewArtifact(_) => Color::Green,
            GetArtifactAnswer::UpToDate(_) => Color::Yellow,
        };
        buf.set_color(ColorSpec::new().set_fg(Some(color)))?;
        match &self.answer {
            GetArtifactAnswer::NotFound => write!(buf, "Not Found!")?,
            GetArtifactAnswer::NewArtifact(file) => {
                write!(buf, "New ")?;
                buf.reset()?;
                write!(buf, "({})", file)?
            }
            GetArtifactAnswer::UpToDate(file) => {
                write!(buf, "Up to date ")?;
                buf.reset()?;
                write!(buf, "({})", file)?;
            }
        };
        buf.reset()
    }
}

impl Display for GetArtifactAnswerOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut buf = create_buf(&self.options);
        self.fmt_default(&mut buf).map_err(|e| {
            error!("Failed to format GetArtifactAnswerOutput: {}", e);
            fmt::Error
        })?;
        let s = buf_to_str(buf)?;
        write!(f, "{}", s)
    }
}

impl FormatOutput<GetArtifactAnswerOutput> for GetArtifactAnswer {
    fn format_output(self, options: &OutputOptions) -> GetArtifactAnswerOutput {
        GetArtifactAnswerOutput {
            answer: self,
            options: options.clone(),
        }
    }
}

fn create_buf(options: &OutputOptions) -> Buffer {
    if options.color {
        Buffer::ansi()
    } else {
        Buffer::no_color()
    }
}

pub fn buf_to_str(buf: Buffer) -> Result<String, fmt::Error> {
    String::from_utf8(buf.into_inner()).map_err(|e| {
        error!("Failed to convert buffer to UTF-8 string: {}", e);
        fmt::Error
    })
}
