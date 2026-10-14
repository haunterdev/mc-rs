//! The game simulation the viewer and the server share, ported 1:1 from 1.12.2. Every module is
//! declared here once.

pub mod collision;
pub mod entity;
pub mod raytrace;
pub mod region;
pub mod terrain;
#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub mod kinds;
pub mod lang;

pub mod inventory;
pub mod item_entity;
pub mod mining;

pub mod container;
