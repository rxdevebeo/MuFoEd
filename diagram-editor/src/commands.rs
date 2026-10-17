use crate::{DiagramEditor, Group, Node, Point, Target};
use std::collections::BTreeSet;
impl DiagramEditor {
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
        let first = nodes.first()?;
        let mut min = first.position;
        let mut max = Point::new(min.x + first.size.x, min.y + first.size.y);
        for n in nodes {
            min.x = min.x.min(n.position.x);
            min.y = min.y.min(n.position.y);
            max.x = max.x.max(n.position.x + n.size.x);
            max.y = max.y.max(n.position.y + n.size.y);
        }
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
        Some(node)
    }
}
