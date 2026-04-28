use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Context;
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tracing::info;

const URL: &str = "https://github.com/astral-sh/uv/releases/latest/download/uv-x86_64-pc-windows-msvc.zip";
const ZIP: &str = "uv-x86_64-pc-windows-msvc.zip";
const EXE: &str = "uv.exe";

#[derive(Error, Debug)]
enum UvError {
    #[error("Network: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("IO: {0}")]
    Io(#[from] std::io::Error),
    #[error("Zip: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("uv.exe not found in archive")]
    NotFound,
    #[error("Task failed: {0}")]
    Join(#[from] tokio::task::JoinError), 
}

// ==========================================
// 纯静态工具类 (无需实例化)
// ==========================================
pub struct UvInstaller;

impl UvInstaller {
    /// 唯一公开入口：传 None 安装到当前目录，传路径安装到指定目录
    pub async fn install(target_dir: Option<&Path>) -> anyhow::Result<()> {
        let dir = target_dir.map(|p| p.to_path_buf()).unwrap_or_else(|| std::env::current_dir().unwrap());
        let zip_path = dir.join(ZIP);
        let exe_path = dir.join(EXE);

        // if Self::is_ok("uv") {
        //     info!("[OK] System uv: {}", Self::ver("uv"));
        //     return Ok(());
        // }
        // if exe_path.exists() {
        //     if Self::is_ok(exe_path.to_str().unwrap()) {
        //         info!("[OK] Local uv: {}", Self::ver(exe_path.to_str().unwrap()));
        //         return Ok(());
        //     }
        //     tokio::fs::remove_file(&exe_path).await?;
        // }

        Self::download(&zip_path).await.context("Download failed")?;
        Self::extract(&zip_path, &exe_path).await.context("Extract failed")?;
        Self::cleanup(&dir, &zip_path).await;

        info!("\n[OK] Installed: {}", Self::ver(exe_path.to_str().unwrap()));
        info!("     Path: {}", exe_path.display());
        Ok(())
    }

    async fn download(path: &Path) -> Result<(), UvError> {
        let mut resp = reqwest::get(URL).await?;
        let total = resp.content_length().unwrap_or(0);
        let mut file = tokio::fs::File::create(path).await?;
        let mut done = 0u64;

        while let Some(chunk) = resp.chunk().await? {
            file.write_all(&chunk).await?;
            done += chunk.len() as u64;
            if total > 0 {
                print!("\r[..] {:.1}/{:.1} MB ({:.0}%)", done as f64 / 1048576.0, total as f64 / 1048576.0, done as f64 / total as f64 * 100.0);
                std::io::stdout().flush()?;
            }
        }
        println!();
        Ok(())
    }

    async fn extract(zip_path: &Path, exe_path: &Path) -> Result<(), UvError> {
        let zip_path = zip_path.to_path_buf();
        let exe_path = exe_path.to_path_buf();
        tokio::task::spawn_blocking(move || {
            let file = std::fs::File::open(&zip_path)?;
            let mut archive = zip::ZipArchive::new(file)?;
            for i in 0..archive.len() {
                let mut entry = archive.by_index(i)?;
                if entry.name().ends_with(EXE) {
                    let mut out = std::fs::File::create(&exe_path)?;
                    std::io::copy(&mut entry, &mut out)?;
                    return Ok(());
                }
            }
            Err(UvError::NotFound)
        }).await?
    }

    async fn cleanup(dir: &Path, zip_path: &Path) {
        tokio::fs::remove_file(zip_path).await.ok();
        if let Ok(mut entries) = tokio::fs::read_dir(dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name().to_string_lossy().to_string();
                if entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false) && name.starts_with("uv-") {
                    tokio::fs::remove_dir_all(entry.path()).await.ok();
                }
            }
        }
    }

    fn is_ok(cmd: &str) -> bool {
        Command::new(cmd).arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
    }

    fn ver(cmd: &str) -> String {
        Command::new(cmd).arg("--version").output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
    }
}

