//! Lightroom selection rule translation into native saved-search terms.
use crate::SavedSearch;
use serde_json::Value;

fn rule(field: &str, op: &str, value: Value) -> SavedSearch {
    SavedSearch::Rule {
        criteria: field.into(),
        operation: op.into(),
        value,
    }
}

fn decision(name: &str) -> SavedSearch {
    rule("decision", ":", Value::String(name.into()))
}

fn bucket(stars: i64) -> SavedSearch {
    match stars {
        0 => decision("undecided"),
        1 => SavedSearch::All(vec![
            decision("keep"),
            SavedSearch::None(vec![rule("grade", ">=", 1.into())]),
        ]),
        2 => rule("grade", "=", 1.into()),
        3 | 4 => rule("grade", "=", 2.into()),
        5 => rule("grade", "=", 3.into()),
        _ => unreachable!(),
    }
}

fn translated(criteria: &str, operation: &str, value: &Value) -> Option<SavedSearch> {
    if criteria == "rating" {
        let (low, high) = if let Some(n) = value.as_i64() {
            (n, n)
        } else if value == "unrated" {
            (0, 0)
        } else if let Some((a, b)) = value.as_str()?.split_once("..") {
            (a.parse::<i64>().ok()?, b.parse::<i64>().ok()?)
        } else {
            return None;
        };
        if !(0..=5).contains(&low) || !(0..=5).contains(&high) || low > high {
            return None;
        }
        let range = low != high;
        if !matches!(
            operation,
            "=" | "==" | ":" | "!=" | ">=" | ">" | "<=" | "<" | "between"
        ) || (range && !matches!(operation, "=" | "==" | ":" | "!=" | "between"))
        {
            return None;
        }
        let buckets = (0..=5)
            .filter(|&stars| match operation {
                "=" | "==" | ":" | "between" => (low..=high).contains(&stars),
                "!=" => !(low..=high).contains(&stars),
                ">=" => stars >= low,
                ">" => stars > low,
                "<=" => stars <= low,
                "<" => stars < low,
                _ => unreachable!(),
            })
            .map(bucket)
            .collect();
        return Some(SavedSearch::Any(buckets));
    }
    if matches!(criteria, "pick" | "reject") {
        let flag = value.as_i64().or_else(|| value.as_bool().map(i64::from))?;
        let name = match (criteria, flag) {
            ("pick", 1) => "keep",
            ("pick", -1) | ("reject", 1) => "reject",
            // A neutral flag is not equivalent to Undecided: rated images
            // with no pick flag become Keep. Leave it unsupported.
            _ => return None,
        };
        return match operation {
            "=" | "==" | ":" => Some(decision(name)),
            "!=" => Some(SavedSearch::None(vec![decision(name)])),
            _ => None,
        };
    }
    None
}

pub(crate) fn translate(search: SavedSearch) -> SavedSearch {
    match search {
        SavedSearch::Rule {
            criteria,
            operation,
            value,
        } => translated(&criteria, &operation, &value).unwrap_or(SavedSearch::Rule {
            criteria,
            operation,
            value,
        }),
        SavedSearch::All(children) => {
            SavedSearch::All(children.into_iter().map(translate).collect())
        }
        SavedSearch::Any(children) => {
            SavedSearch::Any(children.into_iter().map(translate).collect())
        }
        SavedSearch::None(children) => {
            SavedSearch::None(children.into_iter().map(translate).collect())
        }
    }
}
