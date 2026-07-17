//! Test that &mut Storage with Config<T> is rejected at compile time.

use patina::{
    base::error::Result,
    component::{Storage, component, params::Config},
};

pub struct TestComponent;

#[component]
impl TestComponent {
    fn entry_point(self, _storage: &mut Storage, _config: Config<u32>) -> Result<()> {
        Ok(())
    }
}

fn main() {}
