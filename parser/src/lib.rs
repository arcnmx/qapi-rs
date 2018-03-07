#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate serde;

pub mod spec {
    use std::collections::HashMap;
    use std::fmt;
    use serde::de::{Deserializer, Deserialize, Visitor, SeqAccess, Error};

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
    }

    impl<'de> Deserialize<'de> for Data {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            <HashMap<String, Type>>::deserialize(d).map(|h| Data {
                fields: h.into_iter().map(|(n, t)| Value::new(&n, t)).collect(),
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
                name: name,
                ty: ty,
                optional: opt,
            }
        }
    }

    #[derive(Clone)]
    pub struct Type {
        pub name: String,
        pub is_array: bool,
    }

    impl fmt::Debug for Type {
        fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
            if self.is_array {
                write!(fmt, "[{}]", self.name)
            } else {
                write!(fmt, "{}", self.name)
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
                    })
                }

                fn visit_string<E: Error>(self, v: String) -> Result<Self::Value, E> {
                    Ok(Type {
                        name: v,
                        is_array: false,
                    })
                }

                fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                    let v = seq.next_element::<String>()?;
                    if let Some(v) = v {
                        if seq.next_element::<String>()?.is_none() {
                            Ok(Type {
                                name: v,
                                is_array: true,
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
    }

    #[derive(Debug, Clone, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct Struct {
        #[serde(rename = "struct")]
        pub id: String,
        #[serde(default)]
        pub data: Data,
    }

    #[derive(Debug, Clone, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct Alternate {
        #[serde(rename = "alternate")]
        pub id: String,
        #[serde(default)]
        pub data: Data,
    }

    #[derive(Debug, Clone, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct Enum {
        #[serde(rename = "enum")]
        pub id: String,
        #[serde(default)]
        pub data: Vec<String>,
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
    }

    #[derive(Debug, Clone, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct Union {
        #[serde(rename = "union")]
        pub id: String,
        #[serde(default)]
        pub discriminator: Option<String>,
        pub data: Data,
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

    #[derive(Debug, Clone, Deserialize)]
    #[serde(rename_all = "kebab-case")]
    pub struct Event {
        #[serde(rename = "event")]
        pub id: String,
        #[serde(default)]
        pub data: Data,
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
    pub struct PragmaDocRequired {
        pub doc_required: bool,
    }
}

pub use spec::Spec;

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
                repo: repo,
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
        parse_schema(QemuFileRepo::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../schema/")));
    }

    #[test]
    fn parse_qga() {
        parse_schema(QemuFileRepo::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../schema/qga/")));
    }
}
