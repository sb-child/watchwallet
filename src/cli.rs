use crate::config::Config;
use anyhow::Context;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(version, about = "链上钱包活动监听器")]
pub struct Cli {
    /// 配置文件路径
    #[arg(long, default_value = "watchwallet.toml")]
    config: PathBuf,
}

impl Cli {
    pub fn parse_config(&self) -> anyhow::Result<Config> {
        let cfg_file = self
            .config
            .canonicalize()
            .with_context(|| format!("找不到配置文件: {:?}", self.config))?;
        let content = std::fs::read_to_string(&cfg_file)
            .with_context(|| format!("无法读取配置文件: {:?}", cfg_file))?;
        let config: Config = toml::from_str(&content)
            .with_context(|| format!("解析配置文件失败: {:?}", cfg_file))?;
        Ok(config)
    }
}
