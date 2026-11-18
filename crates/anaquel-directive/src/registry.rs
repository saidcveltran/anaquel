//! The closed set of directives. Content cannot add to it.

use crate::attrs::Attributes;

/// What a directive may hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Body {
    /// Markdown, and any directive that has no required parent.
    Markdown,
    /// Only blocks of the named directive.
    Only(&'static str),
    /// Nothing.
    Empty,
}

/// One accepted `key=value` attribute.
#[derive(Debug, Clone, Copy)]
pub struct Key {
    /// The key.
    pub name: &'static str,
    /// Whether the directive is invalid without it.
    pub required: bool,
    /// Checks a value; the message says what is accepted.
    pub check: fn(&str) -> Result<(), &'static str>,
}

/// The rules of one directive.
#[derive(Debug, Clone, Copy)]
pub struct Spec {
    /// The name after the colons.
    pub name: &'static str,
    /// Attributes with a value.
    pub keys: &'static [Key],
    /// Accepted bare words. At most one of `exclusive` flags may be given.
    pub flags: &'static [&'static str],
    /// Whether at most one flag may be given.
    pub one_flag: bool,
    /// What the block holds.
    pub body: Body,
    /// The directive that must contain this one, when there is one.
    pub parent: Option<&'static str>,
}

impl PartialEq for Spec {
    /// Directives are told apart by name; the table has one entry per name.
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

fn any_text(value: &str) -> Result<(), &'static str> {
    if value.trim().is_empty() {
        Err("must not be empty")
    } else {
        Ok(())
    }
}

fn title(value: &str) -> Result<(), &'static str> {
    if value.trim().is_empty() {
        Err("must not be empty")
    } else if value.len() > 200 {
        Err("must be at most 200 bytes")
    } else {
        Ok(())
    }
}

fn ident(value: &str) -> Result<(), &'static str> {
    let ok = !value.is_empty()
        && value.len() <= 32
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if ok {
        Ok(())
    } else {
        Err("must be 1 to 32 characters: a-z, 0-9 or `-`")
    }
}

fn cols(value: &str) -> Result<(), &'static str> {
    match value {
        "1" | "2" | "3" | "4" => Ok(()),
        _ => Err("must be 1, 2, 3 or 4"),
    }
}

fn width(value: &str) -> Result<(), &'static str> {
    match value {
        "1" | "2" | "3" | "4" | "5" | "6" => Ok(()),
        _ => Err("must be a whole number from 1 to 6"),
    }
}

const TITLE: Key = Key {
    name: "title",
    required: true,
    check: title,
};

/// Every directive of the engine.
pub const SPECS: &[Spec] = &[
    Spec {
        name: "hint",
        keys: &[],
        flags: &["info", "success", "warning", "danger"],
        one_flag: true,
        body: Body::Markdown,
        parent: None,
    },
    Spec {
        name: "details",
        keys: &[TITLE],
        flags: &["open"],
        one_flag: false,
        body: Body::Markdown,
        parent: None,
    },
    Spec {
        name: "tabs",
        keys: &[Key {
            name: "sync",
            required: false,
            check: ident,
        }],
        flags: &[],
        one_flag: false,
        body: Body::Only("tab"),
        parent: None,
    },
    Spec {
        name: "tab",
        keys: &[TITLE],
        flags: &[],
        one_flag: false,
        body: Body::Markdown,
        parent: Some("tabs"),
    },
    Spec {
        name: "cards",
        keys: &[Key {
            name: "cols",
            required: false,
            check: cols,
        }],
        flags: &[],
        one_flag: false,
        body: Body::Only("card"),
        parent: None,
    },
    Spec {
        name: "card",
        keys: &[
            TITLE,
            Key {
                name: "href",
                required: false,
                check: any_text,
            },
            Key {
                name: "image",
                required: false,
                check: any_text,
            },
        ],
        flags: &[],
        one_flag: false,
        body: Body::Markdown,
        parent: Some("cards"),
    },
    Spec {
        name: "steps",
        keys: &[],
        flags: &[],
        one_flag: false,
        body: Body::Only("step"),
        parent: None,
    },
    Spec {
        name: "step",
        keys: &[TITLE],
        flags: &[],
        one_flag: false,
        body: Body::Markdown,
        parent: Some("steps"),
    },
    Spec {
        name: "columns",
        keys: &[],
        flags: &[],
        one_flag: false,
        body: Body::Only("column"),
        parent: None,
    },
    Spec {
        name: "column",
        keys: &[Key {
            name: "width",
            required: false,
            check: width,
        }],
        flags: &[],
        one_flag: false,
        body: Body::Markdown,
        parent: Some("columns"),
    },
    Spec {
        name: "embed",
        keys: &[Key {
            name: "url",
            required: true,
            check: any_text,
        }],
        flags: &[],
        one_flag: false,
        body: Body::Empty,
        parent: None,
    },
];

/// The rules of the directive called `name`.
#[must_use]
pub fn spec(name: &str) -> Option<&'static Spec> {
    SPECS.iter().find(|s| s.name == name)
}

/// Checks the attributes of a directive against its rules; returns the first problem.
pub(crate) fn check_attributes(spec: &Spec, attrs: &Attributes) -> Result<(), String> {
    for (key, value) in &attrs.pairs {
        let Some(rule) = spec.keys.iter().find(|k| k.name == key) else {
            return Err(format!(
                "`{}` does not take the attribute `{key}`",
                spec.name
            ));
        };
        (rule.check)(value).map_err(|why| format!("`{key}` {why}"))?;
    }
    for rule in spec.keys.iter().filter(|k| k.required) {
        if attrs.get(rule.name).is_none() {
            return Err(format!("`{}` needs `{}=\"...\"`", spec.name, rule.name));
        }
    }
    for flag in &attrs.flags {
        if !spec.flags.contains(&flag.as_str()) {
            return Err(format!("`{}` does not take the word `{flag}`", spec.name));
        }
    }
    if spec.one_flag && attrs.flags.len() > 1 {
        return Err(format!("`{}` takes at most one of its words", spec.name));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_parent_and_child_names_a_directive() {
        for s in SPECS {
            if let Some(parent) = s.parent {
                assert!(spec(parent).is_some(), "{} names a missing parent", s.name);
            }
            if let Body::Only(child) = s.body {
                let child_spec = spec(child);
                assert!(child_spec.is_some(), "{} names a missing child", s.name);
                assert_eq!(child_spec.and_then(|c| c.parent), Some(s.name));
            }
        }
    }

    #[test]
    fn names_are_unique() {
        for (i, a) in SPECS.iter().enumerate() {
            assert!(SPECS.iter().skip(i + 1).all(|b| b.name != a.name));
        }
    }
}
