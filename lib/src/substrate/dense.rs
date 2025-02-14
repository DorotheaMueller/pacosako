//! Dense Substrate implementation.
//! We reserve memory for all positions and keep them in an array.
//! This already improves the previous dense version using Vec.

use super::zobrist::Zobrist;
use super::{BitBoard, Substrate};
use crate::{BoardPosition, PlayerColor};
use crate::{PacoError, PieceType};
use rand::Rng;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::hash::Hash;
use std::ops::{Index, IndexMut};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DenseSubstrate {
    #[serde(
        serialize_with = "serialize_option_array",
        deserialize_with = "deserialize_option_array"
    )]
    white: [Option<PieceType>; 64],
    #[serde(
        serialize_with = "serialize_option_array",
        deserialize_with = "deserialize_option_array"
    )]
    black: [Option<PieceType>; 64],
    hash: Zobrist,
}

impl Hash for DenseSubstrate {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u64(self.hash.as_u64());
    }
}

/// A default substrate is empty and must be filled with pieces to be useful.
impl Default for DenseSubstrate {
    fn default() -> Self {
        DenseSubstrate {
            white: [None; 64],
            black: [None; 64],
            hash: Zobrist::default(),
        }
    }
}

impl Index<BoardPosition> for [Option<PieceType>; 64] {
    type Output = Option<PieceType>;

    fn index(&self, index: BoardPosition) -> &Self::Output {
        &self[index.0 as usize]
    }
}

impl IndexMut<BoardPosition> for [Option<PieceType>; 64] {
    fn index_mut(&mut self, index: BoardPosition) -> &mut Self::Output {
        &mut self[index.0 as usize]
    }
}

impl From<[Option<PieceType>; 64]> for BitBoard {
    fn from(value: [Option<PieceType>; 64]) -> Self {
        let mut result = 0;
        for i in 0..64 {
            if value[i].is_some() {
                result |= 1 << i;
            }
        }
        BitBoard(result)
    }
}

impl Substrate for DenseSubstrate {
    fn get_piece(&self, player: PlayerColor, pos: BoardPosition) -> Option<PieceType> {
        match player {
            PlayerColor::White => self.white[pos],
            PlayerColor::Black => self.black[pos],
        }
    }
    fn remove_piece(&mut self, player: PlayerColor, pos: BoardPosition) -> Option<PieceType> {
        let result = self.get_piece(player, pos);
        match player {
            PlayerColor::White => self.white[pos] = None,
            PlayerColor::Black => self.black[pos] = None,
        }
        self.hash ^= Zobrist::piece_on_square_opt(player, pos, result);
        result
    }
    fn set_piece(&mut self, player: PlayerColor, pos: BoardPosition, piece: PieceType) {
        match player {
            PlayerColor::White => {
                self.hash ^= Zobrist::piece_on_square_opt(player, pos, self.white[pos]);
                self.hash ^= Zobrist::piece_on_square(player, pos, piece);
                self.white[pos] = Some(piece)
            }
            PlayerColor::Black => {
                self.hash ^= Zobrist::piece_on_square_opt(player, pos, self.black[pos]);
                self.hash ^= Zobrist::piece_on_square(player, pos, piece);
                self.black[pos] = Some(piece)
            }
        }
    }
    fn swap(&mut self, pos1: BoardPosition, pos2: BoardPosition) {
        let w1 = self.white[pos1];
        let w2 = self.white[pos2];
        let b1 = self.black[pos1];
        let b2 = self.black[pos2];

        self.hash ^= Zobrist::piece_on_square_opt(PlayerColor::White, pos1, w1);
        self.hash ^= Zobrist::piece_on_square_opt(PlayerColor::White, pos2, w1);
        self.hash ^= Zobrist::piece_on_square_opt(PlayerColor::White, pos1, w2);
        self.hash ^= Zobrist::piece_on_square_opt(PlayerColor::White, pos2, w2);

        self.hash ^= Zobrist::piece_on_square_opt(PlayerColor::Black, pos1, b1);
        self.hash ^= Zobrist::piece_on_square_opt(PlayerColor::Black, pos2, b1);
        self.hash ^= Zobrist::piece_on_square_opt(PlayerColor::Black, pos1, b2);
        self.hash ^= Zobrist::piece_on_square_opt(PlayerColor::Black, pos2, b2);

        self.white[pos1] = w2;
        self.white[pos2] = w1;
        self.black[pos1] = b2;
        self.black[pos2] = b1;
    }
    fn bitboard_color(&self, player: PlayerColor) -> super::BitBoard {
        match player {
            PlayerColor::White => self.white.into(),
            PlayerColor::Black => self.black.into(),
        }
    }
    fn find_king(&self, player: PlayerColor) -> Result<BoardPosition, crate::PacoError> {
        match player {
            PlayerColor::White => find_king(self.white, player),
            PlayerColor::Black => find_king(self.black, player),
        }
    }
}

fn find_king(
    array: [Option<PieceType>; 64],
    player: PlayerColor,
) -> Result<BoardPosition, crate::PacoError> {
    for i in 0..64 {
        if array[i] == Some(PieceType::King) {
            return Ok(BoardPosition(i as u8));
        }
    }
    Err(PacoError::NoKingOnBoard(player))
}

impl DenseSubstrate {
    pub fn shuffle<R: Rng + ?Sized>(&mut self, rng: &mut R) {
        use rand::seq::SliceRandom;
        self.white.shuffle(rng);
        self.black.shuffle(rng);
        self.refresh_zobrist_hash();
    }

    pub fn get_zobrist_hash(&self) -> Zobrist {
        self.hash
    }

    /// This method recomputes the zobrist hash from scratch.
    /// This is really only exposed for testing and you should just get_zobrist_hash
    /// when you need it.
    pub fn recompute_zobrist_hash(&self) -> Zobrist {
        let mut hash = Zobrist::default();
        for i in 0..64 {
            if let Some(piece) = self.white[i] {
                hash ^= Zobrist::piece_on_square(PlayerColor::White, BoardPosition(i as u8), piece);
            }
            if let Some(piece) = self.black[i] {
                hash ^= Zobrist::piece_on_square(PlayerColor::Black, BoardPosition(i as u8), piece);
            }
        }
        hash
    }

    fn refresh_zobrist_hash(&mut self) {
        self.hash = self.recompute_zobrist_hash();
    }
}

/// Since arrays with a size larger than 32 do not have an implementation for
/// Serialize and Deserialize by default, we implement custom serialization
/// and deserialization logic.
fn serialize_option_array<S>(
    array: &[Option<PieceType>; 64],
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    use serde::ser::SerializeSeq;

    let mut seq = serializer.serialize_seq(Some(64))?;
    for elem in array.iter() {
        seq.serialize_element(elem)?;
    }
    seq.end()
}

/// Since arrays with a size larger than 32 do not have an implementation for
/// Serialize and Deserialize by default, we implement custom serialization
/// and deserialization logic.
fn deserialize_option_array<'de, D>(deserializer: D) -> Result<[Option<PieceType>; 64], D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::{SeqAccess, Visitor};
    use std::fmt;

    struct OptionArrayVisitor;

    impl<'de> Visitor<'de> for OptionArrayVisitor {
        type Value = [Option<PieceType>; 64];

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a sequence of 64 optional PieceType values")
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let mut array = [None; 64];
            for i in 0..64 {
                array[i] = seq.next_element()?;
                if array[i].is_none() {
                    return Err(serde::de::Error::invalid_length(i, &self));
                }
            }
            Ok(array)
        }
    }

    deserializer.deserialize_seq(OptionArrayVisitor)
}

// Test the size of a DenseSubstrate
#[test]
fn test_size() {
    use std::mem::size_of;
    assert_eq!(size_of::<DenseSubstrate>(), 64 * 2 + 8);
}
