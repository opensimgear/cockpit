use super::*;
use serde_json::json;

fn controller() -> Vec<Value> {
    vec![
        json!({"cookie":1,"parentCookie":null,"type":513,"collectionType":1,"usagePage":1,"usage":4}),
        json!({"cookie":2,"parentCookie":1,"type":3,"usagePage":1,"usage":0x30}),
    ]
}

#[test]
fn complete_controller_native_tree_allows_descriptor_unavailable_fallback() {
    assert!(element_fallback(&controller()));
}

#[test]
fn ambiguous_or_sensitive_element_trees_fail_closed() {
    assert!(!element_fallback(&[]));
    let mut elements = controller();
    elements[0]["usagePage"] = json!(0xff00);
    assert!(!element_fallback(&elements));
    elements = controller();
    elements[1]["parentCookie"] = json!(99);
    assert!(!element_fallback(&elements));
    elements = controller();
    elements[1]["parentCookie"] = json!(2);
    assert!(!element_fallback(&elements));
    elements = controller();
    elements[1]["usagePage"] = json!(7);
    assert!(!element_fallback(&elements));
    elements = controller();
    elements.push(elements[1].clone());
    assert!(!element_fallback(&elements));
    elements = controller();
    elements[1]["usagePage"] = json!(0xff00);
    assert!(!element_fallback(&elements));
    elements = controller();
    elements[1]["usage"] = json!(0);
    elements[1]["usageMin"] = json!(0x80);
    elements[1]["usageMax"] = json!(0x8f);
    assert!(elements_restricted(&elements));
    assert!(!element_fallback(&elements));
}

#[test]
fn physical_pointer_axis_group_under_controller_is_not_a_mouse() {
    let mut elements = controller();
    elements.push(
        json!({"cookie":3,"parentCookie":1,"type":513,"collectionType":0,"usagePage":1,"usage":1}),
    );
    elements[1]["parentCookie"] = json!(3);
    assert!(!elements_restricted(&elements));
    assert!(element_fallback(&elements));
    elements[2]["collectionType"] = json!(1);
    assert!(elements_restricted(&elements));
    assert!(!element_fallback(&elements));
}
