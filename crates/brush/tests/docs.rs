//! The author card names every `Mark` field, helper and placement key the prelude defines, so the
//! card cannot drift from the contract.

use std::fs;
use std::path::Path;

use ramose_brush::{HELPERS, MARK_FIELDS, PLACEMENT_KEYS};

#[test]
fn brushes_topic_names_every_mark_field_helper_and_placement_key() {
    let topic = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/topics/brushes.md")).unwrap();
    for field in MARK_FIELDS {
        assert!(topic.contains(&format!("`{field}`")), "brushes.md does not name the Mark field `{field}`");
    }
    for helper in HELPERS {
        assert!(topic.contains(&format!("`{helper}(")), "brushes.md does not name the helper `{helper}`");
    }
    for key in PLACEMENT_KEYS {
        assert!(topic.contains(&format!("`{key}:`")) || topic.contains(&format!("`{key}`")), "brushes.md does not name the placement key `{key}`");
    }
}
