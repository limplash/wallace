use std::time::Duration;
use wallace::*;

struct StringValue(String);

impl WalValue for StringValue {
    fn to_bytes(&self) -> std::io::Result<Vec<u8>> {
        Ok(self.0.as_bytes().to_vec())
    }

    fn from_bytes(bytes: &[u8]) -> std::io::Result<Self> {
        Ok(Self(String::from_utf8_lossy(bytes).into()))
    }
}

struct StringDelta(String);

impl WalDelta<StringValue> for StringDelta {
    fn to_bytes(&self) -> std::io::Result<Vec<u8>> {
        Ok(self.0.as_bytes().to_vec())
    }

    fn from_bytes(bytes: &[u8]) -> std::io::Result<Self> {
        Ok(StringDelta(String::from_utf8_lossy(bytes).into()))
    }

    fn apply(self, value: &mut StringValue) {
        value.0.push_str(&self.0);
    }
}

fn main() -> std::io::Result<()> {
    let mut wal: Wallace<StringValue, StringDelta> = create_new_file(
        "test_data/eample_basic.wal".into(),
        FsyncPolicy {
            every_duration: Duration::from_secs(1),
        },
        1000,
    )?;

    wal.update(b"hello", StringValue("world".into()))?;

    println!("{}", wal.get(b"hello").unwrap().0);

    let delta = StringDelta("!!!".into());
    wal.apply_delta(b"hello", delta)?;

    println!("{}", wal.get(b"hello").unwrap().0);

    Ok(())
}
