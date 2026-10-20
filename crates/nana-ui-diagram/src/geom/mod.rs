//! Geometry: paths, node shapes and line-end markers.

pub mod markers;
pub mod path;
pub mod shapes;

pub use markers::Marker;
pub use path::{PathData, Verb};
pub use shapes::Shape;
