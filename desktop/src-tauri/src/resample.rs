use std::collections::VecDeque;

use crate::model::StereoFrame;

/// Small streaming stereo resampler for the live preview path. It deliberately
/// runs on the inference worker, never on the Core Audio callback.
pub struct StereoLinearResampler {
    step: f64,
    position: f64,
    buffered: VecDeque<StereoFrame>,
}

impl StereoLinearResampler {
    pub fn new(input_rate: u32, output_rate: u32) -> Self {
        Self {
            step: f64::from(input_rate) / f64::from(output_rate),
            position: 0.0,
            buffered: VecDeque::with_capacity(4096),
        }
    }

    pub fn process(&mut self, input: &[StereoFrame], output: &mut Vec<StereoFrame>) {
        if (self.step - 1.0).abs() < f64::EPSILON {
            output.extend_from_slice(input);
            return;
        }

        self.buffered.extend(input.iter().copied());
        self.emit_available(output);
    }

    /// Emits the final interpolated sample by extending the input with its
    /// last value. Call this once after the complete input stream is known.
    pub fn finish(&mut self, output: &mut Vec<StereoFrame>) {
        if (self.step - 1.0).abs() < f64::EPSILON || self.buffered.is_empty() {
            return;
        }
        let last = *self.buffered.back().expect("buffer checked as non-empty");
        self.buffered.push_back(last);
        self.emit_available(output);
        self.buffered.clear();
        self.position = 0.0;
    }

    fn emit_available(&mut self, output: &mut Vec<StereoFrame>) {
        while self.position + 1.0 < self.buffered.len() as f64 {
            let lower = self.position.floor() as usize;
            let fraction = (self.position - lower as f64) as f32;
            let a = self.buffered[lower];
            let b = self.buffered[lower + 1];
            output.push([
                a[0] + (b[0] - a[0]) * fraction,
                a[1] + (b[1] - a[1]) * fraction,
            ]);
            self.position += self.step;
        }

        let discard = self.position.floor() as usize;
        if discard > 0 {
            self.buffered.drain(..discard);
            self.position -= discard as f64;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_rate_is_bit_exact() {
        let input = [[0.1, -0.1], [0.2, -0.2], [0.3, -0.3]];
        let mut output = Vec::new();
        StereoLinearResampler::new(44_100, 44_100).process(&input, &mut output);
        assert_eq!(input.as_slice(), output.as_slice());
    }

    #[test]
    fn converts_48k_to_44k_near_expected_length() {
        let input = vec![[0.25, -0.25]; 4_800];
        let mut output = Vec::new();
        let mut resampler = StereoLinearResampler::new(48_000, 44_100);
        resampler.process(&input, &mut output);
        resampler.finish(&mut output);
        assert_eq!(output.len(), 4_410);
        assert!(output.iter().all(|frame| *frame == [0.25, -0.25]));
    }
}
