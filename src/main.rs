mod gitlab;
mod log;
mod input;
mod output;
mod config;
mod auth;
mod commands;

use std::fs;
use std::io::{self, Read, Seek};
use std::path::{Path, PathBuf};
use std::{fmt::Display, process::exit};

use auth::Login;
use input::read_with_prompt;
use ::log::{error, info, LevelFilter};
use clap::{Parser, Subcommand};
use gitlab::{get_history_gitlab, rebuild_artifact_gitlab, scan_gitlab};
use output::{ArtifactListOutput, FormatOutput, OutputOptions};
use sha2::{Digest, Sha256};
use zip::ZipArchive;

use config::artifacts::{Config, ArtifactConfig, SourceConfig, SourceType};

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
    /// No login exists for the given artifact
    NoLogin {
        source_url: String,
    },
    IOError(io::Error, String),
    /// Failed to deserialize config
    Deserialize(toml::de::Error, String)
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
            ErdError::NoLogin { source_url } => write!(f, "Missing login for {}", source_url),
            ErdError::Deserialize(e, desc) => write!(f, "Failed to deserialize: {}. {}", desc, e),
        }
    }
}

fn get_artifact_config_file(specified: &Option<PathBuf>) -> PathBuf {
    specified.clone().unwrap_or_else(|| {
        let mut path = config::get_local_dir();
        path.push(config::artifacts::ARTIFACTS_FILE);
        path
    })
}

fn get_store_directory(specified: &Option<PathBuf>) -> PathBuf {
    if let Some(s) = specified {
        return s.clone();
    }
    if cfg!(debug_assertions) {
        return "temp".into();
    }
    return config::get_local_dir();
}

fn main() {
    let cli = Cli::parse();

    let level = if cli.verbose {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    };
    log::setup(level);
    let options = OutputOptions {
        color: true,
        short: false,
    };

    match cli.command {
        Commands::Init { silent } => {
            commands::init::init_erd(!silent).unwrap();
            return;
        }
        _ => {},
    }


    let config_file_path = get_artifact_config_file(&cli.config);

    let config_str = match std::fs::read_to_string(&config_file_path) {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to read {:?}: {}", config_file_path, e);
            exit(1);
        }
    };
    let config: Config = match toml::from_str(&config_str) {
        Ok(c) => c,
        Err(e) => {
            error!("Invalid artifact config: {}", e);
            exit(1);
        }
    };

    if let Err(e) = handle_cli(cli, config, &config_file_path, options) {
        error!("{}", e);
        exit(1);
    }
}

fn handle_cli(cli: Cli, config: Config, config_file_path: &Path, options: OutputOptions) -> Result<(), ErdError> {
    let auth_file = auth::get_auth_file().expect("Failed to find suitable local config path");

    match cli.command {
        // TODO: split into multiple but hide from clap - clap(flatten)
        Commands::Init { .. } => panic!("Init should have already been handled!"),
        Commands::Fetch { artifact, build_id } => {
            let logins = auth::read_auth_file(&auth_file)?;
            return commands::fetch::fetch(&config, &logins, artifact, build_id, &options)
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
            let logins = auth::read_auth_file(&auth_file)?;
            let login = logins.find_login(&matched_src.url)
                .ok_or_else(|| ErdError::NoLogin { source_url: matched_src.url.clone() })?;
            scan_source(matched_src, group.clone(), login)?;
        }
        Commands::History { artifact, short } => {
            let found = config.sources.iter().find_map(|s| {
                s.artifacts
                    .iter()
                    .find(|a| a.id == artifact)
                    .map(|a| (s, a))
            });
            let (src, a) = found.ok_or(ErdError::NoSuchArtifact(artifact))?;
            let logins = auth::read_auth_file(&auth_file)?;
            let login = logins.find_login(&src.url)
                .ok_or_else(|| ErdError::NoLogin { source_url: src.url.clone() })?;
            commands::history::get_history(a, &src.kind, login, short)?;
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
            let logins = auth::read_auth_file(&auth_file)?;
            let login = logins.find_login(&src.url)
                .ok_or_else(|| ErdError::NoLogin { source_url: src.url.clone() })?;
            rebuild_artifact(a, &src.kind, &login.password, build_id)?;
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


#[derive(Subcommand, Debug)]
enum Commands {
    /// Initalise erd in the current directory
    Init {
        /// Whether to just create files and skip interactive setup
        #[clap(short, long)]
        silent: bool,
    },
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
    /// Override the config file used
    config: Option<PathBuf>,
}

fn scan_source(source: &SourceConfig, group: Option<String>, login: &Login) -> Result<(), ErdError> {
    match source.kind {
        SourceType::Gitlab => scan_gitlab(group, &login.password),
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

