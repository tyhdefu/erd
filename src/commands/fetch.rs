use std::fs::File;
use std::io::Write;
use std::path::Path;

use log::{debug, info, warn, error};

use crate::logins::Logins;
use crate::gitlab::get_artifact_gitlab;
use crate::output::{self, FormatOutput, OutputOptions};
use crate::{config, sha256sum_file, sha256sum_mem, ErdError, FileData};
use crate::config::artifacts::{ArtifactConfig, Config, SourceType};

pub enum GetArtifactAnswer {
    /// Failed to find an artifact file within the output of a job
    NotFound,
    /// Found a new artifact with the given filename
    NewArtifact(String),
    /// Found an artifact, but it was identical to the existing artifact
    UpToDate(String),
}

pub fn fetch(config: &Config, logins: &Logins, artifact_id: Option<String>, build_id: Option<String>, options: &OutputOptions) -> Result<(), ErdError> {
    match artifact_id {
        Some(art_id) => {
            let answer = fetch_single(config, logins, &art_id, build_id)?;
            print_fetch_answer(answer, &art_id, 0, &options);
        }
        None => {
            let answers = fetch_all(config, logins)?;
            let longest_id = answers.iter()
                .map(|(id, _answer)| id.len())
                .max();
            match longest_id {
                Some(padding) => {
                    for (id, answer) in answers {
                        print_fetch_answer(answer, &id, padding, &options);
                    }
                }
                None => {
                    warn!("No artifacts found!")
                }
            }
            
        }
    }
    Ok(())
}

pub fn fetch_single(config: &Config, logins: &Logins, art_id: &str, build_id: Option<String>)  -> Result<GetArtifactAnswer, ErdError> {
    // Fetch specific artifact
    let (source, artifact) = config
        .sources
        .iter()
        .find_map(|s| s.artifacts.iter().find(|a| a.id == art_id).map(|a| (s, a)))
        .ok_or(ErdError::NoSuchArtifact(art_id.to_owned()))?;
    let login = logins.find_login(&source.url).ok_or_else(|| 
        ErdError::NoLogin { source_url: source.url.clone() }
    )?;
    let answer = get_artifact(artifact, &source.kind, &login.password, build_id)?;
    return Ok(answer);
}

pub fn fetch_all(config: &Config, logins: &Logins) -> Result<Vec<(String, GetArtifactAnswer)>, ErdError> {
    // Fetch all artifacts
    let mut answers = vec![];
    for source in &config.sources {
        for art in &source.artifacts {
            debug!("Retrieving {} from {}", art.id, source.id);
            let login = logins.find_login(&source.url).ok_or_else(||
                ErdError::NoLogin { source_url: source.url.clone() }
            )?;

            let answer = get_artifact(art, &source.kind, &login.password, None)?;
            answers.push((art.id.clone(), answer));
        }
    }
    return Ok(answers);
}

fn get_artifact(
    artifact: &ArtifactConfig,
    kind: &SourceType,
    token: &str,
    build_id: Option<String>,
) -> Result<GetArtifactAnswer, ErdError> {
    let mut output_dir = config::get_local_dir();
    output_dir.push("downloads");

    std::fs::create_dir_all(&output_dir)
        .map_err(|e| ErdError::IOError(e, "Failed to create output dir".to_string()))?;

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