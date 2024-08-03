use std::io::{Cursor, Read};

use log::{debug, info, trace, warn};
use reqwest::blocking::Response;
use reqwest::header::{HeaderName, HeaderValue};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use zip::ZipArchive;

use crate::config::artifacts::{ArtifactConfig, SourceType};
use crate::output::{
    FormatOutput, JobHistoryOutput, OutputOptions, ScanProjectsOutput, ScannedProject,
};
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
    pub artifacts: Vec<JobArtifact>,
    pub web_url: String,
}

impl JobHistory {
    fn get_main_artifact(&self) -> Option<&JobArtifact> {
        self.artifacts
            .iter()
            .find(|a| a.file_type == MAIN_ARTIFACT_TYPE)
    }
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

const MAIN_ARTIFACT_TYPE: &str = "archive";
#[derive(Deserialize)]
pub struct JobArtifact {
    file_type: String,
    //size: usize,
    //filename: String,
    //file_format: String,
}

const TOKEN_HEADER: HeaderName = HeaderName::from_static("private-token");

fn get_token_value(token: &str) -> Result<HeaderValue, ErdError> {
    token
        .parse()
        .map_err(|_| ErdError::InvalidToken(token.to_string()))
}

pub fn scan_gitlab(query: Option<String>, token: Option<&str>) -> Result<(), ErdError> {
    let client = reqwest::blocking::Client::new();
    let token_value: Option<HeaderValue> = token.map(get_token_value).transpose()?;
    // https://docs.gitlab.com/ee/api/projects.html#list-all-projects
    // TODO: filter by owned, group, etc.
    let url = "https://gitlab.com/api/v4/projects";
    let mut request = client
        .get(url)
        .query(&[
            ("membership", "true"),
            ("order_by", "last_activity_at"),
            ("per_page", "30"),
            ("search", query.as_deref().unwrap_or("")),
            ("search_namespaces", "true"),
        ]);
    if let Some(h_value) = token_value {
        request = request.header(TOKEN_HEADER, h_value);
    }
    else {
        warn!("Scanning without login - you might not get any results.");
    }
    let response = request.send()
        .map_err(|e| request_failed(e, "Failed to get project list"))?;
    let response = response.error_for_status()
        .map_err(|e| request_failed(e, "Received Error while getting project list"))?;
    debug!("Got HTTP Code {}", response.status());
    let projects: Vec<ProjectData> = deserialize_response(response)?;
    let options = OutputOptions {
        color: true,
        short: false,
    };
    let projects_output: ScanProjectsOutput = projects.format_output(&options);
    info!("{}", projects_output);
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
        trace!("File name: {}", file_name);
        if file_name.ends_with(&artifact.artifact_pattern) {
            debug!("Found Artifact: {}", file_name);
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
            ("per_page", "6"),
        ])
        .header(TOKEN_HEADER, token_value)
        .send()
        .map_err(|e| request_failed(e, "Failed to get artifact from Gitlab"))?;
    debug!("URL: {}", response.url());
    let job_history: Vec<JobHistory> = deserialize_response(response)?;
    if short {
        show_history_short(artifact, job_name, job_history);
    } else {
        show_history_long(artifact, job_name, job_history);
    }
    Ok(())
}

fn show_history_long(artifact: &ArtifactConfig, job_name: &str, job_history: Vec<JobHistory>) {
    let options = OutputOptions {
        color: true,
        short: false,
    };
    info!(
        "Showing {} jobs for {} on branch {}",
        job_name, artifact.id, artifact.branch
    );
    for job in job_history {
        let job_long = job.format_output(&options);
        info!("{}", job_long);
    }
}

fn show_history_short(artifact: &ArtifactConfig, job_name: &str, job_history: Vec<JobHistory>) {
    let options = OutputOptions {
        color: true,
        short: true,
    };
    info!(
        "Showing {} jobs for {} on branch {}",
        job_name, artifact.id, artifact.branch
    );
    info!("Id - When - Commit (Author) - Status");
    for entry in job_history {
        if entry.name != job_name {
            continue;
        }
        let job_short = entry.format_output(&options);
        info!("{}", job_short);
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

impl FormatOutput<ScanProjectsOutput> for Vec<ProjectData> {
    fn format_output(self, options: &OutputOptions) -> ScanProjectsOutput {
        let projects = self
            .into_iter()
            .map(|p| ScannedProject {
                path: p.path_with_namespace,
                id: p.id.to_string(),
                url: p.web_url,
            })
            .collect();

        ScanProjectsOutput {
            projects,
            options: options.clone(),
        }
    }
}

impl FormatOutput<JobHistoryOutput> for JobHistory {
    fn format_output(self, options: &OutputOptions) -> JobHistoryOutput {
        let has_artifacts = self.get_main_artifact().is_some();
        JobHistoryOutput {
            id: self.id.to_string(),
            job_ref: self.job_ref,
            timestamp: self.created_at,
            status: self.status,
            has_artifacts,
            web_url: self.web_url,
            commit_short_id: self.commit.short_id,
            commit_title: self.commit.title,
            commit_author: self.commit.author_email,
            options: options.clone(),
        }
    }
}
