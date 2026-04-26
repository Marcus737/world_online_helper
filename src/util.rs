use std::{
    collections::HashMap,
    ffi::OsStr,
    fmt::Debug,
    fs,
    process::{Command, Output},
};

use anyhow::{Ok, Result, anyhow};
use image::DynamicImage;
use serde::{Deserialize, Serialize};
use template_matching::{Image, MatchTemplateMethod, TemplateMatcher, find_extremes};
use tracing::{debug, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

const FILTER_NAMES: [&str; 4] = ["wgpu_core", "wgpu_hal", "naga", "droidrun_adb"];

pub fn init_logger() {
    let filter = tracing_subscriber::filter::filter_fn(|m| {
        for name in FILTER_NAMES {
            if m.target().contains(name) {
                return false;
            }
        }
        return true;
    });

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .try_init()
        .unwrap();
}

#[derive(Debug, PartialEq)]
pub struct OcrPoint {
    pub x: u32,
    pub y: u32,
    pub center_x: u32,
    pub center_y: u32,
    //越小越相似
    pub value: f32,
}

pub struct ImageHelper {
    matcher: TemplateMatcher,
    loaded_imgs: HashMap<String, DynamicImage>,
}

impl Debug for ImageHelper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageHelper").finish()
    }
}

fn load_img() -> Result<HashMap<String, DynamicImage>> {
    let mut map = HashMap::new();
    for item in fs::read_dir("res")? {
        let dir_entry = item?;
        if dir_entry.path().is_file() {
            let filename = dir_entry.file_name();
            let filename = filename
                .to_str()
                .ok_or_else(|| anyhow!("cannot convert ostring to &str"))?;
            let filename = &filename[0..filename.len() - 4];

            let path = dir_entry.path();
            let path = path
                .to_str()
                .ok_or_else(|| anyhow!("cannot convert ostring to &str"))?;

            let dyn_img = image::open(path)?;
            map.insert(filename.to_string(), dyn_img);
            info!("loaded template filename:{}\npath:{}", filename, path);
        }
    }
    Ok(map)
}

impl ImageHelper {
    pub fn new() -> Result<Self> {
        Ok(Self {
            matcher: TemplateMatcher::new(),
            loaded_imgs: load_img()?,
        })
    }

    pub fn get_template_img_pos_by_name(
        &mut self,
        input: &DynamicImage,
        template_name: &str,
    ) -> Result<Option<OcrPoint>> {
        let template_img = self
            .loaded_imgs
            .get(template_name)
            .ok_or(anyhow!("template_name {} not found", template_name))?;

        self.matcher.match_template(
            Image::from(&input.to_luma32f()),
            Image::from(&template_img.to_luma32f()),
            MatchTemplateMethod::SumOfSquaredDifferences,
        );
        let result = self
            .matcher
            .wait_for_result()
            .ok_or(anyhow!("template image not found on input image!"))?;
        let extremes = find_extremes(&result);
        debug!("extremes:{:?}", extremes);

        //min_value<300
        if extremes.min_value > 300.0 {
            return Ok(None);
        }

        //一定大于100且小于图片得高度减去100
        if extremes.min_value_location.1 < 100
            || extremes.min_value_location.1 > input.height() - 100
        {
            return Ok(None);
        }

        let point = OcrPoint {
            x: extremes.min_value_location.0,
            y: extremes.min_value_location.1,
            center_x: extremes.min_value_location.0 + template_img.width() / 2,
            center_y: extremes.min_value_location.1 + template_img.height() / 2,
            value: extremes.min_value,
        };
        Ok(Some(point))
    }

    pub fn get_template_img_pos_from_file(
        &mut self,
        input_path: &str,
        template_path: &str,
        method: MatchTemplateMethod,
    ) -> Result<Option<OcrPoint>> {
        let input = image::open(input_path)?;
        let template = image::open(template_path)?;
        self.get_template_img_pos(&input, &template, method)
    }

    pub fn get_template_img_pos(
        &mut self,
        input: &DynamicImage,
        template: &DynamicImage,
        method: MatchTemplateMethod,
    ) -> Result<Option<OcrPoint>> {
        self.matcher.match_template(
            Image::from(&input.to_luma32f()),
            Image::from(&template.to_luma32f()),
            method,
        );
        let result = self
            .matcher
            .wait_for_result()
            .ok_or(anyhow!("template image not found on input image!"))?;
        let extremes = find_extremes(&result);
        //min_value<300
        if extremes.min_value > 300.0 {
            return Ok(None);
        }

        //一定大于100且小于图片得高度减去100
        if extremes.min_value_location.1 < 100
            || extremes.min_value_location.1 > input.height() - 100
        {
            return Ok(None);
        }

        let point = OcrPoint {
            x: extremes.min_value_location.0,
            y: extremes.min_value_location.1,
            center_x: extremes.min_value_location.0 + template.width() / 2,
            center_y: extremes.min_value_location.1 + template.height() / 2,
            value: extremes.min_value,
        };
        Ok(Some(point))
    }
}

pub fn run_command<I, S>(program_path: &str, args: I) -> Result<CommandOutput>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new(program_path).args(args).output()?;
    Ok(CommandOutput::new(output)?)
}

pub fn run_command_with_work_dir<I, S>(
    program_path: &str,
    work_dir: &str,
    args: I,
) -> Result<CommandOutput>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new(program_path)
        .current_dir(work_dir)
        .args(args)
        .output()?;
    Ok(CommandOutput::new(output)?)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorData {
    pub errcode: i32,
    pub errmsg: String,
}

#[derive(Debug)]
pub struct CommandOutput {
    pub success: bool,
    //错误流被定向到输出流了
    pub std_out_str: String,
    pub std_err_str: String,
}

impl CommandOutput {
    pub fn new(output: Output) -> Result<Self> {
        // debug!("output:{:?}", output);
        let success = output.status.success();
        let std_out_str = String::from_utf8(output.stdout)?;
        let std_err_str = String::from_utf8(output.stderr)?;
        if !success {
            if std_err_str.trim().is_empty() {
                let err: ErrorData = serde_json::from_str(&std_out_str)?;
                return Err(anyhow!("{:?}", err));
            }
            return Err(anyhow!(std_err_str));
        }
        let command_output = CommandOutput {
            success,
            std_out_str,
            std_err_str,
        };
        Ok(command_output)
    }
}

#[derive(Debug, Deserialize)]
pub struct Point {
    pub x: i32,
    pub y: i32
}

impl Point {
    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Deserialize)]
pub struct Size {
    pub width: usize,
    pub height: usize
}

impl Size {
    pub fn new(width: usize, height: usize) -> Self{
        Self { width, height }
    }
}


#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_get_template_img_pos() {
        init_logger();

        let mut ihelper = ImageHelper::new().unwrap();
        let input = image::open("0.png").unwrap();
        let res = ihelper
            .get_template_img_pos_by_name(&input, "任务按钮_接受任务2")
            .unwrap();
        println!("{:?}", res);

        let input = image::open("1.png").unwrap();
        let res = ihelper
            .get_template_img_pos_by_name(&input, "任务按钮_确认2")
            .unwrap();
        println!("{:?}", res);
    }
}
