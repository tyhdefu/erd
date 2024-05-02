mod config;
mod gitlab;

use std::io::{self, Read, Seek};

use clap::{Parser, Subcommand};
use config::{ArtifactConfig, SourceConfig, SourceType};
use gitlab::{get_history_gitlab, get_latest_artifact_gitlab, scan_gitlab};
use zip::ZipArchive;

use crate::config::Config;

fn main() {
    let config_str =
        std::fs::read_to_string("test_config.toml").expect("Failed to read test config");
    let config: Config = toml::from_str(&config_str).expect("Invalid config!");

    let cli = Cli::parse();

    match &cli.command {
        Commands::Fetch { artifact } => {
            let mut found = false;
            for source in &config.sources {
                for art in &source.artifacts {
                    if artifact.is_none() || artifact.as_ref().unwrap() == &art.id {
                        println!("Retrieving {} from {}", art.id, source.id);
                        get_latest_artifact(art, &source.kind, &source.token);
                        found = true;
                    }
                }
            }
            if !found {
                if let Some(id) = artifact {
                    eprintln!("No such artifact: '{}'", id);
                } else {
                    eprintln!("No artifacts to retrieve");
                }
            }
        }
        Commands::Scan { source, group } => {
            let matched_src = config
                .sources
                .iter()
                .find(|src| &src.id == source)
                .unwrap_or_else(|| panic!("No source named: {}", source));
            scan_source(matched_src, group.clone())
        }
        Commands::History { artifact } => {
            let found = config.sources.iter().find_map(|s| {
                s.artifacts
                    .iter()
                    .find(|a| &a.id == artifact)
                    .map(|a| (s, a))
            });
            match found {
                Some((src, a)) => get_history(a, &src.kind, &src.token),
                None => {
                    eprintln!("No such artifact: {}", artifact)
                }
            }
        }
    }
}

#[derive(Subcommand, Debug)]
enum Commands {
    Fetch {
        artifact: Option<String>,
    },
    Scan {
        source: String,
        group: Option<String>,
    },
    History {
        artifact: String,
    },
}

#[derive(Parser, Debug)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

fn scan_source(source: &SourceConfig, group: Option<String>) {
    match source.kind {
        SourceType::Gitlab => scan_gitlab(group, &source.token),
    }
}

fn get_latest_artifact(artifact: &ArtifactConfig, kind: &SourceType, token: &str) {
    match kind {
        SourceType::Gitlab => get_latest_artifact_gitlab(artifact, token),
    }
}

fn get_history(artifact: &ArtifactConfig, kind: &SourceType, token: &str) {
    match kind {
        SourceType::Gitlab => get_history_gitlab(artifact, token),
    }
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
