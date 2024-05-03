use std::fs::File;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use reqwest::header::{HeaderName, HeaderValue};
use serde::Deserialize;
use zip::ZipArchive;

use crate::config::{ArtifactConfig, SourceType};
use crate::{extract_file, ErdError};

#[derive(Deserialize)]
pub struct ProjectData {
    pub id: usize,
    pub path_with_namespace: String,
    pub default_branch: Option<String>,
    pub web_url: String,
}

#[derive(Deserialize)]
pub struct JobHistory {
    pub id: usize,
    pub status: String,
    pub stage: String,
    pub created_at: String,
    pub name: String,
    pub commit: JobCommit,
    pub pipeline: JobPipeline,
    pub artifacts_file: Option<JobArtifactsFile>,
}

#[derive(Deserialize)]
pub struct JobCommit {
    pub id: String,
    pub short_id: String,
    pub created_at: String,
    pub author_email: String,
    pub title: String,
}

#[derive(Deserialize)]
pub struct JobPipeline {
    pub status: String,
}

#[derive(Deserialize)]
pub struct JobArtifactsFile {
    filename: String,
}

const TOKEN_HEADER: HeaderName = HeaderName::from_static("private-token");

fn get_token_value(token: &str) -> Result<HeaderValue, ErdError> {
    token
        .parse()
        .map_err(|_| ErdError::InvalidToken(token.to_string()))
}

pub fn scan_gitlab(query: Option<String>, token: &str) -> Result<(), ErdError> {
    let client = reqwest::blocking::Client::new();
    let token_value: HeaderValue = get_token_value(token)?;
    // https://docs.gitlab.com/ee/api/projects.html#list-all-projects
    // TODO: filter by owned, group, etc.
    let url = "https://gitlab.com/api/v4/projects";
    let response = client
        .get(url)
        .query(&[
            ("membership", "true"),
            ("order_by", "last_activity_at"),
            ("per_page", "30"),
            ("search", query.as_deref().unwrap_or("")),
            ("search_namespaces", "true"),
        ])
        .header(TOKEN_HEADER, token_value)
        .send()
        .map_err(|e| ErdError::SourceRequestError {
            source: SourceType::Gitlab,
            url: url.to_string(),
            desc: format!("Failed to get project list: {}", e),
        })?;
    let response_data = response.text().map_err(|e| ErdError::SourceRequestError {
        source: SourceType::Gitlab,
        url: url.to_string(),
        desc: format!("Non-text response from Gitlab: {}", e),
    })?;

    let projects: Vec<ProjectData> =
        serde_json::from_str(&response_data).map_err(|e| ErdError::SourceRequestError {
            source: SourceType::Gitlab,
            url: url.to_string(),
            desc: format!("Failed to deserialize response from Gitlab: {}", e),
        })?;
    println!("Path (ID) - URL");
    let longest_name: usize = projects
        .iter()
        .map(|p| p.path_with_namespace.len())
        .max()
        .unwrap_or(0);
    for project in projects {
        println!(
            "{:longest_name$} ({}) - {}",
            project.path_with_namespace, project.id, project.web_url
        );
    }
    Ok(())
}

pub fn get_artifact_gitlab(
    artifact: &ArtifactConfig,
    token: &str,
    build_id: Option<String>,
) -> Result<(), ErdError> {
    let output_dir = Path::new("temp");

    let buffer = match build_id {
        Some(b_id) => get_artifact_version_gitlab(artifact, token, &b_id)?,
        None => get_latest_artifact_gitlab(artifact, token)?,
    };

    let mut file = File::create(output_dir.join("artifacts.zip"))
        .map_err(|e| ErdError::IOError(e, "Failed to create artifacts.zip file".to_string()))?;
    file.write_all(&buffer)
        .map_err(|e| ErdError::IOError(e, "Failed to save artifacts.zip".to_string()))?;

    let mut found_jar = Option::None;
    let mut zip_archive = ZipArchive::new(Cursor::new(buffer))
        .map_err(|e| ErdError::IOError(e.into(), "Invalid zip archive".to_string()))?;
    for file_name in zip_archive.file_names() {
        println!("File name: {}", file_name);
        if file_name.ends_with(&artifact.artifact_pattern) {
            println!("Artifact: {}", file_name);
            found_jar = Option::Some(file_name.to_string());
        }
    }
    if let Some(jar_name) = found_jar {
        let path: PathBuf = jar_name.parse().expect("Invalid jar path");
        let file_data = extract_file(&mut zip_archive, &jar_name).expect("Failed to extract JAR");
        let file_name = path.file_name().expect("Path was not a file name!");
        println!("Writing Artifact: {:?}", file_name);

        let mut jar_file = File::create(output_dir.join(file_name))
            .map_err(|e| ErdError::IOError(e, "Failed to create Artifact file".to_string()))?;
        jar_file
            .write_all(&file_data)
            .map_err(|e| ErdError::IOError(e, "Failed to write Artifact".into()))?;
    }
    Ok(())
}

fn get_latest_artifact_gitlab(artifact: &ArtifactConfig, token: &str) -> Result<Vec<u8>, ErdError> {
    let url = format!(
        "https://gitlab.com/api/v4/projects/{}/jobs/artifacts/{}/download?job=build",
        artifact.project_id, artifact.branch
    );

    let client = reqwest::blocking::Client::new();
    let token_value = get_token_value(token)?;
    let mut response = client
        .get(&url)
        .header(TOKEN_HEADER, token_value)
        .send()
        .map_err(|e| ErdError::SourceRequestError {
            source: SourceType::Gitlab,
            url: url.clone(),
            desc: format!("Failed to get artifact from Gitlab: {}", e),
        })?;
    let mut buffer = vec![];
    let bytes_read = response
        .read_to_end(&mut buffer)
        .map_err(|e| ErdError::IOError(e, "Failed to read data from artifact zip".to_string()))?;
    println!("{} bytes read", bytes_read);
    Ok(buffer)
}

pub fn get_artifact_version_gitlab(
    artifact: &ArtifactConfig,
    token: &str,
    build_id: &str,
) -> Result<Vec<u8>, ErdError> {
    let url = format!(
        "https://gitlab.com/api/v4/projects/{}/jobs/{}/artifacts",
        artifact.project_id, build_id
    );
    let client = reqwest::blocking::Client::new();
    let token_value = get_token_value(token)?;
    let mut response = client
        .get(&url)
        .header(TOKEN_HEADER, token_value)
        .send()
        .map_err(|e| ErdError::SourceRequestError {
            source: SourceType::Gitlab,
            url: url.clone(),
            desc: format!("Failed to get artifact from Gitlab: {}", e),
        })?;
    let mut buffer = vec![];
    let bytes_read = response
        .read_to_end(&mut buffer)
        .map_err(|e| ErdError::IOError(e, "Failed to read data from artifact zip".to_string()))?;
    println!("{} bytes read", bytes_read);
    Ok(buffer)
}

pub fn get_history_gitlab(
    artifact: &ArtifactConfig,
    token: &str,
    short: bool,
) -> Result<(), ErdError> {
    let client = reqwest::blocking::Client::new();
    let token_value = get_token_value(token)?;
    let url = format!(
        "https://gitlab.com/api/v4/projects/{}/jobs",
        artifact.project_id
    );
    let job_name = "build";
    let response = client
        .get(&url)
        .query(&[
            ("order_by", "updated_at"),
            ("ref", &artifact.branch),
            ("name", job_name),
            ("per_page", "5"),
        ])
        .header(TOKEN_HEADER, token_value)
        .send()
        .map_err(|e| ErdError::SourceRequestError {
            source: SourceType::Gitlab,
            url: url.clone(),
            desc: format!("Failed to get artifact from Gitlab: {}", e),
        })?;
    let response_data = response.text().map_err(|e| ErdError::SourceRequestError {
        source: SourceType::Gitlab,
        url: url.clone(),
        desc: format!("Did not receive text from Gitlab: {}", e),
    })?;
    let job_history: Vec<JobHistory> =
        serde_json::from_str(&response_data).map_err(|e| ErdError::SourceRequestError {
            source: SourceType::Gitlab,
            url: url.clone(),
            desc: format!("Failed to deserialize response from Gitlab: {}", e),
        })?;
    if short {
        show_history_short(artifact, job_name, job_history);
    } else {
        show_history_long(artifact, job_name, job_history);
    }
    Ok(())
}

fn show_history_long(artifact: &ArtifactConfig, job_name: &str, job_history: Vec<JobHistory>) {
    println!(
        "Showing {} jobs for {} on branch {}",
        job_name, artifact.id, artifact.branch
    );
    for job in job_history {
        let has_artifacts = job.artifacts_file.is_some();
        println!("{} - {}", job.commit.short_id, job.commit.title);
        println!("\tId: {}", job.id);
        println!("\tTimestamp: {}", job.created_at);
        println!(
            "\tStatus: {}{}",
            job.status,
            if !has_artifacts { " (expired)" } else { "" }
        );
        println!("\tAuthor: {}", job.commit.author_email);
    }
}

fn show_history_short(artifact: &ArtifactConfig, job_name: &str, job_history: Vec<JobHistory>) {
    println!(
        "Showing {} jobs for {} on branch {}",
        job_name, artifact.id, artifact.branch
    );
    println!("Id - When - Commit (Author) - Status");
    for entry in job_history {
        if entry.name != job_name {
            continue;
        }
        let mut job_status = entry.status.clone();
        if entry.artifacts_file.is_none() {
            job_status += " (expired)";
        }
        println!(
            "{} - {} - {} ({}) - {}",
            entry.id,
            entry.created_at,
            entry.commit.short_id,
            entry.commit.author_email,
            job_status,
        )
    }
}
