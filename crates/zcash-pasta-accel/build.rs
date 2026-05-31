fn main() {
    println!("cargo:rerun-if-env-changed=CUDA_HOME");
    println!("cargo:rerun-if-env-changed=CUDA_PATH");

    if std::env::var_os("CARGO_FEATURE_CUDA").is_some() {
        println!("cargo:rerun-if-changed=cuda");
    }
}
