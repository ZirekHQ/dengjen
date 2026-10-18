use std::env;

const PROTO_FILE: &str = "proto/dengjen_grpc.proto";

fn main() {
    // protoc 3.12 (Ubuntu 22.04) rejects proto3 optional fields without this flag; newer protoc ignores it.
    let compiled = tonic_prost_build::configure()
        .protoc_arg("--experimental_allow_proto3_optional")
        .compile_protos(&[PROTO_FILE], &["proto"]);
    if let Err(err) = compiled {
        panic!("failed to compile gRPC protobuf definitions in {PROTO_FILE}: {err:?}");
    }
    if env::var_os("CARGO_FEATURE_STT").is_some() {
        emit_runtime_search_path();
    }
}

// sherpa-onnx-sys's own rpath applies only to its own targets, not to this binary.
fn emit_runtime_search_path() {
    match env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("linux") => println!("cargo:rustc-link-arg-bins=-Wl,-rpath,$ORIGIN"),
        Ok("macos") => println!("cargo:rustc-link-arg-bins=-Wl,-rpath,@loader_path"),
        _ => {}
    }
}
