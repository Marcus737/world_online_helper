use std::{collections::HashMap, sync::LazyLock};

use anyhow;
use config::{Config, Environment, File};
use serde::Deserialize;
use tracing::error;

use crate::util::{Point, Size};

pub static APP_CONFIG_INSTANCE: LazyLock<AppConfig> =
    LazyLock::new(|| match AppConfig::load_from_file() {
        Ok(config) => config,
        Err(e) => {
            error!("加载配置文件失败:{}", e);
            panic!("load app config fail");
        }
    });


/// 全局单例配置：第一次访问时自动加载，加载失败则 panic
pub static GAME_HELPER_CONFIG: LazyLock<GameHelperConfig> = LazyLock::new(|| {
    GameHelperConfig::load_from_file().unwrap_or_else(|e| {
        error!("加载 GameHelperConfig 失败: {}", e);
        panic!("加载 game_helper 配置文件失败");
    })
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


#[derive(Debug, Deserialize)]
pub struct GameHelperConfig {
    pub bag_button_pos: Point,
    pub bag_first_grid_center_pos: Point,
    pub bag_grid_size: Size,
    pub back_pos: Point,
    pub bag_1_pos: Point,
    pub bag_2_pos: Point,
    pub equipment_names: Vec<String>,
    pub remove_item_names: Vec<String>,
}

impl GameHelperConfig {
    /// 从 config/game_helper.toml 加载配置，支持环境变量覆盖
    pub fn load_from_file() -> anyhow::Result<Self> {
        let conf = Config::builder()
            .add_source(File::with_name("config/game_helper.toml").required(true))
            .add_source(Environment::with_prefix("GAME_HELPER").separator("__"))
            .build()?;
        let config: GameHelperConfig = conf.try_deserialize()?;
        Ok(config)
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct OcrConfig {
    pub server_program_path: String,
    pub server_port: u16
}

pub static OCR_CONFIG_INSTANCE: LazyLock<OcrConfig> =
    LazyLock::new(|| match OcrConfig::load_from_file() {
        Ok(config) => config,
        Err(e) => {
            error!("加载 OCR 配置文件失败，请检查是否遗漏字段: {}", e);
            panic!("load ocr config fail");
        }
    });

impl OcrConfig {
    pub fn load_from_file() -> anyhow::Result<Self> {
        let conf = Config::builder()
            // 指定文件路径
            .add_source(File::with_name("config/ocr_config.toml").required(true))
            // 支持环境变量覆盖，比如设置 OCR_SERVER_PORT=8080
            .add_source(Environment::with_prefix("OCR").separator("_"))
            .build()?;
        
        let config: OcrConfig = conf.try_deserialize()?;
        Ok(config)
    }
}

#[cfg(test)]
mod test {

    use crate::config_util::{APP_CONFIG_INSTANCE, AppConfig, GAME_HELPER_CONFIG, GameHelperConfig, OCR_CONFIG_INSTANCE, OcrConfig};

    #[test]
    fn test_get_app_config() {
        let config = AppConfig::load_from_file().unwrap();
        println!("{:?}", config);
        println!("static config:{:?}", &*APP_CONFIG_INSTANCE);
    }

    #[test]
    fn test_get_game_helper_config() {
        let config = GameHelperConfig::load_from_file().unwrap();
        println!("{:?}", config);
        println!("static config:{:?}", &*GAME_HELPER_CONFIG);
    }

    #[test]
    fn test_get_ocr_config() {
        let config = OcrConfig::load_from_file().unwrap();
        println!("{:?}", config);
        println!("static config:{:?}", &*OCR_CONFIG_INSTANCE);
    }
}
