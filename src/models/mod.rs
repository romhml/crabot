use serde::{Deserialize, Serialize};
use tokio_stream::wrappers::ReceiverStream;

pub mod gpt;
pub mod lorem;
pub mod mistral;

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub enum ChatModel {
    #[default]
    #[serde(rename = "lorem")]
    Lorem,
    #[serde(rename = "gpt3")]
    GPT3,
    #[serde(rename = "mistral")]
    Mistral,
}

pub trait Pipeline {
    fn run(&self, prompt: String) -> ReceiverStream<String>;
}
