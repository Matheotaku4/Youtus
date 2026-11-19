use serde_json::Value;

/// Première valeur trouvée sous `key`, en profondeur. L'ordre est celui de
/// serde_json (clés triées) : à n'utiliser que pour des clés uniques.
pub fn find_key<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    match value {
        Value::Object(map) => {
            if let Some(found) = map.get(key) {
                return Some(found);
            }
            map.values().find_map(|child| find_key(child, key))
        }
        Value::Array(items) => items.iter().find_map(|child| find_key(child, key)),
        _ => None,
    }
}

/// Texte d'un nœud InnerTube : chaîne, `simpleText`, `content` ou `runs[]`.
pub fn text_of(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Object(map) => {
            if let Some(Value::String(text)) = map.get("simpleText") {
                return Some(text.clone());
            }
            if let Some(Value::String(text)) = map.get("content") {
                return Some(text.clone());
            }
            if let Some(Value::Array(runs)) = map.get("runs") {
                let joined: String = runs
                    .iter()
                    .filter_map(|run| run.get("text").and_then(Value::as_str))
                    .collect();
                if !joined.is_empty() {
                    return Some(joined);
                }
            }
            None
        }
        _ => None,
    }
}

pub fn find_string(value: &Value, key: &str) -> Option<String> {
    find_key(value, key).and_then(text_of)
}

/// Nombre d'occurrences de `key` dans tout le document.
pub fn count_key(value: &Value, key: &str) -> usize {
    match value {
        Value::Object(map) => {
            let here = usize::from(map.contains_key(key));
            here + map
                .values()
                .map(|child| count_key(child, key))
                .sum::<usize>()
        }
        Value::Array(items) => items.iter().map(|child| count_key(child, key)).sum(),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn trouve_une_cle_profonde() {
        let value = json!({"a": {"b": [{"target": {"videoId": "abc"}}]}});
        assert_eq!(find_key(&value, "target"), Some(&json!({"videoId": "abc"})));
    }

    #[test]
    fn lit_simple_text_et_runs() {
        let value = json!({
            "x": {"simpleText": "Salut"},
            "y": {"runs": [{"text": "Bon"}, {"text": "jour"}]}
        });
        assert_eq!(find_string(&value, "x").as_deref(), Some("Salut"));
        assert_eq!(find_string(&value, "y").as_deref(), Some("Bonjour"));
    }

    #[test]
    fn compte_les_occurrences() {
        let value = json!({"a": {"videoRenderer": {}}, "b": [{"videoRenderer": {}}, {"c": 1}]});
        assert_eq!(count_key(&value, "videoRenderer"), 2);
    }
}
