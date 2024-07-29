use crate::{auth::Login, gitlab::get_history_gitlab, ErdError};

use crate::config::artifacts::{ArtifactConfig, SourceType};


pub fn get_history(
    artifact: &ArtifactConfig,
    kind: &SourceType,
    login: &Login,
    short: bool,
) -> Result<(), ErdError> {
    match kind {
        SourceType::Gitlab => get_history_gitlab(artifact, &login.password, short),
    }
}