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

pub fn read_logins_file(file: &Path) -> Result<Logins, ErdError> {
    if !file.exists() {
        debug!("No logins file found - continuing without authentication");
        return Ok(Logins::default());
    }
    let s = std::fs::read_to_string(file)
        .map_err(|e| ErdError::IOError(e, format!("Failed to read {:?}", file)))?;
    let logins: Logins = toml::from_str(&s)
        .map_err(|e| ErdError::Deserialize(e, format!("{:?}", file)))?;
    Ok(logins)
}

pub fn save_logins_file(file: &Path, logins: &Logins) -> Result<(), ErdError> {
    let data = toml::to_string(logins)
        .map_err(|e| ErdError::Serialize(e, format!("{:?}", file)))?;
    std::fs::write(file, data)
        .map_err(|e| ErdError::IOError(e, format!("Failed to save {:?}", file)))
}

#[derive(Debug)]
#[derive(Serialize, Deserialize, Default)]
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

    /// Add the given login.
    /// If a login with the given URL already exists, it is replaced.
    pub fn set_login(&mut self, login: Login) -> Option<Login> {
        for l in self.logins.iter_mut() {
            if l.url == login.url {
                return Some(std::mem::replace(l, login));
            }
        }
        self.logins.push(login);
        return None;
    }
}

#[derive(Debug)]
#[derive(Serialize, Deserialize)]
pub struct Login {
    pub url: String,
    pub username: String,
    pub password: String,
}
