//! Test that duplicate &mut Storage parameters are rejected at compile time.

use patina::{
    base::error::Result,
    component::{Storage, component},
};

pub struct TestComponent;

#[component]
impl TestComponent {
    fn entry_point(self, _s1: &mut Storage, _s2: &mut Storage) -> Result<()> {
        Ok(())
    }
}

fn main() {}
