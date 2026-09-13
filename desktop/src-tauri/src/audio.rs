use std::{
    collections::VecDeque,
    fs::{self, File},
    io::BufWriter,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use anyhow::{bail, Context, Result};
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    FromSample, Sample, SampleFormat, SizedSample, Stream, StreamConfig,
};
use hound::{SampleFormat as WavSampleFormat, WavSpec, WavWriter};
use ringbuf::{
    traits::{Consumer, Observer, Producer, Split},
    HeapCons, HeapProd, HeapRb,
};
use serde::Serialize;

use crate::{
    model::{HsTasNet, SeparatedHop, StereoFrame, HOP_FRAMES, MODEL_SAMPLE_RATE},
    resample::StereoLinearResampler,
};

type FloatWavWriter = WavWriter<BufWriter<File>>;
const WAVEFORM_BUCKET_HOPS: u8 = 6;
const WAVEFORM_HISTORY_POINTS: usize = 480;

#[derive(Debug, Clone, Default, Serialize)]
pub struct StemWaveforms {
    pub drums: Vec<[f32; 2]>,
    pub bass: Vec<[f32; 2]>,
    pub other: Vec<[f32; 2]>,
    pub vocals: Vec<[f32; 2]>,
}

#[derive(Default)]
struct WaveformHistory {
    drums: VecDeque<[f32; 2]>,
    bass: VecDeque<[f32; 2]>,
    other: VecDeque<[f32; 2]>,
    vocals: VecDeque<[f32; 2]>,
    bucket_min: [f32; 4],
    bucket_max: [f32; 4],
    bucket_hops: u8,
}

impl WaveformHistory {
    fn push(&mut self, output: &SeparatedHop) -> f32 {
        if self.bucket_hops == 0 {
            self.bucket_min = [f32::MAX; 4];
            self.bucket_max = [f32::MIN; 4];
        }
        for (index, stem) in [&output.drums, &output.bass, &output.other, &output.vocals]
            .into_iter()
            .enumerate()
        {
            for sample in stem.iter().flatten() {
                self.bucket_min[index] = self.bucket_min[index].min(*sample);
                self.bucket_max[index] = self.bucket_max[index].max(*sample);
            }
        }
        self.bucket_hops += 1;
        if self.bucket_hops < WAVEFORM_BUCKET_HOPS {
            return self.bucket_max[0].abs().max(self.bucket_min[0].abs());
        }
        push_waveform_point(&mut self.drums, [self.bucket_min[0], self.bucket_max[0]]);
        push_waveform_point(&mut self.bass, [self.bucket_min[1], self.bucket_max[1]]);
        push_waveform_point(&mut self.other, [self.bucket_min[2], self.bucket_max[2]]);
        push_waveform_point(&mut self.vocals, [self.bucket_min[3], self.bucket_max[3]]);
        self.bucket_hops = 0;
        self.drums
            .back()
            .map(|point| point[0].abs().max(point[1].abs()))
            .unwrap_or(0.0)
    }

    fn snapshot(&self) -> StemWaveforms {
        StemWaveforms {
            drums: self.drums.iter().copied().collect(),
            bass: self.bass.iter().copied().collect(),
            other: self.other.iter().copied().collect(),
            vocals: self.vocals.iter().copied().collect(),
        }
    }
}

fn push_waveform_point(history: &mut VecDeque<[f32; 2]>, point: [f32; 2]) {
    if history.len() == WAVEFORM_HISTORY_POINTS {
        history.pop_front();
    }
    history.push_back(point);
}

#[derive(Default)]
pub struct SharedStatus {
    pub captured_frames: AtomicU64,
    pub dropped_frames: AtomicU64,
    pub separated_frames: AtomicU64,
    pub last_inference_micros: AtomicU64,
    pub peak_bits: AtomicU32,
    pub drum_energy_bits: AtomicU32,
    pub error: Mutex<Option<String>>,
    pub running: AtomicBool,
    pub started: Mutex<Option<Instant>>,
    pub device_name: Mutex<String>,
    pub output_dir: Mutex<String>,
    pub sample_rate: AtomicU32,
    pub model_enabled: AtomicBool,
    waveform_history: Mutex<WaveformHistory>,
}

impl SharedStatus {
    fn set_error(&self, message: impl Into<String>) {
        *self.error.lock().expect("status error mutex poisoned") = Some(message.into());
    }

    pub fn waveform_snapshot(&self) -> StemWaveforms {
        self.waveform_history
            .lock()
            .expect("waveform mutex poisoned")
            .snapshot()
    }
}

pub struct LiveSession {
    stream: Option<Stream>,
    worker: Option<JoinHandle<()>>,
    stop: Arc<AtomicBool>,
    pub status: Arc<SharedStatus>,
    pub output_dir: PathBuf,
}

impl LiveSession {
    pub fn start(output_dir: PathBuf, model_path: Option<&Path>) -> Result<Self> {
        fs::create_dir_all(&output_dir)
            .with_context(|| format!("无法创建输出目录：{}", output_dir.display()))?;

        // Load before opening the audio tap so a model error cannot leave a
        // half-started capture session behind.
        let model = model_path.map(HsTasNet::load).transpose()?;

        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .context("找不到默认音频输出设备")?;
        let description = device.description().context("无法读取输出设备信息")?;
        let config = device
            .default_output_config()
            .context("无法读取默认输出格式")?;

        if config.channels() != 2 {
            bail!(
                "当前输出设备为 {} 声道；实时分轨目前要求立体声输出",
                config.channels()
            );
        }

        let sample_rate = config.sample_rate();
        let capacity_frames = sample_rate as usize * 4;
        let (producer, consumer) = HeapRb::<StereoFrame>::new(capacity_frames).split();
        let stop = Arc::new(AtomicBool::new(false));
        let status = Arc::new(SharedStatus::default());
        status.running.store(true, Ordering::Release);
        status.sample_rate.store(sample_rate, Ordering::Release);
        status
            .model_enabled
            .store(model.is_some(), Ordering::Release);
        *status.started.lock().expect("started mutex poisoned") = Some(Instant::now());
        *status.device_name.lock().expect("device mutex poisoned") = description.name().to_owned();
        *status.output_dir.lock().expect("output mutex poisoned") =
            output_dir.display().to_string();

        let stream = build_loopback_stream(&device, &config, producer, Arc::clone(&status))?;

        let worker_stop = Arc::clone(&stop);
        let worker_status = Arc::clone(&status);
        let worker_output = output_dir.clone();
        let worker = thread::Builder::new()
            .name("stemscore-inference".into())
            .spawn(move || {
                if let Err(error) = run_worker(
                    consumer,
                    worker_stop,
                    Arc::clone(&worker_status),
                    worker_output,
                    sample_rate,
                    model,
                ) {
                    worker_status.set_error(format!("录音线程失败：{error:#}"));
                }
                worker_status.running.store(false, Ordering::Release);
            })
            .context("无法启动录音工作线程")?;

        if let Err(error) = stream.play() {
            stop.store(true, Ordering::Release);
            drop(stream);
            let _ = worker.join();
            return Err(error).context("无法启动系统音频采集");
        }

        Ok(Self {
            stream: Some(stream),
            worker: Some(worker),
            stop,
            status,
            output_dir,
        })
    }

    pub fn stop(mut self) -> Result<PathBuf> {
        // Stop producing first, then let the worker drain every queued frame.
        self.stream.take();
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| anyhow::anyhow!("录音工作线程异常退出"))?;
        }
        self.status.running.store(false, Ordering::Release);
        self.status.peak_bits.store(0, Ordering::Relaxed);
        self.status.drum_energy_bits.store(0, Ordering::Relaxed);
        *self.status.started.lock().expect("started mutex poisoned") = None;
        Ok(self.output_dir.clone())
    }
}

impl Drop for LiveSession {
    fn drop(&mut self) {
        self.stream.take();
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.status.running.store(false, Ordering::Release);
        self.status.peak_bits.store(0, Ordering::Relaxed);
        self.status.drum_energy_bits.store(0, Ordering::Relaxed);
        *self.status.started.lock().expect("started mutex poisoned") = None;
    }
}

fn build_loopback_stream(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    producer: HeapProd<StereoFrame>,
    status: Arc<SharedStatus>,
) -> Result<Stream> {
    macro_rules! stream_for {
        ($sample:ty) => {
            build_typed_stream::<$sample>(device, config.clone().into(), producer, status)
        };
    }

    match config.sample_format() {
        SampleFormat::F32 => stream_for!(f32),
        SampleFormat::F64 => stream_for!(f64),
        SampleFormat::I8 => stream_for!(i8),
        SampleFormat::I16 => stream_for!(i16),
        SampleFormat::I32 => stream_for!(i32),
        SampleFormat::I64 => stream_for!(i64),
        SampleFormat::U8 => stream_for!(u8),
        SampleFormat::U16 => stream_for!(u16),
        SampleFormat::U32 => stream_for!(u32),
        SampleFormat::U64 => stream_for!(u64),
        other => bail!("暂不支持输出设备的采样格式：{other}"),
    }
}

fn build_typed_stream<T>(
    device: &cpal::Device,
    config: StreamConfig,
    mut producer: HeapProd<StereoFrame>,
    status: Arc<SharedStatus>,
) -> Result<Stream>
where
    T: SizedSample + Sample + Copy,
    f32: FromSample<T>,
{
    let error_status = Arc::clone(&status);
    device
        .build_input_stream::<T, _, _>(
            config,
            move |samples, _| {
                let mut peak = 0.0_f32;
                let mut captured = 0_u64;
                let mut dropped = 0_u64;
                let (pairs, _) = samples.as_chunks::<2>();
                for pair in pairs {
                    let frame = [f32::from_sample(pair[0]), f32::from_sample(pair[1])];
                    peak = peak.max(frame[0].abs()).max(frame[1].abs());
                    if producer.try_push(frame).is_err() {
                        dropped += 1;
                    }
                    captured += 1;
                }
                status
                    .captured_frames
                    .fetch_add(captured, Ordering::Relaxed);
                status.dropped_frames.fetch_add(dropped, Ordering::Relaxed);
                status.peak_bits.store(peak.to_bits(), Ordering::Relaxed);
            },
            move |error| error_status.set_error(format!("系统音频流错误：{error}")),
            None,
        )
        .context("无法创建系统音频回环流；请在系统设置中允许 StemFlow 录制系统音频")
}

fn run_worker(
    mut consumer: HeapCons<StereoFrame>,
    stop: Arc<AtomicBool>,
    status: Arc<SharedStatus>,
    output_dir: PathBuf,
    input_rate: u32,
    mut model: Option<HsTasNet>,
) -> Result<()> {
    let mut original = create_writer(&output_dir.join("original.wav"), input_rate)?;
    let mut stem_writers = if model.is_some() {
        let stem_dir = output_dir.join("stems");
        fs::create_dir_all(&stem_dir)?;
        Some(StemWriters::new(&stem_dir)?)
    } else {
        None
    };
    let mut resampler = StereoLinearResampler::new(input_rate, MODEL_SAMPLE_RATE);
    let mut capture_buffer = vec![[0.0_f32; 2]; 4096];
    let mut resampled = Vec::with_capacity(4096);
    let mut pending = VecDeque::with_capacity(HOP_FRAMES * 4);

    loop {
        let read = consumer.pop_slice(&mut capture_buffer);
        if read == 0 {
            if stop.load(Ordering::Acquire) && consumer.is_empty() {
                break;
            }
            thread::sleep(Duration::from_millis(2));
            continue;
        }

        for frame in &capture_buffer[..read] {
            original.write_sample(frame[0])?;
            original.write_sample(frame[1])?;
        }

        if let (Some(active_model), Some(writers)) = (model.as_mut(), stem_writers.as_mut()) {
            resampled.clear();
            resampler.process(&capture_buffer[..read], &mut resampled);
            pending.extend(resampled.iter().copied());
            while pending.len() >= HOP_FRAMES {
                let hop = take_hop(&mut pending);
                match active_model.process(&hop) {
                    Ok(Some(output)) => write_separated(writers, &status, &output)?,
                    Ok(None) => {}
                    Err(error) => {
                        status.set_error(format!("实时分轨已停用，原始录音继续：{error:#}"));
                        status.drum_energy_bits.store(0, Ordering::Relaxed);
                        model = None;
                        stem_writers = None;
                        break;
                    }
                }
            }
        }
    }

    if let (Some(active_model), Some(writers)) = (model.as_mut(), stem_writers.as_mut()) {
        resampled.clear();
        resampler.finish(&mut resampled);
        pending.extend(resampled.iter().copied());
        if !pending.is_empty() {
            let mut padded = [[0.0_f32; 2]; HOP_FRAMES];
            for (index, frame) in pending.drain(..).enumerate() {
                padded[index] = frame;
            }
            if let Some(output) = active_model.process(&padded)? {
                write_separated(writers, &status, &output)?;
            }
        }
        if let Some(output) = active_model.flush()? {
            write_separated(writers, &status, &output)?;
        }
    }

    original.finalize()?;
    if let Some(writers) = stem_writers {
        writers.finalize()?;
    }
    Ok(())
}

fn take_hop(pending: &mut VecDeque<StereoFrame>) -> [StereoFrame; HOP_FRAMES] {
    let mut hop = [[0.0_f32; 2]; HOP_FRAMES];
    for frame in &mut hop {
        *frame = pending.pop_front().expect("hop length checked");
    }
    hop
}

fn create_writer(path: &Path, sample_rate: u32) -> Result<FloatWavWriter> {
    WavWriter::create(
        path,
        WavSpec {
            channels: 2,
            sample_rate,
            bits_per_sample: 32,
            sample_format: WavSampleFormat::Float,
        },
    )
    .with_context(|| format!("无法创建 WAV：{}", path.display()))
}

struct StemWriters {
    drums: FloatWavWriter,
    bass: FloatWavWriter,
    other: FloatWavWriter,
    vocals: FloatWavWriter,
}

impl StemWriters {
    fn new(directory: &Path) -> Result<Self> {
        Ok(Self {
            drums: create_writer(&directory.join("drums.wav"), MODEL_SAMPLE_RATE)?,
            bass: create_writer(&directory.join("bass.wav"), MODEL_SAMPLE_RATE)?,
            other: create_writer(&directory.join("other.wav"), MODEL_SAMPLE_RATE)?,
            vocals: create_writer(&directory.join("vocals.wav"), MODEL_SAMPLE_RATE)?,
        })
    }

    fn finalize(self) -> Result<()> {
        self.drums.finalize()?;
        self.bass.finalize()?;
        self.other.finalize()?;
        self.vocals.finalize()?;
        Ok(())
    }
}

fn write_separated(
    writers: &mut StemWriters,
    status: &SharedStatus,
    output: &SeparatedHop,
) -> Result<()> {
    for frame in 0..HOP_FRAMES {
        write_frame(&mut writers.drums, output.drums[frame])?;
        write_frame(&mut writers.bass, output.bass[frame])?;
        write_frame(&mut writers.other, output.other[frame])?;
        write_frame(&mut writers.vocals, output.vocals[frame])?;
    }
    status
        .separated_frames
        .fetch_add(HOP_FRAMES as u64, Ordering::Relaxed);
    status.last_inference_micros.store(
        output.inference_time.as_micros().min(u128::from(u64::MAX)) as u64,
        Ordering::Relaxed,
    );
    let drum_peak = status
        .waveform_history
        .lock()
        .expect("waveform mutex poisoned")
        .push(output);
    let previous = f32::from_bits(status.drum_energy_bits.load(Ordering::Relaxed));
    let envelope = drum_peak.max(previous * 0.985).clamp(0.0, 1.0);
    status
        .drum_energy_bits
        .store(envelope.to_bits(), Ordering::Relaxed);
    Ok(())
}

fn write_frame(writer: &mut FloatWavWriter, frame: StereoFrame) -> Result<()> {
    writer.write_sample(frame[0])?;
    writer.write_sample(frame[1])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waveform_history_buckets_hops_into_min_max_points() {
        let mut history = WaveformHistory::default();
        let output = SeparatedHop {
            drums: [[-0.25, 0.5]; HOP_FRAMES],
            bass: [[-0.1, 0.2]; HOP_FRAMES],
            other: [[-0.75, 0.8]; HOP_FRAMES],
            vocals: [[-0.4, 0.3]; HOP_FRAMES],
            inference_time: Duration::ZERO,
        };
        let mut drum_peak = 0.0;
        for _ in 0..WAVEFORM_BUCKET_HOPS {
            drum_peak = history.push(&output);
        }
        let snapshot = history.snapshot();
        assert_eq!(snapshot.drums, vec![[-0.25, 0.5]]);
        assert_eq!(snapshot.bass, vec![[-0.1, 0.2]]);
        assert_eq!(snapshot.other, vec![[-0.75, 0.8]]);
        assert_eq!(snapshot.vocals, vec![[-0.4, 0.3]]);
        assert_eq!(drum_peak, 0.5);
    }
}
