use anyhow::{Ok, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    thread::sleep,
    time::{Duration, SystemTime},
};
use tracing::debug;

use crate::util::run_command;

pub fn show_all_vm_window(manager_exe_path: &str) -> Result<()> {
    run_command(
        manager_exe_path,
        vec!["control", "-v", "all", "show_window"],
    )?;
    Ok(())
}

pub fn hide_all_vm_window(manager_exe_path: &str) -> Result<()> {
    run_command(
        manager_exe_path,
        vec!["control", "-v", "all", "hide_window"],
    )?;
    Ok(())
}

pub fn lanch_all_vm(manager_exe_path: &str) -> Result<()> {
    run_command(manager_exe_path, vec!["control", "-v", "all", "launch"])?;
    Ok(())
}

pub fn close_all_vm(manager_exe_path: &str) -> Result<()> {
    let output = run_command(manager_exe_path, vec!["control", "-v", "all", "shutdown"])?;
    if !output.success {
        return Err(anyhow!(output.std_out_str));
    }
    Ok(())
}

pub fn get_all_vm_info(manager_exe_path: &str) -> Result<Vec<VmInfo>> {
    let output = run_command(manager_exe_path, vec!["info", "-v", "all"])?;
    let map: BTreeMap<String, VmInfo> = serde_json::from_str(output.std_out_str.as_str())?;
    let vm_info_vec: Vec<VmInfo> = map.into_values().collect();
    Ok(vm_info_vec)
}

#[derive(Debug)]
pub struct VmClient {
    vm_index: String,
    manager_exe_path: String,
}

impl VmClient {
    pub fn new(vm_index: usize, manager_exe_path: &str) -> Self {
        Self {
            vm_index: vm_index.to_string(),
            manager_exe_path: manager_exe_path.to_string(),
        }
    }

    pub fn get_vm_index(&self) -> String {
        self.vm_index.to_string()
    }

    pub fn set_layout_window(
        &self,
        x: Option<u32>,
        y: Option<u32>,
        height: Option<u32>,
        width: Option<u32>,
    ) -> Result<()> {
        // 构建动态参数列表，使用 Vec<String> 方便动态添加
        let mut args = vec![
            "control".to_string(),
            "-v".to_string(),
            self.vm_index.to_string(),
            "layout_window".to_string(),
        ];

        // 只有 Some 时才添加参数名和参数值（两个独立元素）
        if let Some(x_val) = x {
            args.push("-px".to_string());
            args.push(x_val.to_string());
        }
        if let Some(y_val) = y {
            args.push("-py".to_string());
            args.push(y_val.to_string());
        }
        //好像反了
        if let Some(width_val) = width {
            args.push("-sh".to_string());
            args.push(width_val.to_string());
        }
        if let Some(height_val) = height {
            args.push("-sw".to_string());
            args.push(height_val.to_string());
        }

        debug!("args:{:?}", args);

        run_command(&self.manager_exe_path, &args)?;

        Ok(())
    }

    pub fn get_vm_info(&self) -> Result<VmInfo> {
        let output = run_command(&self.manager_exe_path, vec!["info", "-v", &self.vm_index])?;
        let vm_info: VmInfo = serde_json::from_str(&output.std_out_str)?;
        Ok(vm_info)
    }

    pub fn close_vm(&self) -> Result<()> {
        run_command(
            &self.manager_exe_path,
            vec!["control", "-v", &self.vm_index, "shutdown"],
        )?;
        Ok(())
    }

    pub fn lanch_vm(&self, app_package_name: Option<&str>) -> Result<()> {
        if let Some(pkg_name) = app_package_name {
            run_command(
                &self.manager_exe_path,
                vec!["control", "-v", &self.vm_index, "launch", "-pkg", pkg_name],
            )?;
        } else {
            run_command(
                &self.manager_exe_path,
                vec!["control", "-v", &self.vm_index, "launch"],
            )?;
        }
        Ok(())
    }

    pub async fn wait_for_launch_finsh_async(&self, timeout: Duration) -> Result<()> {
        let mut start = SystemTime::now();
        loop {
            let info = self.get_vm_info()?;
            if info.is_process_started {
                break;
            }
            tokio::time::sleep(Duration::from_millis(1000)).await;
            let now = SystemTime::now();
            let diff = now.duration_since(start)?;
            start = now;
            if diff.gt(&timeout) {
                return Err(anyhow!("Timeout"));
            }
        }
        Ok(())
    }

    pub fn wait_for_launch_finsh(&self, timeout: Duration) -> Result<()> {
        let mut start = SystemTime::now();
        loop {
            let info = self.get_vm_info()?;
            if info.is_process_started {
                break;
            }
            sleep(Duration::from_millis(1000));
            let now = SystemTime::now();
            let diff = now.duration_since(start)?;
            start = now;
            if diff.gt(&timeout) {
                return Err(anyhow!("Timeout"));
            }
        }
        Ok(())
    }

    pub fn scrrenshot() {}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmInfo {
    /// adb 域名，只有启动才会有
    pub adb_host_ip: Option<String>,
    /// adb端口，只有启动才会有
    pub adb_port: Option<u16>,
    /// 模拟器创建时间戳
    pub created_timestamp: i64,
    /// 模拟器磁盘占用大小，以字节为单位
    pub disk_size_bytes: u64,
    /// 模拟器列表错误码
    pub error_code: i32,
    /// 虚拟机进程PID，只有启动才会有
    pub headless_pid: Option<u32>,
    /// HyperV是否开启
    pub hyperv_enabled: bool,
    /// 模拟器索引
    pub index: String,
    /// 是否安卓启动成功
    pub is_android_started: bool,
    /// 是否是主模拟器
    pub is_main: bool,
    /// 是否进程启动
    pub is_process_started: bool,
    /// 启动错误码，只有启动才会有
    pub launch_err_code: Option<i32>,
    /// 启动错误描述，只有启动才会有
    pub launch_err_msg: Option<String>,
    /// 模拟器运行时间，只有启动才会有
    pub launch_time: Option<u64>,
    /// 主窗口句柄，只有启动才会有
    pub main_wnd: Option<String>,
    /// 模拟器名称
    pub name: String,
    /// 模拟器外壳进程PID，只有启动才会有
    pub pid: Option<u32>,
    /// 模拟器外壳启动阶段状态，只有启动才会有
    pub player_state: Option<String>,
    /// 渲染窗口句柄，只有启动才会有
    pub render_wnd: Option<String>,
    /// 是否开启VT虚拟化，只有启动才会有
    pub vt_enabled: Option<bool>,
}

#[cfg(test)]
mod test {
    use crate::util;

    use super::*;

    const MANAGER_PATH: &str = r"c:\Users\10401\software\MuMuPlayer\nx_main\MuMuManager.exe";

    #[test]
    fn test_get_all_vm_info() {
        util::init_logger();
        let infos = get_all_vm_info(MANAGER_PATH).unwrap();
        println!("{:?}", infos);
    }
}
