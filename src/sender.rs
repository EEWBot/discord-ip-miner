use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::request::{JobSender, Request};

#[derive(Debug, Clone)]
pub struct Targets {
    targets: Vec<url::Url>,
}

impl Targets {
    pub fn try_new(path: &Path) -> Result<Self> {
        let file = File::open(path)?;

        let targets: Result<Vec<url::Url>> = BufReader::new(file)
            .lines()
            .map(|line| {
                line.context("Failed to read line")
                    .and_then(|line| line.parse().context("Failed to parse as URL"))
            })
            .map(|v| v)
            .collect();

        let targets = targets?;

        Ok(Self { targets })
    }
}

pub async fn run(sender: JobSender, lure_ins: &Targets, interval: &Duration) {
    tokio::time::sleep(Duration::from_secs(5)).await;

    let mut interval = tokio::time::interval(*interval);

    loop {
        for lure_in in &lure_ins.targets {
            let _ = interval.tick().await;

            sender
                .send(Request {
                    target: lure_in.clone(),
                })
                .await
                .unwrap();
        }
    }
}
