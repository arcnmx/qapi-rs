#![doc(html_root_url = "https://docs.rs/qapi-parser/0.9.1")]

pub mod spec {
    use std::fmt;
    use serde::de::{Deserializer, Visitor, SeqAccess, MapAccess, Error};
    use serde::de::value::MapAccessDeserializer;
    use serde::Deserialize;

    #[derive(Debug, Clone, Deserialize)]
    #[serde(untagged, rename_all = "lowercase")]
    pub enum Spec {
        Include(Include),
        Command(Command),
        Struct(Struct),
        Alternate(Alternate),
        Enum(Enum),
        Event(Event),
        CombinedUnion(CombinedUnion),
        Union(Union),
        PragmaWhitelist {
            pragma: PragmaWhitelist
        },
        PragmaExceptions {
            pragma: PragmaExceptions
        },
        PragmaDocRequired {
            pragma: PragmaDocRequired
        },
    }

    #[derive(Debug, Default, Clone)]
    pub struct Data {
        pub fields: Vec<Value>,
    }

    impl Data {
        pub fn is_empty(&self) -> bool {
            self.fields.is_empty() || self.fields.iter().all(|f| f.optional)
        }

        pub fn newtype(&self) -> Option<&Value> {
            match self.fields.get(0) {
                Some(data) if self.fields.len() == 1 => Some(data),
                _ => None,
            }
        }
    }

    impl<'de> Deserialize<'de> for Data {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            use serde::de::IntoDeserializer;

            let fields: serde_json::Map<String, serde_json::Value> = Deserialize::deserialize(d)?;
            let fields = fields.into_iter().map(|(n, t)|
                Type::deserialize(t.into_deserializer()).map(|t|
                    Value::new(&n, t)
                ).map_err(D::Error::custom)
            ).collect::<Result<_, _>>()?;

            Ok(Data {
                fields,
            })
        }
    }

    #[derive(Debug, Clone)]
    pub struct Value {
        pub name: String,
        pub ty: Type,
        pub optional: bool,
    }

    impl Value {
        pub fn new(name: &str, ty: Type) -> Self {
            let (name, opt) = if name.starts_with("*") {
                (name[1..].into(), true)
            } else {
                (name.into(), false)
            };

            Value {
                name,
                ty,
                optional: opt,
            }
        }
    }

    #[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub enum Feature {
        Deprecated,
        Unstable,
        JsonCli,
        JsonCliHotplug,
        // what are these?
        AllowWriteOnlyOverlay,
        DynamicAutoReadOnly,
        SavevmMonitorNodes,
    }

    #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
    #[serde(untagged)]
    pub enum ConditionalFeature {
        Feature(Feature),
        Conditional {
            name: Feature,
            #[serde(default)]
            conditional: Option<Conditional>,
        },
    }

    impl PartialEq<Feature> for ConditionalFeature {
        fn eq(&self, rhs: &Feature) -> bool {
            match self {
                ConditionalFeature::Feature(name) => name == rhs,
                ConditionalFeature::Conditional { name, .. } => name == rhs,
            }
        }
    }

    #[derive(Debug, Default, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
    #[serde(transparent)]
    pub struct Features {
        // TODO: make this a set instead?
        pub features: Vec<ConditionalFeature>,
    }

    impl Features {
        pub fn is_deprecated(&self) -> bool {
            self.features.iter().any(|f| f == &Feature::Deprecated)
        }
    }

    #[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct Type {
        pub name: String,
        pub is_array: bool,
        pub conditional: Option<Conditional>,
        pub features: Features,
    }

    impl fmt::Debug for Type {
        fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
            if self.is_array {
                write!(fmt, "[{}]", self.name)?
            } else {
                write!(fmt, "{}", self.name)?
            }

            if let Some(cond) = &self.conditional {
                write!(fmt, " {:?}", cond)
            } else {
                Ok(())
            }
        }
    }

    impl<'de> Deserialize<'de> for Type {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            struct V;

            impl<'de> Visitor<'de> for V {
                type Value = Type;

                fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                    write!(formatter, "a Type of string or single element array")
                }

                fn visit_str<E: Error>(self, v: &str) -> Result<Self::Value, E> {
                    Ok(Type {
                        name: v.into(),
                        is_array: false,
                        conditional: None,
                        features: Default::default(),
                    })
                }

                fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Self::Value, A::Error> {
                    #[derive(Debug, Clone, Deserialize)]
                    struct ConditionalType {
                        #[serde(rename = "type")]
                        ty: Type,
                        #[serde(default, rename = "if")]
                        conditional: Option<Conditional>,
                        #[serde(default)]
                        features: Features,
                    }

                    let ty = ConditionalType::deserialize(MapAccessDeserializer::new(map))?;
                    Ok(Type {
                        conditional: ty.conditional,
                        features: ty.features,
                        .. ty.ty
                    })
                }

                fn visit_string<E: Error>(self, v: String) -> Result<Self::Value, E> {
                    Ok(Type {
                        name: v,
                        is_array: false,
                        conditional: None,
                        features: Default::default(),
                    })
                }

                fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                    let v = seq.next_element::<String>()?;
                    if let Some(v) = v {
                        if seq.next_element::<String>()?.is_none() {
                            Ok(Type {
                                name: v,
                                is_array: true,
                                conditional: None,
                                features: Default::default(),
                            })
                        } else {
                            Err(A::Error::invalid_length(2, &"single array item"))
                        }
                    } else {
                        Err(A::Error::invalid_length(0, &"single array item"))
                    }
                }
            }

            d.deserialize_any(V)
        }
    }

    /// A #define'd symbol such as `CONFIG_SPICE`
    pub type ConditionalDefinition = String;

    #[derive(Debug, Clone, Deserialize, PartialOrd, Ord, PartialEq, Eq, Hash)]
    #[serde(untagged, rename_all = "kebab-case")]
    pub enum Conditional {
        Define(ConditionalDefinition),
        All {
            all: Vec<ConditionalDefinition>,
        },
        Any {
            any: Vec<ConditionalDefinition>,
        },
    }

    #[derive(Debug, Clone, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct Include {
        pub include: String,
    }

    #[derive(Debug, Clone, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct Command {
        #[serde(rename = "command")]
        pub id: String,
        #[serde(default)]
        pub data: DataOrType,
        #[serde(default)]
        pub returns: Option<Type>,
        #[serde(default, rename = "if")]
        pub conditional: Option<Conditional>,
        #[serde(default)]
        pub allow_oob: bool,
        #[serde(default)]
        pub features: Features,
        #[serde(default = "Command::gen_default")]
        pub gen: bool,
    }

    impl Command {
        fn gen_default() -> bool { true }
    }

    #[derive(Debug, Clone, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct Struct {
        #[serde(rename = "struct")]
        pub id: String,
        #[serde(default)]
        pub data: Data,
        #[serde(default)]
        pub base: DataOrType,
        #[serde(default, rename = "if")]
        pub conditional: Option<Conditional>,
        #[serde(default)]
        pub features: Features,
    }

    impl Struct {
        pub fn newtype(&self) -> Option<&Value> {
            match &self.base {
                DataOrType::Data(d) if d.fields.is_empty() => (),
                _ => return None,
            }
            self.data.newtype()
        }

        pub fn wrapper_type(&self) -> Option<&Value> {
            match self.id.ends_with("Wrapper") {
                true => self.newtype(),
                false => None,
            }
        }

        pub fn is_empty(&self) -> bool {
            self.base.is_empty() && self.data.is_empty()
        }
    }

    #[derive(Debug, Clone, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct Alternate {
        #[serde(rename = "alternate")]
        pub id: String,
        #[serde(default)]
        pub data: Data,
        #[serde(default, rename = "if")]
        pub conditional: Option<Conditional>,
    }

    #[derive(Debug, Clone, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct Enum {
        #[serde(rename = "enum")]
        pub id: String,
        #[serde(default)]
        pub data: Vec<SpecName>,
        #[serde(default, rename = "if")]
        pub conditional: Option<Conditional>,
    }

    #[derive(Debug, Clone, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct CombinedUnion {
        #[serde(rename = "union")]
        pub id: String,
        pub base: DataOrType,
        #[serde(default)]
        pub discriminator: Option<String>,
        pub data: Data,
        #[serde(default, rename = "if")]
        pub conditional: Option<Conditional>,
    }

    #[derive(Debug, Clone, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct Union {
        #[serde(rename = "union")]
        pub id: String,
        #[serde(default)]
        pub discriminator: Option<String>,
        pub data: Data,
        #[serde(default, rename = "if")]
        pub conditional: Option<Conditional>,
    }

    #[derive(Debug, Clone, Deserialize)]
    #[serde(untagged, rename_all = "kebab-case")]
    pub enum DataOrType {
        Data(Data),
        Type(Type),
    }

    impl Default for DataOrType {
        fn default() -> Self {
            DataOrType::Data(Default::default())
        }
    }

    impl DataOrType {
        pub fn is_empty(&self) -> bool {
            match self {
                &DataOrType::Data(ref data) => data.fields.is_empty(),
                &DataOrType::Type(..) => false,
            }
        }

        pub fn len(&self) -> usize {
            match self {
                &DataOrType::Data(ref data) => data.fields.len(),
                &DataOrType::Type(..) => 1,
            }
        }
    }

    #[derive(Debug, Clone, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct Event {
        #[serde(rename = "event")]
        pub id: String,
        #[serde(default)]
        pub data: Data,
        #[serde(default, rename = "if")]
        pub conditional: Option<Conditional>,
    }

    #[derive(Debug, Clone, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct PragmaWhitelist {
        pub returns_whitelist: Vec<String>,
        #[serde(default)]
        pub name_case_whitelist: Vec<String>,
    }

    #[derive(Debug, Clone, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct PragmaExceptions {
        pub command_returns_exceptions: Vec<String>,
        #[serde(default)]
        pub member_name_exceptions: Vec<String>,
    }

    #[derive(Debug, Clone, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct PragmaDocRequired {
        pub doc_required: bool,
    }

    #[derive(Debug, Clone, Deserialize)]
    #[serde(untagged, rename_all = "kebab-case")]
    pub enum SpecName {
        Name(String),
        Conditional {
            name: String,
            #[serde(rename = "if")]
            conditional: Conditional,
        },
        Explicit {
            name: String,
        },
    }

    impl fmt::Display for SpecName {
        fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
            fmt::Display::fmt(self.as_ref(), fmt)
        }
    }
    impl AsRef<str> for SpecName {
        fn as_ref(&self) -> &str {
            match self {
                SpecName::Name(name) | SpecName::Explicit { name } => &name[..],
                SpecName::Conditional { name, .. } => &name[..],
            }
        }
    }
}

pub use self::spec::Spec;

use std::path::{Path, PathBuf};
use std::ops::{Deref, DerefMut};
use std::io;

pub struct Parser {
    data: String,
    pos: usize,
    eof: bool,
}

impl Parser {
    pub fn from_string<S: Into<String>>(s: S) -> Self {
        Parser {
            data: s.into(),
            pos: 0,
            eof: false,
        }
    }

    pub fn strip_comments(s: &str) -> String {
        let lines: Vec<String> = s.lines()
            .filter(|l| !l.trim().starts_with("#") && !l.trim().is_empty())
            .map(|s| s.replace("'", "\""))
            .map(|s| if let Some(i) = s.find('#') {
                s[..i].to_owned()
            } else {
                s
            }).collect();
        lines.join("\n")
    }
}

impl Iterator for Parser {
    type Item = serde_json::Result<Spec>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.eof {
            None
        } else {
            Some(match serde_json::from_str(&self.data[self.pos..]) {
                Ok(res) => {
                    self.eof = true;
                    Ok(res)
                },
                Err(e) => {
                    let (line, col) = (e.line(), e.column());
                    if line == 0 || col == 0 {
                        Err(e)
                    } else {
                        let count: usize =  self.data[self.pos..].lines().map(|l| l.len() + 1).take(line - 1).sum();
                        let str = &self.data[self.pos .. (self.pos + count + col - 1)];
                        self.pos += count;
                        serde_json::from_str(str)
                    }
                },
            })
        }
    }
}

pub trait QemuRepo {
    type Error;

    fn push_context<P: AsRef<Path>>(&mut self, p: P);
    fn pop_context(&mut self);
    fn context(&self) -> &Path;

    fn include<P: AsRef<Path>>(&mut self, p: P) -> Result<(QemuRepoContext<Self>, String), Self::Error>;
}

#[derive(Debug, Clone)]
pub struct QemuFileRepo {
    paths: Vec<PathBuf>,
}

pub struct QemuRepoContext<'a, R: QemuRepo + ?Sized + 'a> {
    repo: &'a mut R,
}

impl<'a, R: QemuRepo + ?Sized + 'a> QemuRepoContext<'a, R> {
    pub fn from_include<P: AsRef<Path>>(repo: &'a mut R, path: P) -> (Self, PathBuf) {
        let path = path.as_ref();
        let include_path = repo.context().join(path);
        repo.push_context(include_path.parent().unwrap());

        (
            QemuRepoContext {
                repo,
            },
            include_path,
        )
    }
}

impl<'a, R: QemuRepo + ?Sized + 'a> Deref for QemuRepoContext<'a, R> {
    type Target = R;

    fn deref(&self) -> &Self::Target {
        self.repo
    }
}

impl<'a, R: QemuRepo + ?Sized + 'a> DerefMut for QemuRepoContext<'a, R> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.repo
    }
}

impl<'a, R: QemuRepo + ?Sized + 'a> Drop for QemuRepoContext<'a, R> {
    fn drop(&mut self) {
        self.repo.pop_context();
    }
}

impl QemuFileRepo {
    pub fn new<P: Into<PathBuf>>(p: P) -> Self {
        QemuFileRepo {
            paths: vec![p.into()],
        }
    }
}

impl QemuRepo for QemuFileRepo {
    type Error = io::Error;

    fn push_context<P: AsRef<Path>>(&mut self, p: P) {
        self.paths.push(p.as_ref().to_owned());
    }

    fn pop_context(&mut self) {
        self.paths.pop();
        assert!(!self.paths.is_empty());
    }

    fn context(&self) -> &Path {
        self.paths.last().unwrap()
    }

    fn include<P: AsRef<Path>>(&mut self, p: P) -> Result<(QemuRepoContext<Self>, String), Self::Error> {
        use std::fs::File;
        use std::io::Read;

        let (context, path) = QemuRepoContext::from_include(self, p);
        let mut f = File::open(path)?;
        let mut str = String::new();
        f.read_to_string(&mut str)?;
        Ok((context, str))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::path::Path;

    fn parse_include<P: AsRef<Path>>(repo: &mut QemuFileRepo, include: P) {
        let include = include.as_ref();
        println!("including {}", include.display());

        let (mut context, schema) = repo.include(include).expect("include path not found");
        for item in Parser::from_string(Parser::strip_comments(&schema)) {
            match item.expect("schema parse failure") {
                Spec::Include(inc) => parse_include(&mut context, inc.include),
                item => println!("decoded {:?}", item),
            }
        }
    }

    fn parse_schema(mut repo: QemuFileRepo) {
        parse_include(&mut repo, "qapi-schema.json");
    }

    #[test]
    fn parse_qapi() {
        parse_schema(QemuFileRepo::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../schema/qapi/")));
    }

    #[test]
    fn parse_qga() {
        parse_schema(QemuFileRepo::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../schema/qga/")));
    }
}
