use sonic_rs::{json, Value};

pub fn test() {
    let mut v = json!({});
    if let Some(obj) = v.as_object_mut() {
        obj.insert("key", json!("value"));
    }
}
