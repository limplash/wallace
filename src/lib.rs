use std::time::Duration;
use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::{self, Read, Seek, Write},
    marker::PhantomData,
    time::Instant,
};

#[derive(Clone, Copy)]
pub struct FsyncPolicy {
    pub every_duration: Duration,
}

pub trait WalValue: Sized {
    fn to_bytes(&self) -> io::Result<Vec<u8>>;
    fn from_bytes(bytes: &[u8]) -> io::Result<Self>;
}

pub trait WalDelta<V>: Sized {
    fn to_bytes(&self) -> io::Result<Vec<u8>>;
    fn from_bytes(bytes: &[u8]) -> io::Result<Self>;
    fn apply(self, value: &mut V);
}

enum OpType {
    Update = 0x01, // updates/insert complete key-value pairs
    Remove = 0x02, // removes keys
    Delta = 0x03,  // applies a delta to an existing value
}

fn write_record(file: &mut File, op: OpType, payload: &[u8]) -> io::Result<()> {
    let mut buf = Vec::with_capacity(1 + 4 + payload.len() + 4);

    buf.push(op as u8);
    buf.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    buf.extend_from_slice(payload);

    // CRC covers [op | len | payload]
    let crc = crc32fast::hash(&buf[..]);
    buf.extend_from_slice(&crc.to_be_bytes());

    file.write_all(&buf)?;
    file.flush()
}

pub struct Wallace<V, D>
where
    V: WalValue,
    D: WalDelta<V>,
{
    data: HashMap<Vec<u8>, V>,
    log: File,
    path: String,
    ops: usize,
    fsync: FsyncPolicy,
    compact_threshold: usize,
    last_fsync: Instant,

    _delta: PhantomData<D>,
}

impl<V: WalValue, D: WalDelta<V>> Wallace<V, D> {
    fn new(path: String, fsync: FsyncPolicy, compact_threshold: usize) -> io::Result<Self> {
        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&path)?;

        Ok(Self {
            data: HashMap::new(),
            log,
            path,
            ops: 0,
            fsync,
            compact_threshold,
            last_fsync: Instant::now(),
            _delta: PhantomData,
        })
    }

    fn wal_write(&mut self, op: OpType, payload: &[u8]) -> io::Result<()> {
        write_record(&mut self.log, op, payload)?;

        if self.last_fsync.elapsed() >= self.fsync.every_duration {
            self.log.sync_data()?;
            self.last_fsync = Instant::now();
        }

        Ok(())
    }

    pub fn apply_delta(&mut self, key: &[u8], delta: D) -> io::Result<()> {
        let mut payload = Vec::new();

        payload.extend_from_slice(&(key.len() as u32).to_be_bytes());
        payload.extend_from_slice(key);

        let delta_bytes = match delta.to_bytes() {
            Ok(bytes) => bytes,
            Err(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "failed to serialize delta",
                ));
            }
        };
        payload.extend_from_slice(&delta_bytes);

        if !self.data.contains_key(key) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "key not found for delta application",
            ));
        }

        self.wal_write(OpType::Delta, &payload)?;

        let value = self
            .data
            .get_mut(key)
            .expect("key existence already checked above");

        delta.apply(value);

        self.maybe_compact()
    }

    pub fn get(&self, key: &[u8]) -> Option<&V> {
        self.data.get(key)
    }
    // "take" is like "get" but also removes the key from the map. It does NOT write a Remove op to the log, so it's only for in-memory use.
    pub fn take(&mut self, key: &[u8]) -> Option<V> {
        self.data.remove(key)
    }

    pub fn put(&mut self, key: Vec<u8>, value: V) {
        self.data.insert(key, value);
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Vec<u8>, &V)> {
        self.data.iter()
    }

    pub fn update(&mut self, key: &[u8], value: V) -> io::Result<()> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&(key.len() as u32).to_be_bytes());
        payload.extend_from_slice(&key);
        let val = match value.to_bytes() {
            Ok(bytes) => bytes,
            Err(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "failed to serialize value",
                ));
            }
        };
        payload.extend_from_slice(&val);

        self.wal_write(OpType::Update, &payload)?;
        self.data.insert(key.to_vec(), value);

        self.maybe_compact()
    }

    pub fn remove(&mut self, key: &[u8]) -> io::Result<Option<V>> {
        self.wal_write(OpType::Remove, &key)?;
        let ret = self.data.remove(key);
        self.maybe_compact()?;
        Ok(ret)
    }

    pub fn maybe_compact(&mut self) -> io::Result<()> {
        self.ops += 1;
        if self.ops >= self.compact_threshold {
            self.compact()?;
            self.ops = 0;
        }
        Ok(())
    }

    pub fn compact(&mut self) -> io::Result<()> {
        if self.data.is_empty() {
            // No data to compact, just truncate the log
            self.log.set_len(0)?;
            self.log.seek(std::io::SeekFrom::Start(0))?;
            return Ok(());
        }
        self.sync()?; // WAL stable

        let tmp = format!("{}.tmp", self.path);
        let mut f = File::create(&tmp)?;

        for (k, v) in &self.data {
            let mut payload = Vec::new();
            payload.extend_from_slice(&(k.len() as u32).to_be_bytes());
            payload.extend_from_slice(k);
            let val = match v.to_bytes() {
                Ok(bytes) => bytes,
                Err(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "failed to serialize value during compaction",
                    ));
                }
            };
            payload.extend_from_slice(&val);
            write_record(&mut f, OpType::Update, &payload)?;
        }

        f.sync_all()?; // snapshot durable
        std::fs::rename(&tmp, &self.path)?;

        self.log = OpenOptions::new().append(true).open(&self.path)?;
        self.log.sync_data()?; // new WAL durable

        Ok(())
    }

    pub fn maybe_sync(&mut self) -> io::Result<()> {
        if self.last_fsync.elapsed() >= self.fsync.every_duration {
            self.sync()?;
        }
        Ok(())
    }

    pub fn sync(&mut self) -> io::Result<()> {
        self.log.flush()?;
        self.log.sync_data()?;
        self.last_fsync = Instant::now();
        Ok(())
    }
}

pub fn load_from_file<V: WalValue, D: WalDelta<V>>(
    path: String,
    fsync: FsyncPolicy,
    compact_threshold: usize,
) -> io::Result<Wallace<V, D>> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .read(true)
        .open(&path)?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    let mut pos = 0;
    let mut map = HashMap::new();

    while pos + 9 <= buf.len() {
        let op = buf[pos];
        let len =
            u32::from_be_bytes(buf[pos + 1..pos + 5].try_into().expect("should be a u32")) as usize;
        let end = pos + 5 + len;
        let crc_pos = end;

        if crc_pos + 4 > buf.len() {
            // error
            break; // torn write
        }

        let expected = u32::from_be_bytes(
            buf[crc_pos..crc_pos + 4]
                .try_into()
                .expect("should be a u32"),
        );
        let actual = crc32fast::hash(&buf[pos..end]);

        if expected != actual {
            // error
            break; // corruption → stop replay
        }

        match op {
            0x01 => {
                let klen =
                    u32::from_be_bytes(buf[pos + 5..pos + 9].try_into().expect("should be a u32"))
                        as usize;
                let key = buf[pos + 9..pos + 9 + klen].to_vec();
                let val = V::from_bytes(&buf[pos + 9 + klen..end])?;
                map.insert(key, val);
            }
            0x02 => {
                let key = buf[pos + 5..end].to_vec();
                map.remove(&key);
            }
            0x03 => {
                let klen =
                    u32::from_be_bytes(buf[pos + 5..pos + 9].try_into().expect("should be u32"))
                        as usize;

                let key_start = pos + 9;
                let key_end = key_start + klen;

                let key = buf[key_start..key_end].to_vec();

                let delta_bytes = &buf[key_end..end];

                let delta = D::from_bytes(delta_bytes)?;

                // NOTE need to thing about how to handle this replay error situation
                let value = map.get_mut(&key).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "delta applied before value exists",
                    )
                })?;

                delta.apply(value);
            }
            _ => {
                break;
            }
        }

        pos = crc_pos + 4;
    }

    let mut ks = Wallace::new(path, fsync, compact_threshold)?;
    ks.data = map;
    ks.compact()?;
    Ok(ks)
}

pub fn create_new_file<V: WalValue, D: WalDelta<V>>(
    path: String,
    fsync: FsyncPolicy,
    compact_threshold: usize,
) -> io::Result<Wallace<V, D>> {
    let file = File::create(&path)?;
    file.sync_all()?;
    Wallace::new(path, fsync, compact_threshold)
}

pub fn load_or_create_from_file<V: WalValue, D: WalDelta<V>>(
    path: String,
    fsync: FsyncPolicy,
    compact_threshold: usize,
) -> io::Result<Wallace<V, D>> {
    match load_from_file(path.clone(), fsync, compact_threshold) {
        Ok(wal) => Ok(wal),
        Err(_) => create_new_file(path, fsync, compact_threshold),
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    struct TestValue(Vec<u8>);

    impl WalValue for TestValue {
        fn to_bytes(&self) -> io::Result<Vec<u8>> {
            Ok(self.0.clone())
        }

        fn from_bytes(bytes: &[u8]) -> io::Result<Self> {
            Ok(TestValue(bytes.to_vec()))
        }
    }

    #[derive(Clone, Debug)]
    struct TestDelta(Vec<u8>);

    impl WalDelta<TestValue> for TestDelta {
        fn to_bytes(&self) -> io::Result<Vec<u8>> {
            Ok(self.0.clone())
        }

        fn from_bytes(bytes: &[u8]) -> io::Result<Self> {
            Ok(TestDelta(bytes.to_vec()))
        }

        fn apply(self, value: &mut TestValue) {
            value.0.extend(self.0);
        }
    }

    fn create_test_wallace() -> Wallace<TestValue, TestDelta> {
        let path = "test_data/test_wal.log".to_string();
        let fsync_policy = FsyncPolicy {
            every_duration: Duration::from_secs(1),
        };
        let compact_threshold = 10;

        Wallace::new(path, fsync_policy, compact_threshold).unwrap()
    }

    fn remove_test_wallace_file() {
        let _ = std::fs::remove_file("test_data/test_wal.log");
        let _ = std::fs::remove_file("test_data/test_wal_load.log");
    }

    #[test]
    fn test_put_and_get() {
        let mut wal = create_test_wallace();
        let key = b"key1".to_vec();
        let value = TestValue(b"value1".to_vec());

        wal.put(key.clone(), value.clone());
        assert_eq!(wal.get(&key), Some(&value));
        remove_test_wallace_file();
    }

    #[test]
    fn test_update() {
        let mut wal = create_test_wallace();
        let key = b"key1".to_vec();
        let value = TestValue(b"value1".to_vec());
        let updated_value = TestValue(b"value2".to_vec());

        wal.put(key.clone(), value);
        wal.update(&key.clone(), updated_value.clone()).unwrap();
        assert_eq!(wal.get(&key), Some(&updated_value));
        remove_test_wallace_file();
    }

    #[test]
    fn test_remove() {
        let mut wal = create_test_wallace();
        let key = b"key1".to_vec();
        let value = TestValue(b"value1".to_vec());

        wal.put(key.clone(), value);
        wal.remove(&key).unwrap();
        assert!(wal.get(&key).is_none());
        remove_test_wallace_file();
    }

    #[test]
    fn test_apply_delta() {
        let mut wal = create_test_wallace();
        let key = b"key1".to_vec();
        let value = TestValue(b"value".to_vec());
        let delta = TestDelta(b"_delta".to_vec());
        let expected_value = TestValue(b"value_delta".to_vec());

        wal.put(key.clone(), value);
        wal.apply_delta(&key, delta).unwrap();
        assert_eq!(wal.get(&key), Some(&expected_value));
        remove_test_wallace_file();
    }

    #[test]
    fn test_take() {
        let mut wal = create_test_wallace();
        let key = b"key1".to_vec();
        let value = TestValue(b"value1".to_vec());

        wal.put(key.clone(), value.clone());
        let taken_value = wal.take(&key);
        assert_eq!(taken_value, Some(value));
        assert!(wal.get(&key).is_none());
        remove_test_wallace_file();
    }

    #[test]
    fn test_compaction() {
        let mut wal = create_test_wallace();
        let key1 = b"key1".to_vec();
        let key2 = b"key2".to_vec();
        let value1 = TestValue(b"value1".to_vec());
        let value2 = TestValue(b"value2".to_vec());

        wal.put(key1.clone(), value1.clone());
        wal.put(key2.clone(), value2.clone());
        wal.compact().unwrap();

        assert_eq!(wal.get(&key1), Some(&value1));
        assert_eq!(wal.get(&key2), Some(&value2));
        remove_test_wallace_file();
    }
}
