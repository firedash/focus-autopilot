// SPDX-License-Identifier: Apache-2.0
//! A minimal JSON reader for fixture traces, so `focus-core` tests need no dependencies.
//! Test-only. The product parses hook payloads in `focus-receiver` with `serde_json`.

use std::iter::Peekable;
use std::str::Chars;

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(f64),
    Text(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn text(&self, key: &str) -> Option<String> {
        match self.get(key) {
            Some(Json::Text(text)) => Some(text.clone()),
            _ => None,
        }
    }

    pub fn number(&self, key: &str) -> Option<f64> {
        match self.get(key) {
            Some(Json::Number(n)) => Some(*n),
            _ => None,
        }
    }

    pub fn flag(&self, key: &str) -> bool {
        matches!(self.get(key), Some(Json::Bool(true)))
    }

    pub fn len(&self, key: &str) -> usize {
        match self.get(key) {
            Some(Json::Array(items)) => items.len(),
            _ => 0,
        }
    }
}

pub fn parse(text: &str) -> Json {
    let mut chars = text.chars().peekable();
    let value = value(&mut chars);
    skip_space(&mut chars);
    assert!(
        chars.next().is_none(),
        "trailing characters after JSON value"
    );
    value
}

fn skip_space(chars: &mut Peekable<Chars<'_>>) {
    while chars.next_if(|c| c.is_whitespace()).is_some() {}
}

fn value(chars: &mut Peekable<Chars<'_>>) -> Json {
    skip_space(chars);
    match chars.peek().copied() {
        Some('{') => object(chars),
        Some('[') => array(chars),
        Some('"') => Json::Text(string(chars)),
        Some(_) => literal(chars),
        None => panic!("unexpected end of JSON"),
    }
}

fn object(chars: &mut Peekable<Chars<'_>>) -> Json {
    chars.next();
    let mut fields = Vec::new();
    loop {
        skip_space(chars);
        if chars.next_if_eq(&'}').is_some() {
            return Json::Object(fields);
        }
        let key = string(chars);
        skip_space(chars);
        assert_eq!(chars.next(), Some(':'), "expected ':' after key {key}");
        fields.push((key, value(chars)));
        skip_space(chars);
        chars.next_if_eq(&',');
    }
}

fn array(chars: &mut Peekable<Chars<'_>>) -> Json {
    chars.next();
    let mut items = Vec::new();
    loop {
        skip_space(chars);
        if chars.next_if_eq(&']').is_some() {
            return Json::Array(items);
        }
        items.push(value(chars));
        skip_space(chars);
        chars.next_if_eq(&',');
    }
}

fn string(chars: &mut Peekable<Chars<'_>>) -> String {
    assert_eq!(chars.next(), Some('"'), "expected a string");
    let mut out = String::new();
    loop {
        match chars.next().expect("unterminated string") {
            '"' => return out,
            '\\' => match chars.next().expect("unterminated escape") {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                'u' => {
                    let hex: String = chars.by_ref().take(4).collect();
                    let code = u32::from_str_radix(&hex, 16).expect("bad \\u escape");
                    out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                }
                other => out.push(other),
            },
            c => out.push(c),
        }
    }
}

fn literal(chars: &mut Peekable<Chars<'_>>) -> Json {
    let mut word = String::new();
    while let Some(c) = chars.next_if(|c| !matches!(c, ',' | '}' | ']') && !c.is_whitespace()) {
        word.push(c);
    }
    match word.as_str() {
        "null" => Json::Null,
        "true" => Json::Bool(true),
        "false" => Json::Bool(false),
        number => Json::Number(number.parse().expect("bad JSON literal")),
    }
}
