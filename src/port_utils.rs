use anyhow::Result;
use std::sync::{Arc, OnceLock};
use tokio::sync::Mutex;

const MIN_PORT: u16 = 49152; // Start of dynamic/private ports
const MAX_PORT: u16 = 65535; // End of valid ports

static PORT_ALLOCATOR: OnceLock<Arc<Mutex<u16>>> = OnceLock::new();

fn get_port_allocator() -> &'static Arc<Mutex<u16>> {
    PORT_ALLOCATOR.get_or_init(|| Arc::new(Mutex::new(MIN_PORT)))
}

pub async fn get_next_port() -> Result<u16> {
    let mut port = get_port_allocator().lock().await;

    let current = *port;
    if current >= MAX_PORT {
        *port = MIN_PORT;
    } else {
        *port += 1;
    }

    Ok(current)
}
