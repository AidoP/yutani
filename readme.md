# Wl
`wl` is a Wayland client and server library for Rust.

This is **not** a Rust binding for libwayland, rather, this crate is a standalone implementation of the Wayland Protocol.
`wl` consists of primitives for communication using the Wayland wire protocol and a [macro](https://github.com/AidoP/wl-macro) for generating glue code from Wayland protocol descriptions in the TOML format.

# Why TOML

[Wayland protocols](https://gitlab.freedesktop.org/wayland/wayland-protocols) are officially described in XML, so why use TOML for this crate?

- XML is not a good format. Though somewhat opinion, there is objective reasons such as its unnecessary verbosity.
- XML is difficult to parse. There are no correct XML parsers for [Serde](https://serde.rs/) - they fail to parse the valid XML protocol specifications.
- Wayland is supposed to be for the future, XML will keep it stuck in the past. TOML interfaces may also be useful for other wayland-scanner alternatives in other new languages.
- TOML is a perfect fit with the array of maps shorthand making specifications neat and easier to read as a human.
- Converting XML specifications to TOML ahead of time is trivial.
- I just do not want to use XML.

Wayland Protocols converted to TOML are available under [wl-protocols](https://github.com/AidoP/wl-protocols). The default search path for protocol specifications is `protocol/` and can be overriden by setting the `WL_PROTOCOLS` environment variable.

# Example Usage

A minimal example implementing the assumed `wl_display` bootstrapping interface is available at [`examples/basic/`](https://github.com/AidoP/wl/tree/main/examples/basic).