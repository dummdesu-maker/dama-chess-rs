//! Pure game rules for Dama-Chess. No I/O, no UI, no external deps.
//!
//! The public API is `GameState` plus `apply` / `choose_promotion` /
//! `place_king` / `undo`. Everything else is an implementation detail
//! and can change freely.

pub mod piece;
pub mod moves;
pub mod state;

pub use piece::{Color, Kind, Piece};
pub use moves::{Move, Square};
pub use state::{GameState, Outcome};
