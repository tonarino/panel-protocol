# panel-protocol

The protocol used for USB communication between the main tonari system and its hardware controller for lighting, volume, dial input, etc.

This can be used in a `no_std` environment so it's compatible with both normal desktop projects as well as embedded firmware.

## Dependencies

* [cargo, rustc](https://rustup.rs)

## Build

```
cargo build
```

## Test

```
cargo test
```

## Code Linting

```
cargo clippy
```

## Code Formatting

```
cargo +nightly fmt
```
