#![forbid(unsafe_code)]
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

mod lang;
mod model;
mod safety;
mod store;

#[cfg(target_arch = "wasm32")]
mod runtime;
