use crate::{DiagramEditor, Group, Node, Point, Target};
use std::collections::BTreeSet;
/// Alignment of selected objects within their shared visual bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alignment {
    /// Left edges.
    Left,
    /// Horizontal centers.
    Center,
    /// Right edges.
    Right,
    /// Top edges.
    Top,
    /// Vertical centers.
    Middle,
    /// Bottom edges.
    Bottom,
}
/// Distribution direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// Horizontal gaps.
    Horizontal,
    /// Vertical gaps.
    Vertical,
}
impl DiagramEditor {
    pub(crate) fn selection_units(&self) -> Vec<Vec<u64>> {
        let mut remaining = self.selected_nodes();
        let mut units = vec![];
        for group in &self.diagram.groups {
            if group.members.iter().all(|id| remaining.contains(id)) {
                for id in &group.members {
                    remaining.remove(id);
                }
                units.push(group.members.clone());
            }
        }
        units.extend(remaining.into_iter().map(|id| vec![id]));
        units
    }
    fn unit_bounds(&self, ids: &[u64]) -> (Point, Point) {
        let mut min = Point::new(f32::INFINITY, f32::INFINITY);
        let mut max = Point::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
        for n in self.diagram.nodes.iter().filter(|n| ids.contains(&n.id)) {
            let (a, b) = n.visual_bounds();
            min.x = min.x.min(a.x);
            min.y = min.y.min(a.y);
            max.x = max.x.max(b.x);
            max.y = max.y.max(b.y);
        }
        (min, max)
    }
    fn translate_units(&mut self, moves: &[(Vec<u64>, Point)]) -> Result<(), String> {
        let mut next = self.diagram.clone();
        for (ids, delta) in moves {
            for n in &mut next.nodes {
                if ids.contains(&n.id) {
                    n.position.x += delta.x;
                    n.position.y += delta.y;
                }
            }
        }
        next.validate()?;
        self.commit();
        self.begin();
        self.diagram = next;
        self.commit();
        Ok(())
    }
    /// Aligns shapes/groups without changing their sizes or internal geometry.
    /// # Errors
    /// Requires two independent selected objects and valid resulting coordinates.
    pub fn align_selected(&mut self, alignment: Alignment) -> Result<(), String> {
        let units = self.selection_units();
        if units.len() < 2 {
            return Err("Select at least two objects".into());
        }
        let bounds: Vec<_> = units.iter().map(|ids| self.unit_bounds(ids)).collect();
        let min = Point::new(
            bounds.iter().map(|b| b.0.x).fold(f32::INFINITY, f32::min),
            bounds.iter().map(|b| b.0.y).fold(f32::INFINITY, f32::min),
        );
        let max = Point::new(
            bounds
                .iter()
                .map(|b| b.1.x)
                .fold(f32::NEG_INFINITY, f32::max),
            bounds
                .iter()
                .map(|b| b.1.y)
                .fold(f32::NEG_INFINITY, f32::max),
        );
        let moves = units
            .into_iter()
            .zip(bounds)
            .map(|(ids, (a, b))| {
                let delta = match alignment {
                    Alignment::Left => Point::new(min.x - a.x, 0.0),
                    Alignment::Center => Point::new((min.x + max.x - a.x - b.x) / 2.0, 0.0),
                    Alignment::Right => Point::new(max.x - b.x, 0.0),
                    Alignment::Top => Point::new(0.0, min.y - a.y),
                    Alignment::Middle => Point::new(0.0, (min.y + max.y - a.y - b.y) / 2.0),
                    Alignment::Bottom => Point::new(0.0, max.y - b.y),
                };
                (ids, delta)
            })
            .collect::<Vec<_>>();
        self.translate_units(&moves)
    }
    /// Distributes equal gaps; outermost objects stay fixed, groups remain rigid.
    /// # Errors
    /// Requires three independent selected objects and valid resulting coordinates.
    pub fn distribute_selected(&mut self, axis: Axis) -> Result<(), String> {
        let coord = |p: Point| if axis == Axis::Horizontal { p.x } else { p.y };
        let mut units: Vec<_> = self
            .selection_units()
            .into_iter()
            .map(|ids| {
                let (a, b) = self.unit_bounds(&ids);
                (ids, coord(a), coord(b))
            })
            .collect();
        if units.len() < 3 {
            return Err("Select at least three objects".into());
        }
        units.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0[0].cmp(&b.0[0])));
        let first = units[0].1;
        let last = units.last().unwrap().2;
        let total: f32 = units.iter().map(|u| u.2 - u.1).sum();
        let gap =
            (last - first - total) / f32::from(u16::try_from(units.len() - 1).unwrap_or(u16::MAX));
        let mut at = first;
        let mut moves = vec![];
        for (ids, start, end) in units {
            let d = at - start;
            moves.push((
                ids,
                if axis == Axis::Horizontal {
                    Point::new(d, 0.0)
                } else {
                    Point::new(0.0, d)
                },
            ));
            at += end - start + gap;
        }
        self.translate_units(&moves)
    }
    /// Rotates selected nodes around their common center in one transaction.
    /// # Errors
    /// Rejects non-finite angles, empty selection or out-of-bounds geometry.
    pub fn rotate_selected(&mut self, degrees: f32) -> Result<(), String> {
        if !degrees.is_finite() {
            return Err("Invalid rotation".into());
        }
        let ids = self.selected_nodes();
        if ids.is_empty() {
            return Err("Nothing selected".into());
        }
        let (a, b) = self.unit_bounds(&ids.iter().copied().collect::<Vec<_>>());
        let center = Point::new(a.x.midpoint(b.x), a.y.midpoint(b.y));
        let mut next = self.diagram.clone();
        for n in &mut next.nodes {
            if ids.contains(&n.id) {
                let c = crate::model::rotate(n.center(), center, degrees);
                n.position = Point::new(c.x - n.size.x / 2.0, c.y - n.size.y / 2.0);
                n.style.rotation = (n.style.rotation + degrees).rem_euclid(360.0);
            }
        }
        next.validate()?;
        self.commit();
        self.begin();
        self.diagram = next;
        self.commit();
        Ok(())
    }
    /// Duplicates selected shapes, groups and internal links with new IDs.
    /// # Errors
    /// Rejects missing selection or exceeded scene limits without altering it.
    pub fn duplicate_selected(&mut self) -> Result<(), String> {
        let ids = self.selected_nodes();
        if ids.is_empty() {
            return Err("Nothing selected".into());
        }
        let mut next = self.diagram.clone();
        let mut id = self.next_id();
        let mut mapping = std::collections::HashMap::new();
        for n in self.diagram.nodes.iter().filter(|n| ids.contains(&n.id)) {
            let mut n = n.clone();
            mapping.insert(n.id, id);
            n.id = id;
            id += 1;
            n.position.x += 16.0;
            n.position.y += 16.0;
            next.nodes.push(n);
        }
        for c in self
            .diagram
            .connections
            .iter()
            .filter(|c| ids.contains(&c.from) && ids.contains(&c.to))
        {
            let mut c = c.clone();
            c.id = id;
            id += 1;
            c.from = mapping[&c.from];
            c.to = mapping[&c.to];
            next.connections.push(c);
        }
        for g in self
            .diagram
            .groups
            .iter()
            .filter(|g| g.members.iter().all(|id| ids.contains(id)))
        {
            next.groups.push(Group {
                id,
                members: g.members.iter().map(|id| mapping[id]).collect(),
            });
            id += 1;
        }
        next.validate()?;
        self.commit();
        self.begin();
        self.diagram = next;
        self.multi = mapping.values().copied().collect();
        self.selection = self.multi.first().copied().map(Target::Node);
        self.commit();
        Ok(())
    }
    /// IDs in the current selection, expanding group membership.
    #[must_use]
    pub fn selected_nodes(&self) -> BTreeSet<u64> {
        let mut ids = self.multi.clone();
        if let Some(Target::Node(id)) = self.selection {
            ids.insert(id);
        }
        for g in &self.diagram.groups {
            if g.members.iter().any(|id| ids.contains(id)) {
                ids.extend(&g.members);
            }
        }
        ids
    }
    /// Selects several nodes; group members are selected together.
    /// # Errors
    /// Rejects missing IDs without changing selection.
    pub fn select_nodes(&mut self, ids: &[u64]) -> Result<(), String> {
        if ids.iter().any(|id| self.diagram.node(*id).is_none()) {
            return Err("Unknown selection node".into());
        }
        self.commit();
        self.multi = ids.iter().copied().collect();
        self.selection = ids.first().copied().map(Target::Node);
        self.multi = self.selected_nodes();
        Ok(())
    }
    /// Groups selected shapes. Groups are flat and moved to the front together.
    /// # Errors
    /// Requires at least two ungrouped nodes.
    pub fn group_selected(&mut self) -> Result<u64, String> {
        let ids = self.selected_nodes();
        if ids.len() < 2
            || self
                .diagram
                .groups
                .iter()
                .any(|g| g.members.iter().any(|id| ids.contains(id)))
        {
            return Err("Select at least two ungrouped shapes".into());
        }
        self.commit();
        self.begin();
        let id = self.next_id();
        self.diagram.groups.push(Group {
            id,
            members: ids.iter().copied().collect(),
        });
        self.reorder_selected(true);
        self.commit();
        Ok(id)
    }
    /// Removes selected flat groups, preserving all objects and links.
    pub fn ungroup_selected(&mut self) {
        self.commit();
        self.begin();
        let ids = self.selected_nodes();
        self.diagram
            .groups
            .retain(|g| !g.members.iter().any(|id| ids.contains(id)));
        self.commit();
    }
    /// Bounding frame of all selected nodes.
    #[must_use]
    pub fn selection_frame(&self) -> Option<(Point, Point)> {
        let ids = self.selected_nodes();
        let nodes: Vec<_> = self
            .diagram
            .nodes
            .iter()
            .filter(|n| ids.contains(&n.id))
            .collect();
        if nodes.is_empty() {
            return None;
        }
        let (min, max) = self.unit_bounds(&ids.into_iter().collect::<Vec<_>>());
        Some((min, Point::new(max.x - min.x, max.y - min.y)))
    }
    /// Moves or scales selected nodes as one action. Text size is preserved.
    /// # Errors
    /// Rejects transformations exceeding geometry limits; leaves the scene intact.
    pub fn transform_selection(&mut self, position: Point, size: Point) -> Result<(), String> {
        self.commit();
        self.preview_transform(position, size)?;
        self.commit();
        Ok(())
    }
    pub(crate) fn preview_transform(&mut self, position: Point, size: Point) -> Result<(), String> {
        let (old, extent) = self.selection_frame().ok_or("Nothing selected")?;
        let ids = self.selected_nodes();
        let mut next = self.diagram.clone();
        for n in &mut next.nodes {
            if ids.contains(&n.id) {
                n.position = Point::new(
                    position.x + (n.position.x - old.x) * size.x / extent.x,
                    position.y + (n.position.y - old.y) * size.y / extent.y,
                );
                n.size = Point::new(n.size.x * size.x / extent.x, n.size.y * size.y / extent.y);
            }
        }
        next.validate()?;
        self.begin();
        self.diagram = next;
        Ok(())
    }
    /// Brings selected shapes to the front, or sends them to the back.
    pub fn reorder_selected(&mut self, front: bool) {
        self.begin();
        let ids = self.selected_nodes();
        self.diagram.nodes.sort_by_key(|n| {
            if front {
                ids.contains(&n.id)
            } else {
                !ids.contains(&n.id)
            }
        });
        self.commit();
    }
    pub(crate) fn selection_node(&self) -> Option<Node> {
        let Some(Target::Node(id)) = self.selection else {
            return None;
        };
        let mut node = self.diagram.node(id)?.clone();
        let (p, size) = self.selection_frame()?;
        node.position = p;
        node.size = size;
        node.style.rotation = 0.0;
        Some(node)
    }
}
