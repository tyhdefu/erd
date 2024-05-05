mod config;
mod gitlab;
mod log;
mod output;

use std::fs;
use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::{fmt::Display, fs::File, process::exit};

use ::log::{debug, error, info, warn, LevelFilter};
use clap::{Parser, Subcommand};
use config::{ArtifactConfig, SourceConfig, SourceType};
use gitlab::{get_artifact_gitlab, get_history_gitlab, rebuild_artifact_gitlab, scan_gitlab};
use output::{ArtifactListOutput, FormatOutput, OutputOptions};
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
    let config_file_path = Path::new("test_config.toml");
    let config_str = std::fs::read_to_string(config_file_path).expect("Failed to read test config");
    let config: Config = toml::from_str(&config_str).expect("Invalid config!");

    let cli = Cli::parse();
    let level = if cli.verbose {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    };
    log::setup(level);

    if let Err(e) = handle_cli(cli, config, config_file_path) {
        error!("{}", e);
        exit(1);
    }
}

fn print_fetch_answer(
    answer: GetArtifactAnswer,
    artifact_id: &str,
    padding: usize,
    options: &OutputOptions,
) {
    let error = matches!(&answer, GetArtifactAnswer::NotFound);
    let answer_output = answer.format_output(options);
    if error {
        error!("{:padding$} {}", artifact_id, answer_output);
    } else {
        info!("{:padding$} {}", artifact_id, answer_output);
    }
}

fn handle_cli(cli: Cli, config: Config, config_file_path: &Path) -> Result<(), ErdError> {
    let options = OutputOptions {
        color: true,
        short: false,
    };

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
                    let answer = get_artifact(artifact, &source.kind, &source.token, build_id)?;
                    print_fetch_answer(answer, &artifact.id, 0, &options);
                }
                None => {
                    // Fetch all artifacts
                    let longest_id = config
                        .sources
                        .iter()
                        .flat_map(|s| &s.artifacts)
                        .map(|a| a.id.len())
                        .max()
                        .unwrap_or(0);

                    let mut found = false;
                    for source in &config.sources {
                        for art in &source.artifacts {
                            if artifact.is_none() || artifact.as_ref().unwrap() == &art.id {
                                debug!("Retrieving {} from {}", art.id, source.id);
                                let answer = get_artifact(art, &source.kind, &source.token, None)?;
                                print_fetch_answer(answer, &art.id, longest_id, &options);
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
        Commands::Add { source, project_id } => {
            let mut new_config = config.clone();
            let source = new_config
                .sources
                .iter_mut()
                .find(|s| s.id == source)
                .ok_or(ErdError::NoSuchSource(source))?;

            let id = read_with_prompt("Unique ID")?;
            let branch = read_with_prompt("Branch")?;
            let artifact_pattern = read_with_prompt("Artifact Pattern (e.g. *.jar)")?;
            let art = ArtifactConfig {
                id,
                project_id,
                branch,
                artifact_pattern,
            };
            source.artifacts.push(art);
            let config_str = toml::to_string(&new_config).expect("Should be able to serialize");
            std::fs::write(config_file_path, config_str)
                .map_err(|e| ErdError::IOError(e, "Failed to write new config file".into()))?;
        }
    };
    Ok(())
}

fn read_with_prompt(prompt: &str) -> Result<String, ErdError> {
    print!("{}: ", prompt);
    io::stdout()
        .flush()
        .map_err(|e| ErdError::IOError(e, "Failed to flush stdout".into()))?;
    let mut buffer = String::new();
    io::stdin()
        .read_line(&mut buffer)
        .map_err(|e| ErdError::IOError(e, format!("Failed to read answer to {}", prompt)))?;
    let buffer = buffer.trim().into();
    Ok(buffer)
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Retrieve artifacts
    Fetch {
        /// Only fetch the given artifact
        artifact: Option<String>,
        /// Fetch a specific version rather than the latest
        build_id: Option<String>,
    },
    /// Scan for projects to add to configuration
    Scan {
        /// The source to scan
        source: String,
        /// Search for a substring
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
    /// List fetchable artifacts
    List {
        /// Only list artifacts from the given source
        source: Option<String>,
    },
    /// Rebuild an expired artifact
    Rebuild {
        /// The artifact to rebuild
        artifact: String,
        /// The version to rebuild
        build_id: String,
    },
    /// Add a project to configuration
    Add {
        /// The source that this is a part of
        source: String,
        /// The ID of the project to be added
        project_id: String,
    }, // TODO: Perhaps a way to tag versions before rebuilding?
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

pub enum GetArtifactAnswer {
    /// Failed to find an artifact file within the output of a job
    NotFound,
    /// Found a new artifact with the given filename
    NewArtifact(String),
    /// Found an artifact, but it was identical to the existing artifact
    UpToDate(String),
}

fn get_artifact(
    artifact: &ArtifactConfig,
    kind: &SourceType,
    token: &str,
    build_id: Option<String>,
) -> Result<GetArtifactAnswer, ErdError> {
    let output_dir = Path::new("temp");

    let file_data = match kind {
        SourceType::Gitlab => get_artifact_gitlab(artifact, token, build_id)?,
    };

    fn is_new(output_file: &Path, file_data: &FileData) -> Result<bool, ErdError> {
        if !output_file.exists() {
            return Ok(true);
        }
        debug!("{:?} already exists, checking if same", file_data.file_name);
        let existing_hash = sha256sum_file(output_file)
            .map_err(|e| ErdError::IOError(e, "Failed to read existing file".into()))?;
        let new_hash = sha256sum_mem(file_data)
            .map_err(|e| ErdError::IOError(e, "Failed to calculate new hash".into()))?;
        Ok(existing_hash != new_hash)
    }

    Ok(match file_data {
        Some(art) => {
            let filename_string = art.file_name.to_string_lossy().to_string();

            let output_file = output_dir.join(&art.file_name);

            if !is_new(&output_file, &art)? {
                return Ok(GetArtifactAnswer::UpToDate(filename_string));
            }

            let mut jar_file = File::create(output_file)
                .map_err(|e| ErdError::IOError(e, "Failed to create Artifact file".to_string()))?;
            jar_file
                .write_all(&art.data)
                .map_err(|e| ErdError::IOError(e, "Failed to write Artifact".into()))?;

            GetArtifactAnswer::NewArtifact(filename_string)
        }
        None => GetArtifactAnswer::NotFound,
    })
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
    let options = OutputOptions {
        color: true,
        short: false,
    };
    match source {
        Some(src) => {
            let artifact_source = config
                .sources
                .iter()
                .find(|s| s.id == src)
                .ok_or(ErdError::NoSuchSource(src))?;
            let list_output: ArtifactListOutput = artifact_source.format_output(&options);
            info!("{}", list_output);
        }
        None => {
            for src in &config.sources {
                let list_output: ArtifactListOutput = src.format_output(&options);
                info!("{}", list_output);
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
