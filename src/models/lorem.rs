use std::{thread, time::Duration};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

use crate::models::Pipeline;

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
                tx.send(format!("{word} ")).await.unwrap();
                thread::sleep(Duration::from_millis(100))
            }
        });

        rx.into()
    }
}
