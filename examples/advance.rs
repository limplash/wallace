use wallace::*;

#[derive(Debug)]
pub struct Address {
    street: String,
    city: String,
    country: String,
}

#[derive(Debug)]
struct User {
    name: String,
    age: u32,
    phones: Vec<String>,
    addresses: Vec<Address>,
}

pub enum UserDelta {
    SetName(String),
    SetAge(u32),
    PushPhone(String),
    PushAddress(Address),
}

impl WalValue for Address {
    fn to_bytes(&self) -> std::io::Result<Vec<u8>> {
        let mut buf = Vec::new();
        let street_len = self.street.len() as u32;
        buf.extend_from_slice(&street_len.to_be_bytes());
        buf.extend_from_slice(self.street.as_bytes());

        let city_len = self.city.len() as u32;
        buf.extend_from_slice(&city_len.to_be_bytes());
        buf.extend_from_slice(self.city.as_bytes());

        let country_len = self.country.len() as u32;
        buf.extend_from_slice(&country_len.to_be_bytes());
        buf.extend_from_slice(self.country.as_bytes());

        Ok(buf)
    }

    fn from_bytes(bytes: &[u8]) -> std::io::Result<Self> {
        let mut offset = 0;

        let street_len = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        let street = String::from_utf8_lossy(&bytes[offset..offset + street_len]).into();
        offset += street_len;

        let city_len = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        let city = String::from_utf8_lossy(&bytes[offset..offset + city_len]).into();
        offset += city_len;

        let country_len =
            u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        let country = String::from_utf8_lossy(&bytes[offset..offset + country_len]).into();

        Ok(Address {
            street,
            city,
            country,
        })
    }
}

impl WalValue for User {
    fn to_bytes(&self) -> std::io::Result<Vec<u8>> {
        let mut buf = Vec::new();
        let len = self.name.len() as u32;

        buf.extend_from_slice(&len.to_be_bytes());
        buf.extend_from_slice(self.name.as_bytes());
        buf.extend_from_slice(&self.age.to_be_bytes());
        let phones_len = self.phones.len() as u32;
        buf.extend_from_slice(&phones_len.to_be_bytes());
        for phone in &self.phones {
            let phone_len = phone.len() as u32;
            buf.extend_from_slice(&phone_len.to_be_bytes());
            buf.extend_from_slice(phone.as_bytes());
        }

        let addresses_len = self.addresses.len() as u32;
        buf.extend_from_slice(&addresses_len.to_be_bytes());
        for address in &self.addresses {
            let address_bytes = address.to_bytes()?;
            let address_len = address_bytes.len() as u32;
            buf.extend_from_slice(&address_len.to_be_bytes());
            buf.extend_from_slice(&address_bytes);
        }

        Ok(buf)
    }

    fn from_bytes(bytes: &[u8]) -> std::io::Result<Self> {
        let mut offset = 0;

        let name_len = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        let name = String::from_utf8_lossy(&bytes[offset..offset + name_len]).into();
        offset += name_len;

        let age = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap());
        offset += 4;

        let phones_len = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        let mut phones = Vec::with_capacity(phones_len);
        for _ in 0..phones_len {
            let phone_len =
                u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
            offset += 4;
            let phone = String::from_utf8_lossy(&bytes[offset..offset + phone_len]).into();
            offset += phone_len;
            phones.push(phone);
        }

        let addresses_len =
            u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        offset += 4;
        let mut addresses = Vec::with_capacity(addresses_len);
        for _ in 0..addresses_len {
            let address_len =
                u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
            offset += 4;
            let address = Address::from_bytes(&bytes[offset..offset + address_len])?;
            offset += address_len;
            addresses.push(address);
        }

        Ok(User {
            name,
            age,
            phones,
            addresses,
        })
    }
}

impl WalDelta<User> for UserDelta {
    fn to_bytes(&self) -> std::io::Result<Vec<u8>> {
        let mut buf = Vec::new();
        match self {
            UserDelta::SetName(name) => {
                buf.push(0);
                let name_len = name.len() as u32;
                buf.extend_from_slice(&name_len.to_be_bytes());
                buf.extend_from_slice(name.as_bytes());
            }
            UserDelta::SetAge(age) => {
                buf.push(1);
                buf.extend_from_slice(&age.to_be_bytes());
            }
            UserDelta::PushPhone(phone) => {
                buf.push(2);
                let phone_len = phone.len() as u32;
                buf.extend_from_slice(&phone_len.to_be_bytes());
                buf.extend_from_slice(phone.as_bytes());
            }
            UserDelta::PushAddress(address) => {
                buf.push(3);
                let address_bytes = address.to_bytes()?;
                let address_len = address_bytes.len() as u32;
                buf.extend_from_slice(&address_len.to_be_bytes());
                buf.extend_from_slice(&address_bytes);
            }
        }
        Ok(buf)
    }

    fn from_bytes(bytes: &[u8]) -> std::io::Result<Self> {
        let delta_type = bytes[0];
        match delta_type {
            0 => {
                let name_len = u32::from_be_bytes(bytes[1..5].try_into().unwrap()) as usize;
                let name = String::from_utf8_lossy(&bytes[5..5 + name_len]).into();
                Ok(UserDelta::SetName(name))
            }
            1 => {
                let age = u32::from_be_bytes(bytes[1..5].try_into().unwrap());
                Ok(UserDelta::SetAge(age))
            }
            2 => {
                let phone_len = u32::from_be_bytes(bytes[1..5].try_into().unwrap()) as usize;
                let phone = String::from_utf8_lossy(&bytes[5..5 + phone_len]).into();
                Ok(UserDelta::PushPhone(phone))
            }
            3 => {
                let address_len = u32::from_be_bytes(bytes[1..5].try_into().unwrap()) as usize;
                let address = Address::from_bytes(&bytes[5..5 + address_len])?;
                Ok(UserDelta::PushAddress(address))
            }
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid delta type",
            )),
        }
    }

    fn apply(self, value: &mut User) {
        match self {
            UserDelta::SetName(name) => value.name = name,
            UserDelta::SetAge(age) => value.age = age,
            UserDelta::PushPhone(phone) => value.phones.push(phone),
            UserDelta::PushAddress(address) => value.addresses.push(address),
        }
    }
}

fn main() -> std::io::Result<()> {
    let mut wal: Wallace<User, UserDelta> = load_from_file(
        "test_data/eample_advance.wal".into(),
        FsyncPolicy {
            every_duration: std::time::Duration::from_secs(1),
        },
        1000,
    )?;

    let user = wal.get(b"user1");

    println!("loaded user: {:?}", user);

    if user.is_none() {
        wal.update(
            b"user1",
            User {
                name: "Alice".into(),
                age: 30,
                phones: vec!["123-456-7890".into()],
                addresses: vec![Address {
                    street: "123 Main St".into(),
                    city: "Anytown".into(),
                    country: "USA".into(),
                }],
            },
        )?;
        let user = wal.get(b"user1").ok_or(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "User not found",
        ))?;
        println!("created user: {:?}", user);
    }

    let user = wal.get(b"user1").ok_or(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "User not found",
    ))?;

    if user.phones.len() >= 3 {
        wal.update(
            b"user1",
            User {
                name: "Alice".into(),
                age: 30,
                phones: vec!["123-456-7890".into()],
                addresses: vec![Address {
                    street: "123 Main St".into(),
                    city: "Anytown".into(),
                    country: "USA".into(),
                }],
            },
        )?;
        println!("updating user back to initial contidion");
    }

    wal.apply_delta(b"user1", UserDelta::PushPhone("987-654-3210".into()))?;
    let user = wal.get(b"user1").ok_or(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "User not found",
    ))?;
    println!("added user phones: {:?}", user.phones);

    wal.apply_delta(b"user1", UserDelta::SetName("Alice Smith".into()))?;
    let user = wal.get(b"user1").ok_or(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "User not found",
    ))?;
    println!("updated user name: {:?}", user.name);

    wal.apply_delta(
        b"user1",
        UserDelta::PushAddress(Address {
            street: "456 Elm St".into(),
            city: "Othertown".into(),
            country: "USA".into(),
        }),
    )?;

    let user = wal.get(b"user1").ok_or(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "User not found",
    ))?;
    println!("added user address: {:?}", user.addresses);

    Ok(())
}
