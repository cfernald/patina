//! Test that &Storage with ConfigMut<T> is rejected at compile time.

use patina::{
    base::error::Result,
    component::{Storage, component, params::ConfigMut},
};

pub struct TestComponent;

#[component]
impl TestComponent {
    fn entry_point(self, _storage: &Storage, _config: ConfigMut<u32>) -> Result<()> {
        Ok(())
    }
}

fn main() {}
