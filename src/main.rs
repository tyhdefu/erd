mod config;
mod gitlab;
mod log;

use std::fs;
use std::io::{self, Read, Seek, Write};
use std::{
    fmt::Display,
    fs::File,
    path::{Path, PathBuf},
    process::exit,
};

use ::log::{debug, error, info, warn, LevelFilter};
use clap::{Parser, Subcommand};
use config::{ArtifactConfig, SourceConfig, SourceType};
use gitlab::{get_artifact_gitlab, get_history_gitlab, rebuild_artifact_gitlab, scan_gitlab};
use sha2::{Digest, Sha256};
use zip::ZipArchive;

use crate::config::Config;

pub struct FileData {
    file_name: PathBuf,
    data: Vec<u8>,
}

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
    let level = if cli.verbose {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    };
    log::setup(level);

    if let Err(e) = handle_cli(cli, config) {
        error!("{}", e);
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
                                info!("Retrieving {} from {}", art.id, source.id);
                                get_artifact(art, &source.kind, &source.token, None)?;
                                found = true;
                            }
                        }
                    }
                    if !found {
                        warn!("No artifacts to retrieve");
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
        Commands::Rebuild { artifact, build_id } => {
            let found = config.sources.iter().find_map(|s| {
                s.artifacts
                    .iter()
                    .find(|a| a.id == artifact)
                    .map(|a| (s, a))
            });
            let (src, a) = found.ok_or(ErdError::NoSuchArtifact(artifact))?;
            rebuild_artifact(a, &src.kind, &src.token, build_id)?;
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
    /// Rebuild an expired artifact
    /// TODO: This works for branches/tags only on Gitlab.A
    ///       Perhaps add a way to tag commits and subsequently build
    Rebuild { artifact: String, build_id: String },
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
    /// Run with increased output for debugging
    #[clap(short, long)]
    verbose: bool,
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
    let output_dir = Path::new("temp");

    let file_data = match kind {
        SourceType::Gitlab => get_artifact_gitlab(artifact, token, build_id)?,
    };

    match file_data {
        Some(art) => {
            let output_file = output_dir.join(&art.file_name);
            if output_file.exists() {
                debug!("{:?} already exists, checking if same", &art.file_name);
                let existing_hash = sha256sum_file(&output_file)
                    .map_err(|e| ErdError::IOError(e, "Failed to read existing file".into()))?;
                let new_hash = sha256sum_mem(&art)
                    .map_err(|e| ErdError::IOError(e, "Failed to calculate new hash".into()))?;
                if existing_hash == new_hash {
                    info!("No new artifact for {}", artifact.id);
                    return Ok(());
                }
            }
            info!("Fetched new Artifact: {:?}", art.file_name);

            let mut jar_file = File::create(output_file)
                .map_err(|e| ErdError::IOError(e, "Failed to create Artifact file".to_string()))?;
            jar_file
                .write_all(&art.data)
                .map_err(|e| ErdError::IOError(e, "Failed to write Artifact".into()))?;
        }
        None => {
            warn!("No artifact found");
        }
    }

    Ok(())
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
            info!("Artifacts from {}", artifact_source.id);
            for artifact in &artifact_source.artifacts {
                info!("- {} ({})", artifact.id, artifact.branch);
            }
        }
        None => {
            for src in &config.sources {
                info!("== Artifacts from {} ==", src.id);
                for artifact in &src.artifacts {
                    info!("- {} ({})", artifact.id, artifact.branch);
                }
            }
        }
    }
    Ok(())
}

fn rebuild_artifact(
    artifact: &ArtifactConfig,
    kind: &SourceType,
    token: &str,
    build_id: String,
) -> Result<(), ErdError> {
    match kind {
        SourceType::Gitlab => rebuild_artifact_gitlab(artifact, token, build_id),
    }
}

pub fn extract_file(
    archive: &mut ZipArchive<impl Read + Seek>,
    file: &str,
) -> Result<FileData, io::Error> {
    let path: PathBuf = file.parse().expect("Invalid filename");
    let file_name = path.file_name().expect("Could not get filename from path");

    let mut jar = archive.by_name(file)?;
    let mut file_buf = vec![];
    jar.read_to_end(&mut file_buf)?;
    Ok(FileData {
        file_name: file_name.into(),
        data: file_buf,
    })
}

fn sha256sum_file(path: &Path) -> Result<Vec<u8>, io::Error> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher)?;
    Ok(hasher.finalize().iter().cloned().collect())
}

fn sha256sum_mem(data: &FileData) -> Result<Vec<u8>, io::Error> {
    let hash = Sha256::digest(&data.data);
    Ok(hash.iter().cloned().collect())
}
