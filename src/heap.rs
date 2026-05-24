//! Persistent heap of objects and function definitions.

use std::collections::BTreeMap;

use crate::value::{Cell, CellId, FunctionDef, FunctionId, Object, ObjectId, Value};

/// A persistent heap.  Operations consume `self` and return the updated
/// heap, matching the state-threading style of the engine: callers always
/// hand the latest heap forward and never need the previous version.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Heap {
    objects: BTreeMap<ObjectId, Object>,
    functions: BTreeMap<FunctionId, FunctionDef>,
    cells: BTreeMap<CellId, Cell>,
    next_object: u64,
    next_function: u64,
    next_cell: u64,
}

impl Heap {
    /// An empty heap.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Allocate `object`, returning its id and the resulting heap.
    #[must_use]
    pub fn alloc_object(self, object: Object) -> (ObjectId, Self) {
        let id = ObjectId::new(self.next_object);
        let mut next_objects = self.objects;
        let _ = next_objects.insert(id, object);
        let next = Self {
            objects: next_objects,
            functions: self.functions,
            cells: self.cells,
            next_object: self.next_object + 1,
            next_function: self.next_function,
            next_cell: self.next_cell,
        };
        (id, next)
    }

    /// Allocate `function`, returning its id and the resulting heap.
    #[must_use]
    pub fn alloc_function(self, function: FunctionDef) -> (FunctionId, Self) {
        let id = FunctionId::new(self.next_function);
        let mut next_functions = self.functions;
        let _ = next_functions.insert(id, function);
        let next = Self {
            objects: self.objects,
            functions: next_functions,
            cells: self.cells,
            next_object: self.next_object,
            next_function: self.next_function + 1,
            next_cell: self.next_cell,
        };
        (id, next)
    }

    /// Allocate `cell` for a variable binding, returning its id and the
    /// resulting heap.
    #[must_use]
    pub fn alloc_cell(self, cell: Cell) -> (CellId, Self) {
        let id = CellId::new(self.next_cell);
        let mut next_cells = self.cells;
        let _ = next_cells.insert(id, cell);
        let next = Self {
            objects: self.objects,
            functions: self.functions,
            cells: next_cells,
            next_object: self.next_object,
            next_function: self.next_function,
            next_cell: self.next_cell + 1,
        };
        (id, next)
    }

    /// Replace the object at `id` with `object`.  Returns the original
    /// heap unchanged (wrapped in `Err`) if `id` is unknown.
    ///
    /// # Errors
    ///
    /// Returns the original heap as `Err(self)` when `id` is not present.
    pub fn store_object(self, id: ObjectId, object: Object) -> Result<Self, Self> {
        if self.objects.contains_key(&id) {
            let mut next_objects = self.objects;
            let _ = next_objects.insert(id, object);
            Ok(Self {
                objects: next_objects,
                functions: self.functions,
                cells: self.cells,
                next_object: self.next_object,
                next_function: self.next_function,
                next_cell: self.next_cell,
            })
        } else {
            Err(self)
        }
    }

    /// Write `value` into the cell at `id`, preserving mutability.
    /// Returns the original heap as `Err` if the cell is unknown or
    /// immutable.
    ///
    /// # Errors
    ///
    /// Returns the original heap as `Err(self)` when `id` is missing or
    /// the cell is immutable (`const`).
    pub fn store_cell(self, id: CellId, value: Value) -> Result<Self, Self> {
        let cloned = self.cells.get(&id).cloned();
        if let Some(existing) = cloned {
            if existing.is_mutable() {
                let mut next_cells = self.cells;
                let _ = next_cells.insert(id, existing.with_value(value));
                Ok(Self {
                    objects: self.objects,
                    functions: self.functions,
                    cells: next_cells,
                    next_object: self.next_object,
                    next_function: self.next_function,
                    next_cell: self.next_cell,
                })
            } else {
                Err(self)
            }
        } else {
            Err(self)
        }
    }

    /// Look up the object at `id`.
    #[must_use]
    pub fn object(&self, id: ObjectId) -> Option<&Object> {
        self.objects.get(&id)
    }

    /// Look up the function at `id`.
    #[must_use]
    pub fn function(&self, id: FunctionId) -> Option<&FunctionDef> {
        self.functions.get(&id)
    }

    /// Look up the cell at `id`.
    #[must_use]
    pub fn cell(&self, id: CellId) -> Option<&Cell> {
        self.cells.get(&id)
    }

    /// Number of objects in the heap.
    #[must_use]
    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    /// Number of function definitions in the heap.
    #[must_use]
    pub fn function_count(&self) -> usize {
        self.functions.len()
    }

    /// Number of variable cells in the heap.
    #[must_use]
    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }
}
