//! Conversión serde_json::Value -> BoltType para pasar estructuras anidadas
//! como parámetros de Cypher (UNWIND de listas de mapas).

use neo4rs::{BoltBoolean, BoltFloat, BoltInteger, BoltList, BoltMap, BoltNull, BoltString, BoltType};

pub fn to_bolt(value: &serde_json::Value) -> BoltType {
    match value {
        serde_json::Value::Null => BoltType::Null(BoltNull),
        serde_json::Value::Bool(b) => BoltType::Boolean(BoltBoolean::new(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                BoltType::Integer(BoltInteger::new(i))
            } else {
                BoltType::Float(BoltFloat::new(n.as_f64().unwrap_or(f64::NAN)))
            }
        }
        serde_json::Value::String(s) => BoltType::String(BoltString::from(s.as_str())),
        serde_json::Value::Array(items) => {
            let list: Vec<BoltType> = items.iter().map(to_bolt).collect();
            BoltType::List(BoltList::from(list))
        }
        serde_json::Value::Object(map) => {
            let mut bolt_map = BoltMap::default();
            for (k, v) in map {
                bolt_map.put(BoltString::from(k.as_str()), to_bolt(v));
            }
            BoltType::Map(bolt_map)
        }
    }
}
