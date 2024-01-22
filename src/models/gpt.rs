use tokio_stream::wrappers::ReceiverStream;

use std::str;

use tokio::sync::mpsc::channel;

use crate::{models::Pipeline, utils::sse::parse_event_stream};
use serde::{Deserialize, Serialize};
use serde_json;

pub struct GPT3Pipeline {}

#[derive(Debug, Serialize, Deserialize)]
struct GPT3ChatCompletion {
    id: String,
    object: String,
    created: i64,
    model: String,
    choices: Vec<GPT3Choice>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GPT3Choice {
    index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    delta: Option<GPT3Delta>,
    finish_reason: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum GPT3Delta {
    Simple(String),
    Complex(ComplexDelta),
    Empty(EmptyDelta),
}

#[derive(Debug, Serialize, Deserialize)]
struct ComplexDelta {
    content: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct EmptyDelta {}

impl Pipeline for GPT3Pipeline {
    fn run(&self, prompt: String) -> ReceiverStream<String> {
        let url = "https://api.openai.com/v1/chat/completions";
        let model = "gpt-3.5-turbo";

        let response = ureq::post(url)
            .set(
                "Authorization",
                &format!("Bearer {}", std::env::var("OPENAI_API_KEY").unwrap()),
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
                        match serde_json::from_str::<GPT3ChatCompletion>(event.data.as_str()) {
                            Ok(c) => c,
                            Err(e) => {
                                tracing::error!(
                                    "GPT: Could not deserialize event data:\n{}\n{}",
                                    event.data,
                                    e
                                );
                                continue;
                            }
                        };

                    for choice in completion.choices {
                        let msg = match choice.delta {
                            Some(GPT3Delta::Simple(v)) => v,
                            Some(GPT3Delta::Complex(c)) => c.content,
                            _ => continue,
                        };
                        tx.send(msg).await.expect("Failed to send chunk to channel");
                    }
                }
            }
        });

        ReceiverStream::new(rx)
    }
}
