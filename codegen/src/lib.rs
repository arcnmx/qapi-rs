extern crate qapi_parser as parser;

use parser::{Parser, QemuFileRepo, QemuRepo, spec};
use parser::spec::Spec;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::fs::File;
use std::io::{self, Write};

// kebab-case to PascalCase?
fn type_identifier<S: AsRef<str>>(id: S) -> String {
    identifier(id)
}

// kebab-case to snake_case
fn identifier<S: AsRef<str>>(id: S) -> String {
    let id = id.as_ref();
    match id {
        "type" | "static" | "virtual" | "abstract" | "in" | "enum" => format!("{}_", id),
        s if s.as_bytes()[0].is_ascii_digit() => format!("_{}", s),
        id => id.replace("-", "_")
    }
}

// SCREAMING_SNAKE_CASE to PascalCase?
fn event_identifier(id: &str) -> String {
    id.into()
}

// no case change, just check for rust primitives
fn typename_s(ty: &str) -> String {
    match ty {
        "str" => "::std::string::String".into(),
        "any" => "::qapi::Any".into(),
        "null" => "()".into(),
        "number" => "f64".into(),
        "int8" => "i8".into(),
        "uint8" => "u8".into(),
        "int16" => "i16".into(),
        "uint16" => "u16".into(),
        "int32" => "i32".into(),
        "uint32" => "u32".into(),
        "int64" => "i64".into(),
        "uint64" => "u64".into(),
        "size" => "usize".into(),
        "int" => "isize".into(), // ???
        ty => ty.into(),
    }
}

fn typename(ty: &spec::Type) -> String {
    if ty.is_array {
        format!("Vec<{}>", typename_s(&ty.name))
    } else {
        typename_s(&ty.name)
    }
}

fn valuety(value: &spec::Value, pubvis: bool, super_name: &str) -> String {
    // overrides for recursive types:
    let boxed = if value.name == "backing-image" && value.ty.name == "ImageInfo" {
        true
    } else if value.name == "backing" && value.ty.name == "BlockStats" {
        true
    } else if value.name == "parent" && value.ty.name == "BlockStats" {
        true
    } else {
        false
    };

    let base64 = value.ty.name == "str" && (
        ((super_name == "GuestFileRead" || super_name == "guest-file-write") && value.name == "buf-b64") ||
        (super_name == "guest-set-user-password" && value.name == "password") ||
        (super_name == "GuestExecStatus" && (value.name == "out-data" || value.name == "err-data")) ||
        (super_name == "guest-exec" && value.name == "input-data") ||
        (super_name == "QCryptoSecretFormat" && value.name == "base64")
        // "ringbuf-write", "ringbuf-read" can't be done because weird enums
    );

    let dict = value.ty.name == "any" && (
        (super_name == "object-add" && value.name == "props") ||
        (super_name == "CpuModelInfo" && value.name == "props")
    );

    // TODO: handle optional Vec<>s specially?

    let ty = typename(&value.ty);
    let (attr, ty) = if base64 {
        let ty = "Vec<u8>".into();
        if value.optional {
            (", with = \"qapi::base64_opt\"", ty)
        } else {
            (", with = \"qapi::base64\"", ty)
        }
    } else if boxed {
        ("", format!("Box<{}>", ty))
    } else if dict {
        ("", "::qapi::Dictionary".into())
    } else if super_name == "guest-shutdown" && value.name == "mode" {
        ("", "GuestShutdownMode".into())
    } else {
        ("", ty)
    };

    let (attr, ty) = if value.optional {
        (format!("{}, default, skip_serializing_if = \"Option::is_none\"", attr), format!("Option<{}>", ty))
    } else {
        (attr.into(), ty)
    };

    format!("#[serde(rename = \"{}\"{})]\n{}{}: {}",
        value.name,
        attr,
        if pubvis { "pub " } else { "" },
        identifier(&value.name),
        ty
    )
}

struct Context<W> {
    includes: Vec<String>,
    included: HashSet<PathBuf>,
    events: Vec<spec::Event>,
    unions: Vec<spec::CombinedUnion>,
    types: HashMap<String, spec::Struct>,
    out: W,
}

impl<W: Write> Context<W> {
    fn new(out: W) -> Self {
        Context {
            includes: Default::default(),
            included: Default::default(),
            events: Default::default(),
            unions: Default::default(),
            types: Default::default(),
            out: out,
        }
    }

    fn process(&mut self, item: spec::Spec) -> io::Result<()> {
        match item {
            Spec::Include(include) => {
                self.includes.push(include.include);
            },
            Spec::Command(v) => {
                match v.data {
                    spec::DataOrType::Type(ref ty) if type_identifier(&ty.name) == type_identifier(&v.id) => (),
                    _ => {
                        write!(self.out, "
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct {}", type_identifier(&v.id))?;
                        match v.data {
                            spec::DataOrType::Data(ref data) => {
                                if v.id == "device_add" || v.id == "netdev_add" {
                                    writeln!(self.out, "(pub ::qapi::Dictionary);")?;
                                } else {
                                    writeln!(self.out, " {{")?;
                                    for data in &data.fields {
                                        writeln!(self.out, "\t{},", valuety(&data, true, &v.id))?;
                                    }
                                    writeln!(self.out, "}}")?;
                                }
                            },
                            spec::DataOrType::Type(ref ty) => {
                                writeln!(self.out, "(pub {});", type_identifier(&ty.name))?;
                            },
                        }
                    },
                }

                write!(self.out, "
impl ::qapi::Command for {} {{
    const NAME: &'static str = \"{}\";

    type Ok = ", type_identifier(&v.id), v.id)?;
                if let Some(ret) = v.returns {
                    writeln!(self.out, "{};", typename(&ret))
                } else {
                    writeln!(self.out, "::qapi::Empty;")
                }?;
                writeln!(self.out, "}}")?;
            },
            Spec::Struct(v) => {
                write!(self.out, "
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct {} {{
", type_identifier(&v.id))?;
                for item in &v.data.fields {
                    writeln!(self.out, "{},", valuety(item, true, &v.id))?;
                }
                writeln!(self.out, "}}")?;

                self.types.insert(v.id.clone(), v);
            },
            Spec::Alternate(v) => {
                write!(self.out, "
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum {} {{
", type_identifier(&v.id))?;
                for data in &v.data.fields {
                    assert!(!data.optional);
                    let boxed = if data.name == "definition" && data.ty.name == "BlockdevOptions" {
                        true
                    } else {
                        false
                    };
                    let ty = if boxed {
                        format!("Box<{}>", typename(&data.ty))
                    } else {
                        typename(&data.ty)
                    };
                    writeln!(self.out, "\t#[serde(rename = \"{}\")] {}({}),", data.name, type_identifier(&data.name), ty)?;
                }
                writeln!(self.out, "}}")?;
            },
            Spec::Enum(v) => {
                write!(self.out, "
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum {} {{
", type_identifier(&v.id))?;
                for item in &v.data {
                    writeln!(self.out, "\t#[serde(rename = \"{}\")] {},", item, type_identifier(item))?;
                }
                writeln!(self.out, "}}")?;
            },
            Spec::Event(v) => {
                write!(self.out, "
#[derive(Debug, Clone, Serialize, Deserialize{})]
pub struct {} {{
", if v.data.is_empty() { ", Default" } else { "" }, event_identifier(&v.id))?;
                for item in &v.data.fields {
                    writeln!(self.out, "{},", valuety(item, true, &v.id))?;
                }
                writeln!(self.out, "}}")?;
                writeln!(self.out, "
impl ::qapi::Event for {} {{
    const NAME: &'static str = \"{}\";
}}", event_identifier(&v.id), v.id)?;
                self.events.push(v);
            },
            Spec::Union(v) => {
                write!(self.out, "
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = \"{}\")]
pub enum {} {{
", if let Some(ref tag) = v.discriminator { tag } else { "type" }, type_identifier(&v.id))?;
                for data in &v.data.fields {
                    writeln!(self.out, "\t#[serde(rename = \"{}\")]\n\t{} {{ data: {} }},", data.name, type_identifier(&data.name), typename(&data.ty))?;
                }
                writeln!(self.out, "}}")?;
            },
            Spec::CombinedUnion(v) => {
                self.unions.push(v);
            },
            Spec::PragmaWhitelist { .. } => (),
            Spec::PragmaDocRequired { .. } => (),
        }

        Ok(())
    }

    fn process_unions(&mut self) -> io::Result<()> {
        for u in &self.unions {
            let discrim = if let Some(ref tag) = u.discriminator { tag } else { "type" };
            write!(self.out, "
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = \"{}\")]
pub enum {} {{
", discrim, type_identifier(&u.id))?;

            let mut discrim_ty = None;
            for variant in &u.data.fields {
                assert!(!variant.optional);
                assert!(!variant.ty.is_array);

                println!("doing union {}", u.id);
                writeln!(self.out, "\t#[serde(rename = \"{}\")]\n\t{} {{\n\t\t// base", variant.name, type_identifier(&variant.name))?;
                match u.base {
                    spec::DataOrType::Data(ref data) => for base in &data.fields {
                        if base.name == discrim {
                            discrim_ty = Some(base.ty.clone());
                        } else {
                            writeln!(self.out, "\t\t{},", valuety(base, false, &u.id))?;
                        }
                    },
                    spec::DataOrType::Type(ref ty) => {
                        let ty = self.types.get(&ty.name).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("could not find qapi type {}", ty.name)))?;
                        for base in &ty.data.fields {
                            if base.name == discrim {
                                discrim_ty = Some(base.ty.clone());
                            } else {
                                writeln!(self.out, "\t\t{},", valuety(base, false, &u.id))?;
                            }
                        }
                    },
                }

                let ty = self.types.get(&variant.ty.name).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("could not find qapi type {}", variant.ty.name)))?;
                writeln!(self.out, "\t\t// variant fields")?;
                for field in &ty.data.fields {
                    writeln!(self.out, "\t\t{},", valuety(field, false, &u.id))?;
                }
                writeln!(self.out, "\t}},")?;
            }
            writeln!(self.out, "}}")?;

            if let Some(discrim_ty) = discrim_ty {
                write!(self.out, "
impl {} {{
    pub fn {}(&self) -> {} {{
        match *self {{
", type_identifier(&u.id), identifier(&discrim), type_identifier(&discrim_ty.name))?;
                for variant in &u.data.fields {
                    writeln!(self.out, "
            {}::{} {{ .. }} => {}::{},", type_identifier(&u.id), type_identifier(&variant.name), type_identifier(&discrim_ty.name), type_identifier(&variant.name))?;
                }
                writeln!(self.out, "
        }}
    }}
}}")?;
            }
        }

        Ok(())
    }

    fn process_events(&mut self) -> io::Result<()> {
        writeln!(self.out, "
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = \"event\")]
pub enum Event {{")?;
        for event in &self.events {
            let id = event_identifier(&event.id);
            writeln!(self.out, "\t#[serde(rename = \"{}\")] {} {{
        {} data: {},
        timestamp: ::qapi::Timestamp,
    }},", event.id, id, if event.data.is_empty() { "#[serde(default)] " } else { "" }, id)?;
        }
        writeln!(self.out, "}}")?;

        writeln!(self.out, "
impl Event {{
    pub fn timestamp(&self) -> ::qapi::Timestamp {{
        match *self {{")?;
        for event in &self.events {
            writeln!(self.out, "Event::{} {{ timestamp, .. }} => timestamp,", event_identifier(&event.id))?;
        }
        writeln!(self.out, "
        }}
    }}
}}")?;
        Ok(())
    }
}

fn include<W: Write>(context: &mut Context<W>, repo: &mut QemuFileRepo, path: &str) -> io::Result<()> {
    let include_path = repo.context().join(path);
    if context.included.contains(&include_path) {
        return Ok(())
    }
    context.included.insert(include_path);

    let (mut repo, str) = repo.include(path)?;
    for item in Parser::from_string(Parser::strip_comments(&str)) {
        context.process(item?)?;
    }

    while !context.includes.is_empty() {
        let includes: Vec<_> = context.includes.drain(..).collect();

        for inc in includes {
            include(context, &mut repo, &inc)?;
        }
    }

    Ok(())
}

pub fn codegen<S: AsRef<Path>, O: AsRef<Path>>(schema_path: S, out_path: O) -> io::Result<HashSet<PathBuf>> {
    let mut repo = QemuFileRepo::new(schema_path.as_ref());
    {
        let mut context = Context::new(File::create(out_path)?);
        include(&mut context, &mut repo, "qapi-schema.json")?;
        context.process_unions()?;
        context.process_events()?;
        Ok(context.included)
    }
}
