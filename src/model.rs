use std::{thread, time::Duration};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

use std::str;

use std::io::Read;
use tokio::sync::mpsc::{channel, Receiver};

use serde::{Deserialize, Serialize};

pub trait Pipeline {
    fn run(&self, prompt: String) -> ReceiverStream<String>;
}

pub struct LoremPipeline {}
impl Pipeline for LoremPipeline {
    fn run(&self, _prompt: String) -> ReceiverStream<String> {
        let (tx, rx) = mpsc::channel::<String>(10);

        let _id = Uuid::new_v4();
        tokio::spawn(async move {
            use fake::faker::lorem::en::*;
            use fake::Fake;

            let word_generator = Word();

            for _ in 0..20 {
                let word: String = word_generator.fake();
                // TODO: Handle errors and properly close the channel to notify the consumer.
                tx.send(format!("{word} ")).await.unwrap();
                thread::sleep(Duration::from_millis(100))
            }
        });

        rx.into()
    }
}

pub struct GPT3Pipeline {}

#[derive(Debug, Serialize, Deserialize)]
struct GPT3ChatCompletion {
    id: String,
    object: String,
    created: i64,
    model: String,
    system_fingerprint: Option<String>,
    choices: Vec<GPT3Choice>,
}

#[derive(Debug, Serialize, Deserialize)]
struct GPT3Choice {
    index: usize,

    #[serde(skip_serializing_if = "Option::is_none")]
    delta: Option<GPT3Delta>,
    logprobs: Option<serde_json::Value>,
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

        let stream = response.into_reader();
        let rx = parse_event_stream(stream);

        ReceiverStream::new(rx)
    }
}

// TODO: Refactor and generalize
fn parse_event_stream(mut stream: impl Read + 'static + Send) -> Receiver<String> {
    let (tx, rx) = channel(10);

    tokio::spawn(async move {
        let mut buf = Vec::new();
        let mut incomplete_line = String::new();

        loop {
            buf.resize(1024, 0);

            match stream.read(&mut buf) {
                Ok(0) => {
                    break;
                }
                Ok(n) => {
                    buf.resize(n, 0);
                    if let Ok(chunk) = String::from_utf8(buf.clone()) {
                        let combined_chunk = incomplete_line + &chunk;
                        let mut lines = combined_chunk.lines();

                        // Process complete lines
                        for line in lines.by_ref() {
                            if line.starts_with("data:") {
                                let data = line.split("data: ").nth(1).unwrap();
                                if data == "[DONE]" {
                                    return;
                                }

                                let json = serde_json::from_str::<GPT3ChatCompletion>(data);

                                if let Err(e) = json {
                                    tracing::error!("GPT: Serialize error: {} on {}", e, line);
                                } else if let Ok(parsed) = json {
                                    for choice in parsed.choices {
                                        let content = match choice.delta {
                                            Some(GPT3Delta::Simple(d)) => Some(d),
                                            Some(GPT3Delta::Complex(d)) => Some(d.content),
                                            _ => None,
                                        };

                                        if let Some(c) = content {
                                            tx.send(c)
                                                .await
                                                .expect("Failed to send chunk to channel");
                                        }
                                    }
                                }
                            }
                        }

                        // Store incomplete line for next iteration
                        incomplete_line = lines.last().unwrap_or("").to_string();
                    } else {
                        tracing::error!("GPT: Read {} bytes (not UTF-8): {:?}", n, &buf);
                    }
                    buf.clear();
                }

                Err(e) => {
                    tracing::error!("GPT: Error reading from stream: {}", e);
                    break;
                }
            }
        }

        drop(tx);
    });

    rx
}
