use std::sync::LazyLock;

use anyhow;
use serde::Deserialize;
use tracing::error;

pub static APP_CONFIG_INSTANCE: LazyLock<AppConfig> =
    LazyLock::new(|| match AppConfig::load_from_file() {
        Ok(config) => config,
        Err(e) => {
            error!("加载配置文件失败:{}", e);
            panic!("load app config fail");
        }
    });

#[derive(Debug, Deserialize)]
pub struct AppConfig {
    pub manager_path: String,
    pub adb_path: String,
    //创建mumu模拟器的数量
    pub vm_client_num: usize,
    //模拟器启动后要运行的app包名
    pub app_package_names: Vec<String>,
    //模拟器窗口大小
    pub vm_client_window_size: (usize, usize),
    //第一个模拟器窗口起始位置
    pub first_vm_client_pos: (usize, usize),
}

impl AppConfig {
    /// 如果你就是想强行重新加载（极少用），保留你原来的方法
    pub fn load_from_file() -> anyhow::Result<Self> {
        let conf = config::Config::builder()
            .add_source(config::File::with_name("config/app_config.toml").required(true))
            .add_source(config::Environment::with_prefix("APP").separator("__"))
            .build()?;
        let app_config: AppConfig = conf.try_deserialize()?;
        Ok(app_config)
    }
}

#[cfg(test)]
mod test {
    use crate::config_util::{APP_CONFIG_INSTANCE, AppConfig};

    #[test]
    fn test_get_app_config() {
        let config = AppConfig::load_from_file().unwrap();
        println!("{:?}", config);
        println!("static config:{:?}", APP_CONFIG_INSTANCE.app_package_names);
    }
}
