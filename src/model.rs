use std::{thread, time::Duration};
use tokio::sync::mpsc;
use uuid::Uuid;

pub struct Pipeline {}

impl Pipeline {
    pub fn new() -> Self {
        Self {}
    }

    pub fn run(&self, _prompt: String) -> mpsc::Receiver<String> {
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

        rx
    }
}
