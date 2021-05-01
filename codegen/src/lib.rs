#![doc(html_root_url = "http://docs.rs/qapi-codegen/0.6.0")]

use qapi_parser::{Parser, QemuFileRepo, QemuRepo, spec};
use qapi_parser::spec::Spec;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::fs::File;
use std::io::{self, Write};
use std::mem::replace;

// kebab-case to PascalCase?
fn type_identifier<S: AsRef<str>>(id: S) -> String {
    identifier(id)
}

// kebab-case to snake_case
fn identifier<S: AsRef<str>>(id: S) -> String {
    let id = id.as_ref();
    match id {
        "type" | "static" | "virtual" | "abstract" | "in" | "if" | "enum" | "match" => format!("{}_", id),
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
        "any" => "::qapi_spec::Any".into(),
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

fn type_attrs(ty: &spec::Type) -> String {
    feature_attrs(&ty.features)
}

fn feature_attrs(ty: &spec::Features) -> String {
    if ty.is_deprecated() { " #[deprecated]".into() } else { String::new() }
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
            (", with = \"::qapi_spec::base64_opt\"", ty)
        } else {
            (", with = \"::qapi_spec::base64\"", ty)
        }
    } else if boxed {
        ("", format!("Box<{}>", ty))
    } else if dict {
        ("", "::qapi_spec::Dictionary".into())
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

    format!("#[serde(rename = \"{}\"{})]{}\n{}{}: {}",
        value.name,
        attr,
        type_attrs(&value.ty),
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
    struct_discriminators: HashMap<String, String>,
    command_trait: String,
    out: W,
}

impl<W: Write> Context<W> {
    fn new(out: W, command_trait: String) -> Self {
        Context {
            includes: Default::default(),
            included: Default::default(),
            events: Default::default(),
            unions: Default::default(),
            types: Default::default(),
            struct_discriminators: Default::default(),
            command_trait,
            out,
        }
    }

    fn process(&mut self, item: spec::Spec) -> io::Result<()> {
        match item {
            Spec::Include(include) => {
                self.includes.push(include.include);
            },
            Spec::Command(v) => {
                let type_id = type_identifier(&v.id);
                match v.data {
                    spec::DataOrType::Type(ref ty) if type_identifier(&ty.name) == type_id => (),
                    ty => {
                        write!(self.out, "
#[derive(Debug, Clone, Serialize, Deserialize)]{}
pub struct {}", feature_attrs(&v.features), type_id)?;
                        match ty {
                            spec::DataOrType::Data(ref data) => {
                                writeln!(self.out, " {{")?;
                                for data in &data.fields {
                                    writeln!(self.out, "\t{},", valuety(&data, true, &v.id))?;
                                }
                                if !v.gen {
                                    writeln!(self.out, "
    #[serde(flatten)]
    pub arguments: ::qapi_spec::Dictionary,
")?;
                                }
                                writeln!(self.out, "}}")?;
                            },
                            spec::DataOrType::Type(ref ty) => {
                                let ty_name = type_identifier(&ty.name);
                                writeln!(self.out, "({}pub {});", type_attrs(ty), ty_name)?;
                                writeln!(self.out, "
impl From<{}> for {} {{
    fn from(v: {}) -> Self {{
        Self(v)
    }}
}}
", ty_name, type_id, ty_name)?;
                            },
                        }
                    },
                }

                write!(self.out, "
impl crate::{} for {} {{ }}
impl ::qapi_spec::Command for {} {{
    const NAME: &'static str = \"{}\";
    const ALLOW_OOB: bool = {};

    type Ok = ", self.command_trait, type_id, type_id, v.id, v.allow_oob)?;
                if let Some(ret) = v.returns {
                    writeln!(self.out, "{};", typename(&ret))
                } else {
                    writeln!(self.out, "::qapi_spec::Empty;")
                }?;
                writeln!(self.out, "}}")?;
            },
            Spec::Struct(v) => {
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
                let type_id = type_identifier(&v.id);
                write!(self.out, "
#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum {} {{
", type_id)?;
                for item in &v.data {
                    writeln!(self.out, "\t#[serde(rename = \"{}\")] {},", item, type_identifier(item))?;
                }
                writeln!(self.out, "}}")?;
                writeln!(self.out, "
impl ::core::str::FromStr for {} {{
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {{
        ::qapi_spec::Enum::from_name(s).ok_or(())
    }}
}}

unsafe impl ::qapi_spec::Enum for {} {{
    fn discriminant(&self) -> usize {{ *self as usize }}

    const COUNT: usize = {};
    const VARIANTS: &'static [Self] = &[
", type_id, type_id, v.data.len())?;
                for item in &v.data {
                    writeln!(self.out, "{}::{},", type_id, type_identifier(item))?;
                }
                writeln!(self.out, "
    ];
    const NAMES: &'static [&'static str] = &[
")?;
                for item in &v.data {
                    writeln!(self.out, "\"{}\",", item)?;
                }
                writeln!(self.out, "
    ];
}}")?;
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
impl ::qapi_spec::Event for {} {{
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
            Spec::PragmaExceptions { .. } => (),
            Spec::PragmaDocRequired { .. } => (),
        }

        Ok(())
    }

    fn process_structs(&mut self) -> io::Result<()> {
        for (id, discrim) in &self.struct_discriminators {
            let ty = self.types.get_mut(id).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("could not find qapi type {}", id)))?;
            let fields = replace(&mut ty.data.fields, Vec::new());
            ty.data.fields = fields.into_iter().filter(|base| &base.name != discrim).collect();
        }

        for v in self.types.values() {
            write!(self.out, "
#[derive(Debug, Clone, Serialize, Deserialize)]{}
pub struct {} {{
", feature_attrs(&v.features), type_identifier(&v.id))?;
            match v.base {
                spec::DataOrType::Data(ref data) => for base in &data.fields {
                    writeln!(self.out, "{},", valuety(base, true, &v.id))?;
                },
                spec::DataOrType::Type(ref ty) => {
                    let base = spec::Value {
                        name: "base".into(),
                        ty: ty.clone(),
                        optional: false,
                    };
                    writeln!(self.out, "#[serde(flatten)]\n{},", valuety(&base, true, &v.id))?;
                },
            }
            for item in &v.data.fields {
                writeln!(self.out, "{},", valuety(item, true, &v.id))?;
            }
            writeln!(self.out, "}}")?;
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
                let is_newtype = u.base.is_empty();

                println!("doing union {}", u.id);
                write!(self.out, "\t#[serde(rename = \"{}\")]\n\t{}", variant.name, type_identifier(&variant.name))?;
                writeln!(self.out, "{}", if is_newtype { "(" } else { " {" })?;
                match u.base {
                    spec::DataOrType::Data(ref data) => {
                        for base in &data.fields {
                            if base.name == discrim {
                                if let Some(ref ty) = discrim_ty {
                                    assert_eq!(ty, &base.ty);
                                } else {
                                    discrim_ty = Some(base.ty.clone());
                                }
                            } else {
                                writeln!(self.out, "\t\t{},", valuety(base, false, &u.id))?;
                            }
                        }
                    },
                    spec::DataOrType::Type(ref ty) => {
                        let base = spec::Value {
                            name: "base".into(),
                            ty: ty.clone(),
                            optional: false,
                        };
                        writeln!(self.out, "\t\t#[serde(flatten)] {},", valuety(&base, false, &u.id))?;

                        let ty = self.types.get_mut(&ty.name).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("could not find qapi type {}", ty.name)))?;
                        for base in &ty.data.fields {
                            if base.name == discrim {
                                self.struct_discriminators.insert(ty.id.clone(), base.name.clone());

                                if let Some(ref ty) = discrim_ty {
                                    assert_eq!(ty, &base.ty);
                                } else {
                                    discrim_ty = Some(base.ty.clone());
                                }
                            }
                        }
                    },
                }

                if is_newtype {
                    write!(self.out, "{}", type_identifier(&variant.ty.name))?;
                } else {
                    let field = spec::Value {
                        name: variant.name.clone(),
                        ty: variant.ty.clone(),
                        optional: false,
                    };
                    writeln!(self.out, "\t\t#[serde(flatten)] {},", valuety(&field, false, &u.id))?;
                }
                writeln!(self.out, "{}", if is_newtype { ")," } else { "\t}," })?;
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
            } else {
                panic!("missing discriminator type for {}", u.id);
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
        timestamp: ::qapi_spec::Timestamp,
    }},", event.id, id, if event.data.is_empty() { "#[serde(default)] " } else { "" }, id)?;
        }
        writeln!(self.out, "}}")?;

        writeln!(self.out, "
impl Event {{
    pub fn timestamp(&self) -> ::qapi_spec::Timestamp {{
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

pub fn codegen<S: AsRef<Path>, O: AsRef<Path>>(schema_path: S, out_path: O, command_trait: String) -> io::Result<HashSet<PathBuf>> {
    let mut repo = QemuFileRepo::new(schema_path.as_ref());
    {
        let mut context = Context::new(File::create(out_path)?, command_trait);
        include(&mut context, &mut repo, "qapi-schema.json")?;
        context.process_unions()?;
        context.process_structs()?;
        context.process_events()?;
        Ok(context.included)
    }
}
