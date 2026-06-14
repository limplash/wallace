use serde::{Deserialize, Serialize};
use wallace::*;

#[derive(Deserialize, Serialize)]
pub struct Event {
    pub id: u32,
    pub name: String,
}

impl WalValue for Event {
    fn to_bytes(&self) -> std::io::Result<Vec<u8>> {
        serde_json::to_vec(self).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::Other, format!("Encode error: {}", e))
        })
    }

    fn from_bytes(bytes: &[u8]) -> std::io::Result<Self> {
        serde_json::from_slice(bytes).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::Other, format!("Decode error: {}", e))
        })
    }
}

#[derive(Deserialize, Serialize)]
pub enum EventDelta {
    SetName(String),
}

impl WalDelta<Event> for EventDelta {
    fn to_bytes(&self) -> std::io::Result<Vec<u8>> {
        serde_json::to_vec(self).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::Other, format!("Encode error: {}", e))
        })
    }

    fn from_bytes(bytes: &[u8]) -> std::io::Result<Self> {
        serde_json::from_slice(bytes).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::Other, format!("Decode error: {}", e))
        })
    }

    fn apply(self, value: &mut Event) {
        match self {
            EventDelta::SetName(name) => value.name = name,
        }
    }
}

fn main() -> std::io::Result<()> {
    let mut wal: Wallace<Event, EventDelta> = create_new_file(
        "test_data/eample_delta.wal".into(),
        FsyncPolicy {
            every_duration: std::time::Duration::from_secs(1),
        },
        1000,
    )?;

    wal.update(
        b"event1",
        Event {
            id: 1,
            name: "Birthday".into(),
        },
    )?;

    println!("Before delta: {}", wal.get(b"event1").unwrap().name);

    let delta = EventDelta::SetName("Anniversary".into());
    wal.apply_delta(b"event1", delta)?;

    println!("After delta: {}", wal.get(b"event1").unwrap().name);

    Ok(())
}
