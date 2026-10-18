//! Editor state and history-aware commands. The widget in [`crate::widget`]
//! is a thin layer over these; every command validates before it mutates the
//! document so an invalid edit can never be committed.

#![allow(clippy::many_single_char_names)]

use std::collections::BTreeSet;

use crate::geometry;
use crate::locale::Locale;
use crate::mhchem;
use crate::model::{
    Atom, Bond, BondOrder, BondStereo, ChemDocument, ChemObject, Point, Reaction, Species, State,
    Structure, VERSION,
};

/// Which kind of object the structure toolbar or the reaction toolbar edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// 2D structural formulas.
    Structure,
    /// Reaction equations written as `mhchem`.
    Reaction,
}

/// The active canvas tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// Select atoms and bonds, click to inspect, drag empty space for a marquee.
    Select,
    /// Move a whole object regardless of what is under the pointer.
    Move,
    /// Place atoms of the current element.
    Atom(u8),
    /// Draw bonds of the current order and stereo.
    Bond,
    /// Stamp a ring of the current size.
    Ring,
    /// Delete atoms and bonds.
    Erase,
}

/// A selected entity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Target {
    /// A top-level object.
    Object(u64),
    /// An atom inside a structure: `(structure, atom)`.
    Atom(u64, u64),
    /// A bond inside a structure: `(structure, bond)`.
    Bond(u64, u64),
}

/// Axis for alignment and distribution of selected atoms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// Left/right (`x`).
    Horizontal,
    /// Top/bottom (`y`).
    Vertical,
}

/// Which edge of the selection an alignment uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignEdge {
    /// Minimum coordinate or left/top.
    Start,
    /// Midpoint of the selection bounds.
    Center,
    /// Maximum coordinate or right/bottom.
    End,
}

/// A pointer gesture in progress on the canvas.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Drag {
    /// Panning the view; the last pointer position in screen space.
    Pan {
        /// Last pointer position.
        last: egui::Pos2,
    },
    /// Dragging the whole selection; the last pointer position in world space.
    Move {
        /// Last pointer position.
        last: Point,
    },
    /// Drawing a bond from an existing atom.
    Bond {
        /// Structure of the source atom.
        structure: u64,
        /// Source atom.
        from: u64,
        /// Current pointer position, for the preview line.
        current: Point,
    },
    /// Rubber-band selection.
    Marquee {
        /// Where the drag started.
        start: Point,
        /// Current pointer position.
        current: Point,
        /// Whether the marquee adds to the existing selection.
        additive: bool,
    },
}

/// One undo/redo history entry: the document plus what was selected.
#[derive(Clone)]
pub(crate) struct Snapshot {
    document: ChemDocument,
    selected: BTreeSet<Target>,
    active: Option<u64>,
}

/// Reusable editor with bounded document history. The host owns persistence.
#[derive(Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct ChemEditor {
    pub(crate) locale: Locale,
    pub(crate) document: ChemDocument,
    pub(crate) mode: Mode,
    pub(crate) tool: Tool,
    pub(crate) element: u8,
    pub(crate) bond_order: BondOrder,
    pub(crate) bond_stereo: BondStereo,
    pub(crate) ring_size: usize,
    pub(crate) selected: BTreeSet<Target>,
    pub(crate) active: Option<u64>,
    pub(crate) undo: Vec<Snapshot>,
    pub(crate) redo: Vec<Snapshot>,
    pub(crate) edit_base: Option<Snapshot>,
    pub(crate) clipboard: Option<ChemDocument>,
    pub(crate) paste_count: u32,
    pub(crate) pan: egui::Vec2,
    pub(crate) zoom: f32,
    pub(crate) want_fit: bool,
    pub(crate) snap: bool,
    pub(crate) show_carbon: bool,
    pub(crate) colored_atoms: bool,
    pub(crate) revision: u64,
    pub(crate) source: String,
    pub(crate) source_id: Option<u64>,
    pub(crate) drag: Option<Drag>,
}

impl Default for ChemEditor {
    fn default() -> Self {
        Self {
            locale: Locale::default(),
            document: ChemDocument::default(),
            mode: Mode::Structure,
            tool: Tool::Select,
            element: 6,
            bond_order: BondOrder::Single,
            bond_stereo: BondStereo::Plain,
            ring_size: 6,
            selected: BTreeSet::new(),
            active: None,
            undo: vec![],
            redo: vec![],
            edit_base: None,
            clipboard: None,
            paste_count: 0,
            pan: egui::vec2(60.0, 40.0),
            zoom: 1.0,
            want_fit: false,
            snap: true,
            show_carbon: false,
            colored_atoms: true,
            revision: 0,
            source: String::new(),
            source_id: None,
            drag: None,
        }
    }
}

impl ChemEditor {
    /// Creates an editor from a validated document.
    ///
    /// # Errors
    ///
    /// Returns the document validation error.
    pub fn new(document: ChemDocument) -> Result<Self, String> {
        document.validate()?;
        let active = document.objects.first().map(ChemObject::id);
        Ok(Self {
            document,
            active,
            ..Self::default()
        })
    }

    /// Changes interface language without translating content or recording Undo.
    pub fn set_locale(&mut self, locale: Locale) {
        self.locale = locale;
    }

    /// Current catalog, also usable to translate API diagnostics.
    #[must_use]
    pub const fn locale(&self) -> &Locale {
        &self.locale
    }

    /// Read-only document access; edits go through history-aware commands.
    #[must_use]
    pub const fn document(&self) -> &ChemDocument {
        &self.document
    }

    /// The mode the toolbar and canvas edit.
    #[must_use]
    pub const fn mode(&self) -> Mode {
        self.mode
    }

    /// Switches mode and keeps the selection only if it still applies.
    pub fn set_mode(&mut self, mode: Mode) {
        if self.mode != mode {
            self.mode = mode;
            self.tool = Tool::Select;
            self.selected.clear();
            self.active = self
                .document
                .objects
                .iter()
                .find(|object| {
                    matches!(
                        (mode, object),
                        (Mode::Structure, ChemObject::Structure(_))
                            | (Mode::Reaction, ChemObject::Reaction(_))
                    )
                })
                .map(ChemObject::id);
        }
    }

    /// The active tool.
    #[must_use]
    pub const fn tool(&self) -> Tool {
        self.tool
    }

    /// Selects a tool. Choosing an atom tool also sets the current element.
    pub fn set_tool(&mut self, tool: Tool) {
        if let Tool::Atom(element) = tool {
            self.element = element;
        }
        self.tool = tool;
    }

    /// Atomic number placed by the atom tool.
    #[must_use]
    pub const fn element(&self) -> u8 {
        self.element
    }

    /// Sets the atomic number placed by the atom tool.
    pub fn set_element(&mut self, element: u8) {
        self.element = element;
        self.tool = Tool::Atom(element);
    }

    /// Bond order drawn by the bond tool.
    #[must_use]
    pub const fn bond_order(&self) -> BondOrder {
        self.bond_order
    }

    /// Sets the bond order and stereo drawn by the bond tool.
    pub fn set_bond_style(&mut self, order: BondOrder, stereo: BondStereo) {
        self.bond_order = order;
        self.bond_stereo = stereo;
    }

    /// Ring size stamped by the ring tool.
    #[must_use]
    pub const fn ring_size(&self) -> usize {
        self.ring_size
    }

    /// Sets the ring size and selects the ring tool.
    pub fn set_ring_size(&mut self, size: usize) {
        self.ring_size = size.clamp(3, 8);
        self.tool = Tool::Ring;
    }

    /// Whether skeletal carbon labels are shown.
    #[must_use]
    pub const fn show_carbon(&self) -> bool {
        self.show_carbon
    }

    /// Toggles skeletal carbon labels.
    pub fn set_show_carbon(&mut self, show: bool) {
        self.show_carbon = show;
    }

    /// Whether element labels use CPK colours.
    #[must_use]
    pub const fn colored_atoms(&self) -> bool {
        self.colored_atoms
    }

    /// Toggles CPK colouring.
    pub fn set_colored_atoms(&mut self, colored: bool) {
        self.colored_atoms = colored;
    }

    /// Whether dragged atoms and new atoms snap to a grid.
    #[must_use]
    pub const fn snap(&self) -> bool {
        self.snap
    }

    /// Toggles grid snapping.
    pub fn set_snap(&mut self, snap: bool) {
        self.snap = snap;
    }

    /// Requests that the next canvas frame fits the document to the viewport.
    pub fn request_fit(&mut self) {
        self.want_fit = true;
    }

    /// The current selection as an ordered list.
    #[must_use]
    pub fn selected(&self) -> Vec<Target> {
        self.selected.iter().copied().collect()
    }

    /// Number of selected entities.
    #[must_use]
    pub fn selection_count(&self) -> usize {
        self.selected.len()
    }

    /// The first selected entity, or `None`.
    #[must_use]
    pub fn primary(&self) -> Option<Target> {
        self.selected.iter().next().copied()
    }

    /// Whether `target` is selected.
    #[must_use]
    pub fn is_selected(&self, target: Target) -> bool {
        self.selected.contains(&target)
    }

    /// The object the structure/reaction toolbar edits.
    #[must_use]
    pub const fn active(&self) -> Option<u64> {
        self.active
    }

    /// Selects the active object, or clears it.
    pub fn set_active(&mut self, id: Option<u64>) {
        self.active = id.filter(|id| self.document.object(*id).is_some());
    }

    /// Changes after committed edits, Undo and Redo.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Replaces the document as one undoable edit, preserving the previous one
    /// on error.
    ///
    /// # Errors
    ///
    /// Returns a document validation error.
    pub fn replace(&mut self, document: ChemDocument) -> Result<(), String> {
        document.validate()?;
        self.begin();
        self.document = document;
        self.selected.clear();
        self.active = self.document.objects.first().map(ChemObject::id);
        self.commit();
        Ok(())
    }

    /// Replaces the whole selection with `target` (or clears it).
    ///
    /// # Errors
    ///
    /// Rejects IDs absent from the document.
    pub fn select(&mut self, target: Option<Target>) -> Result<(), String> {
        self.selected.clear();
        if let Some(target) = target {
            self.validate_target(target)?;
            self.selected.insert(target);
        }
        Ok(())
    }

    /// Adds `target` to the selection or removes it if already there.
    ///
    /// # Errors
    ///
    /// Rejects IDs absent from the document.
    pub fn toggle(&mut self, target: Target) -> Result<(), String> {
        self.validate_target(target)?;
        if !self.selected.remove(&target) {
            self.selected.insert(target);
        }
        Ok(())
    }

    /// Clears the selection.
    pub fn clear_selection(&mut self) {
        self.selected.clear();
    }

    /// Replaces the selection with a batch of existing entities.
    ///
    /// # Errors
    ///
    /// Rejects IDs absent from the document.
    pub fn set_selection(
        &mut self,
        targets: impl IntoIterator<Item = Target>,
    ) -> Result<(), String> {
        let mut next = BTreeSet::new();
        for target in targets {
            self.validate_target(target)?;
            next.insert(target);
        }
        self.selected = next;
        Ok(())
    }

    fn validate_target(&self, target: Target) -> Result<(), String> {
        let exists = match target {
            Target::Object(id) => self.document.object(id).is_some(),
            Target::Atom(structure, atom) => self
                .structure(structure)
                .is_some_and(|s| s.atom(atom).is_some()),
            Target::Bond(structure, bond) => self
                .structure(structure)
                .is_some_and(|s| s.bonds.iter().any(|b| b.id == bond)),
        };
        if exists {
            Ok(())
        } else {
            Err("Selection is not in this document".into())
        }
    }

    pub(crate) fn begin(&mut self) {
        if self.edit_base.is_none() {
            self.edit_base = Some(self.snapshot());
        }
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            document: self.document.clone(),
            selected: self.selected.clone(),
            active: self.active,
        }
    }

    fn restore(&mut self, state: Snapshot) {
        self.document = state.document;
        self.selected = state.selected;
        self.active = state
            .active
            .filter(|id| self.document.object(*id).is_some());
        self.drag = None;
    }

    /// Commits an interactive edit. One gesture is one Undo step.
    pub fn commit(&mut self) {
        if let Some(before) = self.edit_base.take() {
            if before.document != self.document {
                self.undo.push(before);
                while self.undo.len() > 128
                    || (self.undo.len() > 1
                        && self
                            .undo
                            .iter()
                            .map(|state| document_cost(&state.document))
                            .sum::<usize>()
                            > 32 * 1024 * 1024)
                {
                    self.undo.remove(0);
                }
                self.redo.clear();
                self.revision += 1;
            }
        }
    }

    /// Reverts the last whole user action, restoring the selection too.
    pub fn undo(&mut self) {
        self.commit();
        if let Some(previous) = self.undo.pop() {
            let current = self.snapshot();
            self.redo.push(current);
            self.restore(previous);
            self.revision += 1;
        }
    }

    /// Reapplies the last reverted action, restoring the selection too.
    pub fn redo(&mut self) {
        self.commit();
        if let Some(next) = self.redo.pop() {
            let current = self.snapshot();
            self.undo.push(current);
            self.restore(next);
            self.revision += 1;
        }
    }

    pub(crate) fn next_id(&self) -> u64 {
        let objects = self.document.objects.iter().map(ChemObject::id);
        let mut max = objects.max().unwrap_or(0);
        for object in &self.document.objects {
            if let ChemObject::Structure(structure) = object {
                max = max
                    .max(structure.atoms.iter().map(|a| a.id).max().unwrap_or(0))
                    .max(structure.bonds.iter().map(|b| b.id).max().unwrap_or(0));
            }
        }
        max + 1
    }

    /// The structure with the given ID.
    #[must_use]
    pub fn structure(&self, id: u64) -> Option<&Structure> {
        match self.document.object(id) {
            Some(ChemObject::Structure(structure)) => Some(structure),
            _ => None,
        }
    }

    /// The reaction with the given ID.
    #[must_use]
    pub fn reaction(&self, id: u64) -> Option<&Reaction> {
        match self.document.object(id) {
            Some(ChemObject::Reaction(reaction)) => Some(reaction),
            _ => None,
        }
    }

    /// Adds an empty structure and makes it active.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the document limit is reached.
    pub fn add_structure(&mut self, origin: Point) -> Result<u64, String> {
        self.commit();
        let id = self.next_id();
        let mut next = self.document.clone();
        next.objects.push(ChemObject::Structure(Structure {
            id,
            origin,
            atoms: vec![],
            bonds: vec![],
        }));
        next.validate()?;
        self.begin();
        self.document = next;
        self.active = Some(id);
        self.commit();
        Ok(id)
    }

    /// Adds an empty reaction with a starter `A + B -> C`.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the document limit is reached.
    pub fn add_reaction(&mut self, origin: Point) -> Result<u64, String> {
        self.commit();
        let id = self.next_id();
        let mut starter = mhchem::parse("A + B -> C")?;
        starter.id = id;
        starter.origin = origin;
        let mut next = self.document.clone();
        next.objects.push(ChemObject::Reaction(starter));
        next.validate()?;
        self.begin();
        self.document = next;
        self.active = Some(id);
        self.commit();
        Ok(id)
    }

    /// The active structure, creating an empty one on first use.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the object cannot be created.
    pub fn ensure_structure(&mut self) -> Result<u64, String> {
        if let Some(id) = self.active.filter(|id| self.structure(*id).is_some()) {
            return Ok(id);
        }
        if let Some(ChemObject::Structure(structure)) = self.document.objects.first() {
            self.active = Some(structure.id);
            return Ok(structure.id);
        }
        self.add_structure(Point::new(40.0, 40.0))
    }

    /// Adds an atom (scene coordinates) to the active structure.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the atom cannot be added.
    pub fn add_atom(&mut self, element: u8, position: Point) -> Result<u64, String> {
        let structure_id = self.ensure_structure()?;
        self.commit();
        let atom_id = self.next_id();
        let mut next = self.document.clone();
        let object = next
            .objects
            .iter_mut()
            .find(|object| object.id() == structure_id)
            .ok_or("Active structure is missing")?;
        let ChemObject::Structure(structure) = object else {
            return Err("Active object is not a structure".into());
        };
        let local = position.sub(structure.origin);
        let local = if self.snap {
            snap_to_grid(local)
        } else {
            local
        };
        structure.atoms.push(Atom {
            id: atom_id,
            element,
            position: local,
            charge: 0,
            hydrogens: None,
        });
        next.validate()?;
        self.begin();
        self.document = next;
        self.selected.clear();
        self.selected.insert(Target::Atom(structure_id, atom_id));
        self.commit();
        Ok(atom_id)
    }

    /// Adds a bond between two existing atoms of the active structure.
    ///
    /// # Errors
    ///
    /// Rejects a missing structure, unknown atoms or a duplicate bond.
    pub fn connect(&mut self, a: u64, b: u64) -> Result<u64, String> {
        let structure_id = self.active.ok_or("No active structure")?;
        self.commit();
        let bond_id = self.next_id();
        let mut next = self.document.clone();
        let object = next
            .objects
            .iter_mut()
            .find(|object| object.id() == structure_id)
            .ok_or("Active structure is missing")?;
        let ChemObject::Structure(structure) = object else {
            return Err("Active object is not a structure".into());
        };
        structure.bonds.push(Bond {
            id: bond_id,
            a,
            b,
            order: self.bond_order,
            stereo: self.bond_stereo,
        });
        next.validate()?;
        self.begin();
        self.document = next;
        self.selected.clear();
        self.selected.insert(Target::Bond(structure_id, bond_id));
        self.commit();
        Ok(bond_id)
    }

    /// Creates a new atom at `position` and bonds it to `parent`.
    ///
    /// # Errors
    ///
    /// Propagates [`Self::add_atom`] and [`Self::connect`] diagnostics.
    pub fn bond_from(&mut self, parent: u64, element: u8, position: Point) -> Result<u64, String> {
        let atom = self.add_atom(element, position)?;
        self.connect(parent, atom)?;
        Ok(atom)
    }

    /// Stamps a regular ring (alternating Kekulé bonds) centred on `center`.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the ring cannot be added.
    pub fn add_ring(&mut self, center: Point, sides: usize) -> Result<u64, String> {
        let structure_id = self.ensure_structure()?;
        self.commit();
        let sides = sides.clamp(3, 8);
        let vertices = geometry::ring_vertices(
            center,
            sides,
            geometry::BOND_LENGTH,
            -std::f32::consts::FRAC_PI_2,
        );
        let mut next = self.document.clone();
        let base = self.next_id();
        let object = next
            .objects
            .iter_mut()
            .find(|object| object.id() == structure_id)
            .ok_or("Active structure is missing")?;
        let ChemObject::Structure(structure) = object else {
            return Err("Active object is not a structure".into());
        };
        let mut ids = vec![];
        for (index, vertex) in vertices.iter().enumerate() {
            let id = base + index as u64;
            ids.push(id);
            structure.atoms.push(Atom {
                id,
                element: 6,
                position: vertex.sub(structure.origin),
                charge: 0,
                hydrogens: None,
            });
        }
        for index in 0..sides {
            let order = if sides == 4 {
                BondOrder::Single
            } else if index % 2 == 0 {
                BondOrder::Double
            } else {
                BondOrder::Single
            };
            structure.bonds.push(Bond {
                id: base + sides as u64 + index as u64,
                a: ids[index],
                b: ids[(index + 1) % sides],
                order,
                stereo: BondStereo::Plain,
            });
        }
        next.validate()?;
        self.begin();
        self.document = next;
        self.selected.clear();
        self.selected.insert(Target::Object(structure_id));
        self.commit();
        Ok(structure_id)
    }

    /// Sets an atom's element.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the change is rejected.
    pub fn set_atom_element(
        &mut self,
        structure: u64,
        atom: u64,
        element: u8,
    ) -> Result<(), String> {
        let mut next = self.document.clone();
        let target = Self::find_atom_mut(&mut next, structure, atom)?;
        target.element = element;
        self.apply(next)
    }

    /// Sets an atom's formal charge.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the change is rejected.
    pub fn set_atom_charge(&mut self, structure: u64, atom: u64, charge: i8) -> Result<(), String> {
        let mut next = self.document.clone();
        let target = Self::find_atom_mut(&mut next, structure, atom)?;
        target.charge = charge;
        self.apply(next)
    }

    /// Sets an atom's explicit hydrogen count, or clears the override.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the change is rejected.
    pub fn set_atom_hydrogens(
        &mut self,
        structure: u64,
        atom: u64,
        hydrogens: Option<u8>,
    ) -> Result<(), String> {
        let mut next = self.document.clone();
        let target = Self::find_atom_mut(&mut next, structure, atom)?;
        target.hydrogens = hydrogens;
        self.apply(next)
    }

    /// Moves an atom to a scene position.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the change is rejected.
    pub fn move_atom(&mut self, structure: u64, atom: u64, position: Point) -> Result<(), String> {
        let mut next = self.document.clone();
        let origin = next
            .object(structure)
            .map_or(Point::default(), ChemObject::origin);
        let target = Self::find_atom_mut(&mut next, structure, atom)?;
        target.position = position.sub(origin);
        self.apply(next)
    }

    /// Sets a bond's order and stereo.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the change is rejected.
    pub fn set_bond(
        &mut self,
        structure: u64,
        bond: u64,
        order: BondOrder,
        stereo: BondStereo,
    ) -> Result<(), String> {
        let mut next = self.document.clone();
        let Some(ChemObject::Structure(target)) = next
            .objects
            .iter_mut()
            .find(|object| object.id() == structure)
        else {
            return Err("Structure not found".into());
        };
        let Some(target) = target.bonds.iter_mut().find(|b| b.id == bond) else {
            return Err("Bond not found".into());
        };
        target.order = order;
        target.stereo = stereo;
        self.apply(next)
    }

    /// Moves a whole object by a scene delta.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the change is rejected.
    pub fn move_object(&mut self, id: u64, delta: Point) -> Result<(), String> {
        let mut next = self.document.clone();
        let Some(object) = next.objects.iter_mut().find(|object| object.id() == id) else {
            return Err("Object not found".into());
        };
        let origin = object.origin_mut();
        origin.x += delta.x;
        origin.y += delta.y;
        self.apply(next)
    }

    fn apply(&mut self, next: ChemDocument) -> Result<(), String> {
        next.validate()?;
        self.begin();
        self.document = next;
        self.commit();
        Ok(())
    }

    fn find_atom_mut(
        document: &mut ChemDocument,
        structure: u64,
        atom: u64,
    ) -> Result<&mut Atom, String> {
        let Some(ChemObject::Structure(structure)) = document
            .objects
            .iter_mut()
            .find(|object| object.id() == structure)
        else {
            return Err("Structure not found".into());
        };
        structure
            .atoms
            .iter_mut()
            .find(|candidate| candidate.id == atom)
            .ok_or_else(|| "Atom not found".into())
    }

    /// Deletes every selected object, atom and bond in one Undo step.
    pub fn delete_selected(&mut self) {
        self.commit();
        if self.selected.is_empty() {
            return;
        }
        let selection: BTreeSet<Target> = std::mem::take(&mut self.selected);
        let mut objects: BTreeSet<u64> = BTreeSet::new();
        let mut atoms: BTreeSet<(u64, u64)> = BTreeSet::new();
        let mut bonds: BTreeSet<(u64, u64)> = BTreeSet::new();
        for target in selection {
            match target {
                Target::Object(id) => {
                    objects.insert(id);
                }
                Target::Atom(structure, atom) => {
                    atoms.insert((structure, atom));
                }
                Target::Bond(structure, bond) => {
                    bonds.insert((structure, bond));
                }
            }
        }
        let mut next = self.document.clone();
        next.objects
            .retain(|object| !objects.contains(&object.id()));
        for object in &mut next.objects {
            let ChemObject::Structure(structure) = object else {
                continue;
            };
            let id = structure.id;
            structure
                .atoms
                .retain(|atom| !atoms.contains(&(id, atom.id)));
            structure.bonds.retain(|bond| {
                !bonds.contains(&(id, bond.id))
                    && !atoms.contains(&(id, bond.a))
                    && !atoms.contains(&(id, bond.b))
            });
        }
        self.begin();
        self.document = next;
        self.commit();
    }

    /// Moves every selected atom and object by `delta` as one Undo step.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the change is rejected.
    pub fn nudge_selected(&mut self, delta: Point) -> Result<(), String> {
        if delta.x == 0.0 && delta.y == 0.0 {
            return Ok(());
        }
        let selection: Vec<Target> = self.selected();
        if selection.is_empty() {
            return Ok(());
        }
        let mut next = self.document.clone();
        for target in selection {
            match target {
                Target::Object(id) => {
                    if let Some(object) = next.objects.iter_mut().find(|o| o.id() == id) {
                        let origin = object.origin_mut();
                        origin.x += delta.x;
                        origin.y += delta.y;
                    }
                }
                Target::Atom(structure, atom) => {
                    if let Some(ChemObject::Structure(target)) =
                        next.objects.iter_mut().find(|o| o.id() == structure)
                    {
                        if let Some(target) = target.atoms.iter_mut().find(|a| a.id == atom) {
                            target.position.x += delta.x;
                            target.position.y += delta.y;
                        }
                    }
                }
                Target::Bond(..) => {}
            }
        }
        self.apply(next)
    }

    /// Shifts the whole selection without committing, for a live drag. The
    /// caller commits once at the end of the gesture.
    pub fn drag_selection(&mut self, delta: Point) {
        if delta.x == 0.0 && delta.y == 0.0 {
            return;
        }
        let selection: Vec<Target> = self.selected();
        for target in selection {
            match target {
                Target::Object(id) => {
                    if let Some(object) = self.document.objects.iter_mut().find(|o| o.id() == id) {
                        let origin = object.origin_mut();
                        origin.x += delta.x;
                        origin.y += delta.y;
                    }
                }
                Target::Atom(structure, atom) => {
                    if let Some(ChemObject::Structure(target)) = self
                        .document
                        .objects
                        .iter_mut()
                        .find(|o| o.id() == structure)
                    {
                        if let Some(target) = target.atoms.iter_mut().find(|a| a.id == atom) {
                            target.position.x += delta.x;
                            target.position.y += delta.y;
                            if self.snap {
                                target.position = snap_to_grid(target.position);
                            }
                        }
                    }
                }
                Target::Bond(..) => {}
            }
        }
    }

    /// Aligns the selected atoms along one axis, as one Undo step.
    ///
    /// # Errors
    ///
    /// Returns a diagnostic when fewer than two atoms are selected or the
    /// change is rejected.
    pub fn align_selection(&mut self, axis: Axis, edge: AlignEdge) -> Result<(), String> {
        let atoms = self.selected_atoms_world();
        if atoms.len() < 2 {
            return Err("Select at least two atoms to align".into());
        }
        let coordinate = |point: Point| match axis {
            Axis::Horizontal => point.x,
            Axis::Vertical => point.y,
        };
        let mut min = f32::INFINITY;
        let mut max = f32::NEG_INFINITY;
        for (_, _, point) in &atoms {
            min = min.min(coordinate(*point));
            max = max.max(coordinate(*point));
        }
        let target = match edge {
            AlignEdge::Start => min,
            AlignEdge::Center => min.midpoint(max),
            AlignEdge::End => max,
        };
        let updates: Vec<_> = atoms
            .iter()
            .map(|(structure, atom, point)| {
                let mut moved = *point;
                match axis {
                    Axis::Horizontal => moved.x = target,
                    Axis::Vertical => moved.y = target,
                }
                (*structure, *atom, moved)
            })
            .collect();
        self.apply_positions(&updates)
    }

    /// Spreads the selected atoms evenly along one axis, as one Undo step.
    ///
    /// # Errors
    ///
    /// Returns a diagnostic when fewer than three atoms are selected or the
    /// change is rejected.
    #[allow(clippy::cast_precision_loss)]
    pub fn distribute_selection(&mut self, axis: Axis) -> Result<(), String> {
        let mut atoms = self.selected_atoms_world();
        if atoms.len() < 3 {
            return Err("Select at least three atoms to distribute".into());
        }
        let coordinate = |point: Point| match axis {
            Axis::Horizontal => point.x,
            Axis::Vertical => point.y,
        };
        atoms.sort_by(|a, b| {
            coordinate(a.2)
                .partial_cmp(&coordinate(b.2))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let first = coordinate(atoms[0].2);
        let last = coordinate(atoms[atoms.len() - 1].2);
        if (last - first).abs() < 0.001 {
            return Ok(());
        }
        let steps = (atoms.len() - 1) as f32;
        let updates: Vec<_> = atoms
            .iter()
            .enumerate()
            .map(|(index, (structure, atom, point))| {
                let mut moved = *point;
                let value = first + (last - first) * (index as f32) / steps;
                match axis {
                    Axis::Horizontal => moved.x = value,
                    Axis::Vertical => moved.y = value,
                }
                (*structure, *atom, moved)
            })
            .collect();
        self.apply_positions(&updates)
    }

    fn selected_atoms_world(&self) -> Vec<(u64, u64, Point)> {
        let mut atoms = vec![];
        for target in self.selected() {
            let Target::Atom(structure, atom) = target else {
                continue;
            };
            if let Some(point) = self.atom_world(structure, atom) {
                atoms.push((structure, atom, point));
            }
        }
        atoms
    }

    pub(crate) fn atom_world(&self, structure: u64, atom: u64) -> Option<Point> {
        let structure = self.structure(structure)?;
        Some(structure.atom(atom)?.position.add(structure.origin))
    }

    fn apply_positions(&mut self, updates: &[(u64, u64, Point)]) -> Result<(), String> {
        let mut next = self.document.clone();
        for (structure_id, atom_id, world) in updates {
            let Some(ChemObject::Structure(structure)) =
                next.objects.iter_mut().find(|o| o.id() == *structure_id)
            else {
                continue;
            };
            let origin = structure.origin;
            if let Some(atom) = structure.atoms.iter_mut().find(|a| a.id == *atom_id) {
                atom.position = world.sub(origin);
            }
        }
        self.apply(next)
    }

    /// Copies the whole selection into the internal and system clipboard.
    #[must_use]
    pub fn copy_selection(&mut self) -> Option<String> {
        let fragment = self.fragment_from_selection()?;
        let json = fragment.to_json().ok()?;
        self.clipboard = Some(fragment);
        self.paste_count = 0;
        Some(json)
    }

    /// Copies then deletes the selection, as one Undo step.
    #[must_use]
    pub fn cut_selection(&mut self) -> Option<String> {
        let json = self.copy_selection();
        self.delete_selected();
        json
    }

    /// Pastes the internal clipboard with fresh IDs, offsetting each paste.
    ///
    /// # Errors
    ///
    /// Returns a diagnostic when there is nothing to paste or the copy is
    /// rejected.
    pub fn paste_clipboard(&mut self) -> Result<(), String> {
        let Some(fragment) = self.clipboard.clone() else {
            return Err("Nothing to paste".into());
        };
        self.paste_count += 1;
        let step = f32::from(u16::try_from(self.paste_count).unwrap_or(u16::MAX));
        let offset = Point::new(24.0 * step, 24.0 * step);
        self.insert_fragment(fragment, offset)
    }

    /// Pastes a JSON fragment (for example from the system clipboard).
    ///
    /// # Errors
    ///
    /// Returns a diagnostic when the text is not a fragment or the copy is
    /// rejected.
    pub fn paste_json(&mut self, json: &str) -> Result<(), String> {
        let fragment = ChemDocument::from_json(json.trim())?;
        self.paste_count += 1;
        let offset = Point::new(24.0, 24.0);
        self.insert_fragment(fragment, offset)
    }

    fn insert_fragment(&mut self, fragment: ChemDocument, offset: Point) -> Result<(), String> {
        self.commit();
        let mut next = self.document.clone();
        let mut counter = self.next_id();
        let mut new_targets = vec![];
        let mut new_active = None;
        for mut object in fragment.objects {
            renumber_with(&mut object, &mut counter);
            *object.origin_mut() = object.origin().add(offset);
            let id = object.id();
            match &object {
                ChemObject::Structure(structure) => {
                    new_targets
                        .extend(structure.atoms.iter().map(|atom| Target::Atom(id, atom.id)));
                }
                ChemObject::Reaction(_) => new_targets.push(Target::Object(id)),
            }
            new_active = Some(id);
            next.objects.push(object);
        }
        next.validate()?;
        self.begin();
        self.document = next;
        self.set_selection(new_targets).ok();
        if let Some(id) = new_active {
            self.active = Some(id);
        }
        self.commit();
        Ok(())
    }

    fn fragment_from_selection(&self) -> Option<ChemDocument> {
        let selection = self.selected();
        if selection.is_empty() {
            return None;
        }
        let mut fragment = ChemDocument::default();
        let mut counter = 1_u64;
        let object_ids: BTreeSet<u64> = selection
            .iter()
            .filter_map(|target| match target {
                Target::Object(id) => Some(*id),
                _ => None,
            })
            .collect();
        for id in &object_ids {
            if let Some(object) = self.document.object(*id).cloned() {
                let mut copy = object;
                renumber_with(&mut copy, &mut counter);
                fragment.objects.push(copy);
            }
        }
        let mut per_structure: std::collections::BTreeMap<u64, BTreeSet<u64>> =
            std::collections::BTreeMap::new();
        for target in &selection {
            if let Target::Atom(structure, atom) = target {
                per_structure.entry(*structure).or_default().insert(*atom);
            }
        }
        for (structure_id, atoms) in per_structure {
            if object_ids.contains(&structure_id) {
                continue;
            }
            let Some(source) = self.structure(structure_id) else {
                continue;
            };
            let mut new_structure = Structure {
                id: 0,
                origin: Point::default(),
                atoms: vec![],
                bonds: vec![],
            };
            let mut remapped = std::collections::BTreeMap::new();
            for atom in &source.atoms {
                if atoms.contains(&atom.id) {
                    let mut copy = atom.clone();
                    copy.position = atom.position.add(source.origin);
                    copy.id = counter;
                    remapped.insert(atom.id, counter);
                    counter += 1;
                    new_structure.atoms.push(copy);
                }
            }
            for bond in &source.bonds {
                let (Some(a), Some(b)) = (remapped.get(&bond.a), remapped.get(&bond.b)) else {
                    continue;
                };
                let mut copy = bond.clone();
                copy.id = counter;
                copy.a = *a;
                copy.b = *b;
                counter += 1;
                new_structure.bonds.push(copy);
            }
            if new_structure.atoms.is_empty() {
                continue;
            }
            new_structure.id = counter;
            counter += 1;
            fragment.objects.push(ChemObject::Structure(new_structure));
        }
        fragment.validate().ok()?;
        Some(fragment)
    }

    /// Duplicates the active object, offsetting the copy.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the copy cannot be added.
    pub fn duplicate_active(&mut self) -> Result<u64, String> {
        let Some(id) = self.active else {
            return Err("Nothing to duplicate".into());
        };
        let Some(object) = self.document.object(id).cloned() else {
            return Err("Object not found".into());
        };
        self.commit();
        let mut next = self.document.clone();
        let offset = Point::new(24.0, 24.0);
        let mut copy = object;
        self.renumber(&mut copy);
        *copy.origin_mut() = copy.origin().add(offset);
        next.objects.push(copy);
        next.validate()?;
        let id = next.objects.last().map(ChemObject::id).unwrap_or_default();
        self.begin();
        self.document = next;
        self.active = Some(id);
        self.commit();
        Ok(id)
    }

    fn renumber(&self, object: &mut ChemObject) {
        let mut next = self.next_id();
        renumber_with(object, &mut next);
    }

    /// Duplicates every selected object and atom/bond fragment with fresh IDs.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the copies cannot be added.
    #[allow(clippy::too_many_lines)]
    pub fn duplicate_selected(&mut self) -> Result<(), String> {
        let selection = self.selected();
        if selection.is_empty() {
            return Err("Nothing to duplicate".into());
        }
        self.commit();
        let offset = Point::new(24.0, 24.0);
        let mut counter = self.next_id();
        let mut next = self.document.clone();
        let mut new_targets: Vec<Target> = vec![];
        let mut new_active = None;

        let object_ids: BTreeSet<u64> = selection
            .iter()
            .filter_map(|target| match target {
                Target::Object(id) => Some(*id),
                _ => None,
            })
            .collect();
        for id in &object_ids {
            let Some(object) = self.document.object(*id).cloned() else {
                continue;
            };
            let mut copy = object;
            renumber_with(&mut copy, &mut counter);
            *copy.origin_mut() = copy.origin().add(offset);
            let new_id = copy.id();
            new_targets.push(Target::Object(new_id));
            new_active = Some(new_id);
            next.objects.push(copy);
        }

        let mut per_structure: std::collections::BTreeMap<u64, (BTreeSet<u64>, BTreeSet<u64>)> =
            std::collections::BTreeMap::new();
        for target in &selection {
            match target {
                Target::Atom(structure, atom) => {
                    per_structure.entry(*structure).or_default().0.insert(*atom);
                }
                Target::Bond(structure, bond) => {
                    per_structure.entry(*structure).or_default().1.insert(*bond);
                }
                Target::Object(_) => {}
            }
        }
        for (structure_id, (atoms, _bonds)) in per_structure {
            if object_ids.contains(&structure_id) {
                continue;
            }
            let Some(source) = self.structure(structure_id) else {
                continue;
            };
            let mut new_structure = Structure {
                id: 0,
                origin: source.origin.add(offset),
                atoms: vec![],
                bonds: vec![],
            };
            let mut remapped = std::collections::BTreeMap::new();
            for atom in &source.atoms {
                if atoms.contains(&atom.id) {
                    let mut copy = atom.clone();
                    copy.id = counter;
                    remapped.insert(atom.id, counter);
                    counter += 1;
                    new_structure.atoms.push(copy);
                }
            }
            for bond in &source.bonds {
                let (Some(a), Some(b)) = (remapped.get(&bond.a), remapped.get(&bond.b)) else {
                    continue;
                };
                let mut copy = bond.clone();
                copy.id = counter;
                copy.a = *a;
                copy.b = *b;
                counter += 1;
                new_structure.bonds.push(copy);
            }
            if new_structure.atoms.is_empty() {
                continue;
            }
            new_structure.id = counter;
            counter += 1;
            let new_id = new_structure.id;
            new_targets.extend(
                new_structure
                    .atoms
                    .iter()
                    .map(|atom| Target::Atom(new_id, atom.id)),
            );
            new_targets.extend(
                new_structure
                    .bonds
                    .iter()
                    .map(|bond| Target::Bond(new_id, bond.id)),
            );
            new_active = Some(new_id);
            next.objects.push(ChemObject::Structure(new_structure));
        }

        next.validate()?;
        self.begin();
        self.document = next;
        self.set_selection(new_targets).ok();
        if let Some(id) = new_active {
            self.active = Some(id);
        }
        self.commit();
        Ok(())
    }

    /// The `mhchem` source of a reaction.
    #[must_use]
    pub fn reaction_source(&self, id: u64) -> Option<String> {
        self.reaction(id).map(mhchem::write)
    }

    /// Replaces a reaction from its `mhchem` source, keeping the ID and origin.
    ///
    /// # Errors
    ///
    /// Returns the `mhchem` diagnostic and leaves the reaction unchanged.
    pub fn set_reaction_source(&mut self, id: u64, source: &str) -> Result<(), String> {
        let mut parsed = mhchem::parse(source)?;
        let origin = self
            .reaction(id)
            .map_or(Point::default(), |reaction| reaction.origin);
        parsed.id = id;
        parsed.origin = origin;
        let mut next = self.document.clone();
        let Some(slot) = next.objects.iter_mut().find(|object| object.id() == id) else {
            return Err("Reaction not found".into());
        };
        *slot = ChemObject::Reaction(parsed);
        self.apply(next)
    }

    /// Sets a reaction's arrow conditions text.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the change is rejected.
    pub fn set_reaction_arrow(
        &mut self,
        id: u64,
        kind: crate::model::ArrowKind,
        above: Vec<String>,
        below: Vec<String>,
    ) -> Result<(), String> {
        let mut next = self.document.clone();
        let Some(ChemObject::Reaction(reaction)) =
            next.objects.iter_mut().find(|object| object.id() == id)
        else {
            return Err("Reaction not found".into());
        };
        reaction.arrow.kind = kind;
        reaction.arrow.above = above;
        reaction.arrow.below = below;
        self.apply(next)
    }

    /// Adds a species to a reaction side.
    ///
    /// # Errors
    ///
    /// Returns a validation error when the change is rejected.
    pub fn add_species(&mut self, id: u64, product: bool) -> Result<(), String> {
        let mut next = self.document.clone();
        let Some(ChemObject::Reaction(reaction)) =
            next.objects.iter_mut().find(|object| object.id() == id)
        else {
            return Err("Reaction not found".into());
        };
        let species = Species {
            coefficient: 1,
            parts: vec![crate::model::Part::Element {
                symbol: "C".into(),
                count: 1,
                isotope: None,
            }],
            charge: 0,
            state: State::None,
            marker: crate::model::Marker::None,
        };
        if product {
            reaction.products.push(species);
        } else {
            reaction.reactants.push(species);
        }
        self.apply(next)
    }

    /// A starter document: ethanol and the water synthesis equation.
    #[must_use]
    pub fn sample() -> Self {
        let mut editor = Self::default();
        let mut document = ChemDocument {
            version: VERSION,
            objects: vec![],
        };
        document
            .objects
            .push(ChemObject::Structure(sample_ethanol()));
        if let Ok(mut reaction) = mhchem::parse("2H2 + O2 <=> 2H2O") {
            reaction.id = 100;
            reaction.origin = Point::new(40.0, 260.0);
            document.objects.push(ChemObject::Reaction(reaction));
        }
        if document.validate().is_ok() {
            editor.document = document;
            editor.active = editor.document.objects.first().map(ChemObject::id);
        }
        editor.undo.clear();
        editor.redo.clear();
        editor.revision = 0;
        editor
    }

    /// A localized starter document.
    #[must_use]
    pub fn sample_with_locale(locale: Locale) -> Self {
        let mut editor = Self::sample();
        editor.set_locale(locale);
        editor
    }
}

fn renumber_with(object: &mut ChemObject, counter: &mut u64) {
    match object {
        ChemObject::Structure(structure) => {
            let mut remapped = std::collections::BTreeMap::new();
            structure.id = *counter;
            *counter += 1;
            for atom in &mut structure.atoms {
                remapped.insert(atom.id, *counter);
                atom.id = *counter;
                *counter += 1;
            }
            for bond in &mut structure.bonds {
                bond.id = *counter;
                *counter += 1;
                bond.a = remapped.get(&bond.a).copied().unwrap_or(bond.a);
                bond.b = remapped.get(&bond.b).copied().unwrap_or(bond.b);
            }
        }
        ChemObject::Reaction(reaction) => {
            reaction.id = *counter;
            *counter += 1;
        }
    }
}

fn snap_to_grid(point: Point) -> Point {
    let step = crate::geometry::BOND_LENGTH / 2.0;
    Point::new(
        (point.x / step).round() * step,
        (point.y / step).round() * step,
    )
}

fn document_cost(document: &ChemDocument) -> usize {
    let mut cost = std::mem::size_of::<ChemDocument>();
    for object in &document.objects {
        cost += std::mem::size_of::<ChemObject>();
        match object {
            ChemObject::Structure(structure) => {
                cost += structure.atoms.len() * std::mem::size_of::<Atom>();
                cost += structure.bonds.len() * std::mem::size_of::<Bond>();
            }
            ChemObject::Reaction(reaction) => {
                cost += (reaction.reactants.len() + reaction.products.len()) * 64;
                cost += reaction.arrow.above.iter().map(String::len).sum::<usize>();
                cost += reaction.arrow.below.iter().map(String::len).sum::<usize>();
            }
        }
    }
    cost
}

fn sample_ethanol() -> Structure {
    let mut structure = Structure {
        id: 1,
        origin: Point::new(90.0, 120.0),
        atoms: vec![],
        bonds: vec![],
    };
    let chain: [(u8, f32, f32); 3] = [(6, 0.0, 0.0), (6, 44.0, 0.0), (8, 88.0, 0.0)];
    for (index, (element, x, y)) in chain.iter().enumerate() {
        let id = index as u64 + 2;
        structure.atoms.push(Atom {
            id,
            element: *element,
            position: Point::new(*x, *y),
            charge: 0,
            hydrogens: None,
        });
    }
    structure.bonds.push(Bond {
        id: 10,
        a: 2,
        b: 3,
        order: BondOrder::Single,
        stereo: BondStereo::Plain,
    });
    structure.bonds.push(Bond {
        id: 11,
        a: 3,
        b: 4,
        order: BondOrder::Single,
        stereo: BondStereo::Plain,
    });
    structure
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atom_edits_are_undoable() {
        let mut editor = ChemEditor::default();
        let first = editor.add_atom(6, Point::new(10.0, 10.0)).unwrap();
        let second = editor.add_atom(8, Point::new(54.0, 10.0)).unwrap();
        let revision = editor.revision();
        let bond = editor.connect(first, second).unwrap();
        assert_eq!(
            editor
                .structure(editor.active().unwrap())
                .unwrap()
                .bonds
                .len(),
            1
        );
        editor.undo();
        assert!(editor
            .structure(editor.active().unwrap())
            .unwrap()
            .bonds
            .is_empty());
        editor.redo();
        assert_eq!(
            editor.structure(editor.active().unwrap()).unwrap().bonds[0].id,
            bond
        );
        assert!(editor.revision() >= revision);
    }

    #[test]
    fn ring_templates_are_kekule() {
        let mut editor = ChemEditor::default();
        let id = editor.add_ring(Point::new(0.0, 0.0), 6).unwrap();
        let structure = editor.structure(id).unwrap();
        assert_eq!(structure.atoms.len(), 6);
        assert_eq!(structure.bonds.len(), 6);
        let doubles = structure
            .bonds
            .iter()
            .filter(|bond| bond.order == BondOrder::Double)
            .count();
        assert_eq!(doubles, 3);
        assert_eq!(structure.molecular_formula(), "C6H6");
    }

    #[test]
    fn sample_document_is_valid() {
        let editor = ChemEditor::sample();
        editor.document().validate().unwrap();
    }

    #[test]
    fn reaction_source_round_trips() {
        let mut editor = ChemEditor::default();
        let id = editor.add_reaction(Point::default()).unwrap();
        editor.set_reaction_source(id, "2H2 + O2 -> 2H2O").unwrap();
        let source = editor.reaction_source(id).unwrap();
        assert!(source.contains("->"), "{source}");
        assert!(editor.set_reaction_source(id, "broken").is_err());
    }
}
