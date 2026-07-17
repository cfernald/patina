//! Test that duplicate Commands parameters are rejected at compile time.

use patina::{
    base::error::Result,
    component::{component, params::Commands},
};

pub struct TestComponent;

#[component]
impl TestComponent {
    fn entry_point(self, _cmd1: Commands, _cmd2: Commands) -> Result<()> {
        Ok(())
    }
}

fn main() {}
