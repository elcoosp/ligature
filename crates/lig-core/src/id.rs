//! `u32`-backed newtype ids — "All ids are `u32` newtypes" (§5).
//!
//! Ids serialize as plain JSON numbers (serde newtype transparency), which is
//! what keeps canonical IR fixtures readable.

use serde::{Deserialize, Serialize};

/// Declare a `u32` newtype id with the standard accessor surface.
macro_rules! u32_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(
            Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
        )]
        pub struct $name(pub u32);

        impl $name {
            /// The raw index.
            #[must_use]
            pub const fn get(self) -> u32 {
                self.0
            }

            /// Build from a raw index.
            #[must_use]
            pub const fn new(v: u32) -> Self {
                Self(v)
            }

            /// The next sequential index.
            #[must_use]
            pub const fn next(self) -> Self {
                Self(self.0 + 1)
            }
        }

        impl core::fmt::Display for $name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                write!(f, "{}({})", stringify!($name), self.0)
            }
        }
    };
}

u32_id! {
    /// Interned string index (§5.1 `strings`).
    StrId
}
u32_id! {
    /// Expression index (§5.5).
    ExprId
}
u32_id! {
    /// Deduped template index (§5.2).
    TemplateId
}
u32_id! {
    /// Fragment index (§5.3).
    FragmentId
}
u32_id! {
    /// Reaction index (§7.1).
    ReactionId
}
u32_id! {
    /// Interned static style index (§8).
    StyleConstId
}
u32_id! {
    /// Property-schema index (§8.1).
    StylePropId
}
u32_id! {
    /// Pure-builtin whitelist index (§5.5 `Call`).
    PureFnId
}
u32_id! {
    /// Component declaration index.
    ComponentId
}
u32_id! {
    /// Source-file index (§5.1 `source_files`).
    FileId
}
u32_id! {
    /// Span-table index (§5.1 `spans`).
    SpanId
}

#[cfg(test)]
mod tests {
    use super::{ExprId, StrId};
    use serde::{Deserialize, Serialize};

    #[test]
    fn serializes_transparently_as_number() {
        #[derive(Serialize, Deserialize)]
        struct Wrapper {
            id: StrId,
        }
        let json = serde_json::to_string(&Wrapper { id: StrId::new(7) }).unwrap();
        assert_eq!(json, r#"{"id":7}"#);
        let w: Wrapper = serde_json::from_str(&json).unwrap();
        assert_eq!(w.id, StrId::new(7));
    }

    #[test]
    fn ordering_and_next() {
        assert!(ExprId::new(1) < ExprId::new(2));
        assert_eq!(ExprId::new(9).next(), ExprId::new(10));
        assert_eq!(ExprId::default(), ExprId::new(0));
    }
}
