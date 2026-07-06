use std::fmt;

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

    #[must_use]
    pub fn is_collection(&self) -> bool {
        matches!(self, Self::List(_) | Self::Map(_, _) | Self::Object(_))
    }

    #[must_use]
    pub fn is_variable_length(&self) -> bool {
        matches!(
            self,
            Self::String | Self::Bytes | Self::List(_) | Self::Map(_, _) | Self::Object(_)
        )
    }

    #[must_use]
    pub fn as_str(&self) -> String {
        match self {
            Self::Int8 => "Int8".to_string(),
            Self::Int16 => "Int16".to_string(),
            Self::Int32 => "Int32".to_string(),
            Self::Int64 => "Int64".to_string(),
            Self::UInt8 => "UInt8".to_string(),
            Self::UInt16 => "UInt16".to_string(),
            Self::UInt32 => "UInt32".to_string(),
            Self::UInt64 => "UInt64".to_string(),
            Self::Float32 => "Float32".to_string(),
            Self::Float64 => "Float64".to_string(),
            Self::Bool => "Bool".to_string(),
            Self::String => "String".to_string(),
            Self::Bytes => "Bytes".to_string(),
            Self::DateTime => "DateTime".to_string(),
            Self::Date => "Date".to_string(),
            Self::Uuid => "Uuid".to_string(),
            Self::List(inner) => format!("List<{}>", inner.as_str()),
            Self::Map(key, value) => format!("Map<{}, {}>", key.as_str(), value.as_str()),
            Self::Object(name) => format!("Object({})", name),
        }
    }

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
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
