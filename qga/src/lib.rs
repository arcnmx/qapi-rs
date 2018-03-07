#![allow(non_snake_case, non_camel_case_types)]

extern crate qapi_spec as qapi;
extern crate serde;
#[macro_use]
extern crate serde_derive;

include!(concat!(env!("OUT_DIR"), "/qga.rs"));
