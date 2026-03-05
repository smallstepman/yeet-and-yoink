use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::engine::contract::MoveDecision;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitAxis {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ValueEnum, Deserialize, Serialize)]
pub enum Direction {
    #[serde(alias = "Left", alias = "left", alias = "west", alias = "W")]
    West,
    #[serde(alias = "Right", alias = "right", alias = "east", alias = "E")]
    East,
    #[serde(
        alias = "Up",
        alias = "up",
        alias = "north",
        alias = "N",
        alias = "Above",
        alias = "above"
    )]
    North,
    #[serde(
        alias = "Down",
        alias = "down",
        alias = "south",
        alias = "S",
        alias = "Below",
        alias = "below"
    )]
    South,
}

impl Direction {
    pub const ALL: [Self; 4] = [Self::West, Self::East, Self::North, Self::South];

    pub fn opposite(self) -> Self {
        match self {
            Self::West => Self::East,
            Self::East => Self::West,
            Self::North => Self::South,
            Self::South => Self::North,
        }
    }

    pub fn axis(self) -> SplitAxis {
        match self {
            Self::West | Self::East => SplitAxis::Horizontal,
            Self::North | Self::South => SplitAxis::Vertical,
        }
    }

    pub const fn cardinal(self) -> &'static str {
        match self {
            Self::West => "west",
            Self::East => "east",
            Self::North => "north",
            Self::South => "south",
        }
    }

    /// Positional terms: left/right/top/bottom.
    pub const fn positional(self) -> &'static str {
        match self {
            Self::West => "left",
            Self::East => "right",
            Self::North => "top",
            Self::South => "bottom",
        }
    }

    /// Relational terms: left/right/above/below.
    pub const fn relational(self) -> &'static str {
        match self {
            Self::West => "left",
            Self::East => "right",
            Self::North => "above",
            Self::South => "below",
        }
    }

    /// Egocentric terms: left/right/up/down.
    pub const fn egocentric(self) -> &'static str {
        match self {
            Self::West => "left",
            Self::East => "right",
            Self::North => "up",
            Self::South => "down",
        }
    }

    #[allow(dead_code)]
    pub const fn vectorial(self) -> &'static str {
        match self {
            Self::West => "backward",
            Self::East => "forward",
            Self::North => "upward",
            Self::South => "downward",
        }
    }

    #[allow(dead_code)]
    pub const fn sequential(self) -> &'static str {
        match self {
            Self::West => "previous",
            Self::East => "next",
            Self::North => "higher",
            Self::South => "lower",
        }
    }

    #[allow(dead_code)]
    pub const fn hierarchical(self) -> &'static str {
        match self {
            Self::West => "previous",
            Self::East => "next",
            Self::North => "parent",
            Self::South => "child",
        }
    }

    pub const fn vim_key(self) -> char {
        match self {
            Self::West => 'h',
            Self::East => 'l',
            Self::North => 'k',
            Self::South => 'j',
        }
    }

    pub const fn tmux_flag(self) -> &'static str {
        match self {
            Self::West => "-L",
            Self::East => "-R",
            Self::North => "-U",
            Self::South => "-D",
        }
    }
}

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.cardinal())
    }
}

pub type DomainId = u64;
pub type LeafId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn leading_edge(self, dir: Direction) -> i32 {
        match dir {
            Direction::East => self.x + self.w,
            Direction::West => self.x,
            Direction::South => self.y + self.h,
            Direction::North => self.y,
        }
    }

    pub fn receiving_edge(self, dir: Direction) -> i32 {
        self.leading_edge(dir.opposite())
    }

    pub fn perp_overlap(self, other: Rect, dir: Direction) -> bool {
        match dir.axis() {
            SplitAxis::Horizontal => self.y < other.y + other.h && self.y + self.h > other.y,
            SplitAxis::Vertical => self.x < other.x + other.w && self.x + self.w > other.x,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalLeaf {
    pub id: LeafId,
    pub domain: DomainId,
    pub native_id: Vec<u8>,
    pub rect: Rect,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DirectionalNeighbors {
    pub west: bool,
    pub east: bool,
    pub north: bool,
    pub south: bool,
}

impl DirectionalNeighbors {
    pub fn in_direction(self, dir: Direction) -> bool {
        match dir {
            Direction::West => self.west,
            Direction::East => self.east,
            Direction::North => self.north,
            Direction::South => self.south,
        }
    }

    pub fn has_perpendicular(self, dir: Direction) -> bool {
        match dir {
            Direction::West | Direction::East => self.north || self.south,
            Direction::North | Direction::South => self.west || self.east,
        }
    }

    pub fn set(&mut self, dir: Direction, value: bool) {
        match dir {
            Direction::West => self.west = value,
            Direction::East => self.east = value,
            Direction::North => self.north = value,
            Direction::South => self.south = value,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MoveSurface {
    pub pane_count: u32,
    pub neighbors: DirectionalNeighbors,
    pub supports_rearrange: bool,
}

impl MoveSurface {
    pub fn decision_for(self, dir: Direction) -> MoveDecision {
        if self.pane_count <= 1 {
            return MoveDecision::Passthrough;
        }
        if self.neighbors.in_direction(dir) {
            return MoveDecision::Internal;
        }
        if self.supports_rearrange && self.neighbors.has_perpendicular(dir) {
            return MoveDecision::Rearrange;
        }
        MoveDecision::TearOut
    }
}

#[cfg(test)]
mod tests {
    use super::{Direction, DirectionalNeighbors, MoveSurface, Rect};
    use crate::engine::contract::MoveDecision;

    #[test]
    fn rect_leading_and_receiving_edges_are_opposites() {
        let rect = Rect {
            x: 10,
            y: 20,
            w: 30,
            h: 40,
        };
        assert_eq!(rect.leading_edge(Direction::East), 40);
        assert_eq!(rect.receiving_edge(Direction::East), 10);
        assert_eq!(rect.leading_edge(Direction::South), 60);
        assert_eq!(rect.receiving_edge(Direction::South), 20);
    }

    #[test]
    fn rect_perp_overlap_uses_axis() {
        let a = Rect {
            x: 0,
            y: 0,
            w: 10,
            h: 10,
        };
        let b = Rect {
            x: 20,
            y: 5,
            w: 10,
            h: 10,
        };
        assert!(a.perp_overlap(b, Direction::East));
        assert!(!a.perp_overlap(b, Direction::South));
    }

    #[test]
    fn direction_string_conversions_cover_reference_sets() {
        assert_eq!(Direction::West.positional(), "left");
        assert_eq!(Direction::East.positional(), "right");
        assert_eq!(Direction::North.positional(), "top");
        assert_eq!(Direction::South.positional(), "bottom");

        assert_eq!(Direction::North.relational(), "above");
        assert_eq!(Direction::South.relational(), "below");
        assert_eq!(Direction::North.egocentric(), "up");
        assert_eq!(Direction::South.egocentric(), "down");

        assert_eq!(Direction::West.vectorial(), "backward");
        assert_eq!(Direction::East.vectorial(), "forward");
        assert_eq!(Direction::North.sequential(), "higher");
        assert_eq!(Direction::South.sequential(), "lower");
        assert_eq!(Direction::North.hierarchical(), "parent");
        assert_eq!(Direction::South.hierarchical(), "child");
    }

    #[test]
    fn directional_neighbors_report_direction_and_perpendicular_presence() {
        let mut neighbors = DirectionalNeighbors::default();
        neighbors.set(Direction::West, true);
        neighbors.set(Direction::North, true);

        assert!(neighbors.in_direction(Direction::West));
        assert!(!neighbors.in_direction(Direction::East));
        assert!(neighbors.has_perpendicular(Direction::West));
        assert!(neighbors.has_perpendicular(Direction::North));
    }

    #[test]
    fn move_surface_classifies_by_neighbor_and_rearrange_capability() {
        let surface = MoveSurface {
            pane_count: 2,
            neighbors: DirectionalNeighbors {
                west: false,
                east: false,
                north: true,
                south: false,
            },
            supports_rearrange: true,
        };
        assert!(matches!(
            surface.decision_for(Direction::West),
            MoveDecision::Rearrange
        ));

        let without_rearrange = MoveSurface {
            supports_rearrange: false,
            ..surface
        };
        assert!(matches!(
            without_rearrange.decision_for(Direction::West),
            MoveDecision::TearOut
        ));
    }
}
