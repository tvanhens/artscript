//! The author card names every field of `m`, every builtin, and every placement key the
//! language defines, so the card cannot drift from the contract.

use std::fs;
use std::path::Path;

use ramose_brush::PLACEMENT_KEYS;
use ramose_brush::lang::{Field, builtin_names};

#[test]
fn brushes_topic_names_every_field_builtin_and_placement_key() {
    let topic = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/topics/brushes.md")).unwrap();
    for field in Field::ALL {
        let name = field.name();
        assert!(topic.contains(&format!("`{name}`")) || topic.contains(&format!("`m.{name}`")), "brushes.md does not name the field `{name}`");
    }
    for name in builtin_names() {
        if name == "m" {
            continue;
        }
        assert!(topic.contains(&format!("`{name}")), "brushes.md does not name the builtin `{name}`");
    }
    for key in PLACEMENT_KEYS {
        assert!(topic.contains(&format!("`{key}:`")) || topic.contains(&format!("`{key}`")), "brushes.md does not name the placement key `{key}`");
    }
}
