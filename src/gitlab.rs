use std::fs::File;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use reqwest::header::{HeaderName, HeaderValue};
use serde::Deserialize;
use zip::ZipArchive;

use crate::config::ArtifactConfig;
use crate::extract_file;

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

const TOKEN_HEADER: &str = "PRIVATE-TOKEN";

pub fn scan_gitlab(query: Option<String>, token: &str) {
    let client = reqwest::blocking::Client::new();
    let token_header: HeaderName = TOKEN_HEADER.parse().expect("Should be a valid header");
    let token_value: HeaderValue = token.parse().expect("Token should have been valid");
    // https://docs.gitlab.com/ee/api/projects.html#list-all-projects
    // TODO: filter by owned, group, etc.
    let response = client
        .get("https://gitlab.com/api/v4/projects")
        .query(&[
            ("membership", "true"),
            ("order_by", "last_activity_at"),
            ("per_page", "30"),
            ("search", query.as_deref().unwrap_or("")),
            ("search_namespaces", "true"),
        ])
        .header(token_header, token_value)
        .send()
        .expect("Failed to get artifact from gitlab");
    let response_data = response.text().expect("Non-text response from Gitlab");

    let projects: Vec<ProjectData> = match serde_json::from_str(&response_data) {
        Ok(ps) => ps,
        Err(error) => {
            eprintln!("Received response from gitlab:");
            eprintln!("{}", response_data);
            panic!("Failed to deserialize response: {}", error);
        }
    };
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
}

pub fn get_latest_artifact_gitlab(artifact: &ArtifactConfig, token: &str) {
    let output_dir = Path::new("temp");

    let url = format!(
        "https://gitlab.com/api/v4/projects/{}/jobs/artifacts/{}/download?job=build",
        artifact.project_id, artifact.branch
    );
    let client = reqwest::blocking::Client::new();
    let token_header: HeaderName = TOKEN_HEADER.parse().expect("Should be a valid header");
    let token_value: HeaderValue = token.parse().expect("Token should have been valid");
    let mut response = client
        .get(url)
        .header(token_header, token_value)
        .send()
        .expect("Failed to get artifact from gitlab");
    let mut buffer = vec![];
    let bytes_read = response
        .read_to_end(&mut buffer)
        .expect("Failed to read data");
    println!("{} bytes read", bytes_read);
    let mut file =
        File::create(output_dir.join("artifacts.zip")).expect("Failed to create artifacts.zip");
    file.write_all(&buffer).expect("Failed to write buffer");

    let mut found_jar = Option::None;
    let mut zip_archive = ZipArchive::new(Cursor::new(buffer)).expect("Invalid zip archive!");
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

        let mut jar_file =
            File::create(output_dir.join(file_name)).expect("Failed to create Artifact file");
        jar_file
            .write_all(&file_data)
            .expect("Failed to write Artifact");
    }
}

pub fn get_history_gitlab(artifact: &ArtifactConfig, token: &str) {
    let client = reqwest::blocking::Client::new();
    let token_header: HeaderName = TOKEN_HEADER.parse().expect("Should be a valid header");
    let token_value: HeaderValue = token.parse().expect("Token should have been valid");
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
        ])
        .header(token_header, token_value)
        .send()
        .expect("Failed to get artifact from gitlab");
    let response_data = response.text().expect("Did not get text from gitlab");
    let job_history: Vec<JobHistory> = match serde_json::from_str(&response_data) {
        Ok(history) => history,
        Err(e) => {
            eprintln!("{}", response_data);
            panic!("Failed to deserialize response from gitlab: {}", e);
        }
    };
    show_history_long(artifact, job_name, job_history);
}

fn show_history_long(artifact: &ArtifactConfig, job_name: &str, job_history: Vec<JobHistory>) {
    println!(
        "Showing {} jobs for {} on branch {}",
        job_name, artifact.id, artifact.branch
    );
    for job in job_history {
        println!("{} - {}", job.commit.short_id, job.commit.title);
        println!("\tId: {}", job.id);
        println!("\tTimestamp: {}", job.created_at);
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
