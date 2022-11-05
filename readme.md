# wl
A pure-Rust Wayland library.

## Stability
Note: `wl` is still in pre-`0.1.0` versions and as such breaking changes may be made often.

`wl` also depends on `syslib` for interfacing with Linux rather than `libc` which may be a stability and security concern for some.

## Usage

Include `wl` in your `Cargo.toml`
```toml
wl = { github = "git@github.com:AidoP/wl.git" }
```

[wl-codegen](https://github.com/AidoP/wl-codegen) (and by extension, [wl-protocols](https://github.com/AidoP/wl-protocols)) will be needed if you want to generate dispatch glue / boilerplate.

## Examples

See [wayvk](https://github.com/AidoP/wayvk).