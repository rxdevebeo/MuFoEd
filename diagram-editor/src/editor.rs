use crate::{Connection, Diagram, Node, Point, Port, ShapeKind};

/// Canvas interaction mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// Select, move or resize objects.
    Select,
    /// Place a new shape.
    Shape(ShapeKind),
    /// Connect two existing nodes.
    Connect,
}
/// Selected scene object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// A shape ID.
    Node(u64),
    /// A connector ID.
    Connection(u64),
}

/// Reusable editor with bounded scene history. The host owns persistence.
#[derive(Clone)]
pub struct DiagramEditor {
    pub(crate) locale: crate::Locale,
    pub(crate) diagram: Diagram,
    pub(crate) selection: Option<Target>,
    pub(crate) multi: std::collections::BTreeSet<u64>,
    pub(crate) tool: Tool,
    pub(crate) pending_connection: Option<u64>,
    pub(crate) undo: Vec<Diagram>,
    pub(crate) redo: Vec<Diagram>,
    pub(crate) edit_base: Option<Diagram>,
    pub(crate) marquee: Option<crate::widget::Marquee>,
    pub(crate) drag: Option<crate::widget::Drag>,
    pub(crate) pan: egui::Vec2,
    pub(crate) zoom: f32,
    pub(crate) snap: bool,
    pub(crate) focus_text: bool,
    pub(crate) revision: u64,
}
impl Default for DiagramEditor {
    fn default() -> Self {
        Self {
            locale: crate::Locale::default(),
            diagram: Diagram::default(),
            selection: None,
            multi: std::collections::BTreeSet::new(),
            tool: Tool::Select,
            pending_connection: None,
            undo: vec![],
            redo: vec![],
            edit_base: None,
            drag: None,
            marquee: None,
            pan: egui::vec2(40.0, 40.0),
            zoom: 1.0,
            snap: true,
            focus_text: false,
            revision: 0,
        }
    }
}
impl DiagramEditor {
    /// Changes interface language without translating document text or recording Undo.
    pub fn set_locale(&mut self, locale: crate::Locale) {
        self.locale = locale;
    }
    /// Current catalog, also usable to translate API errors and DOCX reports.
    #[must_use]
    pub const fn locale(&self) -> &crate::Locale {
        &self.locale
    }
    /// Creates an editor from a validated scene.
    /// # Errors
    /// Returns the scene validation error.
    pub fn new(diagram: Diagram) -> Result<Self, String> {
        diagram.validate()?;
        Ok(Self {
            diagram,
            ..Self::default()
        })
    }
    /// Read-only scene access; edits go through history-aware commands.
    #[must_use]
    pub const fn diagram(&self) -> &Diagram {
        &self.diagram
    }
    /// Current selection.
    #[must_use]
    pub const fn selection(&self) -> Option<Target> {
        self.selection
    }
    /// Selects an existing scene object, or clears selection.
    /// # Errors
    /// Rejects IDs that are absent from the scene.
    pub fn select(&mut self, target: Option<Target>) -> Result<(), String> {
        let exists = match target {
            Some(Target::Node(id)) => self.diagram.node(id).is_some(),
            Some(Target::Connection(id)) => self.diagram.connections.iter().any(|c| c.id == id),
            None => true,
        };
        if !exists {
            return Err("Selection is not in this diagram".into());
        }
        self.commit();
        self.selection = target;
        self.multi.clear();
        self.multi = self.selected_nodes();
        Ok(())
    }
    /// Changes after committed edits, Undo and Redo.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }
    /// Replaces the scene as one undoable edit, preserving the previous one on error.
    /// # Errors
    /// Returns a scene validation error.
    pub fn replace(&mut self, diagram: Diagram) -> Result<(), String> {
        diagram.validate()?;
        self.begin();
        self.diagram = diagram;
        self.marquee = None;
        self.drag = None;
        self.selection = None;
        self.multi.clear();
        self.pending_connection = None;
        self.commit();
        Ok(())
    }
    pub(crate) fn begin(&mut self) {
        if self.edit_base.is_none() {
            self.edit_base = Some(self.diagram.clone());
        }
    }
    /// Commits an interactive edit. One mouse gesture or text edit is one Undo.
    pub fn commit(&mut self) {
        if let Some(before) = self.edit_base.take() {
            if before != self.diagram {
                self.undo.push(before);
                while self.undo.len() > 128
                    || (self.undo.len() > 1
                        && self.undo.iter().map(scene_cost).sum::<usize>() > 32 * 1024 * 1024)
                {
                    self.undo.remove(0);
                }
                self.redo.clear();
                self.revision += 1;
            }
        }
    }
    pub(crate) fn next_id(&self) -> u64 {
        self.diagram
            .nodes
            .iter()
            .map(|n| n.id)
            .chain(self.diagram.connections.iter().map(|c| c.id))
            .chain(self.diagram.groups.iter().map(|g| g.id))
            .max()
            .unwrap_or(0)
            + 1
    }
    /// Adds and selects a shape. The initial label is editable in the inspector.
    /// # Errors
    /// Returns a geometry or object limit error without altering the scene.
    pub fn add_node(&mut self, kind: ShapeKind, position: Point) -> Result<u64, String> {
        self.commit();
        let id = self.next_id();
        let node = Node {
            id,
            kind,
            position,
            size: Point::new(160.0, 80.0),
            text: self.locale.text("Text").into(),
            fill: [231, 240, 252],
            stroke: [43, 87, 154],
            font_size: 16.0,
            style: crate::NodeStyle::default(),
        };
        self.update_node_or_add(node, true)?;
        self.selection = Some(Target::Node(id));
        self.multi.clear();
        Ok(id)
    }
    fn update_node_or_add(&mut self, node: Node, add: bool) -> Result<(), String> {
        let mut next = self.diagram.clone();
        if add {
            next.nodes.push(node);
        } else {
            let id = node.id;
            *next
                .nodes
                .iter_mut()
                .find(|n| n.id == id)
                .ok_or("Shape not found")? = node;
        }
        next.validate()?;
        self.begin();
        self.diagram = next;
        self.commit();
        Ok(())
    }
    /// Atomically updates geometry, text or style.
    /// # Errors
    /// Returns a validation error without modifying the scene.
    pub fn update_node(&mut self, node: Node) -> Result<(), String> {
        self.commit();
        self.update_node_or_add(node, false)
    }
    /// Connects two shapes at explicitly chosen ports.
    /// # Errors
    /// Rejects missing nodes, self links or exceeded limits.
    pub fn connect(
        &mut self,
        from: u64,
        from_port: Port,
        to: u64,
        to_port: Port,
    ) -> Result<u64, String> {
        self.commit();
        let id = self.next_id();
        let mut next = self.diagram.clone();
        next.connections.push(Connection {
            routing: crate::Routing::AvoidObstacles,
            id,
            from,
            to,
            from_port,
            to_port,
            color: [75, 85, 105],
            width: 1.7,
            dash: crate::Dash::Solid,
            start_arrow: false,
            end_arrow: true,
        });
        next.validate()?;
        self.begin();
        self.diagram = next;
        self.commit();
        self.selection = Some(Target::Connection(id));
        self.multi.clear();
        Ok(id)
    }
    /// Deletes the selected shape and its links together, or a selected link.
    pub fn delete_selected(&mut self) {
        self.commit();
        self.begin();
        let selected = self.selected_nodes();
        match self.selection.take() {
            Some(Target::Node(_)) => {
                self.diagram.nodes.retain(|n| !selected.contains(&n.id));
                self.diagram
                    .connections
                    .retain(|c| !selected.contains(&c.from) && !selected.contains(&c.to));
                self.diagram
                    .groups
                    .retain(|g| !g.members.iter().any(|id| selected.contains(id)));
            }
            Some(Target::Connection(id)) => self.diagram.connections.retain(|c| c.id != id),
            None => {}
        }
        self.pending_connection = None;
        self.multi.clear();
        self.commit();
    }
    /// Reverts the last whole user action.
    pub fn undo(&mut self) {
        self.commit();
        if let Some(previous) = self.undo.pop() {
            self.redo
                .push(std::mem::replace(&mut self.diagram, previous));
            self.selection = None;
            self.multi.clear();
            self.pending_connection = None;
            self.drag = None;
            self.marquee = None;
            self.revision += 1;
        }
    }
    /// Reapplies the last reverted action.
    pub fn redo(&mut self) {
        self.commit();
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.diagram, next));
            self.selection = None;
            self.multi.clear();
            self.pending_connection = None;
            self.drag = None;
            self.marquee = None;
            self.revision += 1;
        }
    }
    /// Sample flowchart, made through the same public editing commands.
    #[must_use]
    #[allow(clippy::many_single_char_names)]
    pub fn sample() -> Self {
        let mut e = Self::default();
        let a = e
            .add_node(ShapeKind::Rounded, Point::new(80.0, 80.0))
            .expect("valid sample");
        let b = e
            .add_node(ShapeKind::Diamond, Point::new(360.0, 80.0))
            .expect("valid sample");
        let c = e
            .add_node(ShapeKind::Rectangle, Point::new(360.0, 280.0))
            .expect("valid sample");
        for (id, label) in [(a, "Start"), (b, "Ready?"), (c, "Process")] {
            let mut n = e.diagram.node(id).expect("sample node").clone();
            n.text = label.into();
            e.update_node(n).expect("valid sample text");
        }
        e.connect(a, Port::Right, b, Port::Left)
            .expect("valid sample link");
        e.connect(b, Port::Bottom, c, Port::Top)
            .expect("valid sample link");
        e.undo.clear();
        e.redo.clear();
        e.selection = None;
        e.revision = 0;
        e
    }
    /// Creates a localized starter scene. Subsequent language changes never
    /// translate its text or user-authored content.
    #[must_use]
    pub fn sample_with_locale(locale: crate::Locale) -> Self {
        let mut editor = Self::sample();
        for node in &mut editor.diagram.nodes {
            node.text = locale.text(&node.text).to_owned();
        }
        editor.set_locale(locale);
        editor
    }
}

fn scene_cost(scene: &Diagram) -> usize {
    scene
        .nodes
        .iter()
        .map(|n| n.text.len() + std::mem::size_of::<Node>())
        .sum::<usize>()
        + scene.connections.len() * std::mem::size_of::<Connection>()
}
