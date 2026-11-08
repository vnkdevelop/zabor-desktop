use crate::audio_settings::NoiseModel;
use df::tract::{DfParams, DfTract, RuntimeParams};
use ndarray::{ArrayView2, ArrayViewMut2};
use ort::environment::Environment;
use ort::logging::LogLevel;
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::TensorRef;
use std::sync::OnceLock;

const DEEPFILTER_MODEL: &[u8] = include_bytes!("../assets/models/DeepFilterNet3_onnx.tar.gz");
const SILERO_MODEL: &[u8] = include_bytes!("../assets/models/silero_vad.onnx");

#[derive(Debug, thiserror::Error)]
pub enum NeuralError {
    #[error("native audio model operation failed")]
    Processing,
    #[error("native audio model could not be loaded")]
    Load(String),
}

enum Engine {
    Rnnoise(rnnoise2::Denoiser),
    Deepfilter {
        model: Box<DfTract>,
        initial: Box<DfTract>,
    },
}

pub struct Denoiser {
    engine: Engine,
    input: [f32; 480],
}

impl Denoiser {
    pub fn new(model: NoiseModel, strength: f32, post_filter: f32) -> Result<Self, NeuralError> {
        if !strength.is_finite() || !post_filter.is_finite() {
            return Err(NeuralError::Processing);
        }
        let engine = match model {
            NoiseModel::Rnnoise => {
                if rnnoise2::Denoiser::frame_size() != 480 {
                    return Err(NeuralError::Processing);
                }
                Engine::Rnnoise(rnnoise2::Denoiser::new(None).ok_or(NeuralError::Processing)?)
            }
            NoiseModel::Deepfilter => {
                let params = DfParams::from_bytes(DEEPFILTER_MODEL)
                    .map_err(|error| NeuralError::Load(format!("{error:#}")))?;
                let runtime = RuntimeParams::default_with_ch(1)
                    .with_atten_lim(strength.clamp(5.0, 30.0))
                    .with_post_filter(post_filter.clamp(0.02, 0.05));
                let model = DfTract::new(params, &runtime)
                    .map_err(|error| NeuralError::Load(format!("{error:#}")))?;
                if model.sr != 48000 || model.hop_size != 480 || model.ch != 1 {
                    return Err(NeuralError::Processing);
                }
                Engine::Deepfilter {
                    initial: Box::new(model.clone()),
                    model: Box::new(model),
                }
            }
        };
        Ok(Self {
            engine,
            input: [0.0; 480],
        })
    }

    pub fn set_strength(&mut self, strength: f32, post_filter: f32) {
        if !strength.is_finite() || !post_filter.is_finite() {
            return;
        }
        if let Engine::Deepfilter { model, initial } = &mut self.engine {
            model.set_atten_lim(strength.clamp(5.0, 30.0));
            model.set_pf_beta(post_filter.clamp(0.02, 0.05));
            initial.set_atten_lim(strength.clamp(5.0, 30.0));
            initial.set_pf_beta(post_filter.clamp(0.02, 0.05));
        }
    }

    pub fn reset(&mut self) -> Result<(), NeuralError> {
        match &mut self.engine {
            Engine::Rnnoise(model) => {
                *model = rnnoise2::Denoiser::new(None).ok_or(NeuralError::Processing)?;
            }
            Engine::Deepfilter { model, initial } => {
                model.clone_from(initial);
            }
        }
        self.input.fill(0.0);
        Ok(())
    }

    pub fn process(
        &mut self,
        input: &[f32; 480],
        output: &mut [f32; 480],
    ) -> Result<Option<f32>, NeuralError> {
        if input.iter().any(|sample| !sample.is_finite()) {
            output.fill(0.0);
            return Err(NeuralError::Processing);
        }
        let probability = match &mut self.engine {
            Engine::Rnnoise(model) => {
                for (to, from) in self.input.iter_mut().zip(input) {
                    *to = *from * 32768.0;
                }
                let probability = model.process(&self.input, output);
                for sample in output.iter_mut() {
                    *sample /= 32768.0;
                }
                Some(probability.clamp(0.0, 1.0))
            }
            Engine::Deepfilter { model, .. } => {
                let input =
                    ArrayView2::from_shape((1, 480), input).map_err(|_| NeuralError::Processing)?;
                let output = ArrayViewMut2::from_shape((1, 480), output.as_mut_slice())
                    .map_err(|_| NeuralError::Processing)?;
                model
                    .process(input, output)
                    .map_err(|_| NeuralError::Processing)?;
                None
            }
        };
        if output.iter().any(|sample| !sample.is_finite()) {
            output.fill(0.0);
            return Err(NeuralError::Processing);
        }
        Ok(probability)
    }
}

pub struct Silero {
    session: Session,
    input: [f32; 576],
    state: [f32; 256],
    sequence: Option<u64>,
}

fn environment() -> Result<&'static Environment, NeuralError> {
    static ENVIRONMENT: OnceLock<Option<Environment>> = OnceLock::new();
    ENVIRONMENT
        .get_or_init(|| {
            ort::init()
                .with_name("zabor-native")
                .with_telemetry(false)
                .build()
                .ok()
        })
        .as_ref()
        .ok_or(NeuralError::Processing)
}

impl Silero {
    pub fn new() -> Result<Self, NeuralError> {
        let session = Session::builder(environment()?)
            .map_err(|_| NeuralError::Processing)?
            .with_intra_threads(1)
            .map_err(|_| NeuralError::Processing)?
            .with_inter_threads(1)
            .map_err(|_| NeuralError::Processing)?
            .with_log_level(LogLevel::Warning)
            .map_err(|_| NeuralError::Processing)?
            .with_optimization_level(GraphOptimizationLevel::All)
            .map_err(|_| NeuralError::Processing)?
            .commit_from_memory(SILERO_MODEL)
            .map_err(|error| NeuralError::Load(error.to_string()))?;
        Ok(Self {
            session,
            input: [0.0; 576],
            state: [0.0; 256],
            sequence: None,
        })
    }

    pub fn reset(&mut self) {
        self.input.fill(0.0);
        self.state.fill(0.0);
        self.sequence = None;
    }

    pub fn process(&mut self, samples: &[f32; 512], sequence: u64) -> Result<f32, NeuralError> {
        if self
            .sequence
            .is_some_and(|previous| previous.checked_add(1) != Some(sequence))
        {
            self.reset();
        }
        self.sequence = Some(sequence);
        if samples.iter().any(|sample| !sample.is_finite()) {
            self.reset();
            return Err(NeuralError::Processing);
        }
        let result = self.infer(samples);
        if result.is_err() {
            self.reset();
        }
        result
    }

    fn infer(&mut self, samples: &[f32; 512]) -> Result<f32, NeuralError> {
        for (target, sample) in self.input[64..].iter_mut().zip(samples) {
            let scaled = f64::from(*sample) * 6.5;
            let magnitude = scaled.abs();
            *target = if magnitude > 0.65 {
                (scaled.signum() * (0.65 + 0.33 * (magnitude - 0.65) / (0.33 + magnitude - 0.65)))
                    as f32
            } else {
                scaled as f32
            };
        }
        let input = TensorRef::from_array_view(([1, 576], &self.input[..]))
            .map_err(|_| NeuralError::Processing)?;
        let state = TensorRef::from_array_view(([2, 1, 128], &self.state[..]))
            .map_err(|_| NeuralError::Processing)?;
        let rate = [16000_i64];
        let rate =
            TensorRef::from_array_view(([1], &rate[..])).map_err(|_| NeuralError::Processing)?;
        let outputs = self
            .session
            .run(ort::inputs!["input" => input, "state" => state, "sr" => rate])
            .map_err(|_| NeuralError::Processing)?;
        let (_, probability) = outputs["output"]
            .try_extract_tensor::<f32>()
            .map_err(|_| NeuralError::Processing)?;
        let probability = *probability
            .first()
            .filter(|value| value.is_finite())
            .ok_or(NeuralError::Processing)?;
        let (_, state) = outputs["stateN"]
            .try_extract_tensor::<f32>()
            .map_err(|_| NeuralError::Processing)?;
        if state.len() != self.state.len() || state.iter().any(|value| !value.is_finite()) {
            return Err(NeuralError::Processing);
        }
        self.state.copy_from_slice(state);
        self.input.copy_within(512..576, 0);
        Ok(probability.clamp(0.0, 1.0))
    }
}

impl crate::smart_gate::NoiseProcessor for Denoiser {
    fn process(&mut self, input: &[f32; 480], output: &mut [f32; 480]) -> bool {
        Denoiser::process(self, input, output).is_ok()
    }
    fn configure(&mut self, strength: f32, post_filter: f32) {
        self.set_strength(strength, post_filter);
    }
    fn attenuation_limited(&self) -> bool {
        matches!(self.engine, Engine::Deepfilter { .. })
    }
    fn reset(&mut self) -> bool {
        Denoiser::reset(self).is_ok()
    }
}

impl crate::smart_gate::VoiceProbability for Silero {
    fn probability(&mut self, input: &[f32; 512], sequence: u64) -> Option<f32> {
        self.process(input, sequence).ok()
    }
    fn reset(&mut self) {
        Silero::reset(self);
    }
}
