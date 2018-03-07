extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate base64 as b64;

use std::io;
use serde::de::DeserializeOwned;
use serde::Serialize;

pub use serde_json::Value as Any;

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub struct Empty { }

#[doc(hidden)]
pub mod base64 {
    use serde::{Serialize, Serializer, Deserialize, Deserializer};
    use serde::de::{Error, Unexpected};
    use b64::{self, DecodeError};

    pub fn serialize<S: Serializer>(data: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        b64::encode(data).serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        // TODO: deserialize to borrowed &str
        let str = String::deserialize(deserializer)?;

        b64::decode(&str)
            .map_err(|e| de_err(&str, e))
    }

    pub fn de_err<E: Error>(str: &str, err: DecodeError) -> E {
        match err {
            DecodeError::InvalidByte(..) =>
                E::invalid_value(Unexpected::Str(str), &"base64"),
            DecodeError::InvalidLength =>
                E::invalid_length(str.len(), &"valid base64 length"),
        }
    }
}

#[doc(hidden)]
pub mod base64_opt {
    use serde::{Serializer, Deserialize, Deserializer};
    use {b64, base64};

    pub fn serialize<S: Serializer>(data: &Option<Vec<u8>>, serializer: S) -> Result<S::Ok, S::Error> {
        base64::serialize(data.as_ref().expect("use skip_serializing_with"), serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Vec<u8>>, D::Error> {
        // TODO: deserialize to borrowed &str
        let str = <Option<String>>::deserialize(deserializer)?;
        if let Some(str) = str {
            b64::decode(&str)
                .map(Some)
                .map_err(|e| base64::de_err(&str, e))
        } else {
            Ok(None)
        }
    }
}

mod error_serde {
    use serde::{Serialize, Serializer, Deserialize, Deserializer};
    use Error;

    #[derive(Deserialize)]
    struct QapiError {
        error: Error,
    }

    #[derive(Serialize)]
    struct QapiErrorSer<'a> {
        error: &'a Error,
    }

    pub fn serialize<S: Serializer>(data: &Error, serializer: S) -> Result<S::Ok, S::Error> {
        QapiErrorSer {
            error: data,
        }.serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Error, D::Error> {
        QapiError::deserialize(deserializer).map(|e| e.error)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResponseEvent<C, E> {
    Err(
        #[serde(with = "error_serde")]
        Error
    ),
    Event(E),
    Ok {
        #[serde(rename = "return")]
        return_: C,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Response<C> {
    Err(
        #[serde(with = "error_serde")]
        Error
    ),
    Ok {
        #[serde(rename = "return")]
        return_: C,
    },
}

pub trait Command: Serialize {
    type Ok: DeserializeOwned;

    const NAME: &'static str;
}

pub trait Event: DeserializeOwned {
    const NAME: &'static str;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ErrorClass {
    /// this is used for errors that don’t require a specific error class. This should be the default case for most errors
    GenericError,
    /// the requested command has not been found
    CommandNotFound,
    /// a device has failed to be become active
    DeviceNotActive,
    /// the requested device has not been found
    DeviceNotFound,
    /// the requested operation can’t be fulfilled because a required KVM capability is missing
    KVMMissingCap,
}

impl From<ErrorClass> for io::ErrorKind {
    fn from(e: ErrorClass) -> Self {
        match e {
            ErrorClass::GenericError => io::ErrorKind::Other,
            ErrorClass::CommandNotFound => io::ErrorKind::InvalidInput,
            ErrorClass::DeviceNotActive => io::ErrorKind::Other,
            ErrorClass::DeviceNotFound => io::ErrorKind::NotFound,
            ErrorClass::KVMMissingCap => io::ErrorKind::Other,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Error {
    pub class: ErrorClass,
    pub desc: String,
}

impl From<Error> for io::Error {
    fn from(e: Error) -> Self {
        io::Error::new(e.class.into(), e.desc)
    }
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub struct Timestamp {
    seconds: u64,
    microseconds: u64,
}

/// Encode commands for QAPI execution using `#[serde(serialize_with = "serde_command")]`
pub mod serde_command {
    use serde::{Serialize, Serializer};
    use Command;

    #[derive(Serialize)]
    struct QapiCommand<'a, C: 'a> {
        execute: &'static str,
        arguments: &'a C,
    }

    pub fn serialize<C: Command, S: Serializer>(data: &C, serializer: S) -> Result<S::Ok, S::Error> {
        QapiCommand {
            execute: C::NAME,
            arguments: data,
        }.serialize(serializer)
    }
}
