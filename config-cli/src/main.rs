use clap::Parser;
use env_file_reader::read_file;
use glob::glob;
use log::{debug, error, info, trace, LevelFilter};
use simple_logger::SimpleLogger;
use std::collections::HashMap;

use anyhow::Result;
use std::fs::{read_to_string, File};
use std::io::{Cursor, Write};
use std::path::PathBuf;
use thiserror::Error;
use toml::Value;

const END: &str = "\n# GENERATED BY ENV_TO_CONFIG_TOML END\n";
const START: &str = "# GENERATED BY ENV_TO_CONFIG_TOML START\n";

fn main() {
    let args = Args::parse();
    args.init_log();
    match args.get_merge_bytes() {
        Ok(bytes) => {
            let mut file = File::create(args.get_out_path()).expect("Failed to create file");
            file.write_all(&bytes).expect("Failed to write to file");
            info!("Merge env files success");
        }
        Err(e) => error!("Merge env files failed: {}", e),
    }
}

#[derive(Error, Debug)]
pub enum MergeError {
    #[error("Duplicate key: {0} in {1} and {2}")]
    DuplicateKey(String, String, String),
    #[error("No file found for the pattern: {0}")]
    NoFileFound(String),
}

/// Merge multiple .env files into one
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// The directory containing .env files
    #[arg(short, long)]
    pattern: String,

    /// The output file to write the merged .env file to
    #[arg(short, long)]
    out_path: PathBuf,

    /// Optional log level (None = info, v = debug, vvvv = trace)
    #[arg(short, long)]
    log_level: Option<String>,
}

impl Args {
    pub fn init_log(&self) {
        let log_level = match &self.log_level {
            None => LevelFilter::Info,
            Some(level) if level == "v" => LevelFilter::Debug,
            Some(level) if level == "vvvv" => LevelFilter::Trace,
            _ => LevelFilter::Debug,
        };
        SimpleLogger::new().with_level(log_level).init().unwrap();
    }

    pub fn get_out_path(&self) -> &PathBuf {
        &self.out_path
    }

    pub fn get_merge_bytes(&self) -> Result<Vec<u8>> {
        let env_vars = self.get_env_vars()?;

        return match self.out_path.exists() {
            true => {
                debug!("Merging into existing file: {:?}", self.out_path);

                let file_content = read_to_string(self.out_path.clone())?;
                let result = self.merge_existing_toml(&env_vars, &file_content)?;
                Ok(result)
            }
            false => {
                debug!("Creating new file in: {:?}", self.out_path);
                let parent = self
                    .out_path
                    .parent()
                    .expect("Failed to get parent directory");
                std::fs::create_dir_all(parent)?;
                let result = self.merge_existing_toml(&env_vars, "")?;
                Ok(result)
            }
        };
    }

    fn get_env_vars(&self) -> Result<Vec<(String, String)>> {
        let mut env_paths: Vec<PathBuf> = glob(&self.pattern)
            .expect("Failed to read glob pattern")
            .filter_map(Result::ok)
            .filter(|path| path.is_file())
            .collect();
        if env_paths.is_empty() {
            return Err(MergeError::NoFileFound(self.pattern.clone()).into());
        }

        env_paths.sort_by_key(|path| path.to_str().unwrap().to_lowercase());
        let mut env_vars = HashMap::new();
        let mut env_paths_by_key = HashMap::new();
        for env_path in env_paths {
            info!("Reading env file: {:?}", env_path);
            let env = read_file(env_path.clone())?;
            for (key, value) in env {
                let value = value
                    .trim()
                    .lines()
                    .filter(|s| !s.starts_with("#"))
                    .collect::<Vec<_>>()
                    .join("||||");
                if env_vars.contains_key(&key) {
                    let duplicate_path: &PathBuf = env_paths_by_key.get(&key).unwrap();
                    return Err(
                        MergeError::DuplicateKey(key, env_path.display().to_string(), duplicate_path.display().to_string()).into(),
                    );
                }
                env_vars.insert(key.clone(), value);
                env_paths_by_key.insert(key, env_path.clone());
            }
        }
        let mut env_vars: Vec<_> = env_vars
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        env_vars.sort_by_key(|(key, _)| key.to_lowercase());
        Ok(env_vars)
    }

    fn merge_existing_toml(
        &self,
        env_vars: &[(String, String)],
        file_content: &str,
    ) -> Result<Vec<u8>> {
        let mut config: toml::Value = toml::from_str(file_content)?;
        let table = config.as_table_mut().unwrap();

        let env_table = table
            .entry("env".to_owned())
            .or_insert_with(|| {
                debug!("Creating new [env] section");
                toml::Value::Table(toml::value::Table::new())
            })
            .as_table_mut()
            .unwrap();

        for (key, value) in env_vars {
            if env_table.contains_key(key) {
                debug!("Updating env var: {}={}", key, value);
                trace!("Old value: {:?}", env_table.get(key));
            } else {
                debug!("Adding env var: {}={}", key, value);
            }
            env_table.insert(key.to_owned(), Value::String(value.to_owned()));
        }
        let env_table_len = env_table.len();
        let content = self.add_prefix(&config, env_table_len);

        let mut writer = Cursor::new(Vec::new());
        writer.write_all(content.as_bytes())?;
        Ok(writer.into_inner())
    }

    fn add_prefix(&self, value: &Value, len: usize) -> String {
        let env_section_index = {
            let config_table = value.as_table().unwrap();
            let mut index = 0;
            for (key, _) in config_table.iter() {
                if key == "env" {
                    break;
                }
                index += 1;
            }
            index
        };
        let toml_str = toml::to_string_pretty(&value).expect("Failed to serialize TOML value");
        let mut lines: Vec<&str> = toml_str.lines().collect();
        lines.insert(env_section_index, START);
        lines.insert(env_section_index + len + 2, END);
        let toml_str = lines.join("\n");
        toml_str
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    #[test]
    fn test_merge_env_files_new() {
        let out = Path::new("src/test_data/new_config.toml");
        let pattern = "src/test_data/[0-9].env";
        let _ = std::fs::remove_file(out);
        let args = Args {
            pattern: pattern.to_owned(),
            out_path: out.to_owned(),
            log_level: None,
        };

        let bytes = args.get_merge_bytes().unwrap();
        let config_content = String::from_utf8(bytes).unwrap();
        let verify_content = std::fs::read_to_string("src/test_data/new_verify.toml").unwrap();
        assert_eq!(config_content, verify_content);
    }

    #[test]
    fn test_merge_env_files_new_folder() {
        let out = Path::new("src/test_data/new_folder/new_folder_config.toml");
        let new_folder = Path::new("src/test_data/new_folder");
        let _ = std::fs::remove_file(out);
        let _ = std::fs::remove_dir(new_folder);
        assert!(!new_folder.exists());
        let pattern = "src/test_data/[0-9].env";
        let args = Args {
            pattern: pattern.to_owned(),
            out_path: out.to_owned(),
            log_level: None,
        };

        let bytes = args.get_merge_bytes().unwrap();
        let config_content = String::from_utf8(bytes).unwrap();
        let verify_content = std::fs::read_to_string("src/test_data/new_verify.toml").unwrap();
        assert_eq!(config_content, verify_content);
        assert!(new_folder.exists());
    }

    #[test]
    fn test_merge_env_files_exist() {
        let pattern = "src/test_data/[0-9].env";
        let out = Path::new("src/test_data/exist_config.toml");
        let _ = std::fs::copy("src/test_data/old.toml", out).unwrap();
        let args = Args {
            pattern: pattern.to_owned(),
            out_path: out.to_owned(),
            log_level: None,
        };

        let bytes = args.get_merge_bytes().unwrap();
        let config_content = String::from_utf8(bytes).unwrap();
        let verify_content = std::fs::read_to_string("src/test_data/old_verify.toml").unwrap();
        assert_eq!(config_content, verify_content);
    }

    #[test]
    fn test_merge_env_files_overwrite() {
        let out = Path::new("src/test_data/overwrite_config.toml");
        let _ = std::fs::copy("src/test_data/overwrite.toml", out).unwrap();
        let pattern = "src/test_data/[0-9].env";
        let args = Args {
            pattern: pattern.to_owned(),
            out_path: out.to_owned(),
            log_level: None,
        };
        let bytes = args.get_merge_bytes().unwrap();
        let config_content = String::from_utf8(bytes).unwrap();
        let verify_content =
            std::fs::read_to_string("src/test_data/overwrite_verify.toml").unwrap();
        assert_eq!(config_content, verify_content);
    }

    #[test]
    fn test_merge_env_files_duplicate() {
        let out = Path::new("src/test_data/duplicate_config.toml");
        let _ = std::fs::copy("src/test_data/overwrite.toml", out).unwrap();
        let pattern = "src/test_data/*.env";
        let args = Args {
            pattern: pattern.to_owned(),
            out_path: out.to_owned(),
            log_level: None,
        };
        let env_paths: Vec<PathBuf> = glob("src/test_data/duplicate.env")
            .expect("Failed to read glob pattern")
            .filter_map(Result::ok)
            .filter(|path| path.is_file())
            .collect::<Vec<_>>();
        let env_path = env_paths[0].clone();
        let env_paths: Vec<PathBuf> = glob("src/test_data/1.env")
            .expect("Failed to read glob pattern")
            .filter_map(Result::ok)
            .filter(|path| path.is_file())
            .collect::<Vec<_>>();
        let duplicate_path = env_paths[0].clone();

        let result = args.get_merge_bytes().err().unwrap();
        assert_eq!(
            result.to_string(),
            MergeError::DuplicateKey("A".to_owned(), env_path.display().to_string(), duplicate_path.display().to_string())
                .to_string()
        );
    }

    #[test]
    fn test_merge_env_files_invalid_pattern() {
        let out = Path::new("src/test_data/duplicate_config.toml");
        let _ = std::fs::copy("src/test_data/overwrite.toml", out).unwrap();
        let pattern = "src/test_data/";
        let args = Args {
            pattern: pattern.to_owned(),
            out_path: out.to_owned(),
            log_level: None,
        };
        let result = args.get_merge_bytes().err().unwrap();
        assert_eq!(
            result.to_string(),
            MergeError::NoFileFound(pattern.to_owned()).to_string()
        );
    }
}
