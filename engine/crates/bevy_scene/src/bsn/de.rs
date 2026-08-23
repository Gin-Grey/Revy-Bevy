use super::syntax::{BsnStructField, BsnValue};
use core::{fmt, slice};
use serde::de::{
    self, value::StrDeserializer, DeserializeSeed, EnumAccess, MapAccess, SeqAccess, VariantAccess,
    Visitor,
};

#[derive(Debug, Clone)]
pub struct BsnDeserializeError {
    message: String,
}

impl BsnDeserializeError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for BsnDeserializeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl core::error::Error for BsnDeserializeError {}

impl de::Error for BsnDeserializeError {
    fn custom<T: fmt::Display>(message: T) -> Self {
        Self::new(message.to_string())
    }
}

fn unexpected(value: &BsnValue, expected: &str) -> BsnDeserializeError {
    BsnDeserializeError::new(format!("expected {expected}, found {value:?}"))
}

fn number_source(source: &str) -> String {
    let mut normalized = source.replace('_', "");
    for suffix in [
        "usize", "isize", "f64", "f32", "u128", "i128", "u64", "i64", "u32", "i32", "u16", "i16",
        "u8", "i8",
    ] {
        if normalized.len() > suffix.len() && normalized.ends_with(suffix) {
            normalized.truncate(normalized.len() - suffix.len());
            break;
        }
    }
    normalized
}

macro_rules! deserialize_number {
    ($method:ident, $visit:ident, $ty:ty) => {
        fn $method<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            let BsnValue::Number(source) = self else {
                return Err(unexpected(self, stringify!($ty)));
            };
            let value = number_source(source).parse::<$ty>().map_err(|error| {
                BsnDeserializeError::new(format!(
                    "invalid {} literal `{source}`: {error}",
                    stringify!($ty)
                ))
            })?;
            visitor.$visit(value)
        }
    };
}

impl<'de> de::Deserializer<'de> for &'de BsnValue {
    type Error = BsnDeserializeError;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            BsnValue::Unit => visitor.visit_unit(),
            BsnValue::Bool(value) => visitor.visit_bool(*value),
            BsnValue::Number(source) => {
                let normalized = number_source(source);
                if normalized.contains(['.', 'e', 'E']) {
                    visitor.visit_f64(normalized.parse().map_err(de::Error::custom)?)
                } else if normalized.starts_with('-') {
                    visitor.visit_i64(normalized.parse().map_err(de::Error::custom)?)
                } else {
                    visitor.visit_u64(normalized.parse().map_err(de::Error::custom)?)
                }
            }
            BsnValue::String(value) | BsnValue::Path(value) => visitor.visit_borrowed_str(value),
            BsnValue::Char(value) => visitor.visit_char(*value),
            BsnValue::Tuple(values)
            | BsnValue::List(values)
            | BsnValue::Constructor { fields: values, .. } => {
                visitor.visit_seq(ValueSeqAccess::new(values))
            }
            BsnValue::Map(entries) => visitor.visit_map(ValueMapAccess::new(entries)),
            BsnValue::Struct { fields, .. } => visitor.visit_map(FieldMapAccess::new(fields)),
        }
    }

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            BsnValue::Bool(value) => visitor.visit_bool(*value),
            _ => Err(unexpected(self, "a boolean")),
        }
    }

    deserialize_number!(deserialize_i8, visit_i8, i8);
    deserialize_number!(deserialize_i16, visit_i16, i16);
    deserialize_number!(deserialize_i32, visit_i32, i32);
    deserialize_number!(deserialize_i64, visit_i64, i64);
    deserialize_number!(deserialize_i128, visit_i128, i128);
    deserialize_number!(deserialize_u8, visit_u8, u8);
    deserialize_number!(deserialize_u16, visit_u16, u16);
    deserialize_number!(deserialize_u32, visit_u32, u32);
    deserialize_number!(deserialize_u64, visit_u64, u64);
    deserialize_number!(deserialize_u128, visit_u128, u128);
    deserialize_number!(deserialize_f32, visit_f32, f32);
    deserialize_number!(deserialize_f64, visit_f64, f64);

    fn deserialize_char<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            BsnValue::Char(value) => visitor.visit_char(*value),
            _ => Err(unexpected(self, "a character")),
        }
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            BsnValue::String(value) | BsnValue::Path(value) => visitor.visit_borrowed_str(value),
            _ => Err(unexpected(self, "a string")),
        }
    }

    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            BsnValue::String(value) | BsnValue::Path(value) => visitor.visit_string(value.clone()),
            _ => Err(unexpected(self, "a string")),
        }
    }

    fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            BsnValue::String(value) => visitor.visit_borrowed_bytes(value.as_bytes()),
            BsnValue::List(values) => visitor.visit_seq(ValueSeqAccess::new(values)),
            _ => Err(unexpected(self, "bytes")),
        }
    }

    fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            BsnValue::String(value) => visitor.visit_byte_buf(value.as_bytes().to_vec()),
            BsnValue::List(values) => visitor.visit_seq(ValueSeqAccess::new(values)),
            _ => Err(unexpected(self, "bytes")),
        }
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            BsnValue::Path(path) if final_path_segment(path) == "None" => visitor.visit_none(),
            BsnValue::Constructor { type_path, fields }
                if final_path_segment(type_path) == "Some" && fields.len() == 1 =>
            {
                visitor.visit_some(&fields[0])
            }
            _ => visitor.visit_some(self),
        }
    }

    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            BsnValue::Unit => visitor.visit_unit(),
            _ => Err(unexpected(self, "unit `()`")),
        }
    }

    fn deserialize_unit_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_unit(visitor)
    }

    fn deserialize_newtype_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            BsnValue::Constructor { fields, .. } | BsnValue::Tuple(fields) if fields.len() == 1 => {
                visitor.visit_newtype_struct(&fields[0])
            }
            _ => visitor.visit_newtype_struct(self),
        }
    }

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            BsnValue::List(values)
            | BsnValue::Tuple(values)
            | BsnValue::Constructor { fields: values, .. } => {
                visitor.visit_seq(ValueSeqAccess::new(values))
            }
            _ => Err(unexpected(self, "a sequence")),
        }
    }

    fn deserialize_tuple<V>(self, _len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_seq(visitor)
    }

    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            BsnValue::Map(entries) => visitor.visit_map(ValueMapAccess::new(entries)),
            BsnValue::Struct { fields, .. } => visitor.visit_map(FieldMapAccess::new(fields)),
            _ => Err(unexpected(self, "a map")),
        }
    }

    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            BsnValue::Struct { fields, .. } => visitor.visit_map(FieldMapAccess::new(fields)),
            BsnValue::Map(entries) => visitor.visit_map(ValueMapAccess::new(entries)),
            BsnValue::Unit => visitor.visit_map(FieldMapAccess::new(&[])),
            _ => Err(unexpected(self, "named fields")),
        }
    }

    fn deserialize_enum<V>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let (path, body) = match self {
            BsnValue::Path(path) => (path.as_str(), VariantBody::Unit),
            BsnValue::Constructor { type_path, fields } => {
                (type_path.as_str(), VariantBody::Tuple(fields))
            }
            BsnValue::Struct {
                type_path: Some(type_path),
                fields,
            } => (type_path.as_str(), VariantBody::Struct(fields)),
            _ => return Err(unexpected(self, "an enum variant")),
        };
        visitor.visit_enum(BsnEnumAccess {
            variant: final_path_segment(path),
            body,
        })
    }

    fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self {
            BsnValue::String(value) => visitor.visit_borrowed_str(value),
            BsnValue::Path(value) => visitor.visit_borrowed_str(final_path_segment(value)),
            _ => Err(unexpected(self, "an identifier")),
        }
    }

    fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_unit()
    }
}

struct ValueSeqAccess<'a> {
    values: slice::Iter<'a, BsnValue>,
}

impl<'a> ValueSeqAccess<'a> {
    fn new(values: &'a [BsnValue]) -> Self {
        Self {
            values: values.iter(),
        }
    }
}

impl<'de> SeqAccess<'de> for ValueSeqAccess<'de> {
    type Error = BsnDeserializeError;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
    where
        T: DeserializeSeed<'de>,
    {
        self.values
            .next()
            .map(|value| seed.deserialize(value))
            .transpose()
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.values.len())
    }
}

struct FieldMapAccess<'a> {
    fields: &'a [BsnStructField],
    index: usize,
    pending: Option<&'a BsnValue>,
}

impl<'a> FieldMapAccess<'a> {
    fn new(fields: &'a [BsnStructField]) -> Self {
        Self {
            fields,
            index: 0,
            pending: None,
        }
    }
}

impl<'de> MapAccess<'de> for FieldMapAccess<'de> {
    type Error = BsnDeserializeError;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: DeserializeSeed<'de>,
    {
        let Some(field) = self.fields.get(self.index) else {
            return Ok(None);
        };
        self.index += 1;
        self.pending = Some(&field.value);
        seed.deserialize(StrDeserializer::<BsnDeserializeError>::new(&field.name))
            .map(Some)
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: DeserializeSeed<'de>,
    {
        let value = self
            .pending
            .take()
            .ok_or_else(|| BsnDeserializeError::new("field value requested before key"))?;
        seed.deserialize(value)
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.fields.len().saturating_sub(self.index))
    }
}

struct ValueMapAccess<'a> {
    entries: &'a [(BsnValue, BsnValue)],
    index: usize,
    pending: Option<&'a BsnValue>,
}

impl<'a> ValueMapAccess<'a> {
    fn new(entries: &'a [(BsnValue, BsnValue)]) -> Self {
        Self {
            entries,
            index: 0,
            pending: None,
        }
    }
}

impl<'de> MapAccess<'de> for ValueMapAccess<'de> {
    type Error = BsnDeserializeError;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: DeserializeSeed<'de>,
    {
        let Some((key, value)) = self.entries.get(self.index) else {
            return Ok(None);
        };
        self.index += 1;
        self.pending = Some(value);
        seed.deserialize(key).map(Some)
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: DeserializeSeed<'de>,
    {
        let value = self
            .pending
            .take()
            .ok_or_else(|| BsnDeserializeError::new("map value requested before key"))?;
        seed.deserialize(value)
    }
}

enum VariantBody<'a> {
    Unit,
    Tuple(&'a [BsnValue]),
    Struct(&'a [BsnStructField]),
}

struct BsnEnumAccess<'a> {
    variant: &'a str,
    body: VariantBody<'a>,
}

impl<'de> EnumAccess<'de> for BsnEnumAccess<'de> {
    type Error = BsnDeserializeError;
    type Variant = BsnVariantAccess<'de>;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant), Self::Error>
    where
        V: DeserializeSeed<'de>,
    {
        let variant =
            seed.deserialize(StrDeserializer::<BsnDeserializeError>::new(self.variant))?;
        Ok((variant, BsnVariantAccess { body: self.body }))
    }
}

struct BsnVariantAccess<'a> {
    body: VariantBody<'a>,
}

impl<'de> VariantAccess<'de> for BsnVariantAccess<'de> {
    type Error = BsnDeserializeError;

    fn unit_variant(self) -> Result<(), Self::Error> {
        match self.body {
            VariantBody::Unit => Ok(()),
            VariantBody::Tuple(values) if values.is_empty() => Ok(()),
            _ => Err(BsnDeserializeError::new("expected a unit enum variant")),
        }
    }

    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value, Self::Error>
    where
        T: DeserializeSeed<'de>,
    {
        match self.body {
            VariantBody::Tuple(values) if values.len() == 1 => seed.deserialize(&values[0]),
            _ => Err(BsnDeserializeError::new(
                "expected one value for a newtype enum variant",
            )),
        }
    }

    fn tuple_variant<V>(self, _len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.body {
            VariantBody::Tuple(values) => visitor.visit_seq(ValueSeqAccess::new(values)),
            _ => Err(BsnDeserializeError::new("expected a tuple enum variant")),
        }
    }

    fn struct_variant<V>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.body {
            VariantBody::Struct(fields) => visitor.visit_map(FieldMapAccess::new(fields)),
            _ => Err(BsnDeserializeError::new("expected a struct enum variant")),
        }
    }
}

fn final_path_segment(path: &str) -> &str {
    path.rsplit("::").next().unwrap_or(path)
}
