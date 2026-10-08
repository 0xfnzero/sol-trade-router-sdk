# Local Windows compatibility patch

This is a copy of `yellowstone-grpc-client` 13.5.0. The original crate imports
`tokio::net::UnixStream` and compiles `connect_uds` on every target, although
Unix domain sockets are unavailable on Windows.

Only `src/lib.rs` is changed: the Unix-only imports and `connect_uds` method
are gated with `#[cfg(unix)]`; the `Uri` import remains available for the
`test-tools` feature. The public TCP/gRPC API is unchanged.

The workspace root uses `[patch.crates-io]` to select this copy. Remove the
patch when an upstream release supports Windows directly.
