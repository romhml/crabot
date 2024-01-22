use tokio_stream::wrappers::ReceiverStream;

use std::str;

use tokio::sync::mpsc::channel;

use crate::{models::Pipeline, utils::sse::parse_event_stream};
use serde::{Deserialize, Serialize};
use serde_json;

pub struct MistralPipeline {}

#[derive(Debug, Serialize, Deserialize)]
struct MistralChatCompletion {
    id: String,
    object: String,
    created: i64,
    model: String,
    choices: Vec<MistralChoice>,
}

#[derive(Debug, Serialize, Deserialize)]
struct MistralChoice {
    index: usize,
    delta: MistralMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct MistralMessage {
    role: Option<String>,
    content: String,
}

impl Pipeline for MistralPipeline {
    fn run(&self, prompt: String) -> ReceiverStream<String> {
        let url = "https://api.mistral.ai/v1/chat/completions";
        let model = "mistral-tiny";

        let response = ureq::post(url)
            .set(
                "Authorization",
                &format!("Bearer {}", std::env::var("MISTRAL_API_KEY").unwrap()),
            )
            .send_json(ureq::json!({
                "model": model,
                "stream": true,
                "messages": [
                    {
                        "role": "user",
                        "content": prompt
                    }
                ]
            }))
            .unwrap();

        let reader = response.into_reader();
        let mut stream = parse_event_stream(reader);

        let (tx, rx) = channel::<String>(1024);

        tokio::spawn(async move {
            while let Some(event) = stream.recv().await {
                if event.data == "[DONE]\n" {
                    break;
                }

                if event.name == "message" {
                    let completion =
                        match serde_json::from_str::<MistralChatCompletion>(event.data.as_str()) {
                            Ok(c) => c,
                            Err(e) => {
                                tracing::error!(
                                    "Mistral: Could not deserialize event data:\n{}\n{}",
                                    event.data,
                                    e
                                );
                                continue;
                            }
                        };

                    for choice in completion.choices {
                        tx.send(choice.delta.content)
                            .await
                            .expect("Failed to send chunk to channel");
                    }
                }
            }
        });

        ReceiverStream::new(rx)
    }
}
