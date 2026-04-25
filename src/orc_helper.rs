use std::process::{Child, Command};
use std::{io, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time;
use tracing::debug;

const SERVER_PATH: &str = r"C:\Users\10401\Desktop\rust_projects\shi_jie_ol_helper_v3\py_ocr";
const OCR_SERVER_ADDR: &str = "127.0.0.1:9000";

#[derive(Debug, serde::Deserialize)]
pub struct OcrItem {
    pub text: String,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

pub struct OcrServer {
    child: Child,
    server_addr: String
}

impl OcrServer {
    /// 启动 Python 服务器，阻塞等待端口可用后返回
    pub fn launch() -> io::Result<Self> {
        let mut child = Command::new("uv")
            .current_dir(SERVER_PATH)
            .arg("run")
            .arg("server.py")
            .spawn()?;

        for _ in 0..60 {
            if std::net::TcpStream::connect(OCR_SERVER_ADDR).is_ok() {
                return Ok(Self { child, server_addr: OCR_SERVER_ADDR.into() });
            }
            std::thread::sleep(Duration::from_millis(500));
        }

        let _ = child.kill();
        Err(io::Error::new(io::ErrorKind::TimedOut, "server not ready"))
    }

    pub fn get_client(&self) -> OcrClient {
        OcrClient::new(&self.server_addr)
    }
}

impl Drop for OcrServer {
    fn drop(&mut self) {
        debug!("正在退出ocr服务器");
        let _ = self.child.kill();
        let _ = self.child.wait();
        debug!("退出ocr服务器完成");
    }
}

#[derive(Debug)]
pub struct OcrClient {
    server: String,
    timeout: Duration,
}

impl OcrClient {
    pub fn new(server: &str) -> Self {
        Self { server: server.into(), timeout: Duration::from_secs(60) }
    }

    pub fn with_timeout(server: &str, timeout: Duration) -> Self {
        Self { server: server.into(), timeout }
    }

    pub async fn recognize(&self, path: &str) -> io::Result<Vec<OcrItem>> {
        let s = time::timeout(Duration::from_secs(5), TcpStream::connect(&self.server))
            .await.map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "connect"))??;
        let mut s = s;

        time::timeout(Duration::from_secs(5), async {
            s.write_all(&(path.len() as u32).to_be_bytes()).await?;
            s.write_all(path.as_bytes()).await?;
            s.flush().await
        }).await.map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "write"))??;

        let raw = time::timeout(self.timeout, async {
            let mut h = [0u8; 4]; s.read_exact(&mut h).await?;
            let mut b = vec![0u8; u32::from_be_bytes(h) as usize];
            s.read_exact(&mut b).await?;
            Ok::<_, io::Error>(String::from_utf8_lossy(&b).into_owned())
        }).await.map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "read"))??;

        Ok(serde_json::from_str(&raw)?)
    }
}

mod test {
    use super::*;

    #[tokio::test]
    async fn test_ocr() {
        let orc_client = OcrClient::new("127.0.0.1:9000");
        let res = orc_client.recognize(r"C:\Users\10401\Desktop\rust_projects\shi_jie_ol_helper_v3\sub_image_vm_index0.png").await.unwrap();
        println!("{:?}", res);
    }
}