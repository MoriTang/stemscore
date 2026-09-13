const { invoke, convertFileSrc } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const el = (id) => document.getElementById(id);
const recordButton = el("record-button");
const stopButton = el("stop-button");
const downloadButton = el("download-button");
const recordOnly = el("record-only");
const chooseFileButton = el("choose-file-button");
const separateFileButton = el("separate-file-button");

let modelReady = false;
let busy = false;
let fileBusy = false;
let selectedFile = null;
let activePlayer = null;
let liveRecording = false;
let drumLightTarget = 0;
let drumLightLevel = 0;
let currentTab = "file";

const stemLabels = {
  drums: "鼓",
  bass: "贝斯",
  other: "其他",
  vocals: "人声",
};

function setMessage(target, text, kind = "") {
  el(`${target}-message`).textContent = text;
  el(`${target}-message`).className = `message ${kind}`;
}

function formatTime(seconds) {
  const mins = Math.floor(seconds / 60).toString().padStart(2, "0");
  const secs = Math.floor(seconds % 60).toString().padStart(2, "0");
  return `${mins}:${secs}`;
}

function formatPlayerTime(seconds) {
  return Number.isFinite(seconds) ? formatTime(seconds) : "00:00";
}

function switchTab(target) {
  currentTab = target;
  for (const tab of ["file", "live"]) {
    const active = tab === target;
    el(`${tab}-tab`).hidden = !active;
    el(`${tab}-tab`).classList.toggle("active", active);
    el(`${tab}-tab-button`).classList.toggle("active", active);
    el(`${tab}-tab-button`).setAttribute("aria-selected", String(active));
  }
}

function hideStemPlayers(target) {
  const container = el(`${target}-stem-players`);
  container.querySelectorAll("audio").forEach((audio) => audio.pause());
  if (container.contains(activePlayer)) activePlayer = null;
  el(`${target}-players-section`).hidden = true;
  container.replaceChildren();
}

function showStemPlayers(target, outputDirectory, nestedStems) {
  hideStemPlayers(target);
  const container = el(`${target}-stem-players`);
  Object.entries(stemLabels).forEach(([stem, label]) => {
    const row = document.createElement("div");
    row.className = "stem-player";
    row.innerHTML = `
      <div class="stem-player-name"><i class="stem-player-dot"></i>${label}</div>
      <button type="button" aria-label="播放${label}">▶</button>
      <input type="range" min="0" max="1" step="0.001" value="0" aria-label="${label}播放进度" />
      <span class="stem-player-time">00:00 / 00:00</span>
      <audio preload="metadata"></audio>`;
    row.querySelector(".stem-player-dot").style.background = waveformColors[stem];
    const button = row.querySelector("button");
    const seek = row.querySelector("input");
    const time = row.querySelector(".stem-player-time");
    const audio = row.querySelector("audio");
    const separator = outputDirectory.includes("\\") ? "\\" : "/";
    const relative = nestedStems ? `stems${separator}${stem}.wav` : `${stem}.wav`;
    audio.src = convertFileSrc(`${outputDirectory}${separator}${relative}`);

    const refresh = () => {
      const duration = Number.isFinite(audio.duration) ? audio.duration : 0;
      seek.value = duration ? audio.currentTime / duration : 0;
      time.textContent = `${formatPlayerTime(audio.currentTime)} / ${formatPlayerTime(duration)}`;
      button.textContent = audio.paused ? "▶" : "Ⅱ";
      button.setAttribute("aria-label", `${audio.paused ? "播放" : "暂停"}${label}`);
    };
    button.addEventListener("click", async () => {
      if (audio.paused) {
        if (activePlayer && activePlayer !== audio) activePlayer.pause();
        activePlayer = audio;
        try { await audio.play(); } catch (error) { setMessage(target, `无法播放${label}：${error}`, "error"); }
      } else {
        audio.pause();
      }
      refresh();
    });
    seek.addEventListener("input", () => {
      if (Number.isFinite(audio.duration)) audio.currentTime = Number(seek.value) * audio.duration;
    });
    audio.addEventListener("timeupdate", refresh);
    audio.addEventListener("loadedmetadata", refresh);
    audio.addEventListener("play", refresh);
    audio.addEventListener("pause", refresh);
    audio.addEventListener("ended", refresh);
    audio.addEventListener("error", () => setMessage(target, `无法读取${label}音轨`, "error"));
    container.append(row);
  });
  el(`${target}-players-output`).textContent = outputDirectory;
  el(`${target}-players-section`).hidden = false;
}

const waveformColors = {
  drums: "#ff816e",
  bass: "#e6c65c",
  other: "#77a6f7",
  vocals: "#c780ec",
};

function drawWaveform(stem, points = []) {
  const canvas = el(`wave-${stem}`);
  const bounds = canvas.getBoundingClientRect();
  const scale = window.devicePixelRatio || 1;
  const width = Math.max(1, Math.round(bounds.width * scale));
  const height = Math.max(1, Math.round(bounds.height * scale));
  if (canvas.width !== width || canvas.height !== height) {
    canvas.width = width;
    canvas.height = height;
  }
  const context = canvas.getContext("2d");
  context.clearRect(0, 0, width, height);
  context.strokeStyle = "#343934";
  context.lineWidth = scale;
  context.beginPath();
  context.moveTo(0, height / 2);
  context.lineTo(width, height / 2);
  context.stroke();
  if (!points.length) return;

  const step = width / Math.max(1, points.length - 1);
  context.fillStyle = `${waveformColors[stem]}35`;
  context.strokeStyle = waveformColors[stem];
  context.lineWidth = Math.max(scale, step * 0.55);
  context.beginPath();
  points.forEach(([minimum, maximum], index) => {
    const x = index * step;
    const top = height / 2 - Math.max(-1, Math.min(1, maximum)) * height * 0.46;
    const bottom = height / 2 - Math.max(-1, Math.min(1, minimum)) * height * 0.46;
    context.moveTo(x, top);
    context.lineTo(x, bottom);
  });
  context.stroke();
}

async function refreshModel() {
  const info = await invoke("live_model_info");
  modelReady = info.installed;
  el("model-label").textContent = info.installed ? "已安装" : "未安装（约 106 MB）";
  el("model-detail").textContent = info.installed ? info.path : "HS-TasNet · 4 stems · 44.1 kHz";
  downloadButton.textContent = info.installed ? "重新下载" : "下载模型";
}

function renderStatus(status) {
  const recording = status.state === "recording";
  liveRecording = recording && status.modelEnabled;
  if (!liveRecording) drumLightTarget = 0;
  el("state-pill").textContent = recording ? "正在录制" : status.state === "stopping" ? "正在停止" : "待机";
  el("state-pill").className = `pill ${recording ? "recording" : "idle"}`;
  el("device-name").textContent = status.deviceName || "等待检测";
  el("sample-rate").textContent = status.sampleRate ? `${status.sampleRate.toLocaleString()} Hz` : "—";
  el("frames").textContent = `${status.capturedFrames.toLocaleString()} 帧`;
  el("dropped").textContent = status.droppedFrames.toLocaleString();
  el("inference-time").textContent = status.lastInferenceMicros ? `${(status.lastInferenceMicros / 1000).toFixed(2)} ms` : "—";
  el("timer").textContent = formatTime(status.elapsedSeconds || 0);
  el("meter-fill").style.width = `${Math.min(100, Math.sqrt(status.peak || 0) * 100)}%`;
  el("live-output").textContent = status.outputDir ? `输出：${status.outputDir}` : "";
  Object.keys(waveformColors).forEach((stem) => drawWaveform(stem, status.waveforms?.[stem]));
  recordButton.disabled = busy || recording || (!recordOnly.checked && !modelReady);
  stopButton.disabled = busy || !recording;
  recordOnly.disabled = recording;
  downloadButton.disabled = busy || fileBusy || recording;
  chooseFileButton.disabled = busy || fileBusy || recording;
  separateFileButton.disabled = busy || fileBusy || recording || !selectedFile || !modelReady;
  if (status.error) setMessage("live", status.error, "error");
}

el("file-tab-button").addEventListener("click", () => switchTab("file"));
el("live-tab-button").addEventListener("click", () => switchTab("live"));

recordOnly.addEventListener("change", async () => renderStatus(await invoke("live_status")));

downloadButton.addEventListener("click", async () => {
  busy = true;
  downloadButton.disabled = true;
  setMessage(currentTab, "正在下载并校验 HS-TasNet 模型…");
  try {
    const path = await invoke("download_live_model");
    setMessage(currentTab, `模型已安装：${path}`, "success");
    await refreshModel();
  } catch (error) {
    setMessage(currentTab, String(error), "error");
  } finally {
    busy = false;
    renderStatus(await invoke("live_status"));
  }
});

chooseFileButton.addEventListener("click", async () => {
  try {
    const path = await invoke("pick_audio_file");
    if (!path) return;
    selectedFile = path;
    const parts = path.split(/[\\/]/);
    el("file-name").textContent = parts.at(-1) || path;
    el("file-path").textContent = path;
    el("file-progress-label").textContent = "准备就绪";
    el("file-progress-fill").style.width = "0%";
    renderStatus(await invoke("live_status"));
  } catch (error) {
    setMessage("file", String(error), "error");
  }
});

separateFileButton.addEventListener("click", async () => {
  if (!selectedFile) return;
  fileBusy = true;
  hideStemPlayers("file");
  el("file-progress-fill").style.width = "0%";
  el("file-progress-label").textContent = "正在加载模型并解码音频…";
  setMessage("file", "正在处理本地音频文件，请保持应用运行…");
  renderStatus(await invoke("live_status"));
  try {
    const output = await invoke("separate_audio_file", { inputPath: selectedFile });
    el("file-progress-fill").style.width = "100%";
    el("file-progress-label").textContent = "分轨完成";
    showStemPlayers("file", output, false);
    setMessage("file", `文件分轨完成：${output}`, "success");
  } catch (error) {
    el("file-progress-label").textContent = "处理失败";
    setMessage("file", String(error), "error");
  } finally {
    fileBusy = false;
    renderStatus(await invoke("live_status"));
  }
});

recordButton.addEventListener("click", async () => {
  busy = true;
  hideStemPlayers("live");
  setMessage("live", "正在初始化系统音频和推理引擎…");
  try {
    await invoke("start_live_capture", { recordOnly: recordOnly.checked });
    setMessage("live", recordOnly.checked ? "正在录制系统原始音频。" : "正在录制并实时生成四个声部。", "success");
  } catch (error) {
    setMessage("live", String(error), "error");
  } finally {
    busy = false;
    renderStatus(await invoke("live_status"));
  }
});

stopButton.addEventListener("click", async () => {
  busy = true;
  setMessage("live", "正在写完缓冲区并关闭 WAV 文件…");
  try {
    const output = await invoke("stop_live_capture");
    if (!recordOnly.checked) showStemPlayers("live", output, true);
    setMessage("live", `录制完成：${output}`, "success");
  } catch (error) {
    setMessage("live", String(error), "error");
  } finally {
    busy = false;
    renderStatus(await invoke("live_status"));
  }
});

listen("model-download-progress", ({ payload }) => {
  const percent = payload.total ? Math.floor(payload.downloaded / payload.total * 100) : 0;
  setMessage(currentTab, `正在下载模型… ${percent}%`);
});

listen("file-separation-progress", ({ payload }) => {
  const processedSeconds = payload.sampleRate ? payload.processedFrames / payload.sampleRate : 0;
  if (payload.totalFrames) {
    const percent = Math.min(100, payload.processedFrames / payload.totalFrames * 100);
    el("file-progress-fill").style.width = `${percent}%`;
    el("file-progress-label").textContent = `已处理 ${formatTime(processedSeconds)} · ${percent.toFixed(0)}%`;
  } else {
    el("file-progress-label").textContent = `已处理 ${formatTime(processedSeconds)}`;
  }
});

await refreshModel();
renderStatus(await invoke("live_status"));
setInterval(async () => {
  try { renderStatus(await invoke("live_status")); } catch (_) { /* app is shutting down */ }
}, 300);

setInterval(async () => {
  if (!liveRecording) return;
  try { drumLightTarget = Math.max(0, Math.min(1, await invoke("live_drum_level"))); }
  catch (_) { drumLightTarget = 0; }
}, 50);

function animateDrumLight() {
  const response = drumLightTarget > drumLightLevel ? 0.34 : 0.09;
  drumLightLevel += (drumLightTarget - drumLightLevel) * response;
  const intensity = Math.sqrt(Math.max(0, drumLightLevel));
  const light = el("drum-light");
  light.style.opacity = String(0.12 + intensity * 0.88);
  light.style.transform = `scale(${0.82 + intensity * 0.42})`;
  light.style.boxShadow = `0 0 ${5 + intensity * 26}px ${intensity * 8}px rgba(255,129,110,${0.18 + intensity * 0.62})`;
  requestAnimationFrame(animateDrumLight);
}
requestAnimationFrame(animateDrumLight);
