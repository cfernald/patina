//! Test that &mut Storage with ConfigMut<T> is rejected at compile time.

use patina::{
    base::error::Result,
    component::{Storage, component, params::ConfigMut},
};

pub struct TestComponent;

#[component]
impl TestComponent {
    fn entry_point(self, _storage: &mut Storage, _config: ConfigMut<u32>) -> Result<()> {
        Ok(())
    }
}

fn main() {}
