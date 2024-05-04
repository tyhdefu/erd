use std::io::{Cursor, Read};

use log::{debug, info, warn};
use reqwest::blocking::Response;
use reqwest::header::{HeaderName, HeaderValue};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use zip::ZipArchive;

use crate::config::{ArtifactConfig, SourceType};
use crate::{extract_file, ErdError, FileData};

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
    #[serde(rename = "ref")]
    pub job_ref: String,
    pub commit: JobCommit,
    pub pipeline: JobPipeline,
    pub artifacts_file: Option<JobArtifactsFile>,
    pub web_url: String,
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
    pub id: usize,
    pub status: String,
    #[serde(rename = "ref")]
    pub job_ref: String,
    pub web_url: String,
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
        .map_err(|e| request_failed(e, "Failed to get project list"))?;
    let projects: Vec<ProjectData> = deserialize_response(response)?;
    info!("Path (ID) - URL");
    let longest_name: usize = projects
        .iter()
        .map(|p| p.path_with_namespace.len())
        .max()
        .unwrap_or(0);
    for project in projects {
        info!(
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
) -> Result<Option<FileData>, ErdError> {
    let buffer = match build_id {
        Some(b_id) => get_artifact_version_gitlab(artifact, token, &b_id)?,
        None => get_latest_artifact_gitlab(artifact, token)?,
    };

    let mut found_jar = Option::None;
    let mut zip_archive = ZipArchive::new(Cursor::new(buffer))
        .map_err(|e| ErdError::IOError(e.into(), "Invalid zip archive".to_string()))?;
    for file_name in zip_archive.file_names() {
        debug!("File name: {}", file_name);
        if file_name.ends_with(&artifact.artifact_pattern) {
            info!("Artifact: {}", file_name);
            found_jar = Option::Some(file_name.to_string());
        }
    }
    match found_jar {
        Some(jar_name) => {
            let file_data = extract_file(&mut zip_archive, &jar_name)
                .map_err(|e| ErdError::IOError(e, "Failed to extract artifact from zip".into()))?;
            Ok(Some(file_data))
        }
        None => Ok(None),
    }
}

fn get_latest_artifact_gitlab(artifact: &ArtifactConfig, token: &str) -> Result<Vec<u8>, ErdError> {
    let url = format!(
        "https://gitlab.com/api/v4/projects/{}/jobs/artifacts/{}/download?job=build",
        artifact.project_id, artifact.branch
    );

    let client = reqwest::blocking::Client::new();
    let token_value = get_token_value(token)?;
    let mut response = client
        .get(url)
        .header(TOKEN_HEADER, token_value)
        .send()
        .map_err(|e| request_failed(e, "Failed to get artifact from Gitlab"))?;
    let mut buffer = vec![];
    let bytes_read = response
        .read_to_end(&mut buffer)
        .map_err(|e| ErdError::IOError(e, "Failed to read data from artifact zip".to_string()))?;
    debug!("{} bytes read", bytes_read);
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
        .get(url)
        .header(TOKEN_HEADER, token_value)
        .send()
        .map_err(|e| request_failed(e, "Failed to get artifact Gitlab"))?;
    let mut buffer = vec![];
    let bytes_read = response
        .read_to_end(&mut buffer)
        .map_err(|e| ErdError::IOError(e, "Failed to read data from artifact zip".to_string()))?;
    debug!("{} bytes read", bytes_read);
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
        .get(url)
        .query(&[
            ("order_by", "updated_at"),
            ("ref", &artifact.branch),
            ("name", job_name),
            ("per_page", "10"),
        ])
        .header(TOKEN_HEADER, token_value)
        .send()
        .map_err(|e| request_failed(e, "Failed to get artifact from Gitlab"))?;
    let job_history: Vec<JobHistory> = deserialize_response(response)?;
    if short {
        show_history_short(artifact, job_name, job_history);
    } else {
        show_history_long(artifact, job_name, job_history);
    }
    Ok(())
}

fn show_history_long(artifact: &ArtifactConfig, job_name: &str, job_history: Vec<JobHistory>) {
    info!(
        "Showing {} jobs for {} on branch {}",
        job_name, artifact.id, artifact.branch
    );
    for job in job_history {
        let has_artifacts = job.artifacts_file.is_some();
        info!(
            "{} ({}) - {}",
            job.commit.short_id, job.job_ref, job.commit.title
        );
        info!("\tBuild id: {}", job.id);
        info!("\tTimestamp: {}", job.created_at);
        info!(
            "\tStatus: {}{}",
            job.status,
            if !has_artifacts {
                " (no artifacts)"
            } else {
                ""
            }
        );
        info!("\tURL: {}", job.web_url);
        info!("\tAuthor: {}", job.commit.author_email);
    }
}

fn show_history_short(artifact: &ArtifactConfig, job_name: &str, job_history: Vec<JobHistory>) {
    info!(
        "Showing {} jobs for {} on branch {}",
        job_name, artifact.id, artifact.branch
    );
    info!("Id - When - Commit (Author) - Status");
    for entry in job_history {
        if entry.name != job_name {
            continue;
        }
        let mut job_status = entry.status.clone();
        if entry.artifacts_file.is_none() {
            job_status += " (expired)";
        }
        info!(
            "{} - {} - {} ({}) - {}",
            entry.id,
            entry.created_at,
            entry.commit.short_id,
            entry.commit.author_email,
            job_status,
        )
    }
}

pub fn rebuild_artifact_gitlab(
    artifact: &ArtifactConfig,
    token: &str,
    build_id: String,
) -> Result<(), ErdError> {
    let client = reqwest::blocking::Client::new();
    let token_value = get_token_value(token)?;
    let create_pipeline_url = format!(
        "https://gitlab.com/api/v4/projects/{}/pipeline",
        artifact.project_id,
    );
    let create_pipeline_response = client
        .post(&create_pipeline_url)
        .header(TOKEN_HEADER, token_value.clone())
        .query(&[("ref", &build_id)])
        .send()
        .map_err(|e| request_failed(e, &format!("Failed to retry job {} on Gitlab", build_id)))?;
    let new_pipeline: JobPipeline = deserialize_response(create_pipeline_response)?;
    info!(
        "Started pipeline {} to rebuild {}",
        new_pipeline.id, build_id
    );
    let list_jobs_url = format!(
        "https://gitlab.com/api/v4/projects/{}/pipeline",
        new_pipeline.id
    );
    let list_jobs_response = client
        .get(&list_jobs_url)
        .header(TOKEN_HEADER, token_value)
        .send()
        .map_err(|e| request_failed(e, "Failed to list jobs for created pipeline"))?;
    let pipeline_jobs: Vec<JobHistory> = deserialize_response(list_jobs_response)?;
    match pipeline_jobs.first() {
        Some(job) => {
            info!(
                "> {} ({}) - {}",
                job.commit.short_id, new_pipeline.job_ref, job.commit.title
            );
        }
        None => {
            warn!("No jobs appear to have been started");
        }
    }
    for job in pipeline_jobs {
        info!("> Started job {} ({}) - {}", job.name, job.id, job.web_url);
    }
    info!("> {}", new_pipeline.job_ref);
    info!(
        "> New pipeline {} - {}",
        new_pipeline.id, new_pipeline.web_url
    );
    info!("Check the job history to see when the pipeline is complete and its job id");
    Ok(())
}

fn deserialize_response<T: DeserializeOwned>(response: Response) -> Result<T, ErdError> {
    let url = response.url().to_string();
    let response_text = response.text().map_err(unexpected_response)?;
    serde_json::from_str(&response_text).map_err(|e| ErdError::SourceRequestError {
        source: SourceType::Gitlab,
        url: url.clone(),
        desc: format!("Failed to deserialize response from Gitlab: {}", e),
    })
}

fn unexpected_response(error: reqwest::Error) -> ErdError {
    let url = error
        .url()
        .map(|url| url.to_string())
        .unwrap_or_else(|| "UNKNOWN".to_string());
    ErdError::SourceRequestError {
        source: SourceType::Gitlab,
        url,
        desc: format!("Unexpected response from Gitlab: {}", error),
    }
}

fn request_failed(error: reqwest::Error, what: &str) -> ErdError {
    let url = error
        .url()
        .map(|url| url.to_string())
        .unwrap_or_else(|| "UNKNOWN".to_string());
    ErdError::SourceRequestError {
        source: SourceType::Gitlab,
        url,
        desc: format!("{}: {}", what, error),
    }
}
