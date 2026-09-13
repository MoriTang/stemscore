mod audio;
mod file_separator;
mod model;
mod resample;

use std::{
    fs,
    io::{Read, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};

use audio::{LiveSession, SharedStatus, StemWaveforms};
use directories::{ProjectDirs, UserDirs};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;

#[derive(Default)]
struct AppState {
    session: Mutex<Option<LiveSession>>,
    last_status: Mutex<Arc<SharedStatus>>,
    file_busy: Arc<AtomicBool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LiveStatus {
    state: String,
    device_name: String,
    output_dir: String,
    sample_rate: u32,
    captured_frames: u64,
    dropped_frames: u64,
    separated_frames: u64,
    last_inference_micros: u64,
    peak: f32,
    elapsed_seconds: f64,
    model_enabled: bool,
    waveforms: StemWaveforms,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelInfo {
    installed: bool,
    path: String,
    expected_size: u64,
}

#[derive(Debug, Clone, Serialize)]
struct DownloadProgress {
    downloaded: u64,
    total: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct FileSeparationProgress {
    processed_frames: u64,
    total_frames: Option<u64>,
    sample_rate: u32,
}

#[tauri::command]
fn live_status(state: State<'_, AppState>) -> LiveStatus {
    let session = state.session.lock().expect("session mutex poisoned");
    if let Some(session) = session.as_ref() {
        snapshot(&session.status, true)
    } else {
        let status = state.last_status.lock().expect("status mutex poisoned");
        snapshot(&status, false)
    }
}

#[tauri::command]
fn live_drum_level(state: State<'_, AppState>) -> f32 {
    let session = state.session.lock().expect("session mutex poisoned");
    session
        .as_ref()
        .map(|session| f32::from_bits(session.status.drum_energy_bits.load(Ordering::Relaxed)))
        .unwrap_or(0.0)
}

#[tauri::command]
fn live_model_info() -> Result<ModelInfo, String> {
    let path = live_model_path()?;
    let installed = path
        .metadata()
        .map(|metadata| metadata.is_file() && metadata.len() == model::MODEL_SIZE)
        .unwrap_or(false);
    Ok(ModelInfo {
        installed,
        path: path.display().to_string(),
        expected_size: model::MODEL_SIZE,
    })
}

#[tauri::command]
async fn download_live_model(app: AppHandle, state: State<'_, AppState>) -> Result<String, String> {
    {
        let session = state.session.lock().map_err(|_| "录音状态锁损坏")?;
        if session.is_some() {
            return Err("请先停止录音再下载模型".into());
        }
        if state
            .file_busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err("请等待当前后台任务完成".into());
        }
    }

    let busy = Arc::clone(&state.file_busy);
    tauri::async_runtime::spawn_blocking(move || {
        struct ResetBusy(Arc<AtomicBool>);
        impl Drop for ResetBusy {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Release);
            }
        }
        let _reset = ResetBusy(busy);
        download_model_file(&app)
    })
    .await
    .map_err(|error| format!("模型下载任务失败：{error}"))?
}

#[tauri::command]
async fn pick_audio_file(app: AppHandle) -> Result<Option<String>, String> {
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    app.dialog()
        .file()
        .add_filter("音频文件", &["wav", "mp3", "flac", "m4a", "aac", "ogg"])
        .pick_file(move |selected| {
            let _ = sender.send(selected);
        });
    let selected = tauri::async_runtime::spawn_blocking(move || receiver.recv())
        .await
        .map_err(|error| format!("文件选择任务失败：{error}"))?
        .map_err(|error| format!("文件选择器意外关闭：{error}"))?;
    Ok(selected
        .and_then(|path| path.into_path().ok())
        .map(|path| path.display().to_string()))
}

#[tauri::command]
async fn separate_audio_file(
    input_path: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    {
        let session = state.session.lock().map_err(|_| "录音状态锁损坏")?;
        if session.is_some() {
            return Err("请先停止实时录音再处理音频文件".into());
        }
        if state
            .file_busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err("已有音频文件正在分轨".into());
        }
    }

    let busy = Arc::clone(&state.file_busy);
    let input = PathBuf::from(input_path);
    let model_path = match live_model_path() {
        Ok(path) => path,
        Err(error) => {
            busy.store(false, Ordering::Release);
            return Err(error);
        }
    };
    if !model_path.is_file() {
        busy.store(false, Ordering::Release);
        return Err("请先下载实时分轨模型".into());
    }
    let output_dir = match next_file_output_directory(&input) {
        Ok(path) => path,
        Err(error) => {
            busy.store(false, Ordering::Release);
            return Err(error);
        }
    };
    let result = tauri::async_runtime::spawn_blocking(move || {
        struct ResetBusy(Arc<AtomicBool>);
        impl Drop for ResetBusy {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Release);
            }
        }
        let _reset = ResetBusy(busy);
        file_separator::separate_audio_file(&input, &output_dir, &model_path, |progress| {
            let _ = app.emit(
                "file-separation-progress",
                FileSeparationProgress {
                    processed_frames: progress.processed_input_frames,
                    total_frames: progress.total_input_frames,
                    sample_rate: progress.sample_rate,
                },
            );
        })
        .map(|()| output_dir.display().to_string())
        .map_err(|error| format!("音频文件分轨失败：{error:#}"))
    })
    .await;
    result.map_err(|error| format!("文件分轨任务失败：{error}"))?
}

#[tauri::command]
fn start_live_capture(record_only: bool, state: State<'_, AppState>) -> Result<LiveStatus, String> {
    let mut guard = state.session.lock().map_err(|_| "录音状态锁损坏")?;
    if state.file_busy.load(Ordering::Acquire) {
        return Err("请等待文件分轨完成后再开始录音".into());
    }
    if guard.is_some() {
        return Err("录音已经在进行中".into());
    }

    let output_dir = next_output_directory()?;
    let model_path = if record_only {
        None
    } else {
        let path = live_model_path()?;
        if !path.is_file() {
            return Err("请先下载实时分轨模型，或勾选“仅录制原始音频”".into());
        }
        Some(path)
    };

    let session = LiveSession::start(output_dir, model_path.as_deref())
        .map_err(|error| format!("无法开始录音：{error:#}"))?;
    let status = Arc::clone(&session.status);
    *state.last_status.lock().map_err(|_| "状态锁损坏")? = Arc::clone(&status);
    *guard = Some(session);
    Ok(snapshot(&status, true))
}

#[tauri::command]
fn stop_live_capture(state: State<'_, AppState>) -> Result<String, String> {
    let session = state
        .session
        .lock()
        .map_err(|_| "录音状态锁损坏")?
        .take()
        .ok_or_else(|| "当前没有正在进行的录音".to_string())?;
    let path = session
        .stop()
        .map_err(|error| format!("停止录音失败：{error:#}"))?;
    Ok(path.display().to_string())
}

fn snapshot(status: &SharedStatus, active: bool) -> LiveStatus {
    let elapsed_seconds = status
        .started
        .lock()
        .expect("started mutex poisoned")
        .map(|started| started.elapsed().as_secs_f64())
        .unwrap_or(0.0);
    LiveStatus {
        state: if active { "recording" } else { "idle" }.into(),
        device_name: status
            .device_name
            .lock()
            .expect("device mutex poisoned")
            .clone(),
        output_dir: status
            .output_dir
            .lock()
            .expect("output mutex poisoned")
            .clone(),
        sample_rate: status.sample_rate.load(Ordering::Relaxed),
        captured_frames: status.captured_frames.load(Ordering::Relaxed),
        dropped_frames: status.dropped_frames.load(Ordering::Relaxed),
        separated_frames: status.separated_frames.load(Ordering::Relaxed),
        last_inference_micros: status.last_inference_micros.load(Ordering::Relaxed),
        peak: f32::from_bits(status.peak_bits.load(Ordering::Relaxed)),
        elapsed_seconds,
        model_enabled: status.model_enabled.load(Ordering::Relaxed),
        waveforms: status.waveform_snapshot(),
        error: status.error.lock().expect("error mutex poisoned").clone(),
    }
}

fn live_model_path() -> Result<PathBuf, String> {
    let project = ProjectDirs::from("com", "MoriTang", "StemScore")
        .ok_or_else(|| "无法确定应用数据目录".to_string())?;
    Ok(project
        .data_local_dir()
        .join("models")
        .join("hstasnet.onnx"))
}

fn next_output_directory() -> Result<PathBuf, String> {
    let base = UserDirs::new()
        .and_then(|dirs| dirs.audio_dir().map(PathBuf::from))
        .or_else(|| std::env::current_dir().ok())
        .ok_or_else(|| "无法确定录音输出目录".to_string())?
        .join("StemScore Recordings");
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("系统时间错误：{error}"))?
        .as_secs();
    Ok(base.join(format!("recording-{timestamp}")))
}

fn next_file_output_directory(input: &std::path::Path) -> Result<PathBuf, String> {
    let base = UserDirs::new()
        .and_then(|dirs| dirs.audio_dir().map(PathBuf::from))
        .or_else(|| std::env::current_dir().ok())
        .ok_or_else(|| "无法确定分轨输出目录".to_string())?
        .join("StemScore Separations");
    let raw_name = input
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("audio");
    let safe_name: String = raw_name
        .chars()
        .map(|character| {
            if character.is_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("系统时间错误：{error}"))?
        .as_secs();
    Ok(base.join(format!("{safe_name}-{timestamp}")))
}

fn download_model_file(app: &AppHandle) -> Result<String, String> {
    let path = live_model_path()?;
    let parent = path.parent().ok_or_else(|| "模型目录无效".to_string())?;
    fs::create_dir_all(parent).map_err(|error| format!("无法创建模型目录：{error}"))?;
    let temporary = path.with_extension("onnx.part");

    let mut response = reqwest::blocking::Client::new()
        .get(model::MODEL_URL)
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|error| format!("无法下载实时模型：{error}"))?;
    let total = response.content_length().unwrap_or(model::MODEL_SIZE);
    let mut output =
        fs::File::create(&temporary).map_err(|error| format!("无法创建模型临时文件：{error}"))?;
    let mut hasher = Sha256::new();
    let mut downloaded = 0_u64;
    let mut buffer = vec![0_u8; 256 * 1024];

    loop {
        let read = response
            .read(&mut buffer)
            .map_err(|error| format!("读取模型下载流失败：{error}"))?;
        if read == 0 {
            break;
        }
        output
            .write_all(&buffer[..read])
            .map_err(|error| format!("写入模型失败：{error}"))?;
        hasher.update(&buffer[..read]);
        downloaded += read as u64;
        let _ = app.emit(
            "model-download-progress",
            DownloadProgress { downloaded, total },
        );
    }
    output
        .sync_all()
        .map_err(|error| format!("同步模型失败：{error}"))?;

    let digest = format!("{:x}", hasher.finalize());
    if downloaded != model::MODEL_SIZE || digest != model::MODEL_SHA256 {
        let _ = fs::remove_file(&temporary);
        return Err(format!(
            "模型校验失败：大小 {downloaded} 字节，SHA-256 {digest}"
        ));
    }
    if path.exists() {
        fs::remove_file(&path).map_err(|error| format!("无法替换旧模型：{error}"))?;
    }
    fs::rename(&temporary, &path).map_err(|error| format!("无法安装模型：{error}"))?;
    Ok(path.display().to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            live_status,
            live_drum_level,
            live_model_info,
            download_live_model,
            pick_audio_file,
            separate_audio_file,
            start_live_capture,
            stop_live_capture,
        ])
        .run(tauri::generate_context!())
        .expect("error while running StemScore Live");
}
