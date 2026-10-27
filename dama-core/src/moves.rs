//! A single move, fully self-describing so `undo` needs no extra state.

use crate::piece::Piece;

pub type Square = (usize, usize);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Move {
    pub from: Square,
    pub to: Square,
    pub piece: Piece,
    /// If the move captures, `(captured piece, square it lived on)`.
    /// For en passant and dama takes this is NOT `to`.
    pub captured: Option<(Piece, Square)>,
    pub is_promotion: bool,
}

impl Move {
    pub fn simple(from: Square, to: Square, piece: Piece) -> Self {
        Self { from, to, piece, captured: None, is_promotion: false }
    }
}
