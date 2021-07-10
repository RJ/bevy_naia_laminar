# bevy_naia_laminar

Server and client bevy plugins using:

* [laminar](https://github.com/amethyst/laminar) - an application-level transport protocol which provides configurable reliability and ordering guarantees built on top of UDP
* [naia-socket](https://github.com/naia-rs/naia-socket) - a transport that uses native sockets, or WebRTC if targetting wasm (udp in the browser)

## Status

I'm only just digging in to laminar, not had a chance to explore it yet. But wanted to make sure it'll run ok on a wasm target. Any and all feedback/patches welcome. Will publish the crate in due course.

## Running examples

### Native UDP

**SERVER**

```
cargo run --example server
```

**CLIENT**

Edit the examples/client.rs and set the IP to what the server reports it's bound to, then:

```
cargo run --example client
```

### Wasm / WebRTC:

**SERVER**

```
cargo run --example server --features use-webrtc --no-default-features
```

**CLIENT**

Edit the examples/client.rs and set the IP to what the server reports it's bound to, then:

```
cargo build --example client --target wasm32-unknown-unknown --no-default-features --features use-webrtc
wasm-bindgen --out-dir target --target web target/wasm32-unknown-unknown/debug/examples/client.wasm
cargo install simple-http-server
simple-http-server -p 4000 --cors
```

Now visit `http://your.ip.address.here:4000/index.html` and open the devtools console. (127.0.0.1 is problematic for WebRTC in some browsers - use the IP the server bound to).


## Notes

Cargo.toml uses my [wasm branch of laminar](https://github.com/RJ/laminar/tree/wasm), a fairly small change to allow laminar to compile for wasm32.

## See Also

* [bevy_networking_turbulence](https://github.com/smokku/bevy_networking_turbulence) that uses turbulence's abstractions over UDP, that also uses naia, and works on wasm.
