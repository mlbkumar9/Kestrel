//! Local widget kit. Stateless pieces live in `basic`, controls generic over their
//! owning view in `controls`, and the text input entity in `input`.

pub mod basic;
pub mod controls;
pub mod input;
pub mod overlay;
pub mod table;

pub use basic::*;
pub use controls::*;
pub use table::{Col, body_row, header_row, scroll_table};
