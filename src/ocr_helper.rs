use std::io::{self, Write};
use std::path::Path;
use std::process::Command;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::debug;

use crate::config_util;

#[derive(Debug, serde::Deserialize)]
pub struct OcrItem {
    pub text: String,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

pub struct OcrServer {
    server_addr: String,
}

impl OcrServer {
    /// 启动 Python 服务器，阻塞等待端口可用后返回
    pub fn launch() -> io::Result<Self> {
        let mut child = Command::new("uv")
            .current_dir(Path::new(&config_util::OCR_CONFIG.server_program_path))
            .arg("run")
            .arg("server.py")
            .arg("127.0.0.1")
            .arg(config_util::OCR_CONFIG.server_port.to_string())
            .spawn()?;
        let server_addr = format!("127.0.0.1:{}", &config_util::OCR_CONFIG.server_port);
        for _ in 0..60 {
            if std::net::TcpStream::connect(&server_addr).is_ok() {
                return Ok(Self { server_addr });
            }
            std::thread::sleep(Duration::from_millis(500));
        }

        let _ = child.kill();
        Err(io::Error::new(io::ErrorKind::TimedOut, "server not ready"))
    }

    pub async fn get_client(&self) -> anyhow::Result<OcrClient> {
        OcrClient::new(&self.server_addr).await
    }
}

impl Drop for OcrServer {
    fn drop(&mut self) {
        // 忽略错误，Drop 中不应该 panic
        if let Ok(mut stream) = std::net::TcpStream::connect(&self.server_addr) {
            // 协议格式：[4B type_len][msg_type]
            // "Exit" 长度是 4，大端序 u32 是 0x00000004
            let frame = [
                0x00, 0x00, 0x00, 0x04, // type_len = 4
                b'E', b'x', b'i', b't', // msg_type = "Exit"
            ];

            let _ = stream.write_all(&frame);
            let _ = stream.flush();
            // stream 离开作用域自动断开
        }
    }
}

#[derive(Debug)]
pub struct OcrClient {
    stream: TcpStream,
}

impl OcrClient {
    /// 连接到服务器
    pub async fn new(addr: &str) -> anyhow::Result<Self> {
        let stream = TcpStream::connect(addr).await?;
        Ok(Self { stream })
    }

    /// 底层方法：发送一帧数据
    async fn write_frame(&mut self, msg_type: &str, payload: Option<&[u8]>) -> anyhow::Result<()> {
        let type_bytes = msg_type.as_bytes();

        // 1. 写入类型长度 (大端序 u32)
        self.stream.write_u32(type_bytes.len() as u32).await?;
        // 2. 写入类型字符串
        self.stream.write_all(type_bytes).await?;

        // 3. 如果有载荷，写入载荷长度和载荷数据
        if let Some(data) = payload {
            self.stream.write_u32(data.len() as u32).await?;
            self.stream.write_all(data).await?;
        }

        self.stream.flush().await?;
        Ok(())
    }

    /// 底层方法：读取服务器的响应
    async fn read_response(&mut self) -> anyhow::Result<String> {
        // 读取响应体长度 (大端序 u32)
        let len = self.stream.read_u32().await? as usize;

        if len == 0 {
            return Ok(String::new());
        }

        let mut buf = vec![0u8; len];
        self.stream.read_exact(&mut buf).await?;

        // 将 UTF-8 字节转为 String，忽略无效的 Unicode 字符
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }

    /// 发送图片进行 OCR 识别
    pub async fn recognize(&mut self, image_bytes: &[u8]) -> anyhow::Result<Vec<OcrItem>> {
        // debug!("image_bytes_len:{}", image_bytes.len());
        // 发送 Image 帧
        self.write_frame("Image", Some(&image_bytes)).await?;

        // 接收并返回结果
        let json = self.read_response().await?;

        // debug!("recognize result:{}", json);
        Ok(serde_json::from_str(&json)?)
    }

    /// 发送退出指令，关闭服务器
    pub async fn exit(&mut self) -> anyhow::Result<String> {
        // 发送 Exit 帧 (无载荷)
        self.write_frame("Exit", None).await?;

        self.read_response().await
    }
}

#[cfg(test)]
mod test {
    use std::fs;

    use image::EncodableLayout;

    use crate::ocr_helper::OcrServer;

    #[tokio::test]
    pub async fn test_orc() {
        let server = OcrServer::launch().unwrap();
        let mut cli = server.get_client().await.unwrap();
        let text = cli
            .recognize(
                fs::read("test_res/sub_image_vm_index0.png")
                    .unwrap()
                    .as_bytes(),
            )
            .await
            .unwrap();
        println!("{:?}", text);
    }
}
