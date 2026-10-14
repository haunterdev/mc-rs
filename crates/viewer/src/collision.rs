//! Shim: block collision lives in `sim::collision`.

pub use sim::collision::*;

#[cfg(test)]
pub mod tests {
    pub use sim::testing::*;
}
