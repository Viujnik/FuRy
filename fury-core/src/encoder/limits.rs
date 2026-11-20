/// Resource limits applied while validating and decoding an incoming wire-v2 frame.
///
/// Limit a transport's body size before allocating the input `Vec` as well. These
/// checks bound parsing and materialization after the bytes have arrived. Unknown
/// fields and opaque Object payloads count towards frame size, but their contents
/// cannot be recursively validated without a schema resolver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecodeLimits {
    pub max_frame_bytes: usize,
    pub max_fields: usize,
    pub max_collection_items: usize,
    /// Root values have depth zero. There is also a hard safety ceiling of 16.
    pub max_depth: usize,
    /// Shared budget for all known values, including collection containers,
    /// elements, map keys and map values, in a single frame.
    pub max_total_values: usize,
}

impl Default for DecodeLimits {
    /// Use a 16 MiB frame, 4,096 fields, 100,000 items and total values, and depth 16.
    fn default() -> Self {
        Self {
            max_frame_bytes: 16 * 1024 * 1024,
            max_fields: 4096,
            max_collection_items: 100_000,
            max_depth: 16,
            max_total_values: 100_000,
        }
    }
}

pub(crate) const HARD_MAX_DEPTH: usize = 16;

pub(crate) struct DecodeBudget<'a> {
    pub(crate) limits: &'a DecodeLimits,
    pub(crate) remaining: usize,
}

impl<'a> DecodeBudget<'a> {
    /// Borrow this decode configuration and initialize its aggregate value budget.
    pub(crate) fn new(limits: &'a DecodeLimits) -> Self {
        Self {
            limits,
            remaining: limits.max_total_values,
        }
    }

    /// Charge one decoded value and reject excess depth or total values.
    pub(crate) fn consume(&mut self, depth: usize) -> Option<()> {
        if depth > self.limits.max_depth.min(HARD_MAX_DEPTH) {
            return None;
        }
        self.remaining = self.remaining.checked_sub(1)?;
        Some(())
    }

    /// Check an item count before descending or allocating a collection.
    pub(crate) fn allows_children(&self, count: usize) -> bool {
        count <= self.remaining
    }
}

/// Bounds owned values while a binding converts host-language objects, before
/// large Rust strings, byte vectors or child collections are allocated.
/// This supplements (and never replaces) BinaryRecord's final frame validation.
pub struct ValueBudget {
    limits: DecodeLimits,
    remaining_values: usize,
    remaining_bytes: usize,
}

impl Default for ValueBudget {
    /// Use the same limit values as the default codec configuration.
    fn default() -> Self {
        Self::new(DecodeLimits::default())
    }
}

impl ValueBudget {
    /// Initialize the host-conversion budget for the supplied codec limits.
    pub fn new(limits: DecodeLimits) -> Self {
        Self {
            remaining_values: limits.max_total_values,
            remaining_bytes: limits.max_frame_bytes,
            limits,
        }
    }
    /// Charge one host value and its type-dependent fixed or length-prefix bytes.
    pub fn enter(
        &mut self,
        ty: &crate::schema::field::FieldType,
        is_null: bool,
        depth: usize,
    ) -> crate::error::Result<()> {
        if depth > self.limits.max_depth.min(HARD_MAX_DEPTH) || self.remaining_values == 0 {
            return Err(crate::error::FuryError::Encoding(
                "conversion value/depth budget exceeded".into(),
            ));
        }
        self.remaining_values -= 1;
        self.claim_bytes(if is_null {
            1
        } else {
            1 + ty.fixed_size().unwrap_or(4)
        })
    }
    /// Charge variable-length content using checked arithmetic before copying it.
    pub fn claim_bytes(&mut self, bytes: usize) -> crate::error::Result<()> {
        self.remaining_bytes = self.remaining_bytes.checked_sub(bytes).ok_or_else(|| {
            crate::error::FuryError::Encoding("conversion byte budget exceeded".into())
        })?;
        Ok(())
    }
    /// Check the number of children against collection and aggregate limits.
    pub fn check_children(&self, count: usize, values_per_item: usize) -> crate::error::Result<()> {
        if count > self.limits.max_collection_items
            || count
                .checked_mul(values_per_item)
                .is_none_or(|n| n > self.remaining_values)
        {
            return Err(crate::error::FuryError::Encoding(
                "conversion collection/value budget exceeded".into(),
            ));
        }
        Ok(())
    }
}
