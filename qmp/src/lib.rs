#![allow(non_snake_case, non_camel_case_types)]

extern crate qapi_spec as qapi;
extern crate serde;
#[macro_use]
extern crate serde_derive;

use std::string;

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
    pub fn new(driver: string::String, id: Option<string::String>, bus: Option<string::String>, props: qapi::Dictionary) -> Self {
        let mut dict = qapi::Dictionary::default();
        dict.insert("driver".into(), qapi::Any::String(driver));
        if let Some(id) = id {
            dict.insert("id".into(), qapi::Any::String(id));
        }
        if let Some(bus) = bus {
            dict.insert("bus".into(), qapi::Any::String(bus));
        }
        dict.extend(props.into_iter());

        device_add(dict)
    }
}
