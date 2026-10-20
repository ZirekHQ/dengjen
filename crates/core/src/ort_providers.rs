use ort::environment::Environment;
use ort::session::{
    builder::{AutoDevicePolicy, SessionBuilder},
    Session,
};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

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

static ENVIRONMENT: OnceLock<Environment> = OnceLock::new();
static ENVIRONMENT_INIT: Mutex<()> = Mutex::new(());

fn environment_guard() -> MutexGuard<'static, ()> {
    ENVIRONMENT_INIT
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// The process-wide ONNX Runtime environment, created with
/// [`execution_providers`] on first use unless [`install_environment`] ran first.
pub fn environment() -> ort::Result<&'static Environment> {
    if let Some(environment) = ENVIRONMENT.get() {
        return Ok(environment);
    }
    let _guard = environment_guard();
    if let Some(environment) = ENVIRONMENT.get() {
        return Ok(environment);
    }
    let created = ort::init()
        .with_name("dengjen")
        .with_execution_providers(execution_providers())
        .build()?;
    Ok(ENVIRONMENT.get_or_init(|| created))
}

/// A session builder on the process-wide environment that prefers the CPU and registers
/// [`execution_providers`], so automatic device selection never picks an accelerator the
/// model cannot run on.
pub fn session_builder() -> ort::Result<SessionBuilder> {
    let builder = Session::builder(environment()?)?
        .with_auto_device(AutoDevicePolicy::PreferCPU)?
        .with_execution_providers(execution_providers())?;
    Ok(builder)
}

/// Makes `environment` the process-wide one. Fails when one is already in place.
pub fn install_environment(environment: Environment) -> ort::Result<()> {
    let _guard = environment_guard();
    ENVIRONMENT
        .set(environment)
        .map_err(|_| ort::Error::new("the onnxruntime environment is already initialized"))
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
