//! Bump allocation of vertex/index ranges inside the fixed-size GPU buffers Task 6 uploads
//! meshes into: no frees except a full [`SegmentAllocator::reset`], which the renderer does
//! whenever it (re)creates the shared buffers, e.g. to grow them.

/// A vertex/index range inside the shared GPU buffers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment {
    pub vertex_start: u32,
    pub index_start: u32,
    pub index_count: u32,
}

/// A bump allocator over two fixed-capacity buffers (vertices and indices), sized in elements
/// (not bytes).
pub struct SegmentAllocator {
    vertex_capacity: u32,
    index_capacity: u32,
    vertex_used: u32,
    index_used: u32,
}

impl SegmentAllocator {
    pub fn new(vertex_capacity: u32, index_capacity: u32) -> SegmentAllocator {
        SegmentAllocator {
            vertex_capacity,
            index_capacity,
            vertex_used: 0,
            index_used: 0,
        }
    }

    /// `None` when either buffer is full.
    pub fn allocate(&mut self, vertices: u32, indices: u32) -> Option<Segment> {
        let vertex_start = self.vertex_used;
        let index_start = self.index_used;
        let vertex_used = vertex_start.checked_add(vertices)?;
        let index_used = index_start.checked_add(indices)?;
        if vertex_used > self.vertex_capacity || index_used > self.index_capacity {
            return None;
        }
        self.vertex_used = vertex_used;
        self.index_used = index_used;
        Some(Segment {
            vertex_start,
            index_start,
            index_count: indices,
        })
    }

    /// Forgets every allocation and adopts new capacities.
    pub fn reset(&mut self, vertex_capacity: u32, index_capacity: u32) {
        self.vertex_capacity = vertex_capacity;
        self.index_capacity = index_capacity;
        self.vertex_used = 0;
        self.index_used = 0;
    }

    pub fn used(&self) -> (u32, u32) {
        (self.vertex_used, self.index_used)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocates_until_full_then_resets() {
        let mut allocator = SegmentAllocator::new(10, 20);
        assert_eq!(
            allocator.allocate(4, 6),
            Some(Segment {
                vertex_start: 0,
                index_start: 0,
                index_count: 6
            })
        );
        assert_eq!(
            allocator.allocate(6, 14),
            Some(Segment {
                vertex_start: 4,
                index_start: 6,
                index_count: 14
            })
        );
        assert_eq!(allocator.allocate(1, 0), None);
        allocator.reset(100, 100);
        assert_eq!(allocator.used(), (0, 0));
        assert!(allocator.allocate(1, 3).is_some());
    }
}
