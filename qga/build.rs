extern crate qapi_codegen;

use std::{io, env, path};

fn main() {
    match main_result() {
        Ok(()) => (),
        Err(e) => panic!("{:?}", e),
    }
}

fn main_result() -> io::Result<()> {
    println!("rerun-if-changed=build.rs");

    let out_dir = path::Path::new(&env::var("OUT_DIR").unwrap()).join("qga.rs");
    let schema_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../schema/qga/");

    for inc in qapi_codegen::codegen(schema_dir, out_dir)? {
        println!("rerun-if-changed={}", inc.display());
    }

    Ok(())
}
