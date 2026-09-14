use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use directories::ProjectDirs;
use sha2::{Digest, Sha256};

use crate::model::{MODEL_SHA256, MODEL_SIZE, MODEL_URL};

#[derive(Debug, Clone, Copy)]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: u64,
}

pub fn default_model_path() -> Result<PathBuf> {
    let project =
        ProjectDirs::from("com", "MoriTang", "StemScore").context("无法确定应用数据目录")?;
    Ok(project
        .data_local_dir()
        .join("models")
        .join("hstasnet.onnx"))
}

pub fn is_installed(path: &Path) -> bool {
    path.metadata()
        .map(|metadata| metadata.is_file() && metadata.len() == MODEL_SIZE)
        .unwrap_or(false)
}

pub fn verify(path: &Path) -> Result<()> {
    let metadata = path
        .metadata()
        .with_context(|| format!("无法读取模型：{}", path.display()))?;
    if !metadata.is_file() || metadata.len() != MODEL_SIZE {
        bail!("模型文件大小不正确，请重新下载");
    }

    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 256 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let digest = format!("{:x}", hasher.finalize());
    if digest != MODEL_SHA256 {
        bail!("模型 SHA-256 校验失败，请重新下载");
    }
    Ok(())
}

pub fn download(path: &Path, mut report_progress: impl FnMut(DownloadProgress)) -> Result<()> {
    let parent = path.parent().context("模型目录无效")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("无法创建模型目录：{}", parent.display()))?;
    let temporary = path.with_extension("onnx.part");

    let result = (|| -> Result<()> {
        let mut response = reqwest::blocking::Client::new()
            .get(MODEL_URL)
            .send()
            .and_then(|response| response.error_for_status())
            .context("无法下载分轨模型")?;
        let total = response.content_length().unwrap_or(MODEL_SIZE);
        let mut output = File::create(&temporary)
            .with_context(|| format!("无法创建模型临时文件：{}", temporary.display()))?;
        let mut hasher = Sha256::new();
        let mut downloaded = 0_u64;
        let mut buffer = vec![0_u8; 256 * 1024];

        loop {
            let read = response.read(&mut buffer).context("读取模型下载流失败")?;
            if read == 0 {
                break;
            }
            output.write_all(&buffer[..read]).context("写入模型失败")?;
            hasher.update(&buffer[..read]);
            downloaded += read as u64;
            report_progress(DownloadProgress { downloaded, total });
        }
        output.sync_all().context("同步模型文件失败")?;

        let digest = format!("{:x}", hasher.finalize());
        if downloaded != MODEL_SIZE || digest != MODEL_SHA256 {
            bail!("模型校验失败：大小 {downloaded} 字节，SHA-256 {digest}");
        }

        if path.exists() {
            fs::remove_file(path).with_context(|| format!("无法替换旧模型：{}", path.display()))?;
        }
        fs::rename(&temporary, path)
            .with_context(|| format!("无法安装模型：{}", path.display()))?;
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_wrong_sized_model() {
        let path =
            std::env::temp_dir().join(format!("stemflow-invalid-model-{}", std::process::id()));
        fs::write(&path, b"not an onnx model").unwrap();
        let error = verify(&path).unwrap_err().to_string();
        assert!(error.contains("大小不正确"));
        fs::remove_file(path).unwrap();
    }
}
