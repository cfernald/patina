//! Test that &Storage and &mut Storage parameters cannot be mixed.

use patina::{
    base::error::Result,
    component::{Storage, component},
};

pub struct TestComponent;

#[component]
impl TestComponent {
    fn entry_point(self, _s1: &Storage, _s2: &mut Storage) -> Result<()> {
        Ok(())
    }
}

fn main() {}
