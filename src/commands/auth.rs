use log::info;

use crate::ErdError;
use crate::input::read_with_prompt;
use crate::logins::{Login, Logins};

pub fn auth(url: String, mut logins: Logins) -> Result<Logins, ErdError> {
    let login = prompt_auth(url)?;
    logins.set_login(login);
    Ok(logins)
}

fn prompt_auth(url: String) -> Result<Login, ErdError> {
    info!("Authenticate for {url}");
    let username = read_with_prompt("username")?;
    let password = read_with_prompt("password")?;
    return Ok(Login { url, username, password })
}