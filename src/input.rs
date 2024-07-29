use std::io::{self, Write};

use crate::ErdError;


pub fn read_with_prompt(prompt: &str) -> Result<String, ErdError> {
    print!("{}: ", prompt);
    io::stdout()
        .flush()
        .map_err(|e| ErdError::IOError(e, "Failed to flush stdout".into()))?;
    let mut buffer = String::new();
    io::stdin()
        .read_line(&mut buffer)
        .map_err(|e| ErdError::IOError(e, format!("Failed to read answer to {}", prompt)))?;
    let buffer = buffer.trim().into();
    Ok(buffer)
}