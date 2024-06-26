//! This module provides methods to retrieve allocation information, such as static variables.
use crate::{cycle_check, SerializeCycleCheck};
use crate::mir::mono::{Instance, StaticDef};
use crate::target::{Endian, MachineInfo};
use crate::ty::{Allocation, Binder, ExistentialTraitRef, IndexedVal, Ty};
use crate::{with, Error};
use serde::{Serialize, Serializer};
use std::io::Read;

derive_serialize! {
/// An allocation in the SMIR global memory can be either a function pointer,
/// a static, or a "real" allocation with some data in it.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum GlobalAlloc {
    /// The alloc ID is used as a function pointer.
    Function(Instance),
    // TODO: might need to further processing on this
    /// This alloc ID points to a symbolic (not-reified) vtable.
    /// The `None` trait ref is used to represent auto traits.
    VTable(Ty, Option<Binder<ExistentialTraitRef>>),
    /// The alloc ID points to a "lazy" static variable that did not get computed (yet).
    /// This is also used to break the cycle in recursive statics.
    Static(StaticDef),
    /// The alloc ID points to memory.
    Memory(Allocation),
}
}

impl From<AllocId> for GlobalAlloc {
    fn from(value: AllocId) -> Self {
        with(|cx| cx.global_alloc(value))
    }
}

impl GlobalAlloc {
    /// Retrieve the allocation id for a global allocation if it exists.
    ///
    /// For `[GlobalAlloc::VTable]`, this will return the allocation for the VTable of the given
    /// type for the optional trait if the type implements the trait.
    ///
    /// This method will always return `None` for allocations other than `[GlobalAlloc::VTable]`.
    pub fn vtable_allocation(&self) -> Option<AllocId> {
        with(|cx| cx.vtable_allocation(self))
    }
}

/// A unique identification number for each provenance
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct AllocId(usize);

fn get_index_and_populate_allocs(scc: &mut SerializeCycleCheck, alloc_id: &AllocId) -> usize {
    if !scc.seen_allocs.contains(alloc_id) {
        scc.seen_allocs.insert(*alloc_id);
        scc.allocs_ordered.push(*alloc_id);
        match GlobalAlloc::from(*alloc_id) {
            GlobalAlloc::Memory(allocation) => {
                allocation.provenance
                    .ptrs
                    .into_iter()
                    .for_each(|(_, prov)| { get_index_and_populate_allocs(scc, &prov.0); })
            },
            _ => {},
        }
        scc.seen_allocs.len() - 1
    } else {
        (&scc.allocs_ordered)
            .into_iter()
            .position(|alloc| alloc_id == alloc)
            .unwrap()
    }

}

impl Serialize for AllocId {
    #[instrument(level = "debug", skip(serializer))]
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        cycle_check(|scc| {
            let index = get_index_and_populate_allocs(scc, self);
            serializer.serialize_newtype_struct("AllocId", &(self.0, index))
        })
    }
}

impl IndexedVal for AllocId {
    fn to_val(index: usize) -> Self {
        AllocId(index)
    }
    fn to_index(&self) -> usize {
        self.0
    }
}

/// Utility function used to read an allocation data into a unassigned integer.
pub(crate) fn read_target_uint(mut bytes: &[u8]) -> Result<u128, Error> {
    let mut buf = [0u8; std::mem::size_of::<u128>()];
    match MachineInfo::target_endianness() {
        Endian::Little => {
            bytes.read_exact(&mut buf[..bytes.len()])?;
            Ok(u128::from_le_bytes(buf))
        }
        Endian::Big => {
            bytes.read_exact(&mut buf[16 - bytes.len()..])?;
            Ok(u128::from_be_bytes(buf))
        }
    }
}

/// Utility function used to read an allocation data into an assigned integer.
pub(crate) fn read_target_int(mut bytes: &[u8]) -> Result<i128, Error> {
    let mut buf = [0u8; std::mem::size_of::<i128>()];
    match MachineInfo::target_endianness() {
        Endian::Little => {
            bytes.read_exact(&mut buf[..bytes.len()])?;
            Ok(i128::from_le_bytes(buf))
        }
        Endian::Big => {
            bytes.read_exact(&mut buf[16 - bytes.len()..])?;
            Ok(i128::from_be_bytes(buf))
        }
    }
}
