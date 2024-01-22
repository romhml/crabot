use std::io::Read;
use tokio::sync::mpsc::{channel, Receiver};

#[derive(Debug)]
pub struct SSEvent<T> {
    pub id: Option<String>,
    pub name: String,
    pub data: T,
}

pub fn parse_event_stream(mut stream: impl Read + 'static + Send) -> Receiver<SSEvent<String>> {
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
                    let chunk = match String::from_utf8(buf.clone()) {
                        Ok(c) => c,
                        Err(e) => {
                            tracing::error!("GPT: Error {}: {:?}", e, &buf);
                            buf.clear();
                            continue;
                        }
                    };

                    let combined_chunk = incomplete_line + &chunk;
                    let mut lines = combined_chunk.lines();

                    // Process complete lines
                    for line in lines.by_ref() {
                        let mut event = SSEvent::<String> {
                            id: None,
                            data: "".into(),
                            name: "message".into(),
                        };

                        let (field, value) = match line.find(':') {
                            Some(pos) => {
                                let (f, v) = line.split_at(pos);
                                let v = v.trim_start_matches(": ").trim_start();
                                (f, v)
                            }
                            None => (line, ""),
                        };

                        match field {
                            "id" => {
                                event.id = Some(value.to_string());
                            }
                            "event" => {
                                event.name = value.to_string();
                            }
                            "data" => {
                                event.data.push_str(value);
                                event.data.push('\n');
                            }

                            _ => (),
                        }

                        // Ignore empty events
                        if event.data.is_empty() && event.name == "message" {
                            continue;
                        }

                        tx.send(event)
                            .await
                            .expect("Failed to send chunk to channel");
                    }

                    // Store incomplete line for next iteration
                    incomplete_line = lines.last().unwrap_or("").to_string();
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
