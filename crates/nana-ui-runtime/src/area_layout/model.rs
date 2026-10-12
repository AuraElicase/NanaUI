//! The area tree of an [`crate::AreaLayout`] (#340): what the window is cut
//! into, which editor each area shows, which one is maximized. Pure data with
//! its geometry, so an application can build, check and persist a layout
//! without a document.

use serde::{Deserialize, Serialize};

use crate::LayoutBox;

/// Which way a split cuts its rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AreaAxis {
    /// Side by side: the dividing line is vertical.
    Horizontal,
    /// One above the other: the dividing line is horizontal.
    Vertical,
}

/// A node of the area tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AreaNode {
    Area {
        id: String,
    },
    Split {
        axis: AreaAxis,
        /// The first child's share of the split's extent along `axis`.
        ratio: f32,
        first: Box<AreaNode>,
        second: Box<AreaNode>,
    },
}

/// One area: which editor it shows and the application state it restores.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Area {
    pub id: String,
    /// The application's editor type id, e.g. `viewport`.
    pub editor: String,
    /// The key the application keeps the area's own state under.
    #[serde(default)]
    pub state: Option<String>,
}

/// A layout that does not hold together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AreaLayoutError {
    /// Two leaves, or two areas, share an id.
    DuplicateArea(String),
    /// A leaf has no area, or an area no leaf.
    Unmatched(String),
    /// The maximized area is not in the layout.
    UnknownMaximized(String),
    Json(String),
}

impl std::fmt::Display for AreaLayoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateArea(id) => write!(f, "area {id} appears twice"),
            Self::Unmatched(id) => write!(f, "area {id} is not both a leaf and an area"),
            Self::UnknownMaximized(id) => write!(f, "maximized area {id} is not in the layout"),
            Self::Json(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for AreaLayoutError {}

/// The whole layout: the tree, its areas and the maximized one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AreaLayoutModel {
    pub root: AreaNode,
    pub areas: Vec<Area>,
    /// Shown alone over the whole layout until restored.
    #[serde(default)]
    pub maximized: Option<String>,
}

/// A divider between the two sides of a split.
#[derive(Debug, Clone, PartialEq)]
pub struct AreaSplitter {
    /// The way to the split from the root: `false` the first child, `true`
    /// the second.
    pub path: Vec<bool>,
    pub axis: AreaAxis,
    /// The gap between the two sides, where the divider is grabbed.
    pub bar: LayoutBox,
    /// The split's whole rectangle.
    pub span: LayoutBox,
}

/// Where everything goes for one size of the layout.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AreaGeometry {
    pub areas: Vec<(String, LayoutBox)>,
    pub splitters: Vec<AreaSplitter>,
}

impl AreaGeometry {
    pub fn area(&self, id: &str) -> Option<LayoutBox> {
        self.areas
            .iter()
            .find(|(area, _)| area == id)
            .map(|(_, bounds)| *bounds)
    }

    /// The area at `(x, y)`.
    pub fn area_at(&self, x: f32, y: f32) -> Option<&str> {
        self.areas
            .iter()
            .find(|(_, bounds)| bounds.contains(x, y))
            .map(|(id, _)| id.as_str())
    }
}

impl AreaLayoutModel {
    /// One area showing `editor`.
    pub fn single(id: impl Into<String>, editor: impl Into<String>) -> Self {
        let id = id.into();
        Self {
            root: AreaNode::Area { id: id.clone() },
            areas: vec![Area {
                id,
                editor: editor.into(),
                state: None,
            }],
            maximized: None,
        }
    }

    pub fn area(&self, id: &str) -> Option<&Area> {
        self.areas.iter().find(|area| area.id == id)
    }

    pub fn area_mut(&mut self, id: &str) -> Option<&mut Area> {
        self.areas.iter_mut().find(|area| area.id == id)
    }

    /// Show `editor` in `id`. The content is the application's to swap.
    pub fn set_editor(&mut self, id: &str, editor: impl Into<String>) -> bool {
        match self.area_mut(id) {
            Some(area) => {
                area.editor = editor.into();
                true
            }
            None => false,
        }
    }

    /// An id no area has yet.
    pub fn fresh_id(&self) -> String {
        (1..)
            .map(|n| format!("area-{n}"))
            .find(|id| self.area(id).is_none())
            .expect("an unused id")
    }

    /// Cut `id` along `axis`; the new area `new` (showing the same editor)
    /// takes the second side, `1 - ratio` of it.
    pub fn split(&mut self, id: &str, axis: AreaAxis, ratio: f32, new: impl Into<String>) -> bool {
        let new = new.into();
        let Some(editor) = self.area(id).map(|area| area.editor.clone()) else {
            return false;
        };
        if self.area(&new).is_some() {
            return false;
        }
        let Some(node) = find_leaf(&mut self.root, id) else {
            return false;
        };
        *node = AreaNode::Split {
            axis,
            ratio: ratio.clamp(0.0, 1.0),
            first: Box::new(AreaNode::Area { id: id.to_owned() }),
            second: Box::new(AreaNode::Area { id: new.clone() }),
        };
        self.areas.push(Area {
            id: new,
            editor,
            state: None,
        });
        self.maximized = None;
        true
    }

    /// `keep` and `remove` are the two sides of one split: `keep` takes the
    /// whole of it.
    pub fn can_join(&self, keep: &str, remove: &str) -> bool {
        sibling_split(&self.root, keep, remove)
    }

    /// Join `remove` into `keep` ([`Self::can_join`]).
    pub fn join(&mut self, keep: &str, remove: &str) -> bool {
        if !self.can_join(keep, remove) {
            return false;
        }
        replace_split(&mut self.root, keep, remove);
        self.areas.retain(|area| area.id != remove);
        if self.maximized.as_deref() == Some(remove) {
            self.maximized = None;
        }
        true
    }

    /// Show `id` alone, or everything again when it already is (`None`
    /// restores).
    pub fn toggle_maximized(&mut self, id: Option<&str>) {
        self.maximized = match id {
            Some(id) if self.maximized.as_deref() != Some(id) && self.area(id).is_some() => {
                Some(id.to_owned())
            }
            _ => None,
        };
    }

    /// Set the ratio of the split at `path`.
    pub fn set_ratio(&mut self, path: &[bool], ratio: f32) -> bool {
        let mut node = &mut self.root;
        for second in path {
            match node {
                AreaNode::Split {
                    first, second: s, ..
                } => {
                    node = if *second { s } else { first };
                }
                AreaNode::Area { .. } => return false,
            }
        }
        match node {
            AreaNode::Split { ratio: r, .. } => {
                *r = ratio.clamp(0.0, 1.0);
                true
            }
            AreaNode::Area { .. } => false,
        }
    }

    /// The ids of the leaves, first to last.
    pub fn leaves(&self) -> Vec<&str> {
        let mut out = Vec::new();
        collect_leaves(&self.root, &mut out);
        out
    }

    /// Check the tree and the areas match, ids are unique and the maximized
    /// area exists.
    pub fn validate(&self) -> Result<(), AreaLayoutError> {
        let leaves = self.leaves();
        let mut seen = std::collections::HashSet::new();
        for leaf in &leaves {
            if !seen.insert(*leaf) {
                return Err(AreaLayoutError::DuplicateArea((*leaf).to_owned()));
            }
            if self.area(leaf).is_none() {
                return Err(AreaLayoutError::Unmatched((*leaf).to_owned()));
            }
        }
        let mut areas = std::collections::HashSet::new();
        for area in &self.areas {
            if !areas.insert(area.id.as_str()) {
                return Err(AreaLayoutError::DuplicateArea(area.id.clone()));
            }
            if !seen.contains(area.id.as_str()) {
                return Err(AreaLayoutError::Unmatched(area.id.clone()));
            }
        }
        if let Some(id) = &self.maximized
            && !seen.contains(id.as_str())
        {
            return Err(AreaLayoutError::UnknownMaximized(id.clone()));
        }
        Ok(())
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("an area layout serializes")
    }

    pub fn from_json(json: &str) -> Result<Self, AreaLayoutError> {
        let model: Self =
            serde_json::from_str(json).map_err(|e| AreaLayoutError::Json(e.to_string()))?;
        model.validate()?;
        Ok(model)
    }

    /// Where every area and divider goes in `bounds`, `gap` px between
    /// areas, each side of a split at least `min` px (as far as the split
    /// is large enough).
    pub fn geometry(&self, bounds: LayoutBox, gap: f32, min: f32) -> AreaGeometry {
        let mut geometry = AreaGeometry::default();
        if let Some(id) = &self.maximized {
            geometry.areas.push((id.clone(), bounds));
            return geometry;
        }
        place(&self.root, bounds, gap, min, &mut Vec::new(), &mut geometry);
        geometry
    }
}

/// `ratio` kept so both sides of a `extent` px split, `gap` between them,
/// are at least `min` px.
pub(crate) fn clamp_ratio(ratio: f32, extent: f32, gap: f32, min: f32) -> f32 {
    let usable = (extent - gap).max(0.0);
    if usable <= 2.0 * min {
        return 0.5;
    }
    let low = min / usable;
    ratio.clamp(low, 1.0 - low)
}

fn place(
    node: &AreaNode,
    bounds: LayoutBox,
    gap: f32,
    min: f32,
    path: &mut Vec<bool>,
    out: &mut AreaGeometry,
) {
    match node {
        AreaNode::Area { id } => out.areas.push((id.clone(), bounds)),
        AreaNode::Split {
            axis,
            ratio,
            first,
            second,
        } => {
            let extent = match axis {
                AreaAxis::Horizontal => bounds.width,
                AreaAxis::Vertical => bounds.height,
            };
            let ratio = clamp_ratio(*ratio, extent, gap, min);
            let usable = (extent - gap).max(0.0);
            let a = (usable * ratio).round();
            let (one, bar, two) = match axis {
                AreaAxis::Horizontal => (
                    LayoutBox { width: a, ..bounds },
                    LayoutBox {
                        x: bounds.x + a,
                        width: gap,
                        ..bounds
                    },
                    LayoutBox {
                        x: bounds.x + a + gap,
                        width: usable - a,
                        ..bounds
                    },
                ),
                AreaAxis::Vertical => (
                    LayoutBox {
                        height: a,
                        ..bounds
                    },
                    LayoutBox {
                        y: bounds.y + a,
                        height: gap,
                        ..bounds
                    },
                    LayoutBox {
                        y: bounds.y + a + gap,
                        height: usable - a,
                        ..bounds
                    },
                ),
            };
            out.splitters.push(AreaSplitter {
                path: path.clone(),
                axis: *axis,
                bar,
                span: bounds,
            });
            path.push(false);
            place(first, one, gap, min, path, out);
            path.pop();
            path.push(true);
            place(second, two, gap, min, path, out);
            path.pop();
        }
    }
}

fn find_leaf<'a>(node: &'a mut AreaNode, id: &str) -> Option<&'a mut AreaNode> {
    match node {
        AreaNode::Area { id: leaf } if leaf == id => Some(node),
        AreaNode::Area { .. } => None,
        AreaNode::Split { first, second, .. } => {
            if let Some(found) = find_leaf(first, id) {
                return Some(found);
            }
            find_leaf(second, id)
        }
    }
}

fn is_leaf(node: &AreaNode, id: &str) -> bool {
    matches!(node, AreaNode::Area { id: leaf } if leaf == id)
}

fn sibling_split(node: &AreaNode, a: &str, b: &str) -> bool {
    match node {
        AreaNode::Area { .. } => false,
        AreaNode::Split { first, second, .. } => {
            (is_leaf(first, a) && is_leaf(second, b))
                || (is_leaf(first, b) && is_leaf(second, a))
                || sibling_split(first, a, b)
                || sibling_split(second, a, b)
        }
    }
}

fn replace_split(node: &mut AreaNode, keep: &str, remove: &str) -> bool {
    let AreaNode::Split { first, second, .. } = node else {
        return false;
    };
    if (is_leaf(first, keep) && is_leaf(second, remove))
        || (is_leaf(first, remove) && is_leaf(second, keep))
    {
        *node = AreaNode::Area {
            id: keep.to_owned(),
        };
        return true;
    }
    replace_split(first, keep, remove) || replace_split(second, keep, remove)
}

fn collect_leaves<'a>(node: &'a AreaNode, out: &mut Vec<&'a str>) {
    match node {
        AreaNode::Area { id } => out.push(id),
        AreaNode::Split { first, second, .. } => {
            collect_leaves(first, out);
            collect_leaves(second, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn four() -> AreaLayoutModel {
        let mut model = AreaLayoutModel::single("outliner", "outliner");
        assert!(model.split("outliner", AreaAxis::Horizontal, 0.2, "viewport"));
        model.set_editor("viewport", "viewport");
        assert!(model.split("viewport", AreaAxis::Horizontal, 0.75, "params"));
        model.set_editor("params", "parameters");
        assert!(model.split("params", AreaAxis::Vertical, 0.5, "props"));
        model.set_editor("props", "properties");
        model
    }

    #[test]
    fn splits_place_areas_side_by_side_with_gaps() {
        let model = four();
        assert_eq!(model.leaves(), ["outliner", "viewport", "params", "props"]);
        let geometry = model.geometry(
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 1002.0,
                height: 602.0,
            },
            2.0,
            40.0,
        );
        let outliner = geometry.area("outliner").unwrap();
        assert_eq!(
            (outliner.x, outliner.width, outliner.height),
            (0.0, 200.0, 602.0)
        );
        let viewport = geometry.area("viewport").unwrap();
        // 0.75 of the 798 px left beside the outliner's divider
        assert_eq!((viewport.x, viewport.width), (202.0, 599.0));
        let params = geometry.area("params").unwrap();
        let props = geometry.area("props").unwrap();
        assert_eq!(
            (params.x, params.width, params.height),
            (803.0, 199.0, 300.0)
        );
        assert_eq!((props.y, props.height), (302.0, 300.0));
        assert_eq!(geometry.splitters.len(), 3);
        assert_eq!(geometry.splitters[0].bar.x, 200.0);
        assert_eq!(geometry.area_at(900.0, 400.0), Some("props"));
    }

    #[test]
    fn a_ratio_keeps_both_sides_at_least_the_minimum() {
        assert_eq!(clamp_ratio(0.01, 402.0, 2.0, 40.0), 0.1);
        assert_eq!(clamp_ratio(0.99, 402.0, 2.0, 40.0), 0.9);
        assert_eq!(clamp_ratio(0.3, 60.0, 2.0, 40.0), 0.5, "too small for both");
    }

    #[test]
    fn join_takes_a_sibling_only() {
        let mut model = four();
        assert!(!model.can_join("outliner", "props"), "not one split");
        assert!(model.can_join("props", "params"));
        assert!(model.join("props", "params"));
        assert_eq!(model.leaves(), ["outliner", "viewport", "props"]);
        assert!(model.area("params").is_none());
        assert!(model.validate().is_ok());
    }

    #[test]
    fn maximize_shows_one_area_and_restores() {
        let mut model = four();
        model.toggle_maximized(Some("viewport"));
        let bounds = LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
        };
        let geometry = model.geometry(bounds, 2.0, 40.0);
        assert_eq!(geometry.areas, [("viewport".to_owned(), bounds)]);
        assert!(geometry.splitters.is_empty());
        model.toggle_maximized(Some("viewport"));
        assert_eq!(model.maximized, None);
    }

    #[test]
    fn layouts_round_trip_through_json_and_bad_ones_are_refused() {
        let mut model = four();
        model.area_mut("viewport").unwrap().state = Some("camera-1".into());
        model.toggle_maximized(Some("props"));
        let back = AreaLayoutModel::from_json(&model.to_json()).unwrap();
        assert_eq!(back, model);
        let mut broken = model.clone();
        broken.areas.pop();
        assert!(matches!(
            AreaLayoutModel::from_json(&broken.to_json()),
            Err(AreaLayoutError::Unmatched(_))
        ));
        assert_eq!(model.fresh_id(), "area-1");
    }
}
