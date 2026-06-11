use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A DOM element extracted from a scraped page.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Element {
    /// Visible text content.
    pub text: Option<String>,
    /// Full `class` attribute value.
    pub class: Option<String>,
    /// `href` attribute, if present.
    pub href: Option<String>,
    /// `value` attribute, if present.
    pub value: Option<String>,
    /// Extra attributes requested via `query_all`'s `attrs` list, keyed by
    /// attribute name (e.g. `data-price`, `aria-label`). Empty unless requested.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub attrs: HashMap<String, String>,
}

impl Element {
    /// Return the value of a named field. The built-in fields are `"text"`,
    /// `"class"`, `"href"`, and `"value"`; any other name is looked up among the
    /// extra `attrs` captured by `query_all`.
    #[must_use]
    pub fn get_field(&self, field: &str) -> Option<&str> {
        match field {
            "text" => self.text.as_deref(),
            "class" => self.class.as_deref(),
            "href" => self.href.as_deref(),
            "value" => self.value.as_deref(),
            other => self.attrs.get(other).map(String::as_str),
        }
    }

    /// Set a named field. Built-in names (`text`/`class`/`href`/`value`) write
    /// the corresponding struct field; any other name is stored among `attrs`,
    /// so it round-trips through [`get_field`](Self::get_field).
    pub fn set_field(&mut self, field: &str, value: String) {
        match field {
            "text" => self.text = Some(value),
            "class" => self.class = Some(value),
            "href" => self.href = Some(value),
            "value" => self.value = Some(value),
            other => {
                self.attrs.insert(other.to_owned(), value);
            }
        }
    }
}
