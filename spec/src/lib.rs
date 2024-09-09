#![doc(html_root_url = "https://docs.rs/qapi-spec/0.3.1")]

use std::{io, error, fmt, str};
use std::marker::PhantomData;
use serde::{Serialize, Serializer, Deserialize, Deserializer};
use serde::de::DeserializeOwned;

pub use serde_json::Value as Any;
pub type Dictionary = serde_json::Map<String, Any>;

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub struct Empty { }

pub enum Never { }

impl Serialize for Never {
    fn serialize<S: Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        match *self { }
    }
}

impl<'de> Deserialize<'de> for Never {
    fn deserialize<D: Deserializer<'de>>(_: D) -> Result<Self, D::Error> {
        use serde::de::Error;

        Err(D::Error::custom("Cannot instantiate Never type"))
    }
}

#[doc(hidden)]
pub mod base64 {
    use serde::{Serialize, Serializer, Deserialize, Deserializer};
    use serde::de::{Error, Unexpected};
    use base64::{prelude::*, DecodeError};

    pub fn serialize<S: Serializer>(data: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        BASE64_STANDARD.encode(data).serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        // TODO: deserialize to borrowed &str
        let str = String::deserialize(deserializer)?;

        BASE64_STANDARD.decode(&str)
            .map_err(|e| de_err(&str, e))
    }

    pub fn de_err<E: Error>(str: &str, err: DecodeError) -> E {
        match err {
            DecodeError::InvalidByte(..) | DecodeError::InvalidPadding =>
                E::invalid_value(Unexpected::Str(str), &"base64"),
            DecodeError::InvalidLength(len) =>
                E::invalid_length(len, &"valid base64 length"),
            DecodeError::InvalidLastSymbol(..) =>
                E::invalid_value(Unexpected::Str(str), &"truncated or corrupted base64"),
        }
    }
}

#[doc(hidden)]
pub mod base64_opt {
    use serde::{Serializer, Deserialize, Deserializer};
    use crate::base64;
    use ::base64::prelude::*;

    pub fn serialize<S: Serializer>(data: &Option<Vec<u8>>, serializer: S) -> Result<S::Ok, S::Error> {
        base64::serialize(data.as_ref().expect("use skip_serializing_with"), serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Vec<u8>>, D::Error> {
        // TODO: deserialize to borrowed &str
        let str = <Option<String>>::deserialize(deserializer)?;
        if let Some(ref str) = str {
            BASE64_STANDARD.decode(str)
                .map(Some)
                .map_err(|e| base64::de_err(str, e))
        } else {
            Ok(None)
        }
    }
}

mod error_serde {
    use serde::{Serialize, Serializer, Deserialize, Deserializer};
    use crate::{Error, ErrorClass, Any};

    #[derive(Deserialize)]
    pub struct ErrorValue {
        pub class: ErrorClass,
        pub desc: String,
    }

    #[derive(Deserialize)]
    struct QapiError {
        error: ErrorValue,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<Any>,
    }

    #[derive(Serialize)]
    pub struct ErrorValueSer<'a> {
        pub class: &'a ErrorClass,
        pub desc: &'a str,
    }

    #[derive(Serialize)]
    struct QapiErrorSer<'a> {
        error: ErrorValueSer<'a>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<&'a Any>,
    }

    impl Serialize for Error {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            QapiErrorSer {
                error: ErrorValueSer {
                    class: &self.class,
                    desc: &self.desc[..],
                },
                id: self.id.as_ref(),
            }.serialize(serializer)
        }
    }

    impl<'de> Deserialize<'de> for Error {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            QapiError::deserialize(deserializer).map(|e| Error {
                class: e.error.class,
                desc: e.error.desc,
                id: e.id,
            })
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseValue<C> {
    #[serde(rename = "return")]
    return_: C,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    id: Option<Any>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Response<C> {
    Err(Error),
    Ok(ResponseValue<C>),
}

impl<C> Response<C> {
    pub fn result(self) -> Result<C, Error> {
        match self {
            Response::Ok(ResponseValue { return_, .. }) => Ok(return_),
            Response::Err(e) => Err(e),
        }
    }

    pub fn id(&self) -> Option<&Any> {
        match self {
            Response::Err(err) => err.id.as_ref(),
            Response::Ok(value) => value.id.as_ref(),
        }
    }
}

pub trait Command: Serialize + Sync + Send {
    type Ok: DeserializeOwned;

    const NAME: &'static str;
    const ALLOW_OOB: bool;
}

impl<'a, C: Command> Command for &'a C {
    type Ok = C::Ok;

    const NAME: &'static str = C::NAME;
    const ALLOW_OOB: bool = C::ALLOW_OOB;
}

impl<'a, C: Command> Command for &'a mut C {
    type Ok = C::Ok;

    const NAME: &'static str = C::NAME;
    const ALLOW_OOB: bool = C::ALLOW_OOB;
}

pub trait Event: DeserializeOwned {
    const NAME: &'static str;
}

pub unsafe trait Enum: DeserializeOwned + str::FromStr + Copy + 'static {
    fn discriminant(&self) -> usize;

    fn name(&self) -> &'static str {
        unsafe {
            Self::NAMES.get_unchecked(self.discriminant())
        }
    }

    fn from_name(s: &str) -> Option<Self> {
        Self::NAMES.iter().zip(Self::VARIANTS)
            .find(|&(&n, _)| n == s)
            .map(|(_, &v)| v)
    }

    const COUNT: usize;
    const VARIANTS: &'static [Self];
    const NAMES: &'static [&'static str];
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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

#[derive(Debug, Clone)]
pub struct Error {
    pub class: ErrorClass,
    pub desc: String,
    pub id: Option<Any>,
}

pub type CommandResult<C> = Result<<C as Command>::Ok, Error>;

fn serialize_command_name<C: Command, S: Serializer>(_: &PhantomData<&'static str>, s: S) -> Result<S::Ok, S::Error> {
    C::NAME.serialize(s)
}

#[derive(Serialize)]
pub struct Execute<C, I = Never> {
    #[serde(serialize_with = "serialize_command_name::<C, _>", bound = "C: Command")]
    pub execute: PhantomData<&'static str>,
    pub arguments: C,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<I>,
}

#[derive(Serialize)]
pub struct ExecuteOob<C, I = Any> {
    #[serde(rename = "exec-oob", serialize_with = "serialize_command_name::<C, _>", bound = "C: Command")]
    pub execute_oob: PhantomData<&'static str>,
    pub arguments: C,
    pub id: I,
}

impl<C: Command, I> Execute<C, I> {
    pub fn new(arguments: C, id: Option<I>) -> Self {
        Self {
            execute: PhantomData,
            arguments,
            id,
        }
    }

    pub fn with_command(arguments: C) -> Self {
        Self {
            execute: PhantomData,
            arguments,
            id: None,
        }
    }

    pub fn with_id(arguments: C, id: I) -> Self {
        Self {
            execute: PhantomData,
            arguments,
            id: Some(id),
        }
    }
}

impl<C: Command, I> From<C> for Execute<C, I> {
    fn from(command: C) -> Self {
        Self::with_command(command)
    }
}

impl<C: Command, I> ExecuteOob<C, I> {
    pub fn new(arguments: C, id: I) -> Self {
        Self {
            execute_oob: PhantomData,
            arguments,
            id,
        }
    }
}

impl error::Error for Error {
    fn description(&self) -> &str {
        &self.desc
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&self.desc, f)
    }
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
