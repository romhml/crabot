use anyhow::{Error as E, Result as R};
use clap::{Parser, ValueEnum};

use candle_core::{DType, Device};
use candle_examples::token_output_stream::TokenOutputStream;
use candle_transformers::generation::LogitsProcessor;
use hf_hub::{api::sync::Api, Repo, RepoType};
use tokenizers::Tokenizer;

/// This follows the lines of:
/// https://github.com/johnma2006/mamba-minimal/blob/master/model.py
/// Simple, minimal implementation of Mamba in one file of PyTorch.
use candle_core::{IndexOp, Module, Result, Tensor, D};
use candle_nn::{RmsNorm, VarBuilder};

use candle_transformers::models::with_tracing::{linear, linear_no_bias, Linear};

struct TextGeneration {
    model: Model,
    device: Device,
    tokenizer: TokenOutputStream,
    logits_processor: LogitsProcessor,
    repeat_penalty: f32,
    repeat_last_n: usize,
}

impl TextGeneration {
    #[allow(clippy::too_many_arguments)]
    fn new(
        model: Model,
        tokenizer: Tokenizer,
        seed: u64,
        temp: Option<f64>,
        top_p: Option<f64>,
        repeat_penalty: f32,
        repeat_last_n: usize,
        device: &Device,
    ) -> Self {
        let logits_processor = LogitsProcessor::new(seed, temp, top_p);
        Self {
            model,
            tokenizer: TokenOutputStream::new(tokenizer),
            logits_processor,
            repeat_penalty,
            repeat_last_n,
            device: device.clone(),
        }
    }

    fn run(&mut self, prompt: &str, sample_len: usize) -> R<()> {
        use std::io::Write;
        self.tokenizer.clear();
        let mut tokens = self
            .tokenizer
            .tokenizer()
            .encode(prompt, true)
            .map_err(E::msg)?
            .get_ids()
            .to_vec();
        for &t in tokens.iter() {
            if let Some(t) = self.tokenizer.next_token(t)? {
                print!("{t}")
            }
        }
        std::io::stdout().flush()?;

        let mut generated_tokens = 0usize;
        let eos_token = match self.tokenizer.get_token("<|endoftext|>") {
            Some(token) => token,
            None => anyhow::bail!("cannot find the </s> token"),
        };
        let start_gen = std::time::Instant::now();
        for _ in 0..sample_len {
            let input = Tensor::new(tokens.as_slice(), &self.device)?.unsqueeze(0)?;
            let logits = self.model.forward(&input)?;
            let logits = logits.squeeze(0)?.squeeze(0)?.to_dtype(DType::F32)?;
            let logits = if self.repeat_penalty == 1. {
                logits
            } else {
                let start_at = tokens.len().saturating_sub(self.repeat_last_n);
                candle_transformers::utils::apply_repeat_penalty(
                    &logits,
                    self.repeat_penalty,
                    &tokens[start_at..],
                )?
            };

            let next_token = self.logits_processor.sample(&logits)?;
            tokens.push(next_token);
            generated_tokens += 1;
            if next_token == eos_token {
                break;
            }
            if let Some(t) = self.tokenizer.next_token(next_token)? {
                print!("{t}");
                std::io::stdout().flush()?;
            }
        }
        let dt = start_gen.elapsed();
        if let Some(rest) = self.tokenizer.decode_rest().map_err(E::msg)? {
            print!("{rest}");
        }
        std::io::stdout().flush()?;
        println!(
            "\n{generated_tokens} tokens generated ({:.2} token/s)",
            generated_tokens as f64 / dt.as_secs_f64(),
        );
        Ok(())
    }
}

#[derive(Parser, ValueEnum, Clone, Copy, PartialEq, Eq, Debug)]
enum Which {
    Mamba130m,
    Mamba370m,
    Mamba790m,
    Mamba1_4b,
    Mamba2_8b,
    Mamba2_8bSlimPj,
}

impl std::fmt::Display for Which {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl Which {
    fn model_id(&self) -> &'static str {
        match self {
            Self::Mamba130m => "state-spaces/mamba-130m",
            Self::Mamba370m => "state-spaces/mamba-370m",
            Self::Mamba790m => "state-spaces/mamba-790m",
            Self::Mamba1_4b => "state-spaces/mamba-1.4b",
            Self::Mamba2_8b => "state-spaces/mamba-2.8b",
            Self::Mamba2_8bSlimPj => "state-spaces/mamba-2.8b-slimpj'",
        }
    }

    fn revision(&self) -> &'static str {
        match self {
            Self::Mamba130m
            | Self::Mamba370m
            | Self::Mamba790m
            | Self::Mamba1_4b
            | Self::Mamba2_8bSlimPj => "refs/pr/1",
            Self::Mamba2_8b => "refs/pr/4",
        }
    }
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Run on CPU rather than on GPU.
    #[arg(long)]
    cpu: bool,

    /// Enable tracing (generates a trace-timestamp.json file).
    #[arg(long)]
    tracing: bool,

    #[arg(long)]
    prompt: String,

    /// The temperature used to generate samples.
    #[arg(long)]
    temperature: Option<f64>,

    /// Nucleus sampling probability cutoff.
    #[arg(long)]
    top_p: Option<f64>,

    /// The seed to use when generating random samples.
    #[arg(long, default_value_t = 299792458)]
    seed: u64,

    /// The length of the sample to generate (in tokens).
    #[arg(long, short = 'n', default_value_t = 5000)]
    sample_len: usize,

    #[arg(long, default_value = "mamba130m")]
    which: Which,

    #[arg(long)]
    model_id: Option<String>,

    #[arg(long)]
    revision: Option<String>,

    #[arg(long)]
    tokenizer_file: Option<String>,

    #[arg(long)]
    weight_files: Option<String>,

    #[arg(long)]
    config_file: Option<String>,

    /// Penalty to be applied for repeating tokens, 1. means no penalty.
    #[arg(long, default_value_t = 1.0)]
    repeat_penalty: f32,

    /// The context size to consider for the repeat penalty.
    #[arg(long, default_value_t = 8)]
    repeat_last_n: usize,
}

fn main() -> R<()> {
    use tracing_chrome::ChromeLayerBuilder;
    use tracing_subscriber::prelude::*;

    let args = Args::parse();
    let _guard = if args.tracing {
        let (chrome_layer, guard) = ChromeLayerBuilder::new().build();
        tracing_subscriber::registry().with(chrome_layer).init();
        Some(guard)
    } else {
        None
    };
    println!(
        "avx: {}, neon: {}, simd128: {}, f16c: {}",
        candle_core::utils::with_avx(),
        candle_core::utils::with_neon(),
        candle_core::utils::with_simd128(),
        candle_core::utils::with_f16c()
    );
    println!(
        "temp: {:.2} repeat-penalty: {:.2} repeat-last-n: {}",
        args.temperature.unwrap_or(0.),
        args.repeat_penalty,
        args.repeat_last_n
    );

    let start = std::time::Instant::now();
    let api = Api::new()?;
    let repo = api.repo(Repo::with_revision(
        args.model_id
            .unwrap_or_else(|| args.which.model_id().to_string()),
        RepoType::Model,
        args.revision
            .unwrap_or_else(|| args.which.revision().to_string()),
    ));
    let tokenizer_filename = match args.tokenizer_file {
        Some(file) => std::path::PathBuf::from(file),
        None => api
            .model("EleutherAI/gpt-neox-20b".to_string())
            .get("tokenizer.json")?,
    };
    let config_filename = match args.config_file {
        Some(file) => std::path::PathBuf::from(file),
        None => repo.get("config.json")?,
    };
    let filenames = match args.weight_files {
        Some(files) => files
            .split(',')
            .map(std::path::PathBuf::from)
            .collect::<Vec<_>>(),
        None => {
            vec![repo.get("model.safetensors")?]
        }
    };
    println!("retrieved the files in {:?}", start.elapsed());
    let tokenizer = Tokenizer::from_file(tokenizer_filename).map_err(E::msg)?;

    let start = std::time::Instant::now();
    let config: Config = serde_json::from_slice(&std::fs::read(config_filename)?)?;
    let device = candle_examples::device(args.cpu)?;
    let vb = unsafe { VarBuilder::from_mmaped_safetensors(&filenames, DType::F32, &device)? };
    let model = Model::new(&config, vb.pp("backbone"))?;
    println!("loaded the model in {:?}", start.elapsed());

    let mut pipeline = TextGeneration::new(
        model,
        tokenizer,
        args.seed,
        args.temperature,
        args.top_p,
        args.repeat_penalty,
        args.repeat_last_n,
        &device,
    );
    pipeline.run(&args.prompt, args.sample_len)?;
    Ok(())
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Config {
    d_model: usize,
    n_layer: usize,
    vocab_size: usize,
    pad_vocab_size_multiple: usize,
}

impl Config {
    fn vocab_size(&self) -> usize {
        let pad = self.pad_vocab_size_multiple;
        (self.vocab_size + pad - 1) / pad * pad
    }

    fn dt_rank(&self) -> usize {
        (self.d_model + 15) / 16
    }

    fn d_conv(&self) -> usize {
        4
    }

    fn d_state(&self) -> usize {
        16
    }

    fn d_inner(&self) -> usize {
        self.d_model * 2
    }
}

// https://github.com/johnma2006/mamba-minimal/blob/61f01953ca153f8c4a850d7111beecbf4be9cee1/model.py#L177
#[derive(Clone, Debug)]
pub struct MambaBlock {
    in_proj: Linear,
    conv1d: candle_nn::Conv1d,
    x_proj: Linear,
    dt_proj: Linear,
    a_log: Tensor,
    d: Tensor,
    out_proj: Linear,
    dt_rank: usize,
}

impl MambaBlock {
    pub fn new(cfg: &Config, vb: VarBuilder) -> Result<Self> {
        let d_inner = cfg.d_inner();
        let d_conv = cfg.d_conv();
        let d_state = cfg.d_state();
        let dt_rank = cfg.dt_rank();
        let in_proj = linear_no_bias(cfg.d_model, d_inner * 2, vb.pp("in_proj"))?;
        let conv_cfg = candle_nn::Conv1dConfig {
            groups: d_inner,
            padding: d_conv - 1,
            ..Default::default()
        };
        let conv1d = candle_nn::conv1d(d_inner, d_inner, d_conv, conv_cfg, vb.pp("conv1d"))?;
        let x_proj = linear_no_bias(d_inner, dt_rank + d_state * 2, vb.pp("x_proj"))?;
        let dt_proj = linear(dt_rank, d_inner, vb.pp("dt_proj"))?;
        let a_log = vb.get((d_inner, d_state), "A_log")?;
        let d = vb.get(d_inner, "D")?;
        let out_proj = linear_no_bias(d_inner, cfg.d_model, vb.pp("out_proj"))?;
        Ok(Self {
            in_proj,
            conv1d,
            x_proj,
            dt_proj,
            a_log,
            d,
            out_proj,
            dt_rank,
        })
    }

    fn ssm(&self, xs: &Tensor) -> Result<Tensor> {
        let (_d_in, n) = self.a_log.dims2()?;
        let a = self.a_log.to_dtype(candle_core::DType::F32)?.exp()?.neg()?;
        let d = self.d.to_dtype(candle_core::DType::F32)?;
        let x_dbl = xs.apply(&self.x_proj)?;
        let delta = x_dbl.narrow(D::Minus1, 0, self.dt_rank)?;
        let b = x_dbl.narrow(D::Minus1, self.dt_rank, n)?;
        let c = x_dbl.narrow(D::Minus1, self.dt_rank + n, n)?;
        let delta = delta.contiguous()?.apply(&self.dt_proj)?;
        // softplus without threshold
        let delta = (delta.exp()? + 1.)?.log()?;
        let ss = selective_scan(xs, &delta, &a, &b, &c, &d)?;
        Ok(ss)
    }
}

// https://github.com/johnma2006/mamba-minimal/blob/61f01953ca153f8c4a850d7111beecbf4be9cee1/model.py#L275
fn selective_scan(
    u: &Tensor,
    delta: &Tensor,
    a: &Tensor,
    b: &Tensor,
    c: &Tensor,
    d: &Tensor,
) -> Result<Tensor> {
    let (b_sz, l, d_in) = u.dims3()?;
    let n = a.dim(1)?;
    let delta = delta.t()?.reshape((b_sz, d_in, l, 1))?; // b d_in l 1
    let delta_a = delta.broadcast_mul(&a.reshape((1, d_in, 1, n))?)?.exp()?;
    let delta_b_u = delta
        .broadcast_mul(&b.reshape((b_sz, 1, l, n))?)?
        .broadcast_mul(&u.t()?.reshape((b_sz, d_in, l, 1))?)?;
    let mut xs = Tensor::zeros((b_sz, d_in, n), delta_a.dtype(), delta_a.device())?;
    let mut ys = Vec::with_capacity(l);
    for i in 0..l {
        xs = ((delta_a.i((.., .., i))? * xs)? + delta_b_u.i((.., .., i))?)?;
        let y = xs.matmul(&c.i((.., i, ..))?.unsqueeze(2)?)?.squeeze(2)?;
        ys.push(y)
    }
    let ys = Tensor::stack(ys.as_slice(), 1)?;
    ys + u.broadcast_mul(d)
}

impl Module for MambaBlock {
    // https://github.com/johnma2006/mamba-minimal/blob/61f01953ca153f8c4a850d7111beecbf4be9cee1/model.py#L206
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let (_b_sz, seq_len, _dim) = xs.dims3()?;
        let xs_and_res = xs.apply(&self.in_proj)?.chunk(2, D::Minus1)?;
        let (xs, res) = (&xs_and_res[0], &xs_and_res[1]);
        let xs = xs
            .t()?
            .apply(&self.conv1d)?
            .narrow(D::Minus1, 0, seq_len)?
            .t()?;
        let xs = candle_nn::ops::silu(&xs)?;
        let ys = (self.ssm(&xs)? * candle_nn::ops::silu(res))?;
        ys.apply(&self.out_proj)
    }
}

// https://github.com/johnma2006/mamba-minimal/blob/61f01953ca153f8c4a850d7111beecbf4be9cee1/model.py#L143
#[derive(Clone, Debug)]
pub struct ResidualBlock {
    mixer: MambaBlock,
    norm: RmsNorm,
}

impl ResidualBlock {
    pub fn new(cfg: &Config, vb: VarBuilder) -> Result<Self> {
        let norm = candle_nn::rms_norm(cfg.d_model, 1e-5, vb.pp("norm"))?;
        let mixer = MambaBlock::new(cfg, vb.pp("mixer"))?;
        Ok(Self { mixer, norm })
    }
}

impl Module for ResidualBlock {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        xs.apply(&self.norm)?.apply(&self.mixer)? + xs
    }
}

// https://github.com/johnma2006/mamba-minimal/blob/61f01953ca153f8c4a850d7111beecbf4be9cee1/model.py#L56
#[derive(Clone, Debug)]
pub struct Model {
    embedding: candle_nn::Embedding,
    layers: Vec<ResidualBlock>,
    norm_f: RmsNorm,
    lm_head: Linear,
}

impl Model {
    pub fn new(cfg: &Config, vb: VarBuilder) -> Result<Self> {
        let embedding = candle_nn::embedding(cfg.vocab_size(), cfg.d_model, vb.pp("embedding"))?;
        let mut layers = Vec::with_capacity(cfg.n_layer);
        let vb_l = vb.pp("layers");
        for layer_idx in 0..cfg.n_layer {
            let layer = ResidualBlock::new(cfg, vb_l.pp(layer_idx))?;
            layers.push(layer)
        }
        let norm_f = candle_nn::rms_norm(cfg.d_model, 1e-5, vb.pp("norm_f"))?;
        let lm_head = Linear::from_weights(embedding.embeddings().clone(), None);
        Ok(Self {
            embedding,
            layers,
            norm_f,
            lm_head,
        })
    }
}

impl Module for Model {
    fn forward(&self, input_ids: &Tensor) -> Result<Tensor> {
        let (_b_size, seq_len) = input_ids.dims2()?;
        let mut xs = self.embedding.forward(input_ids)?;
        for layer in self.layers.iter() {
            xs = layer.forward(&xs)?
        }
        xs.narrow(1, seq_len - 1, 1)?
            .apply(&self.norm_f)?
            .apply(&self.lm_head)
    }
}
