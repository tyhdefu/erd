use std::fs::create_dir;

use crate::{config, ErdError};
use crate::config::artifacts::{Config, SourceConfig, SourceType, ARTIFACTS_FILE};
use crate::input::read_with_prompt;
use log::error;
use toml;

pub fn init_erd(interactive: bool) -> Result<(), ErdError> {
    let erd_dir = config::get_local_dir();
    if erd_dir.exists() {
        error!("erd already initialised in this directory!");
        return Ok(());
    }
    create_dir(&erd_dir)
        .map_err(|e| ErdError::IOError(e, format!("Failed to create {:?} directory", erd_dir)))?;

    let artifact_file = erd_dir.join(ARTIFACTS_FILE);
    if !interactive {
        todo!();
    }
    //println!("2) Github")
    let source_type: SourceType = loop {
        println!("To get setup, lets add the first Repository Source (GitLab/GitHub)");
        println!(" - GitLab");
        let source_type_str = read_with_prompt("> ")?;
        let source_type = source_type_str.to_lowercase().parse();
        match source_type {
            Ok(x) => {
                break x;
            },
            Err(()) => {
                println!("Invalid type, please try again"); 
                continue
            },
        }
    };
    let url = match source_type {
        SourceType::Gitlab => {
            println!("Custom GitLab URL? Leave blank for gitlab.com");
            let mut url = read_with_prompt("> ")?;
            // TODO: URL validation
            if url.is_empty() {
                url = "https://gitlab.com/".to_string();
            }
            url
        }
    };
    let id = format!("{:?}", source_type).to_lowercase();
    let source_config = SourceConfig {
        id: id.clone(),
        url,
        kind: source_type,
        artifacts: vec![],
    };
    let config = Config {
        sources: vec![source_config],
    };
    let config_str = toml::to_string(&config).expect("Failed to serialize init config!");
    std::fs::write(&artifact_file, config_str)
        .map_err(|e| ErdError::IOError(e, format!("Failed to write {:?}", artifact_file)))?;

    println!("First source added. Try adding some repositories with erd scan {id}");
    Ok(())
}