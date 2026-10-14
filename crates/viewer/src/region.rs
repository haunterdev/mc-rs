//! Shim: the 3x3 chunk neighborhood and block properties live in `sim::region`.

pub use sim::region::*;

#[cfg(test)]
pub mod tests {
    pub use sim::testing::*;
}
