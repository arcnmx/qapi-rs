#![allow(non_snake_case, non_camel_case_types)]
#![doc(html_root_url = "http://docs.rs/qapi-qmp/0.4.0")]

use std::string;
use serde_derive::{Deserialize, Serialize};

include!(concat!(env!("OUT_DIR"), "/qmp.rs"));

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QMP {
    pub version: VersionInfo,
    pub capabilities: Vec<()>, // what type is this..?
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QapiCapabilities {
    pub QMP: QMP,
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
