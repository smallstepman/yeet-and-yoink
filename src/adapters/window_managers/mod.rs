//! Adapter-owned window manager glue.
//!
//! The shared capability/planning contract is engine-owned and must not be
//! imported from this adapter module.
//!
//! ```compile_fail
//! use yeet_and_yoink::adapters::window_managers::WindowManagerCapabilities;
//! ```
//!
//! ```compile_fail
//! use yeet_and_yoink::adapters::window_managers::plan_tear_out;
//! ```
//!
#[cfg(target_os = "linux")]
pub mod i3;
#[cfg(any(test, target_os = "linux"))]
pub mod niri;
#[cfg(target_os = "macos")]
pub mod paneru;
#[cfg(target_os = "macos")]
pub mod yabai;

#[cfg(any(test, target_os = "linux"))]
pub use self::niri::NiriAdapter;

use anyhow::{anyhow, Context, Result};

#[cfg(target_os = "linux")]
use crate::adapters::window_managers::i3::I3_SPEC;
#[cfg(target_os = "linux")]
use crate::adapters::window_managers::niri::NIRI_SPEC;
#[cfg(target_os = "macos")]
use crate::adapters::window_managers::paneru::PANERU_SPEC;
#[cfg(target_os = "macos")]
use crate::adapters::window_managers::yabai::YABAI_SPEC;
use crate::config::{selected_wm_backend, WmBackend};
use crate::engine::window_manager::{ConfiguredWindowManager, WindowManagerSpec};

struct UnsupportedWindowManagerSpec {
    backend: WmBackend,
    name: &'static str,
}

impl WindowManagerSpec for UnsupportedWindowManagerSpec {
    fn backend(&self) -> WmBackend {
        self.backend
    }

    fn name(&self) -> &'static str {
        self.name
    }

    fn connect(&self) -> Result<ConfiguredWindowManager> {
        Err(anyhow!(
            "wm backend '{}' is not supported on {}",
            self.name,
            std::env::consts::OS
        ))
    }
}

#[cfg(not(target_os = "linux"))]
static UNSUPPORTED_NIRI_SPEC: UnsupportedWindowManagerSpec = UnsupportedWindowManagerSpec {
    backend: WmBackend::Niri,
    name: "niri",
};
#[cfg(not(target_os = "linux"))]
static UNSUPPORTED_I3_SPEC: UnsupportedWindowManagerSpec = UnsupportedWindowManagerSpec {
    backend: WmBackend::I3,
    name: "i3",
};
#[cfg(not(target_os = "macos"))]
static UNSUPPORTED_PANERU_SPEC: UnsupportedWindowManagerSpec = UnsupportedWindowManagerSpec {
    backend: WmBackend::Paneru,
    name: "paneru",
};
#[cfg(not(target_os = "macos"))]
static UNSUPPORTED_YABAI_SPEC: UnsupportedWindowManagerSpec = UnsupportedWindowManagerSpec {
    backend: WmBackend::Yabai,
    name: "yabai",
};

pub fn spec_for_backend(backend: WmBackend) -> &'static dyn WindowManagerSpec {
    match backend {
        WmBackend::Niri => {
            #[cfg(target_os = "linux")]
            {
                &NIRI_SPEC
            }
            #[cfg(not(target_os = "linux"))]
            {
                &UNSUPPORTED_NIRI_SPEC
            }
        }
        WmBackend::I3 => {
            #[cfg(target_os = "linux")]
            {
                &I3_SPEC
            }
            #[cfg(not(target_os = "linux"))]
            {
                &UNSUPPORTED_I3_SPEC
            }
        }
        WmBackend::Paneru => {
            #[cfg(target_os = "macos")]
            {
                &PANERU_SPEC
            }
            #[cfg(not(target_os = "macos"))]
            {
                &UNSUPPORTED_PANERU_SPEC
            }
        }
        WmBackend::Yabai => {
            #[cfg(target_os = "macos")]
            {
                &YABAI_SPEC
            }
            #[cfg(not(target_os = "macos"))]
            {
                &UNSUPPORTED_YABAI_SPEC
            }
        }
    }
}

fn connect_backend(
    backend: WmBackend,
    spec: &'static dyn WindowManagerSpec,
) -> Result<ConfiguredWindowManager> {
    if spec.backend() != backend {
        return Err(anyhow!(
            "wm backend '{}' resolved to mismatched spec '{}'",
            backend.as_str(),
            spec.name()
        ));
    }

    spec.connect()
        .with_context(|| format!("failed to connect configured wm '{}'", spec.name()))
}

#[cfg(test)]
fn connect_backend_for_test(
    backend: WmBackend,
    spec: &'static dyn WindowManagerSpec,
) -> Result<ConfiguredWindowManager> {
    connect_backend(backend, spec)
}

pub fn connect_selected() -> Result<ConfiguredWindowManager> {
    let _span = tracing::debug_span!("window_managers.connect_selected").entered();
    let backend = selected_wm_backend();
    let spec = spec_for_backend(backend);
    connect_backend(backend, spec)
}

#[cfg(test)]
mod tests {
    use super::{ConfiguredWindowManager, WindowManagerSpec};
    use crate::config::WmBackend;
    use crate::engine::window_manager::{
        CapabilitySupport, WindowCycleProvider, WindowManagerCapabilityDescriptor,
        WindowManagerSession, WindowTearOutComposer,
    };
    use anyhow::Result;

    #[test]
    fn built_in_connectors_are_typed_as_configured_window_managers() {
        fn assert_spec(_spec: &'static dyn WindowManagerSpec) {}

        assert_spec(super::spec_for_backend(WmBackend::Niri));
        assert_spec(super::spec_for_backend(WmBackend::I3));
        assert_spec(super::spec_for_backend(WmBackend::Paneru));
        assert_spec(super::spec_for_backend(WmBackend::Yabai));
        let _ = super::connect_selected as fn() -> Result<ConfiguredWindowManager>;
    }

    #[test]
    fn connect_selected_reports_configured_backend_failure_without_fallback() {
        let err = match connect_backend_for_test(WmBackend::Niri, failing_spec(WmBackend::Niri)) {
            Ok(_) => panic!("configured backend should fail without fallback"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("niri"));
        assert!(!err.to_string().contains("i3"));
    }

    #[test]
    fn failing_spec_uses_requested_backend() {
        let spec = failing_spec(WmBackend::Yabai);

        assert_eq!(spec.backend(), WmBackend::Yabai);
        assert_eq!(spec.name(), "yabai");
    }

    #[test]
    fn niri_backend_wrapper_remains_available_from_adapter_boundary() {
        fn assert_niri_traits<T>()
        where
            T: WindowManagerCapabilityDescriptor
                + WindowManagerSession
                + WindowCycleProvider
                + WindowTearOutComposer,
        {
        }

        type Adapter = crate::adapters::window_managers::NiriAdapter;

        assert_niri_traits::<Adapter>();

        let spec = super::spec_for_backend(WmBackend::Niri);
        let capabilities = <Adapter as WindowManagerCapabilityDescriptor>::CAPABILITIES;

        assert_eq!(spec.backend(), WmBackend::Niri);
        assert_eq!(spec.name(), <Adapter as WindowManagerCapabilityDescriptor>::NAME);
        capabilities
            .validate()
            .expect("re-exported niri adapter capabilities should stay valid after relocation");
        assert_eq!(capabilities.tear_out.east, CapabilitySupport::Native);
        assert_eq!(capabilities.tear_out.west, CapabilitySupport::Composed);
        assert!(capabilities.primitives.move_column);
        assert!(capabilities.primitives.consume_into_column_and_move);
    }

    fn connect_backend_for_test(
        backend: WmBackend,
        spec: &'static dyn WindowManagerSpec,
    ) -> Result<ConfiguredWindowManager> {
        super::connect_backend_for_test(backend, spec)
    }

    fn failing_spec(backend: WmBackend) -> &'static dyn WindowManagerSpec {
        Box::leak(Box::new(FailingSpec { backend }))
    }

    struct FailingSpec {
        backend: WmBackend,
    }

    impl WindowManagerSpec for FailingSpec {
        fn backend(&self) -> WmBackend {
            self.backend
        }

        fn name(&self) -> &'static str {
            self.backend.as_str()
        }

        fn connect(&self) -> Result<ConfiguredWindowManager> {
            Err(anyhow::anyhow!("{} connection failed", self.name()))
        }
    }
}
