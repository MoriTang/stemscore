use std::time::{Duration, Instant};
use std::{fs::File, io::Read, path::Path};

use anyhow::{bail, Context, Result};
use ort::{
    inputs,
    session::{builder::GraphOptimizationLevel, Session},
    value::TensorRef,
};
use sha2::{Digest, Sha256};

pub const MODEL_SAMPLE_RATE: u32 = 44_100;
pub const HOP_FRAMES: usize = 128;
const CHANNELS: usize = 2;
const STEMS: usize = 4;
const HISTORY_SAMPLES: usize = CHANNELS * 896;
const HIDDEN_SAMPLES: usize = 2 * 1000;
const TAIL_SAMPLES: usize = STEMS * CHANNELS * HOP_FRAMES;

pub const MODEL_URL: &str = "https://media.githubusercontent.com/media/sweetspotsoundsystem/stemgen-rt/main/model/model.onnx";
pub const MODEL_SHA256: &str = "b8574ac2e67bcd1df533e3fc7464c0659d4bfa6744cfbb4389c0967271594fe3";
pub const MODEL_SIZE: u64 = 111_344_465;

pub type StereoFrame = [f32; 2];

#[derive(Debug)]
pub struct SeparatedHop {
    pub drums: [StereoFrame; HOP_FRAMES],
    pub bass: [StereoFrame; HOP_FRAMES],
    pub other: [StereoFrame; HOP_FRAMES],
    pub vocals: [StereoFrame; HOP_FRAMES],
    pub inference_time: Duration,
}

pub struct HsTasNet {
    session: Session,
    history: Vec<f32>,
    hidden: Vec<f32>,
    spectral_tail: Vec<f32>,
    waveform_tail: Vec<f32>,
    previous_input: [StereoFrame; HOP_FRAMES],
    primed: bool,
}

impl HsTasNet {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.is_file() {
            bail!("实时模型不存在：{}", path.display());
        }
        if path.metadata()?.len() != MODEL_SIZE {
            bail!("实时模型大小不正确，请重新下载");
        }
        verify_sha256(path)?;

        let session = Session::builder()
            .context("无法初始化 ONNX Runtime")?
            .with_intra_threads(1)
            .map_err(|error| anyhow::anyhow!("无法设置 ONNX intra-op 线程数：{error}"))?
            .with_inter_threads(1)
            .map_err(|error| anyhow::anyhow!("无法设置 ONNX inter-op 线程数：{error}"))?
            .with_parallel_execution(false)
            .map_err(|error| anyhow::anyhow!("无法设置顺序推理：{error}"))?
            .with_intra_op_spinning(false)
            .map_err(|error| anyhow::anyhow!("无法关闭 ONNX 线程自旋：{error}"))?
            .with_inter_op_spinning(false)
            .map_err(|error| anyhow::anyhow!("无法关闭 ONNX 线程自旋：{error}"))?
            .with_optimization_level(GraphOptimizationLevel::All)
            .map_err(|error| anyhow::anyhow!("无法配置 ONNX 图优化：{error}"))?
            .commit_from_file(path)
            .with_context(|| format!("无法加载实时模型：{}", path.display()))?;

        let expected_inputs = [
            "audio_chunk",
            "audio_history",
            "fusion_hidden",
            "spectral_numerator_tail",
            "waveform_tail",
        ];
        let expected_outputs = [
            "separated_chunk",
            "next_audio_history",
            "next_fusion_hidden",
            "next_spectral_numerator_tail",
            "next_waveform_tail",
        ];
        let actual_inputs: Vec<_> = session.inputs().iter().map(|item| item.name()).collect();
        let actual_outputs: Vec<_> = session.outputs().iter().map(|item| item.name()).collect();
        if actual_inputs != expected_inputs || actual_outputs != expected_outputs {
            bail!("ONNX 模型接口不兼容，拒绝启动实时分轨");
        }

        Ok(Self {
            session,
            history: vec![0.0; HISTORY_SAMPLES],
            hidden: vec![0.0; HIDDEN_SAMPLES],
            spectral_tail: vec![0.0; TAIL_SAMPLES],
            waveform_tail: vec![0.0; TAIL_SAMPLES],
            previous_input: [[0.0; 2]; HOP_FRAMES],
            primed: false,
        })
    }

    /// Runs one 128-frame hop. The graph returns the preceding input hop, so
    /// the first call after reset intentionally returns `None`.
    pub fn process(&mut self, input: &[StereoFrame; HOP_FRAMES]) -> Result<Option<SeparatedHop>> {
        let mut planar = vec![0.0_f32; CHANNELS * HOP_FRAMES];
        for (frame_index, frame) in input.iter().enumerate() {
            planar[frame_index] = frame[0];
            planar[HOP_FRAMES + frame_index] = frame[1];
        }

        let started = Instant::now();
        let (separated, next_history, next_hidden, next_spectral, next_waveform) = {
            let audio = TensorRef::from_array_view(([1_usize, 2, HOP_FRAMES], planar.as_slice()))?;
            let history = TensorRef::from_array_view(([1_usize, 2, 896], self.history.as_slice()))?;
            let hidden = TensorRef::from_array_view(([2_usize, 1, 1000], self.hidden.as_slice()))?;
            let spectral = TensorRef::from_array_view((
                [1_usize, STEMS, CHANNELS, HOP_FRAMES],
                self.spectral_tail.as_slice(),
            ))?;
            let waveform = TensorRef::from_array_view((
                [1_usize, STEMS, CHANNELS, HOP_FRAMES],
                self.waveform_tail.as_slice(),
            ))?;

            let outputs = self.session.run(inputs! {
                "audio_chunk" => audio,
                "audio_history" => history,
                "fusion_hidden" => hidden,
                "spectral_numerator_tail" => spectral,
                "waveform_tail" => waveform,
            })?;

            (
                outputs["separated_chunk"]
                    .try_extract_tensor::<f32>()?
                    .1
                    .to_vec(),
                outputs["next_audio_history"]
                    .try_extract_tensor::<f32>()?
                    .1
                    .to_vec(),
                outputs["next_fusion_hidden"]
                    .try_extract_tensor::<f32>()?
                    .1
                    .to_vec(),
                outputs["next_spectral_numerator_tail"]
                    .try_extract_tensor::<f32>()?
                    .1
                    .to_vec(),
                outputs["next_waveform_tail"]
                    .try_extract_tensor::<f32>()?
                    .1
                    .to_vec(),
            )
        };
        let inference_time = started.elapsed();

        if separated.len() != TAIL_SAMPLES
            || next_history.len() != HISTORY_SAMPLES
            || next_hidden.len() != HIDDEN_SAMPLES
            || next_spectral.len() != TAIL_SAMPLES
            || next_waveform.len() != TAIL_SAMPLES
        {
            bail!("ONNX 模型返回了意外的张量大小");
        }

        self.history = next_history;
        self.hidden = next_hidden;
        self.spectral_tail = next_spectral;
        self.waveform_tail = next_waveform;

        let was_primed = self.primed;
        self.primed = true;
        let aligned_input = self.previous_input;
        self.previous_input = *input;
        if !was_primed {
            return Ok(None);
        }

        // Graph order: drums, bass, vocals, other. Reconstruct Other from the
        // aligned dry mixture so the four recorded files sum to the original.
        let mut hop = SeparatedHop {
            drums: [[0.0; 2]; HOP_FRAMES],
            bass: [[0.0; 2]; HOP_FRAMES],
            other: [[0.0; 2]; HOP_FRAMES],
            vocals: [[0.0; 2]; HOP_FRAMES],
            inference_time,
        };
        for frame in 0..HOP_FRAMES {
            for channel in 0..CHANNELS {
                let at =
                    |stem: usize| separated[((stem * CHANNELS + channel) * HOP_FRAMES) + frame];
                hop.drums[frame][channel] = finite_or_zero(at(0));
                hop.bass[frame][channel] = finite_or_zero(at(1));
                hop.vocals[frame][channel] = finite_or_zero(at(2));
                hop.other[frame][channel] = finite_or_zero(
                    aligned_input[frame][channel]
                        - hop.drums[frame][channel]
                        - hop.bass[frame][channel]
                        - hop.vocals[frame][channel],
                );
            }
        }
        Ok(Some(hop))
    }

    pub fn flush(&mut self) -> Result<Option<SeparatedHop>> {
        self.process(&[[0.0; 2]; HOP_FRAMES])
    }
}

fn verify_sha256(path: &Path) -> Result<()> {
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
    let actual = format!("{:x}", hasher.finalize());
    if actual != MODEL_SHA256 {
        bail!("实时模型 SHA-256 不匹配，请重新下载");
    }
    Ok(())
}

fn finite_or_zero(value: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_finite_model_values_are_sanitized() {
        assert_eq!(finite_or_zero(f32::NAN), 0.0);
        assert_eq!(finite_or_zero(f32::INFINITY), 0.0);
        assert_eq!(finite_or_zero(-0.25), -0.25);
    }

    #[test]
    #[ignore = "requires the 106 MB model; set STEMSCORE_HSTASNET_MODEL"]
    fn real_model_contract_and_state_smoke_test() {
        let path = std::env::var_os("STEMSCORE_HSTASNET_MODEL")
            .map(std::path::PathBuf::from)
            .expect("STEMSCORE_HSTASNET_MODEL is required");
        let mut model = HsTasNet::load(&path).expect("model should load");
        assert!(model
            .process(&[[0.0; 2]; HOP_FRAMES])
            .expect("first inference should succeed")
            .is_none());
        let output = model
            .process(&[[0.0; 2]; HOP_FRAMES])
            .expect("second inference should succeed")
            .expect("second inference should emit the previous hop");
        for stem in [&output.drums, &output.bass, &output.other, &output.vocals] {
            assert!(stem.iter().flatten().all(|sample| sample.is_finite()));
        }

        let measured_hops = 32_u32;
        let mut inference_time = Duration::ZERO;
        for _ in 0..measured_hops {
            inference_time += model
                .process(&[[0.0; 2]; HOP_FRAMES])
                .expect("continuous inference should succeed")
                .expect("primed model should emit every hop")
                .inference_time;
        }
        let audio_time = Duration::from_secs_f64(
            f64::from(measured_hops) * HOP_FRAMES as f64 / f64::from(MODEL_SAMPLE_RATE),
        );
        eprintln!(
            "average inference {:.3} ms/hop; real-time factor {:.2}x",
            inference_time.as_secs_f64() * 1000.0 / f64::from(measured_hops),
            inference_time.as_secs_f64() / audio_time.as_secs_f64(),
        );
    }
}
