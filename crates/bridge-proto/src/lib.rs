#![allow(clippy::all)]

pub mod rpc {
    tonic::include_proto!("cash.z.wallet.sdk.rpc");
}

pub use rpc::*;
