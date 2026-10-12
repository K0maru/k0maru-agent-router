use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use k0maru_router::error::RouterError;
use k0maru_router::memory::{MemoryHook, NoopMemoryHook, OptionalMemoryHook};

#[derive(Debug)]
struct MockProvider {
    name: &'static str,
    available: bool,
    context_to_return: Option<String>,
    should_error: bool,
    called: Arc<AtomicBool>,
}

impl MockProvider {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            available: true,
            context_to_return: None,
            should_error: false,
            called: Arc::new(AtomicBool::new(false)),
        }
    }

    fn with_available(mut self, available: bool) -> Self {
        self.available = available;
        self
    }

    fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context_to_return = Some(context.into());
        self
    }

    fn with_error(mut self, should_error: bool) -> Self {
        self.should_error = should_error;
        self
    }
}

impl MemoryHook for MockProvider {
    fn name(&self) -> &'static str {
        self.name
    }

    fn is_available(&self) -> bool {
        self.available
    }

    fn preload_context(
        &self,
        _workspace: &Path,
        _project_name: &str,
    ) -> Result<Option<String>, RouterError> {
        self.called.store(true, Ordering::SeqCst);
        if self.should_error {
            Err(RouterError::DriverExecutionFailed(
                "Simulated memory hub SQLite lock / process crash".into(),
            ))
        } else {
            Ok(self.context_to_return.clone())
        }
    }
}

#[test]
fn test_noop_memory_hook_returns_none_zero_overhead() {
    let hook = NoopMemoryHook;
    assert_eq!(hook.name(), "noop");
    assert!(!hook.is_available());

    let res = hook.preload_context(Path::new("/workspace"), "billing");
    assert_eq!(res.unwrap(), None);
}

#[test]
fn test_optional_memory_hook_success() {
    let provider = MockProvider::new("k0maru-memory")
        .with_context("Domain Invariants:\n- Permission Wall\n- Safety Staging Gate");
    let called = provider.called.clone();

    let hook = OptionalMemoryHook::new(provider);
    assert_eq!(hook.name(), "k0maru-memory");
    assert!(hook.is_available());

    let context = hook
        .preload_context(Path::new("/workspace/billing"), "billing")
        .unwrap();
    assert!(called.load(Ordering::SeqCst));
    assert!(context.is_some());
    assert!(context.unwrap().contains("Domain Invariants"));
}

#[test]
fn test_optional_memory_hook_unavailable_provider() {
    let provider = MockProvider::new("k0maru-memory")
        .with_available(false)
        .with_context("Some context");
    let called = provider.called.clone();

    let hook = OptionalMemoryHook::new(provider);
    assert!(!hook.is_available());

    let res = hook
        .preload_context(Path::new("/workspace"), "billing")
        .unwrap();
    assert_eq!(res, None);
    assert!(!called.load(Ordering::SeqCst));
}

#[test]
fn test_optional_memory_hook_degrades_gracefully_on_error() {
    let provider = MockProvider::new("failing-memory-hub")
        .with_available(true)
        .with_error(true);
    let called = provider.called.clone();

    let hook = OptionalMemoryHook::new(provider);
    assert!(hook.is_available());

    // When the underlying provider fails, OptionalMemoryHook MUST NOT return Err or panic!
    let res = hook.preload_context(Path::new("/workspace"), "crashed-project");
    assert!(called.load(Ordering::SeqCst));
    assert!(
        res.is_ok(),
        "OptionalMemoryHook must degrade to Ok(None) upon provider error"
    );
    assert_eq!(res.unwrap(), None);
}

#[test]
fn test_optional_memory_hook_disabled_toggle() {
    let provider = MockProvider::new("k0maru-memory")
        .with_available(true)
        .with_context("Important context");
    let called = provider.called.clone();

    let hook = OptionalMemoryHook::new(provider).with_enabled(false);
    assert!(!hook.is_available());
    assert!(!hook.is_enabled());

    let res = hook
        .preload_context(Path::new("/workspace"), "billing")
        .unwrap();
    assert_eq!(res, None);
    assert!(!called.load(Ordering::SeqCst));
}
