//! Headless explicit-tick foundation; no gameplay or movement model is provided.
//! Floating-point results are not promised to be bit-exact across platforms.

pub mod collision;
pub mod input;
pub mod reference;
pub mod tick;

pub use tick::{MarioState, Runner, TickModel};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("nonfinite state field: {field}")]
    NonFiniteState { field: &'static str },
    #[error("tick counter overflow")]
    TickOverflow,
    #[error("model error: {0}")]
    Model(String),
}
