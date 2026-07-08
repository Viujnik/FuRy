use std::fmt;

/// Supported database field data types inside the FuRy serialization engine.
///
/// Handles primitive scalars, fixed-size temporal types, and recursive complex
/// structures like lists, maps, and nested database objects.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FieldType {
    Int8,
    Int16,
    Int32,
    Int64,
    UInt8,
    UInt16,
    UInt32,
    UInt64,
    Float32,
    Float64,
    Bool,
    String,
    Bytes,
    DateTime,
    Date,
    Uuid,
    List(Box<FieldType>),
    Map(Box<FieldType>, Box<FieldType>),
    Object(String),
}

impl FieldType {
    /// Returns the data footprint size in bytes for fixed-length data types.
    ///
    /// Returns `Some(usize)` for types with known size at compile time,
    /// or `None` for variable-length types.
    ///
    /// # Examples
    ///
    /// ```
    /// use fury_core::schema::field::FieldType;
    ///
    /// assert_eq!(FieldType::Int32.fixed_size(), Some(4));
    /// assert_eq!(FieldType::String.fixed_size(), None);
    /// ```
    #[must_use]
    pub fn fixed_size(&self) -> Option<usize> {
        match self {
            Self::Int8 | Self::UInt8 | Self::Bool => Some(1),
            Self::Int16 | Self::UInt16 => Some(2),
            Self::Int32 | Self::UInt32 | Self::Float32 => Some(4),
            Self::Int64 | Self::UInt64 | Self::Float64 | Self::DateTime => Some(8),
            Self::Date => Some(4),
            Self::Uuid => Some(16),
            Self::String | Self::Bytes | Self::List(_) | Self::Map(_, _) | Self::Object(_) => None,
        }
    }

    /// Returns `true` if this type is a collection (List, Map, or Object).
    ///
    /// Note: All collections are also variable-length types.
    #[must_use]
    pub fn is_collection(&self) -> bool {
        matches!(self, Self::List(_) | Self::Map(_, _) | Self::Object(_))
    }

    /// Returns `true` if this type has variable length at runtime.
    ///
    /// This includes all collections plus String and Bytes.
    #[must_use]
    pub fn is_variable_length(&self) -> bool {
        self.is_collection() || matches!(self, Self::String | Self::Bytes)
    }

    /// Returns the type name as a static string slice.
    ///
    /// This is a zero-allocation operation suitable for logging and debugging.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Int8 => "Int8",
            Self::Int16 => "Int16",
            Self::Int32 => "Int32",
            Self::Int64 => "Int64",
            Self::UInt8 => "UInt8",
            Self::UInt16 => "UInt16",
            Self::UInt32 => "UInt32",
            Self::UInt64 => "UInt64",
            Self::Float32 => "Float32",
            Self::Float64 => "Float64",
            Self::String => "String",
            Self::Bytes => "Bytes",
            Self::Bool => "Bool",
            Self::Date => "Date",
            Self::DateTime => "DateTime",
            Self::Uuid => "Uuid",
            Self::List(_) => "List",
            Self::Map(_, _) => "Map",
            Self::Object(_) => "Object",
        }
    }

    /// Computes the maximum nested depth level of recursive complex collections.
    ///
    /// * Primitive scalars return `0`.
    /// * Simple lists or flat objects return `1`.
    /// * Highly nested deep matrix layouts increment proportionally based on allocation branches.
    #[must_use]
    pub fn field_depth(&self) -> usize {
        match self {
            Self::List(inner) => 1 + inner.field_depth(),
            Self::Map(key, value) => 1 + key.field_depth().max(value.field_depth()),
            Self::Object(_) => 1,
            _ => 0,
        }
    }
}

impl fmt::Display for FieldType {
    /// Formats the data type into an expressive human-readable string representation.
    ///
    /// Primitives are printed using lightning-fast `as_str()` compilation lookups,
    /// while recursive structures deep-dive to dynamically unwrap their internal types
    /// without a single memory allocation.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::List(inner) => write!(f, "List<{inner}>"),
            Self::Map(key, val) => write!(f, "Map<{key}, {val}>"),

            _ => f.write_str(self.as_str()),
        }
    }
}
