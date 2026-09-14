use std::{
    collections::VecDeque,
    fs::{self, File},
    io::BufWriter,
    path::Path,
};

use anyhow::{bail, Context, Result};
use hound::{SampleFormat as WavSampleFormat, WavSpec, WavWriter};
use symphonia::core::{
    audio::SampleBuffer,
    codecs::DecoderOptions,
    errors::Error as SymphoniaError,
    formats::FormatOptions,
    io::{MediaSourceStream, MediaSourceStreamOptions},
    meta::MetadataOptions,
    probe::Hint,
};
use symphonia::default::{get_codecs, get_probe};

use crate::{
    model::{HsTasNet, SeparatedHop, StereoFrame, HOP_FRAMES, MODEL_SAMPLE_RATE},
    resample::StereoLinearResampler,
};

type FloatWavWriter = WavWriter<BufWriter<File>>;

#[derive(Debug, Clone, Copy)]
pub struct SeparationProgress {
    pub processed_input_frames: u64,
    pub total_input_frames: Option<u64>,
    pub sample_rate: u32,
}

pub fn separate_audio_file(
    input_path: &Path,
    output_dir: &Path,
    model_path: &Path,
    mut report_progress: impl FnMut(SeparationProgress),
) -> Result<()> {
    if !input_path.is_file() {
        bail!("音频文件不存在：{}", input_path.display());
    }
    fs::create_dir_all(output_dir)
        .with_context(|| format!("无法创建输出目录：{}", output_dir.display()))?;

    let source = File::open(input_path)
        .with_context(|| format!("无法打开音频文件：{}", input_path.display()))?;
    let stream = MediaSourceStream::new(Box::new(source), MediaSourceStreamOptions::default());
    let mut hint = Hint::new();
    if let Some(extension) = input_path.extension().and_then(|value| value.to_str()) {
        hint.with_extension(extension);
    }
    let probed = get_probe()
        .format(
            &hint,
            stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .context("无法识别音频文件格式")?;
    let mut format = probed.format;
    let (track_id, codec_params) = {
        let track = format
            .default_track()
            .context("音频文件中没有可解码的音轨")?;
        (track.id, track.codec_params.clone())
    };
    let input_rate = codec_params.sample_rate.context("音频文件没有采样率信息")?;
    let total_input_frames = codec_params.n_frames;
    let mut decoder = get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .context("不支持该音频编码")?;
    let mut model = HsTasNet::load(model_path)?;
    let mut writers = StemWriters::new(output_dir)?;
    let mut resampler = StereoLinearResampler::new(input_rate, MODEL_SAMPLE_RATE);
    let mut resampled = Vec::with_capacity(8192);
    let mut pending = VecDeque::with_capacity(HOP_FRAMES * 4);
    let mut processed_input_frames = 0_u64;
    let mut next_progress_frame = 0_u64;
    let mut submitted_hops = 0_u64;

    report_progress(SeparationProgress {
        processed_input_frames,
        total_input_frames,
        sample_rate: input_rate,
    });

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(error) => return Err(error).context("读取音频数据失败"),
        };
        if packet.track_id() != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(error) => return Err(error).context("解码音频数据失败"),
        };
        let channels = decoded.spec().channels.count();
        let mut samples = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
        samples.copy_interleaved_ref(decoded);
        let frames = append_stereo(samples.samples(), channels, &mut resampled);
        processed_input_frames += frames as u64;

        let mut converted = Vec::with_capacity(resampled.len());
        resampler.process(&resampled, &mut converted);
        resampled.clear();
        pending.extend(converted);
        process_complete_hops(&mut pending, &mut model, &mut writers, &mut submitted_hops)?;

        if processed_input_frames >= next_progress_frame {
            report_progress(SeparationProgress {
                processed_input_frames,
                total_input_frames,
                sample_rate: input_rate,
            });
            next_progress_frame = processed_input_frames.saturating_add(u64::from(input_rate));
        }
    }

    let mut final_samples = Vec::new();
    resampler.finish(&mut final_samples);
    pending.extend(final_samples);

    // Container decoders and floating-point resampler boundaries can differ
    // by one frame. Normalize to the duration implied by the decoded source
    // before padding the final model hop.
    let target_output_frames = ((u128::from(processed_input_frames)
        * u128::from(MODEL_SAMPLE_RATE)
        + u128::from(input_rate / 2))
        / u128::from(input_rate)) as u64;
    let already_submitted_frames = submitted_hops * HOP_FRAMES as u64;
    let target_pending_frames =
        usize::try_from(target_output_frames.saturating_sub(already_submitted_frames))
            .context("音频文件过长，无法分配尾部缓冲区")?;
    pending.truncate(target_pending_frames);
    let padding = pending.back().copied().unwrap_or([0.0, 0.0]);
    while pending.len() < target_pending_frames {
        pending.push_back(padding);
    }
    process_complete_hops(&mut pending, &mut model, &mut writers, &mut submitted_hops)?;

    let tail_frames = pending.len();
    if tail_frames > 0 {
        let mut padded = [[0.0_f32; 2]; HOP_FRAMES];
        for (index, frame) in pending.drain(..).enumerate() {
            padded[index] = frame;
        }
        submitted_hops += 1;
        if let Some(output) = model.process(&padded)? {
            writers.write_hop(&output, HOP_FRAMES)?;
        }
    }

    if submitted_hops > 0 {
        if let Some(output) = model.flush()? {
            writers.write_hop(
                &output,
                if tail_frames > 0 {
                    tail_frames
                } else {
                    HOP_FRAMES
                },
            )?;
        }
    }
    writers.finalize()?;

    report_progress(SeparationProgress {
        processed_input_frames,
        total_input_frames: Some(total_input_frames.unwrap_or(processed_input_frames)),
        sample_rate: input_rate,
    });
    Ok(())
}

pub fn create_accompaniment_mix(
    output_dir: &Path,
    excluded_stem: &str,
) -> Result<std::path::PathBuf> {
    const STEMS: [&str; 4] = ["drums", "bass", "other", "vocals"];
    if !STEMS.contains(&excluded_stem) {
        bail!("未知声部 '{excluded_stem}'，可选值：drums, bass, other, vocals");
    }

    let mut spec = None;
    let mut merged: Option<Vec<f32>> = None;
    for name in STEMS {
        if name == excluded_stem {
            continue;
        }
        let path = output_dir.join(format!("{name}.wav"));
        let reader = hound::WavReader::open(&path)
            .with_context(|| format!("无法读取分轨结果：{}", path.display()))?;
        let current_spec = reader.spec();
        if current_spec.sample_format != WavSampleFormat::Float
            || current_spec.bits_per_sample != 32
        {
            bail!("分轨结果不是 32-bit float WAV：{}", path.display());
        }
        if let Some(expected) = spec {
            if current_spec != expected {
                bail!("分轨结果 WAV 格式不一致");
            }
        } else {
            spec = Some(current_spec);
        }
        let samples = reader
            .into_samples::<f32>()
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if let Some(target) = merged.as_mut() {
            if target.len() != samples.len() {
                bail!("分轨结果 WAV 长度不一致");
            }
            for (target, sample) in target.iter_mut().zip(samples) {
                *target += sample;
            }
        } else {
            merged = Some(samples);
        }
    }

    let output_path = output_dir.join(format!("accompaniment-without-{excluded_stem}.wav"));
    let mut writer = WavWriter::create(&output_path, spec.context("没有可合并的分轨")?)?;
    for sample in merged.unwrap_or_default() {
        writer.write_sample(sample)?;
    }
    writer.finalize()?;
    Ok(output_path)
}

fn append_stereo(samples: &[f32], channels: usize, output: &mut Vec<StereoFrame>) -> usize {
    if channels == 0 {
        return 0;
    }
    let before = output.len();
    for frame in samples.chunks_exact(channels) {
        output.push(if channels == 1 {
            [frame[0], frame[0]]
        } else {
            [frame[0], frame[1]]
        });
    }
    output.len() - before
}

fn process_complete_hops(
    pending: &mut VecDeque<StereoFrame>,
    model: &mut HsTasNet,
    writers: &mut StemWriters,
    submitted_hops: &mut u64,
) -> Result<()> {
    while pending.len() >= HOP_FRAMES {
        let mut hop = [[0.0_f32; 2]; HOP_FRAMES];
        for frame in &mut hop {
            *frame = pending.pop_front().expect("hop length checked");
        }
        *submitted_hops += 1;
        if let Some(output) = model.process(&hop)? {
            writers.write_hop(&output, HOP_FRAMES)?;
        }
    }
    Ok(())
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
            drums: create_writer(&directory.join("drums.wav"))?,
            bass: create_writer(&directory.join("bass.wav"))?,
            other: create_writer(&directory.join("other.wav"))?,
            vocals: create_writer(&directory.join("vocals.wav"))?,
        })
    }

    fn write_hop(&mut self, output: &SeparatedHop, frames: usize) -> Result<()> {
        for frame in 0..frames {
            write_frame(&mut self.drums, output.drums[frame])?;
            write_frame(&mut self.bass, output.bass[frame])?;
            write_frame(&mut self.other, output.other[frame])?;
            write_frame(&mut self.vocals, output.vocals[frame])?;
        }
        Ok(())
    }

    fn finalize(self) -> Result<()> {
        self.drums.finalize()?;
        self.bass.finalize()?;
        self.other.finalize()?;
        self.vocals.finalize()?;
        Ok(())
    }
}

fn create_writer(path: &Path) -> Result<FloatWavWriter> {
    WavWriter::create(
        path,
        WavSpec {
            channels: 2,
            sample_rate: MODEL_SAMPLE_RATE,
            bits_per_sample: 32,
            sample_format: WavSampleFormat::Float,
        },
    )
    .with_context(|| format!("无法创建 WAV：{}", path.display()))
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
    fn mono_samples_are_duplicated() {
        let mut output = Vec::new();
        assert_eq!(append_stereo(&[0.1, 0.2], 1, &mut output), 2);
        assert_eq!(output, vec![[0.1, 0.1], [0.2, 0.2]]);
    }

    #[test]
    fn stereo_samples_remain_interleaved() {
        let mut output = Vec::new();
        assert_eq!(append_stereo(&[0.1, -0.1, 0.2, -0.2], 2, &mut output), 2);
        assert_eq!(output, vec![[0.1, -0.1], [0.2, -0.2]]);
    }

    #[test]
    fn accompaniment_mix_preserves_stems_and_excludes_requested_track() {
        let root = std::env::temp_dir().join(format!("stemflow-solo-test-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let spec = WavSpec {
            channels: 2,
            sample_rate: MODEL_SAMPLE_RATE,
            bits_per_sample: 32,
            sample_format: WavSampleFormat::Float,
        };
        for (name, value) in [
            ("drums", 0.1_f32),
            ("bass", 0.2),
            ("other", 0.3),
            ("vocals", 0.4),
        ] {
            let mut writer = WavWriter::create(root.join(format!("{name}.wav")), spec).unwrap();
            writer.write_sample(value).unwrap();
            writer.write_sample(value).unwrap();
            writer.finalize().unwrap();
        }

        let output = create_accompaniment_mix(&root, "vocals").unwrap();
        for stem in ["drums", "bass", "other", "vocals"] {
            assert!(root.join(format!("{stem}.wav")).is_file());
        }
        let merged = hound::WavReader::open(output)
            .unwrap()
            .into_samples::<f32>()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        assert!((merged[0] - 0.6).abs() < 1e-6);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "requires the 106 MB model; set STEMSCORE_HSTASNET_MODEL"]
    fn separates_a_real_wav_end_to_end() {
        let model_path = std::env::var_os("STEMSCORE_HSTASNET_MODEL")
            .map(std::path::PathBuf::from)
            .expect("STEMSCORE_HSTASNET_MODEL is required");
        let root = std::env::temp_dir().join(format!(
            "stemscore-file-separation-test-{}",
            std::process::id()
        ));
        let input = root.join("input.wav");
        let output = root.join("output");
        fs::create_dir_all(&root).expect("temporary directory should be created");

        let mut writer = WavWriter::create(
            &input,
            WavSpec {
                channels: 2,
                sample_rate: 48_000,
                bits_per_sample: 32,
                sample_format: WavSampleFormat::Float,
            },
        )
        .expect("test WAV should be created");
        for index in 0..4_800 {
            let sample = (index as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.1;
            writer.write_sample(sample).unwrap();
            writer.write_sample(sample).unwrap();
        }
        writer.finalize().unwrap();

        separate_audio_file(&input, &output, &model_path, |_| {})
            .expect("end-to-end file separation should succeed");
        for stem in ["drums", "bass", "other", "vocals"] {
            let path = output.join(format!("{stem}.wav"));
            let mut reader = hound::WavReader::open(path).expect("stem WAV should be readable");
            assert_eq!(reader.spec().sample_rate, MODEL_SAMPLE_RATE);
            assert_eq!(reader.samples::<f32>().count(), 4_410 * 2);
        }

        fs::remove_dir_all(root).expect("temporary directory should be removed");
    }
}
