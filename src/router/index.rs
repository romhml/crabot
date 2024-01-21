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
use tokio_stream::wrappers::ReceiverStream;

use crate::model::Pipeline;
use crate::template::HtmlTemplate;
use tokio_stream::StreamExt as _;

pub fn index_router() -> Router {
    Router::new()
        .route("/", get(get_messages))
        .route("/", post(post_message))
        .route("/sse", post(post_message_sse))
}

#[derive(Template)]
#[template(path = "pages/index.html")]
struct MessagesTemplate {
    messages: Vec<MessageTemplate>,
}

async fn get_messages() -> impl IntoResponse {
    use fake::faker::lorem::en::*;
    use fake::Fake;

    let messages = vec![MessageTemplate {
        input: PostMessage {
            prompt: Sentence(10..20).fake(),
        },
        response: Sentence(10..100).fake(),
    }];
    HtmlTemplate(MessagesTemplate { messages })
}

#[derive(Deserialize, Clone)]
struct PostMessage {
    prompt: String,
}

#[derive(Template, Clone)]
#[template(path = "elements/message.html")]
struct MessageTemplate {
    input: PostMessage,
    response: String,
}

async fn post_message(Form(data): Form<PostMessage>) -> impl IntoResponse {
    use fake::faker::lorem::en::*;
    use fake::Fake;

    let response = MessageTemplate {
        input: data,
        response: Sentence(10..20).fake(),
    };
    HtmlTemplate(response)
}

// Note: This endpoint does not work with HTMX because the SSE extension is based on the
// EventSource API which only supports GET requests.
// There's multiple ways around:
// - Use a standard endpoint to POST the message and a SSE endpoint to get chunks as they are
//   generated. -> This is not ideal because it requires storing the MPSC channels on the APIs
//   (becomes stateful).
//
// - Extend HTMX with sse.js (https://github.com/mpetazzoni/sse.js) by adding a new verb e.g.
// hx-sse-post that will trigger the request and listen for events (mix between hx-post and
// sse-connect).
async fn post_message_sse(
    Form(data): Form<PostMessage>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let pipeline = Pipeline::new();

    let rx = pipeline.run(data.prompt.clone());

    let response = MessageTemplate {
        input: data,
        response: "".into(),
    };

    let res = response.render().unwrap().replace(['\r', '\n'], "");

    let initial_event = once(async move { Ok(Event::default().event("m").data(res)) });

    let rx_stream =
        ReceiverStream::new(rx).map(|word| Ok(Event::default().event("chunk").data(word)));

    let stream = initial_event.chain(rx_stream);

    // TODO: Fix HTMX SSE injections
    Sse::new(stream).keep_alive(KeepAlive::default())
}
