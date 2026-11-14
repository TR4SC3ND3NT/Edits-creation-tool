//! The window's logic that has no window in it, ported from the egui window
//! (`crates/edits-gui`) so both edit projects the same way: the edits
//! (`editing`), time arithmetic (`timemap`), timeline rows (`rows`) and live
//! reload (`reload`). Their tests stay with the originals in `edits-gui`.

#![allow(dead_code)]

pub mod editing;
pub mod reload;
pub mod rows;
pub mod timemap;
