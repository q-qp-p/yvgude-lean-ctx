//! `lean-ctx embeddings` — semantic-embedding runtime management (GH #732).

pub(crate) fn cmd_embeddings(rest: &[String]) {
    let _ = rest;
    eprintln!("Embeddings provisioning is unavailable (addon subsystem removed).");
    eprintln!("Set ORT_DYLIB_PATH to a locally installed ONNX Runtime instead.");
    std::process::exit(1);
}
