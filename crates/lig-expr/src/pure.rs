//! The pure-builtin whitelist (§6.5 Allow tier): `Math.*`, `String`
//! prototype methods, number formatting. Ships as pure data, unit-tested
//! independently — the dom-expressions prop-table discipline (§6.1).

use lig_ir::expr::PureFnKind;

/// One whitelisted callable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PureBuiltin {
    /// Lookup key, e.g. `Math.abs`.
    pub key: &'static str,
    /// Rust-side lowering hint for codegen.
    pub rust_path: &'static str,
}

/// The whitelist, in table order (order is canonical — pure data).
pub const PURE_BUILTINS: &[PureBuiltin] = &[
    PureBuiltin {
        key: "Math.abs",
        rust_path: "i64::abs",
    },
    PureBuiltin {
        key: "Math.min",
        rust_path: "lig_runtime::pure::min",
    },
    PureBuiltin {
        key: "Math.max",
        rust_path: "lig_runtime::pure::max",
    },
    PureBuiltin {
        key: "Math.floor",
        rust_path: "f64::floor",
    },
    PureBuiltin {
        key: "Math.ceil",
        rust_path: "f64::ceil",
    },
    PureBuiltin {
        key: "Math.round",
        rust_path: "f64::round",
    },
    PureBuiltin {
        key: "Math.sqrt",
        rust_path: "f64::sqrt",
    },
    PureBuiltin {
        key: "str.toUpperCase",
        rust_path: "str::to_uppercase",
    },
    PureBuiltin {
        key: "arr.join",
        rust_path: "lig_runtime::pure::join",
    },
    PureBuiltin {
        key: "str.toLowerCase",
        rust_path: "str::to_lowercase",
    },
    PureBuiltin {
        key: "str.trim",
        rust_path: "str::trim",
    },
    PureBuiltin {
        key: "str.slice",
        rust_path: "str::get",
    },
    PureBuiltin {
        key: "str.includes",
        rust_path: "str::contains",
    },
    PureBuiltin {
        key: "str.startsWith",
        rust_path: "str::starts_with",
    },
    PureBuiltin {
        key: "str.endsWith",
        rust_path: "str::ends_with",
    },
    PureBuiltin {
        key: "num.toFixed",
        rust_path: "f64_to_fixed",
    },
    PureBuiltin {
        key: "num.toString",
        rust_path: "i64_to_string",
    },
];

/// Look up a builtin by its JS-style key.
#[must_use]
pub fn lookup_builtin(key: &str) -> Option<&'static PureBuiltin> {
    PURE_BUILTINS.iter().find(|b| b.key == key)
}

/// Whether `receiver.method` is a whitelisted method-style builtin
/// (`str.*` / `num.*` pseudo-receivers).
#[must_use]
pub fn method_builtin(receiver: &str, method: &str) -> Option<&'static PureBuiltin> {
    match receiver {
        "String" => lookup_builtin(&format!("str.{method}")),
        "Number" => lookup_builtin(&format!("num.{method}")),
        _ => None,
    }
}

/// The kind each whitelist entry carries in the module's pure-fn table.
pub const BUILTIN_KIND: PureFnKind = PureFnKind::Builtin;

#[cfg(test)]
mod tests {
    use super::{PURE_BUILTINS, lookup_builtin, method_builtin};

    #[test]
    fn table_has_no_duplicate_keys() {
        let mut seen = std::collections::BTreeSet::new();
        for b in PURE_BUILTINS {
            assert!(seen.insert(b.key), "duplicate builtin key {}", b.key);
            assert!(b.key.contains('.'), "builtin keys are dotted: {}", b.key);
            assert_ne!(b.rust_path, "", "rust_path required");
        }
    }

    #[test]
    fn math_and_string_proto_resolve() {
        assert!(lookup_builtin("Math.abs").is_some());
        assert!(lookup_builtin("Math nonsense").is_none());
        assert!(method_builtin("String", "trim").is_some());
        assert!(method_builtin("Number", "toFixed").is_some());
        assert!(
            method_builtin("Array", "filter").is_none(),
            "callback-taking fns stay out"
        );
    }

    #[test]
    fn filter_reduce_sort_are_not_admitted() {
        // §6.5 Deny examples: calls with callbacks never enter the table.
        for banned in ["Array.filter", "Array.reduce", "Array.sort", "Array.map"] {
            assert!(
                lookup_builtin(banned).is_none(),
                "{banned} must not be whitelisted"
            );
        }
    }
}
