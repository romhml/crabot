use askama::Template;

use axum::response::sse::{Event, KeepAlive, Sse};
use axum::{
    response::IntoResponse,
    routing::{get, post},
    Form, Router,
};
use futures::stream::{once, Stream};
use serde::Deserialize;
use std::convert::Infallible;
use uuid::Uuid;

use crate::models::{
    gpt::GPT3Pipeline, lorem::LoremPipeline, mistral::MistralPipeline, ChatModel, Pipeline,
};
use crate::template::HtmlTemplate;
use tokio_stream::StreamExt as _;

pub fn index_router() -> Router {
    Router::new()
        .route("/", get(get_messages))
        .route("/", post(post_message))
}

#[derive(Template)]
#[template(path = "pages/index.html")]
struct MessagesTemplate {
    messages: Vec<MessageTemplate>,
}

async fn get_messages() -> impl IntoResponse {
    // TODO: Add a database?
    let messages = vec![];
    HtmlTemplate(MessagesTemplate { messages })
}

#[derive(Template)]
#[template(path = "elements/message.html")]
struct MessageTemplate {
    id: Uuid,
    input: PostMessage,
    response: String,
}

impl MessageTemplate {
    pub fn new(input: PostMessage, response: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            input,
            response,
        }
    }
}

#[derive(Deserialize, Clone)]
struct PostMessage {
    prompt: String,
    #[serde(default)]
    model: ChatModel,
}

async fn post_message(
    Form(data): Form<PostMessage>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let pipeline: Box<dyn Pipeline> = match data.model {
        ChatModel::GPT3 => Box::new(GPT3Pipeline {}),
        ChatModel::Mistral => Box::new(MistralPipeline {}),
        ChatModel::Lorem => Box::new(LoremPipeline {}),
    };

    let rx = pipeline.run(data.prompt.clone());
    let response = MessageTemplate::new(data, "".into());
    let res = response.render().unwrap().replace(['\r', '\n'], "");

    let initial_event = once(async move { Ok(Event::default().data(res)) });

    let rx_stream = rx.map(move |word| {
        Ok(Event::default().event("chunk").data(format!(
            "<span hx-swap-oob='beforeend:#chunk-{id}'>{word}</span>",
            id = response.id,
            word = word
        )))
    });

    let end_event = once(async move { Ok(Event::default().event("end")) });
    let stream = initial_event.chain(rx_stream).chain(end_event);

    Sse::new(stream).keep_alive(KeepAlive::default())
}
