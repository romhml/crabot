use tokio_stream::wrappers::ReceiverStream;

pub mod gpt;
pub mod lorem;

pub trait Pipeline {
    fn run(&self, prompt: String) -> ReceiverStream<String>;
}
