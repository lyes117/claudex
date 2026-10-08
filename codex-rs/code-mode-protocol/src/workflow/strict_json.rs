//! Adapted from the existing native bridge's bounded RawValue decoder.
//! Raw scalar parsing preserves serde_json arbitrary_precision numbers exactly.
use super::Fault;
use super::Result;
use serde::Deserialize;
use serde::Deserializer;
use serde::de::DeserializeSeed;
use serde::de::MapAccess;
use serde::de::SeqAccess;
use serde::de::Visitor;
use serde_json::Value;
use std::fmt;

pub fn parse(text: &str) -> Result<Value> {
    parse_bounded(text, /*remaining*/ 256, /*limit_depth*/ 12)
}

pub fn parse_envelope(text: &str) -> Result<Value> {
    parse_bounded(text, /*remaining*/ 32768, /*limit_depth*/ 24)
}

fn parse_bounded(text: &str, mut remaining: usize, limit_depth: usize) -> Result<Value> {
    let mut decoder = serde_json::Deserializer::from_str(text);
    let value = Bounded {
        depth: 0,
        limit_depth,
        remaining: &mut remaining,
    }
    .deserialize(&mut decoder)
    .map_err(|_| Fault::Protocol)?;
    decoder.end().map_err(|_| Fault::Protocol)?;
    Ok(value)
}
struct Bounded<'a> {
    depth: usize,
    limit_depth: usize,
    remaining: &'a mut usize,
}
impl<'de> DeserializeSeed<'de> for Bounded<'_> {
    type Value = Value;
    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        decoder: D,
    ) -> std::result::Result<Value, D::Error> {
        if self.depth > self.limit_depth || *self.remaining == 0 {
            return Err(serde::de::Error::custom("JSON budget"));
        }
        *self.remaining -= 1;
        let raw = Box::<serde_json::value::RawValue>::deserialize(decoder)?;
        let mut decoder = serde_json::Deserializer::from_str(raw.get());
        match raw.get().trim_start().as_bytes().first() {
            Some(b'{') => decoder
                .deserialize_map(self)
                .map_err(serde::de::Error::custom),
            Some(b'[') => decoder
                .deserialize_seq(self)
                .map_err(serde::de::Error::custom),
            _ => serde_json::from_str(raw.get()).map_err(serde::de::Error::custom),
        }
    }
}
impl<'de> Visitor<'de> for Bounded<'_> {
    type Value = Value;
    fn expecting(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output.write_str("bounded JSON")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut input: A) -> std::result::Result<Value, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = input.next_element_seed(Bounded {
            depth: self.depth + 1,
            limit_depth: self.limit_depth,
            remaining: self.remaining,
        })? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut input: A) -> std::result::Result<Value, A::Error> {
        let mut values = serde_json::Map::new();
        while let Some(key) = input.next_key::<String>()? {
            if values.contains_key(&key) {
                return Err(serde::de::Error::custom("duplicate key"));
            }
            let value = input.next_value_seed(Bounded {
                depth: self.depth + 1,
                limit_depth: self.limit_depth,
                remaining: self.remaining,
            })?;
            values.insert(key, value);
        }
        Ok(Value::Object(values))
    }
}
