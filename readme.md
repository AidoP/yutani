# Yutani
A pure-Rust Wayland library.

## Stability
Note: `yutani` is still in pre-`0.1.0` versions and as such breaking changes may be made often.

`yutani` also depends on `syslib` for interfacing with Linux rather than `libc` which may be a stability and security concern for some.

## Usage

Include `yutani` in your `Cargo.toml`
```toml
yutani = { github = "git@github.com:AidoP/yutani.git" }
```

[yutani-codegen](https://github.com/AidoP/yutani-codegen) (and by extension, [yutani-protocols](https://github.com/AidoP/yutani-protocols)) will be needed if you want to generate dispatch glue / boilerplate.

## Examples

See [wayvk](https://github.com/AidoP/wayvk).