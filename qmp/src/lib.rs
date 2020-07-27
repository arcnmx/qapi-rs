#![allow(non_snake_case, non_camel_case_types)]
#![doc(html_root_url = "http://docs.rs/qapi-qmp/0.4.0")]

use std::{io, string};
use std::convert::TryFrom;
use serde::{Deserialize, Serialize};

include!(concat!(env!("OUT_DIR"), "/qmp.rs"));

pub type QmpMessageAny = QmpMessage<qapi_spec::Any>;

pub trait QmpCommand: qapi_spec::Command { }
impl<'a, T: QmpCommand> QmpCommand for &'a T { }
impl<'a, T: QmpCommand> QmpCommand for &'a mut T { }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum QmpMessage<C> {
    Event(Event),
    Response(qapi_spec::Response<C>),
}

impl<C> TryFrom<QmpMessage<C>> for qapi_spec::Response<C> {
    type Error = io::Error;

    fn try_from(m: QmpMessage<C>) -> Result<Self, Self::Error> {
        match m {
            QmpMessage::Response(res) => Ok(res),
            QmpMessage::Event(..) =>
                Err(io::Error::new(io::ErrorKind::InvalidData, "QMP event where a response was expected")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QMP {
    pub version: VersionInfo,
    pub capabilities: Vec<QmpCapability>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum QmpCapability {
    #[serde(rename = "oob")]
    OutOfBand,
    Unknown(qapi_spec::Any),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QapiCapabilities {
    pub QMP: QMP,
}

impl QapiCapabilities {
    pub fn supports_oob(&self) -> bool {
        self.QMP.capabilities.iter().any(|c| match c {
            QmpCapability::OutOfBand => true,
            _ => false,
        })
    }

    pub fn capabilities<'a>(&'a self) -> impl Iterator<Item=QMPCapability> + 'a {
        self.QMP.capabilities.iter().filter_map(|c| match c {
            QmpCapability::OutOfBand => Some(QMPCapability::oob),
            QmpCapability::Unknown(..) => None,
        })
    }
}

impl device_add {
    pub fn new<P: IntoIterator<Item=(string::String, qapi_spec::Any)>>(driver: string::String, id: Option<string::String>, bus: Option<string::String>, props: P) -> Self {
        let mut dict = qapi_spec::Dictionary::default();
        dict.insert("driver".into(), qapi_spec::Any::String(driver));
        if let Some(id) = id {
            dict.insert("id".into(), qapi_spec::Any::String(id));
        }
        if let Some(bus) = bus {
            dict.insert("bus".into(), qapi_spec::Any::String(bus));
        }
        dict.extend(props);

        device_add(dict)
    }
}
