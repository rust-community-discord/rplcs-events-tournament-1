use anyhow::{Context, Result};
use log::{debug, info, warn};
use reqwest::Client;
use serde::Serialize;
use std::{
    env,
    time::{Duration, Instant},
};
use tokio::{process::Command, time::sleep};

use crate::port_utils::get_next_port;

#[derive(Debug)]
pub struct Container {
    name: String,
    handle: ContainerHandle,
}

#[derive(Debug, Clone)]
pub struct ContainerHandle {
    port: u16,
    http_client: Client,
}

impl Container {
    pub async fn new(submission_name: &str) -> Result<Self> {
        let port = get_next_port().await.context("Failed to get next port")?;
        info!(
            "Starting container for {} on port {}",
            submission_name, port
        );
        let image_name = format!("localhost/rplcs-tournament-1/{}:latest", submission_name);
        let name: String = format!("rplcs-tournament-1__{}", submission_name);

        let mut command = Command::new("podman");
        command.args([
            "run",
            "-d",
            "--rm",
            "--name",
            &name,
            "-p",
            &format!("{}:3000", port),
            "-e",
            "RUST_LOG=debug",
            &image_name,
        ]);

        debug!("Running command: {:?}", command);
        command
            .output()
            .await
            .context("Failed to start container")?;

        let timeout = env::var("CONTAINER_TIMEOUT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);

        let handle = ContainerHandle {
            port,
            http_client: Client::builder()
                .timeout(Duration::from_secs(timeout))
                .build()
                .context("Failed to create HTTP client")?,
        };

        let container = Container {
            name: name.clone(),
            handle,
        };

        // Wait for container to be ready
        container
            .wait_until_ready()
            .await
            .context("Container failed to start")?;
        Ok(container)
    }

    pub async fn shutdown(&self) -> Result<()> {
        debug!("Stopping container {}", self.name);
        Command::new("podman")
            .args(["stop", &self.name])
            .output()
            .await
            .context("Failed to stop container")?;
        Ok(())
    }

    pub fn handle(&self) -> ContainerHandle {
        self.handle.clone()
    }

    async fn wait_until_ready(&self) -> Result<()> {
        let start_time = Instant::now();
        let timeout = Duration::from_secs(30);
        let check_interval = Duration::from_millis(100);

        debug!("Waiting for container {} to be ready", self.name);
        while start_time.elapsed() < timeout {
            // First check if container is running
            let output = Command::new("podman")
                .args(["inspect", "-f", "{{.State.Running}}", &self.name])
                .output()
                .await
                .context("Failed to inspect container")?;

            let running = String::from_utf8(output.stdout)
                .context("Failed to parse container status")?
                .trim()
                == "true";

            if !running {
                sleep(check_interval).await;
                continue;
            }

            // Then check if HTTP endpoint is responding
            match self.health_check().await {
                Ok(()) => {
                    info!("Container {} is ready and responding", self.name);
                    return Ok(());
                }
                Err(e) => {
                    debug!("Container {} not yet responding: {}", self.name, e);
                    sleep(check_interval).await;
                }
            }
        }

        warn!(
            "Container {} failed to start within timeout period",
            self.name
        );
        anyhow::bail!("Container failed to start within timeout period")
    }

    async fn health_check(&self) -> Result<()> {
        self.handle.health_check().await
    }
}

impl ContainerHandle {
    fn get_url(&self) -> String {
        format!("http://localhost:{}", self.port)
    }

    pub async fn health_check(&self) -> Result<()> {
        self.http_client
            .get(&format!("{}/health", self.get_url()))
            .send()
            .await
            .context("Failed to send health check request")?
            .error_for_status()
            .context("Health check failed")?;
        Ok(())
    }

    pub async fn call<T: Serialize, R: serde::de::DeserializeOwned>(
        &self,
        endpoint: &str,
        game_id: i64,
        payload: &T,
    ) -> Result<R> {
        debug!(
            "Calling {} on port {} for game {}",
            endpoint, self.port, game_id
        );
        self.http_client
            .post(&format!("{}/{}", self.get_url(), endpoint))
            .query(&[("game_id", game_id.to_string())])
            .json(payload)
            .send()
            .await
            .context("Failed to send request")?
            .json()
            .await
            .context("Failed to deserialize response")
    }
}
