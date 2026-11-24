// SPDX-License-Identifier: MIT
//! `.rsfpatch` v1 admission and the JSON source rules it carries.

use super::{ANCHOR, FORMAT, Files, HASH, Outcome, SOURCE, check, refuse, sha256};
use crate::sys::rsf_patch_bytes;
use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

// Values nested this deep are refused, so the root and 127 levels below it are accepted.
const JSON_DEPTH: usize = 128;
const BOM: &[u8] = b"\xef\xbb\xbf";

/// A parsed JSON value. Strings and keys are UTF-8 bytes.
pub(crate) enum Json {
    Other,
    // Signed and unsigned integers both read as their 64-bit pattern.
    Int(u64),
    Str(Vec<u8>),
    Arr(Vec<Json>),
    Obj(Vec<(Vec<u8>, Json)>),
}

impl Json {
    pub(crate) fn get(&self, key: &[u8]) -> Option<&Json> {
        match self {
            Json::Obj(members) => members.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub(crate) fn items(value: Option<&Json>) -> &[Json] {
        match value {
            Some(Json::Arr(items)) => items,
            _ => &[],
        }
    }
}

/// An unsigned read of an integer, or 0 for anything else, as the schema has always read it.
pub(crate) fn uint(value: Option<&Json>) -> u64 {
    match value {
        Some(Json::Int(n)) => *n,
        _ => 0,
    }
}

/// A required string field. Embedded NULs are refused.
pub(crate) fn string(value: Option<&Json>) -> Outcome<&[u8]> {
    let Some(Json::Str(s)) = value else {
        return Err(refuse(FORMAT, "expected string"));
    };
    check(!s.contains(&0), FORMAT, "embedded NUL")?;
    Ok(s)
}

pub(crate) fn text<'a>(value: Option<&'a Json>, key: &[u8]) -> Outcome<&'a [u8]> {
    string(value.and_then(|v| v.get(key)))
}

/// One JSON value at `depth` containers below the root. Keys and nesting are checked as the text
/// is read; `failed` names the rule that refused it.
struct Node<'a> {
    depth: usize,
    failed: &'a Cell<Option<&'static str>>,
}

impl Node<'_> {
    fn refuse<E: de::Error>(&self, reason: &'static str) -> E {
        self.failed.set(Some(reason));
        E::custom(reason)
    }

    fn child(&self) -> Self {
        Node {
            depth: self.depth + 1,
            failed: self.failed,
        }
    }
}

impl<'de> DeserializeSeed<'de> for Node<'_> {
    type Value = Json;

    fn deserialize<D: Deserializer<'de>>(self, reader: D) -> Result<Json, D::Error> {
        if self.depth >= JSON_DEPTH {
            return Err(self.refuse("JSON nesting"));
        }
        reader.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Node<'_> {
    type Value = Json;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_bool<E>(self, _: bool) -> Result<Json, E> {
        Ok(Json::Other)
    }

    fn visit_unit<E>(self) -> Result<Json, E> {
        Ok(Json::Other)
    }

    fn visit_f64<E>(self, _: f64) -> Result<Json, E> {
        Ok(Json::Other)
    }

    fn visit_u64<E>(self, number: u64) -> Result<Json, E> {
        Ok(Json::Int(number))
    }

    fn visit_i64<E>(self, number: i64) -> Result<Json, E> {
        Ok(Json::Int(number as u64))
    }

    fn visit_str<E>(self, text: &str) -> Result<Json, E> {
        Ok(Json::Str(text.as_bytes().to_vec()))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut items: A) -> Result<Json, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = items.next_element_seed(self.child())? {
            values.push(value);
        }
        Ok(Json::Arr(values))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut entries: A) -> Result<Json, A::Error> {
        let mut members = Vec::new();
        let mut keys = BTreeSet::new();
        while let Some(key) = entries.next_key::<String>()? {
            if key.contains('\0') {
                return Err(self.refuse("embedded NUL"));
            }
            if !keys.insert(key.clone()) {
                return Err(self.refuse("duplicate JSON key"));
            }
            members.push((key.into_bytes(), entries.next_value_seed(self.child())?));
        }
        Ok(Json::Obj(members))
    }
}

pub(crate) fn json(data: &[u8]) -> Outcome<Json> {
    let failed = Cell::new(None);
    let mut reader = serde_json::Deserializer::from_slice(data);
    // Node bounds the depth itself: the root and 127 levels below it.
    reader.disable_recursion_limit();
    let parsed = Node {
        depth: 0,
        failed: &failed,
    }
    .deserialize(&mut reader)
    .and_then(|root| reader.end().map(|()| root));
    match (parsed, failed.get()) {
        (_, Some(reason)) => Err(refuse(FORMAT, reason)),
        (Ok(root), None) => Ok(root),
        (Err(_), None) => Err(refuse(FORMAT, "invalid JSON")),
    }
}
fn integer(data: &[u8], at: u64, n: u64) -> Outcome<u64> {
    let len = data.len() as u64;
    check(at <= len && n <= len - at, FORMAT, "truncated integer")?;
    let at = at as usize;
    Ok((0..n as usize).fold(0, |value, i| value | u64::from(data[at + i]) << (8 * i)))
}

fn slice(data: &[u8], at: u64, n: u64) -> Outcome<&[u8]> {
    let len = data.len() as u64;
    check(at <= len && n <= len - at, FORMAT, "section outside file")?;
    Ok(&data[at as usize..(at + n) as usize])
}

pub(crate) struct Pack {
    pub(crate) id: Vec<u8>,
    pub(crate) hash: String,
    pub(crate) rules: Json,
    payload: Files,
}

impl Pack {
    pub(crate) fn load(data: &[u8]) -> Outcome<Pack> {
        check(
            slice(data, 0, 8)? == b"RSFPATCH" && integer(data, 8, 4)? == 1,
            FORMAT,
            "pack version",
        )?;
        let count = integer(data, 12, 4)?;
        let table = integer(data, 16, 8)?;
        check(
            count <= data.len() as u64 / 24 && table >= 24,
            FORMAT,
            "section table",
        )?;
        slice(data, table, count * 24)?;
        let mut ranges = vec![(0, 24), (table, table + count * 24)];
        let mut sections = BTreeMap::new();
        for i in 0..count {
            let at = table + i * 24;
            let kind = integer(data, at, 4)?;
            check(
                integer(data, at + 4, 4)? == 0,
                FORMAT,
                "reserved section field",
            )?;
            let offset = integer(data, at + 8, 8)?;
            let size = integer(data, at + 16, 8)?;
            let part = slice(data, offset, size)?;
            ranges.push((offset, offset + size));
            check(
                sections.insert(kind, part).is_none(),
                FORMAT,
                "duplicate section",
            )?;
        }
        ranges.sort_unstable();
        let disjoint = ranges.windows(2).all(|pair| pair[1].0 >= pair[0].1);
        check(disjoint, FORMAT, "overlapping sections")?;
        let (Some(meta), Some(rules), Some(files)) =
            (sections.get(&1), sections.get(&2), sections.get(&3))
        else {
            return Err(refuse(FORMAT, "missing section"));
        };
        let id = text(Some(&json(meta)?), b"id")?.to_vec();
        check(!id.is_empty(), FORMAT, "empty pack id")?;
        let rules = json(rules)?;
        check(uint(rules.get(b"schema")) == 1, FORMAT, "rule schema")?;
        let payload = payload(files)?;
        Ok(Pack {
            id,
            hash: sha256(data)?,
            rules,
            payload,
        })
    }

    /// Applies every file rule to the acquired originals, then adds the authored payload.
    pub(crate) fn transform(&self, originals: &Files) -> Outcome<Files> {
        let mut out = originals.clone();
        let Some(Json::Obj(sources)) = self.rules.get(b"sources") else {
            return Err(refuse(FORMAT, "source hashes"));
        };
        for (name, expected) in sources {
            let found = originals
                .get(name)
                .ok_or_else(|| refuse(SOURCE, "source missing"))?;
            let matches = sha256(found)?.as_bytes() == string(Some(expected))?;
            check(matches, HASH, "source hash mismatch")?;
        }
        let Some(Json::Obj(files)) = self.rules.get(b"files") else {
            return Err(refuse(FORMAT, "file rules"));
        };
        for (name, rule) in files {
            let rule = Some(rule);
            let op = text(rule, b"op")?;
            let input = if op == b"extract" {
                text(rule, b"source")?
            } else {
                name
            };
            let original = originals
                .get(input)
                .ok_or_else(|| refuse(SOURCE, "rule input missing"))?;
            let output = match op {
                b"wrap" => [text(rule, b"prefix")?, original, text(rule, b"suffix")?].concat(),
                b"inserts" => inserts(original, rule)?,
                b"extract" => extract(original, rule)?,
                b"clone_function" => clone_function(original, rule)?,
                _ => return Err(refuse(FORMAT, "unknown source operation")),
            };
            out.insert(name.clone(), output);
        }
        for (name, content) in &self.payload {
            let added = !out.contains_key(name);
            check(added, FORMAT, "payload overrides acquired source")?;
            out.insert(name.clone(), content.clone());
        }
        Ok(out)
    }
}

fn payload(files: &[u8]) -> Outcome<Files> {
    let count = integer(files, 0, 4)?;
    check(count <= files.len() as u64 / 12, FORMAT, "payload table")?;
    let mut payload = Files::new();
    let mut at = 4;
    for _ in 0..count {
        let name_size = integer(files, at, 4)?;
        let size = integer(files, at + 4, 8)?;
        at += 12;
        let name = slice(files, at, name_size)?;
        at += name_size;
        check(
            !name.is_empty() && !name.contains(&0),
            FORMAT,
            "payload name",
        )?;
        let content = slice(files, at, size)?;
        at += size;
        let added = payload.insert(name.to_vec(), content.to_vec()).is_none();
        check(added, FORMAT, "duplicate payload")?;
    }
    check(at == files.len() as u64, FORMAT, "trailing payload")?;
    Ok(payload)
}

pub(crate) fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from > haystack.len() {
        return None;
    }
    if needle.is_empty() {
        return Some(from);
    }
    haystack[from..]
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|at| at + from)
}

fn rfind(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(haystack.len());
    }
    haystack
        .windows(needle.len())
        .rposition(|window| window == needle)
}

fn unique_at(source: &[u8], anchor: &[u8]) -> Outcome<usize> {
    check(!anchor.is_empty(), FORMAT, "empty anchor")?;
    let at = find(source, anchor, 0).filter(|&at| find(source, anchor, at + 1).is_none());
    at.ok_or_else(|| refuse(ANCHOR, "missing or ambiguous anchor"))
}

/// Inserts authored text at reviewed offsets, last offset first, each beside its anchor.
fn inserts(original: &[u8], rule: Option<&Json>) -> Outcome<Vec<u8>> {
    let Some(Json::Arr(edits)) = rule.and_then(|r| r.get(b"edits")) else {
        return Err(refuse(FORMAT, "insertion list"));
    };
    let mut source = original.to_vec();
    let mut last = source.len() as u64 + 1;
    for edit in edits {
        let at = uint(edit.get(b"at"));
        let anchor_at = uint(edit.get(b"anchor_at"));
        let anchor = text(Some(edit), b"anchor")?;
        let len = source.len() as u64;
        let anchored = at < last
            && at <= len
            && anchor_at <= len
            && original
                .get(anchor_at as usize..)
                .is_some_and(|rest| rest.starts_with(anchor));
        check(anchored, ANCHOR, "insertion anchor")?;
        let addition = text(Some(edit), b"text")?;
        source.splice(at as usize..at as usize, addition.iter().copied());
        last = at;
    }
    Ok(source)
}

/// Copies the range from a unique start anchor to the last end anchor, renaming identifiers.
fn extract(source: &[u8], rule: Option<&Json>) -> Outcome<Vec<u8>> {
    let start = unique_at(source, text(rule, b"start")?)?;
    let end = rfind(source, text(rule, b"end")?).filter(|&end| end > start);
    let end = end.ok_or_else(|| refuse(ANCHOR, "extract boundary"))?;
    let remap = remap(&source[start..end], rule.and_then(|r| r.get(b"remap")))?;
    Ok([
        text(rule, b"prefix")?,
        remap.as_slice(),
        text(rule, b"suffix")?,
    ]
    .concat())
}

/// Keeps a function and adds a renamed copy of its balanced body after it.
fn clone_function(original: &[u8], rule: Option<&Json>) -> Outcome<Vec<u8>> {
    let source = original.strip_prefix(BOM).unwrap_or(original);
    let start = unique_at(source, text(rule, b"start")?)?;
    let body = source[start..].iter().position(|&b| b == b'{');
    let body = body.ok_or_else(|| refuse(ANCHOR, "function body"))? + start;
    let mut end = body + 1;
    let mut depth = 1i64;
    while end < source.len() && depth != 0 {
        depth += i64::from(source[end] == b'{') - i64::from(source[end] == b'}');
        end += 1;
    }
    check(depth == 0, ANCHOR, "unbalanced function")?;
    let borrowed = &source[body..end];
    let renamed = remap(borrowed, rule.and_then(|r| r.get(b"remap")))?;
    Ok([
        text(rule, b"prefix")?,
        &source[..start],
        text(rule, b"before")?,
        borrowed,
        text(rule, b"between")?,
        renamed.as_slice(),
        text(rule, b"after")?,
        &source[end..],
    ]
    .concat())
}

/// Replaces whole ASCII identifiers named in the table.
fn remap(source: &[u8], table: Option<&Json>) -> Outcome<Vec<u8>> {
    check(matches!(table, Some(Json::Obj(_))), FORMAT, "remap table")?;
    let mut out = Vec::with_capacity(source.len());
    let mut i = 0;
    while i < source.len() {
        let c = source[i];
        if c.is_ascii_alphabetic() || c == b'_' {
            let begin = i;
            i += 1;
            while i < source.len() && (source[i].is_ascii_alphanumeric() || source[i] == b'_') {
                i += 1;
            }
            let word = &source[begin..i];
            match table.and_then(|t| t.get(word)) {
                Some(replacement) => out.extend_from_slice(string(Some(replacement))?),
                None => out.extend_from_slice(word),
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    Ok(out)
}

/// Bytes borrowed from a C caller. Null is accepted only for an empty range.
pub(crate) fn bytes<'a>(b: rsf_patch_bytes) -> Outcome<&'a [u8]> {
    check(
        !b.data.is_null() || b.size == 0,
        super::ARGUMENT,
        "null bytes",
    )?;
    Ok(if b.size == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(b.data.cast(), b.size) }
    })
}
