mod config;
mod gitlab;

use std::{
    fmt::Display,
    io::{self, Read, Seek},
    process::exit,
};

use clap::{Parser, Subcommand};
use config::{ArtifactConfig, SourceConfig, SourceType};
use gitlab::{get_artifact_gitlab, get_history_gitlab, scan_gitlab};
use zip::ZipArchive;

use crate::config::Config;

#[derive(Debug)]
pub enum ErdError {
    NoSuchArtifact(String),
    NoSuchSource(String),
    SourceRequestError {
        source: SourceType,
        url: String,
        desc: String,
    },
    InvalidToken(String),
    IOError(io::Error, String),
}

impl Display for ErdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ErdError::NoSuchArtifact(artifact) => write!(f, "No such artifact: '{}'", artifact),
            ErdError::NoSuchSource(source) => write!(f, "No such source: '{}'", source),
            ErdError::SourceRequestError { source, url, desc } => {
                write!(f, "Error requesting {} from {:?}: {}", url, source, desc)
            }
            ErdError::InvalidToken(token) => write!(f, "Token was invalid: '{}'", token),
            ErdError::IOError(err, desc) => write!(f, "{desc}: {err}"),
        }
    }
}

fn main() {
    let config_str =
        std::fs::read_to_string("test_config.toml").expect("Failed to read test config");
    let config: Config = toml::from_str(&config_str).expect("Invalid config!");

    let cli = Cli::parse();

    if let Err(e) = handle_cli(cli, config) {
        eprintln!("{e}");
        exit(1);
    }
}

fn handle_cli(cli: Cli, config: Config) -> Result<(), ErdError> {
    match cli.command {
        Commands::Fetch { artifact, build_id } => {
            match artifact {
                Some(art) => {
                    // Fetch specific artifact
                    let (source, artifact) = config
                        .sources
                        .iter()
                        .find_map(|s| s.artifacts.iter().find(|a| a.id == art).map(|a| (s, a)))
                        .ok_or(ErdError::NoSuchArtifact(art))?;
                    get_artifact(artifact, &source.kind, &source.token, build_id)?;
                }
                None => {
                    // Fetch all artifacts

                    let mut found = false;
                    for source in &config.sources {
                        for art in &source.artifacts {
                            if artifact.is_none() || artifact.as_ref().unwrap() == &art.id {
                                println!("Retrieving {} from {}", art.id, source.id);
                                get_artifact(art, &source.kind, &source.token, None)?;
                                found = true;
                            }
                        }
                    }
                    if !found {
                        eprintln!("No artifacts to retrieve");
                    }
                }
            }
        }
        Commands::Scan {
            source,
            search: group,
        } => {
            let matched_src = config
                .sources
                .iter()
                .find(|src| src.id == source)
                .ok_or(ErdError::NoSuchSource(source))?;
            scan_source(matched_src, group.clone())?;
        }
        Commands::History { artifact, short } => {
            let found = config.sources.iter().find_map(|s| {
                s.artifacts
                    .iter()
                    .find(|a| a.id == artifact)
                    .map(|a| (s, a))
            });
            let (src, a) = found.ok_or(ErdError::NoSuchArtifact(artifact))?;
            get_history(a, &src.kind, &src.token, short)?;
        }
        Commands::List { source } => {
            list_artifacts(&config, source.clone())?;
        }
    };
    Ok(())
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Retrieve the given artifact
    Fetch {
        /// The id of the artifact to fetch
        artifact: Option<String>,
        /// The specific version id of the artifact
        build_id: Option<String>,
    },
    /// Scan for projects to add to configuration
    Scan {
        /// The source to scan
        source: String,
        search: Option<String>,
    },
    /// View the job history
    History {
        /// The particular artifact to view
        artifact: String,
        /// Display in a condensed view
        #[clap(long)]
        short: bool,
    },
    /// List artifacts
    List { source: Option<String> },
}

#[derive(Parser, Debug)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

fn scan_source(source: &SourceConfig, group: Option<String>) -> Result<(), ErdError> {
    match source.kind {
        SourceType::Gitlab => scan_gitlab(group, &source.token),
    }
}

fn get_artifact(
    artifact: &ArtifactConfig,
    kind: &SourceType,
    token: &str,
    build_id: Option<String>,
) -> Result<(), ErdError> {
    match kind {
        SourceType::Gitlab => get_artifact_gitlab(artifact, token, build_id),
    }
}

fn get_history(
    artifact: &ArtifactConfig,
    kind: &SourceType,
    token: &str,
    short: bool,
) -> Result<(), ErdError> {
    match kind {
        SourceType::Gitlab => get_history_gitlab(artifact, token, short),
    }
}

fn list_artifacts(config: &Config, source: Option<String>) -> Result<(), ErdError> {
    match source {
        Some(src) => {
            let artifact_source = config
                .sources
                .iter()
                .find(|s| s.id == src)
                .ok_or(ErdError::NoSuchSource(src))?;
            println!("Artifacts from {}", artifact_source.id);
            for artifact in &artifact_source.artifacts {
                println!("- {} ({})", artifact.id, artifact.branch);
            }
        }
        None => {
            for src in &config.sources {
                println!("== Artifacts from {} ==", src.id);
                for artifact in &src.artifacts {
                    println!("- {} ({})", artifact.id, artifact.branch);
                }
            }
        }
    }
    Ok(())
}
pub fn extract_file(
    archive: &mut ZipArchive<impl Read + Seek>,
    file: &str,
) -> Result<Vec<u8>, io::Error> {
    let mut jar = archive.by_name(file)?;
    let mut file_buf = vec![];
    jar.read_to_end(&mut file_buf)?;
    Ok(file_buf)
}
