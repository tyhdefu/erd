use std::path::{Path, PathBuf};

use dirs::config_dir;
use log::debug;
use serde::{Deserialize, Serialize};

use crate::ErdError;

const AUTH_FILE: &str = "erd-logins.toml";


pub fn get_auth_file() -> Option<PathBuf> {
    let mut config_dir = config_dir()?;
    config_dir.push(AUTH_FILE);
    Some(config_dir)
}

pub fn read_auth_file(file: &Path) -> Result<Logins, ErdError> {
    let s = std::fs::read_to_string(file)
        .map_err(|e| ErdError::IOError(e, format!("Failed to read {:?}", file)))?;
    let logins: Logins = toml::from_str(&s)
        .map_err(|e| ErdError::Deserialize(e, format!("{:?}", file)))?;
    Ok(logins)
}

#[derive(Debug)]
#[derive(Serialize, Deserialize)]
pub struct Logins {
    logins: Vec<Login>,
}

impl Logins {
    pub fn find_login(&self, url: &str) -> Option<&Login> {
        let mut best_match = None;
        let mut match_length = 0;

        for login in &self.logins {
            if url.len() > match_length && url.starts_with(&login.url) {
                debug!("Better match found (length {}) '{}'", url.len(), login.url);
                best_match = Some(login);
                match_length = url.len()
            }
        }
        debug!("Best match: {:?}", best_match);
        return best_match;
    }
}

#[derive(Debug)]
#[derive(Serialize, Deserialize)]
pub struct Login {
    pub url: String,
    pub username: String,
    pub password: String,
}
