use std::path::PathBuf;
use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Entry {
  pub directory: PathBuf,
  pub file: PathBuf,
  pub arguments: Vec<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub output: Option<PathBuf>,
}
