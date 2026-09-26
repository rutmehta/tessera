//! Catalog people calls, separate from per-recipe edits and their history.

use serde::{Deserialize, Serialize};

use crate::id::PersonId;
use crate::people::{FaceRef, PeopleWriteOptions};

/// Library-scoped identity mutations. Hosts must validate all references and
/// serialize catalog edits. No call silently grants permission for file I/O.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "tool", rename_all = "snake_case")]
pub enum LibraryToolCall {
    /// Assign faces to an existing stable identity, even if quality-ineligible.
    AssignPerson {
        /// Non-empty set of current face references.
        faces: Vec<FaceRef>,
        /// Destination identity.
        person_id: PersonId,
        /// Protect the new assignments from automatic refitting.
        #[serde(default)]
        confirmed: bool,
        /// Explicit metadata-write opt-ins.
        #[serde(default)]
        writes: PeopleWriteOptions,
    },
    /// Confirm/unconfirm current assignments; fail if any belongs to another
    /// identity rather than confirming a stale face-strip view.
    ConfirmPerson {
        /// Faces whose assignment is being confirmed.
        faces: Vec<FaceRef>,
        /// Expected current identity.
        person_id: PersonId,
        /// True confirms; false unconfirms. Required to avoid implicit edits.
        confirmed: bool,
        /// Explicit metadata-write opt-ins.
        #[serde(default)]
        writes: PeopleWriteOptions,
    },
    /// Move all source members to target, retaining target ID/name and
    /// confirmation flags. Hosts reject identical or missing identities.
    MergePeople {
        /// Identity to merge away.
        source: PersonId,
        /// Surviving identity.
        target: PersonId,
        /// Explicit metadata-write opt-ins.
        #[serde(default)]
        writes: PeopleWriteOptions,
    },
    /// Move a non-empty subset to a fresh unnamed identity; reset moved
    /// confirmations. Hosts reject faces not assigned to the source.
    SplitPerson {
        /// Source identity.
        person_id: PersonId,
        /// Members to split out.
        faces: Vec<FaceRef>,
        /// Explicit metadata-write opt-ins.
        #[serde(default)]
        writes: PeopleWriteOptions,
    },
    /// Set or explicitly clear a cluster-wide name. Does not implicitly
    /// confirm its faces. Keywords are additive, including on rename.
    NamePerson {
        /// Identity to name.
        person_id: PersonId,
        /// New name, or explicit null to clear. Omission is invalid.
        #[serde(deserialize_with = "Option::<String>::deserialize")]
        name: Option<String>,
        /// Explicit metadata-write opt-ins.
        #[serde(default)]
        writes: PeopleWriteOptions,
    },
}

impl LibraryToolCall {
    /// Stable names in declaration order.
    pub const NAMES: [&'static str; 5] = [
        "assign_person",
        "confirm_person",
        "merge_people",
        "split_person",
        "name_person",
    ];

    /// MCP command name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::AssignPerson { .. } => "assign_person",
            Self::ConfirmPerson { .. } => "confirm_person",
            Self::MergePeople { .. } => "merge_people",
            Self::SplitPerson { .. } => "split_person",
            Self::NamePerson { .. } => "name_person",
        }
    }

    /// These calls all mutate the catalog, not a recipe/document history.
    pub fn is_read_only(&self) -> bool {
        false
    }
}

/// Library mutation envelope. Rationale is available for host audit logs;
/// these catalog operations are not recipe or document history entries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LibraryToolRequest {
    /// Call.
    #[serde(flatten)]
    pub call: LibraryToolCall,
    /// Optional human-readable reason for the mutation.
    #[serde(default)]
    pub rationale: Option<String>,
}
