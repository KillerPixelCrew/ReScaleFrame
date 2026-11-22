// SPDX-License-Identifier: MIT
//! `.rsfpatch` v1 admission and the JSON source rules it carries.

use super::{ANCHOR, FORMAT, Files, HASH, Outcome, SOURCE, check, refuse, sha256};
use crate::shader_patch_abi::{
    RSF_PATCH_JSON_ARRAY, RSF_PATCH_JSON_OBJECT, RSF_PATCH_JSON_SINT, RSF_PATCH_JSON_STRING,
    RSF_PATCH_JSON_UINT, rsf_patch_bytes, rsf_patch_platform,
};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{c_char, c_void};

// Values nested this deep are refused, so the root and 127 levels below it are accepted.
const JSON_DEPTH: usize = 128;
const BOM: &[u8] = b"\xef\xbb\xbf";

/// A parsed JSON value. Strings and keys are raw UTF-8 bytes as yyjson accepted them.
pub(crate) enum Json {
    Other,
    // yyjson keeps signed and unsigned integers apart but reads both as a 64-bit pattern.
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

enum Frame {
    Arr(Vec<Json>, usize),
    Obj(
        Vec<(Vec<u8>, Json)>,
        usize,
        Option<Vec<u8>>,
        BTreeSet<Vec<u8>>,
    ),
}

#[derive(Default)]
struct Builder {
    stack: Vec<Frame>,
    root: Option<Json>,
    failed: Option<&'static str>,
}

impl Builder {
    fn node(&mut self, kind: u32, number: u64, bytes: &[u8], size: usize) {
        if self.failed.is_some() {
            return;
        }
        if let Some(Frame::Obj(_, _, key @ None, keys)) = self.stack.last_mut() {
            if kind != RSF_PATCH_JSON_STRING as u32 || bytes.contains(&0) {
                self.failed = Some("embedded NUL");
            } else if !keys.insert(bytes.to_vec()) {
                self.failed = Some("duplicate JSON key");
            } else {
                *key = Some(bytes.to_vec());
            }
            return;
        }
        if self.stack.len() >= JSON_DEPTH {
            self.failed = Some("JSON nesting");
            return;
        }
        let value = match kind {
            k if k == RSF_PATCH_JSON_UINT as u32 || k == RSF_PATCH_JSON_SINT as u32 => {
                Json::Int(number)
            }
            k if k == RSF_PATCH_JSON_STRING as u32 => Json::Str(bytes.to_vec()),
            k if k == RSF_PATCH_JSON_ARRAY as u32 && size > 0 => {
                self.stack.push(Frame::Arr(Vec::new(), size));
                return;
            }
            k if k == RSF_PATCH_JSON_OBJECT as u32 && size > 0 => {
                let keys = BTreeSet::new();
                self.stack.push(Frame::Obj(Vec::new(), size, None, keys));
                return;
            }
            k if k == RSF_PATCH_JSON_ARRAY as u32 => Json::Arr(Vec::new()),
            k if k == RSF_PATCH_JSON_OBJECT as u32 => Json::Obj(Vec::new()),
            _ => Json::Other,
        };
        self.complete(value);
    }

    fn complete(&mut self, mut value: Json) {
        loop {
            let done = match self.stack.last_mut() {
                None => {
                    self.root = Some(value);
                    return;
                }
                Some(Frame::Arr(items, left)) => {
                    items.push(value);
                    *left -= 1;
                    *left == 0
                }
                Some(Frame::Obj(members, left, key, _)) => {
                    members.push((key.take().unwrap_or_default(), value));
                    *left -= 1;
                    *left == 0
                }
            };
            if !done {
                return;
            }
            value = match self.stack.pop() {
                Some(Frame::Arr(items, _)) => Json::Arr(items),
                Some(Frame::Obj(members, ..)) => Json::Obj(members),
                None => return,
            };
        }
    }
}

unsafe extern "C" fn visit(
    user: *mut c_void,
    kind: u32,
    number: u64,
    text: *const c_char,
    size: usize,
) {
    let builder = unsafe { &mut *user.cast::<Builder>() };
    let bytes = if kind == RSF_PATCH_JSON_STRING as u32 && size > 0 {
        unsafe { std::slice::from_raw_parts(text.cast::<u8>(), size) }
    } else {
        &[]
    };
    builder.node(kind, number, bytes, size);
}

pub(crate) fn json(platform: &rsf_patch_platform, data: &[u8]) -> Outcome<Json> {
    let mut builder = Builder::default();
    let text = super::raw(data);
    let parse = platform.parse_json.expect("bound shader patch platform");
    let parsed = unsafe { parse(text, (&raw mut builder).cast(), Some(visit)) } == 0;
    check(parsed, FORMAT, "invalid JSON")?;
    if let Some(reason) = builder.failed {
        return Err(refuse(FORMAT, reason));
    }
    builder.root.ok_or_else(|| refuse(FORMAT, "invalid JSON"))
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
    pub(crate) fn load(platform: &rsf_patch_platform, data: &[u8]) -> Outcome<Pack> {
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
        let id = text(Some(&json(platform, meta)?), b"id")?.to_vec();
        check(!id.is_empty(), FORMAT, "empty pack id")?;
        let rules = json(platform, rules)?;
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
