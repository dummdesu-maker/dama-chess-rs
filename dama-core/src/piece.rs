//! Piece primitives. The whole `"--"` string-sentinel of the Python
//! original is replaced by `Option<Piece>` in the board.

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Color {
    White,
    Black,
}

impl Color {
    pub fn opposite(self) -> Self {
        match self {
            Color::White => Color::Black,
            Color::Black => Color::White,
        }
    }

    pub fn char(self) -> char {
        match self {
            Color::White => 'w',
            Color::Black => 'b',
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Kind {
    Pawn,
    Knight,
    Bishop,
    Rook,
    Queen,
    King,
}

impl Kind {
    pub fn char(self) -> char {
        match self {
            Kind::Pawn => 'P',
            Kind::Knight => 'N',
            Kind::Bishop => 'B',
            Kind::Rook => 'R',
            Kind::Queen => 'Q',
            Kind::King => 'K',
        }
    }

    /// Filled Unicode glyphs (♚♛♜♝♞♟). Colour is applied by the UI.
    pub fn unicode(self) -> &'static str {
        match self {
            Kind::Pawn => "\u{265F}",   // ♟
            Kind::Knight => "\u{265E}", // ♞
            Kind::Bishop => "\u{265D}", // ♝
            Kind::Rook => "\u{265C}",   // ♜
            Kind::Queen => "\u{265B}",  // ♛
            Kind::King => "\u{265A}",   // ♚
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Piece {
    pub color: Color,
    pub kind: Kind,
}

impl Piece {
    pub const fn new(color: Color, kind: Kind) -> Self {
        Self { color, kind }
    }
}
