use serde::{Deserialize, Serialize};

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
}

impl Element {
    /// Return the value of a named field (`"text"`, `"class"`, `"href"`, `"value"`).
    #[must_use]
    pub fn get_field(&self, field: &str) -> Option<&str> {
        match field {
            "text" => self.text.as_deref(),
            "class" => self.class.as_deref(),
            "href" => self.href.as_deref(),
            "value" => self.value.as_deref(),
            _ => None,
        }
    }
}
