use homedir::my_home;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::Path;

pub fn get_config() -> AppConfig {
    let config = match load_or_initialize() {
        Ok(v) => v,
        Err(err) => {
            match err {
                ConfigError::IoError(err) => {
                    eprintln!("An error occurred while loading the config: {err}");
                }
                ConfigError::InvalidConfig(err) => {
                    eprintln!("An error occurred while parsing the config:");
                    eprintln!("{err}");
                }
            }
            AppConfig {
                host: "".to_string(),
                user: "".to_string(),
                password: "".to_string(),
                database: "".to_string(),
            }
        }
    };
    return config;
    //    return "xxxx".to_string();
}

enum ConfigError {
    IoError(io::Error),
    InvalidConfig(toml::de::Error),
}

impl From<io::Error> for ConfigError {
    fn from(value: io::Error) -> Self {
        Self::IoError(value)
    }
}

impl From<toml::de::Error> for ConfigError {
    fn from(value: toml::de::Error) -> Self {
        Self::InvalidConfig(value)
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AppConfig {
    pub host: String,
    pub user: String,
    pub database: String,
    pub password: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            host: "".to_string(),
            user: "".to_string(),
            password: "".to_string(),
            database: "".to_string(),
        }
    }
}

fn load_or_initialize() -> Result<AppConfig, ConfigError> {
    //  https://dev.to/zofia/why-do-we-need-configuration-creating-and-handling-configuration-files-in-rust-4a46?ysclid=m00bsa1iuz12379992
    let home = my_home().unwrap().unwrap();
    let _config_path = &format!("{0}/config.toml", home.display());
    let config_path = Path::new(_config_path);
    //println!("{:?}", config_path);
    if config_path.exists() {
        let content = fs::read_to_string(config_path)?;
        let config = toml::from_str(&content)?;

        return Ok(config);
    }

    // The config file does not exist, so we must initialize it with the default values.

    let mut config = AppConfig::default();
    let toml = toml::to_string(&config).unwrap();

    fs::write(config_path, toml)?;
    Ok(config)
}
