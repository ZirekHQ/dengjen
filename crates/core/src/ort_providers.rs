/// ONNX Runtime execution providers enabled by the `cuda`, `directml` and
/// `coreml` features, in that priority order.
///
/// Empty when no GPU feature is enabled, which leaves ONNX Runtime on its CPU
/// provider.
#[allow(clippy::vec_init_then_push)]
pub fn execution_providers() -> Vec<ort::ep::ExecutionProviderDispatch> {
    #[allow(unused_mut)]
    let mut providers = Vec::new();
    #[cfg(feature = "cuda")]
    providers.push(ort::ep::CUDA::default().build());
    #[cfg(feature = "directml")]
    providers.push(ort::ep::DirectML::default().build());
    #[cfg(feature = "coreml")]
    providers.push(ort::ep::CoreML::default().build());
    providers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(not(any(feature = "cuda", feature = "directml", feature = "coreml")))]
    fn execution_providers_is_empty_when_no_gpu_feature_is_enabled() {
        assert!(execution_providers().is_empty());
    }
}
