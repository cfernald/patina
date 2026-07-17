//! Test that duplicate StandardRuntimeServices parameters are rejected at compile time.

use patina::{base::error::Result, component::component, uefi::runtime_services::StandardRuntimeServices};

pub struct TestComponent;

#[component]
impl TestComponent {
    fn entry_point(self, _rt1: StandardRuntimeServices, _rt2: StandardRuntimeServices) -> Result<()> {
        Ok(())
    }
}

fn main() {}
