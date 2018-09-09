# qapi-rs

[![travis-badge][]][travis] [![release-badge][]][cargo] [![docs-badge][]][docs] [![license-badge][]][license]

A rust library for interfacing with [QEMU](https://www.qemu.org/) QAPI sockets.

## [Documentation][docs]

See the [documentation][docs] for up to date information, as well as the
reference documentation for both the [QEMU Machine Protocol](https://qemu.weilnetz.de/doc/qemu-qmp-ref.html)
and [Guest Agent](https://qemu.weilnetz.de/doc/qemu-ga-ref.html) APIs.

There are two features (`qga` and `qmp`) which enable their respective functionality.
They can be enabled in your `Cargo.toml`:

```toml
[dependencies]
qapi = { version = "0.2.0", features = [ "qmp" ] }
```

### Examples

Short examples are available for both [QMP](examples/qmp_query.rs) and [Guest
Agent](examples/guest_info.rs). Async/nonblocking examples using tokio [are also
available](tokio/examples/).

[travis-badge]: https://img.shields.io/travis/arcnmx/qapi-rs/master.svg?style=flat-square
[travis]: https://travis-ci.org/arcnmx/qapi-rs
[release-badge]: https://img.shields.io/crates/v/qapi.svg?style=flat-square
[cargo]: https://crates.io/crates/qapi
[docs-badge]: https://img.shields.io/badge/API-docs-blue.svg?style=flat-square
[docs]: http://arcnmx.github.io/qapi-rs/qapi/
[license-badge]: https://img.shields.io/badge/license-MIT-ff69b4.svg?style=flat-square
[license]: https://github.com/arcnmx/qapi-rs/blob/master/COPYING
