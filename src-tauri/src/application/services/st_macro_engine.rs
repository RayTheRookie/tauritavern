use chrono::{Local, Timelike};
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Default)]
pub struct MacroContext {
    pub user_name: String,
    pub char_name: String,
    pub input: String,
    pub last_message_id: Option<String>,
    pub variables: HashMap<String, String>,
}

pub fn apply_macros(text: &str, context: &MacroContext) -> String {
    let angle_replaced = replace_angle_macros(text, context);
    replace_braced_macros(&angle_replaced, context)
}

fn replace_angle_macros(text: &str, context: &MacroContext) -> String {
    let replacements = [
        ("<user>", context.user_name.as_str()),
        ("<User>", context.user_name.as_str()),
        ("<USER>", context.user_name.as_str()),
        ("<char>", context.char_name.as_str()),
        ("<Char>", context.char_name.as_str()),
        ("<CHAR>", context.char_name.as_str()),
        ("<input>", context.input.as_str()),
        ("<Input>", context.input.as_str()),
        ("<INPUT>", context.input.as_str()),
    ];
    let mut output = text.to_string();
    for (from, to) in replacements {
        output = output.replace(from, to);
    }
    output
}

fn replace_braced_macros(text: &str, context: &MacroContext) -> String {
    let re = Regex::new(r"\{\{([^{}]+)\}\}").expect("valid macro regex");
    re.replace_all(text, |captures: &regex::Captures| {
        resolve_macro(captures.get(1).map(|m| m.as_str()).unwrap_or(""), context).unwrap_or_else(
            || {
                captures
                    .get(0)
                    .map(|m| m.as_str())
                    .unwrap_or("")
                    .to_string()
            },
        )
    })
    .to_string()
}

fn resolve_macro(token: &str, context: &MacroContext) -> Option<String> {
    let token = token.trim();
    match token {
        "user" | "User" | "USER" => Some(context.user_name.clone()),
        "char" | "Char" | "CHAR" => Some(context.char_name.clone()),
        "input" | "Input" | "INPUT" => Some(context.input.clone()),
        "lastMessageId" | "lastmessageid" => context.last_message_id.clone(),
        "time" | "Time" => Some(Local::now().format("%H:%M").to_string()),
        "date" | "Date" => Some(Local::now().format("%Y-%m-%d").to_string()),
        "weekday" | "Weekday" => Some(Local::now().format("%A").to_string()),
        "isotime" | "isoTime" => Some(Local::now().to_rfc3339()),
        _ => resolve_prefixed_macro(token, context),
    }
}

fn resolve_prefixed_macro(token: &str, context: &MacroContext) -> Option<String> {
    if let Some(rest) = token
        .strip_prefix("getvar::")
        .or_else(|| token.strip_prefix("getvar:"))
    {
        return Some(resolve_variable(rest.trim(), &context.variables));
    }

    if let Some(rest) = token.strip_prefix("random:") {
        let options = split_options(rest);
        return choose_option(&options);
    }

    if let Some(rest) = token
        .strip_prefix("pick::")
        .or_else(|| token.strip_prefix("pick:"))
    {
        let options = split_options(rest);
        return choose_option(&options);
    }

    if let Some(rest) = token.strip_prefix("roll:") {
        return roll_dice(rest.trim());
    }

    if let Some(rest) = token.strip_prefix("calc:") {
        return eval_calc(rest.trim()).map(format_number);
    }

    None
}

fn resolve_variable(reference: &str, variables: &HashMap<String, String>) -> String {
    let (name, path) = split_variable_reference(reference);
    let Some(raw) = variables.get(name) else {
        return String::new();
    };
    if path.is_empty() {
        return raw.clone();
    }
    let Ok(value) = serde_json::from_str::<Value>(raw) else {
        return String::new();
    };
    json_path_get(&value, &path)
        .map(json_to_string)
        .unwrap_or_default()
}

fn split_variable_reference(reference: &str) -> (&str, Vec<PathSegment>) {
    let mut split_at = reference.len();
    for (idx, ch) in reference.char_indices() {
        if ch == '.' || ch == '[' {
            split_at = idx;
            break;
        }
    }
    (
        &reference[..split_at],
        parse_json_path(&reference[split_at..]),
    )
}

#[derive(Debug, Clone)]
enum PathSegment {
    Key(String),
    Index(usize),
}

fn parse_json_path(path: &str) -> Vec<PathSegment> {
    let mut result = Vec::new();
    let mut chars = path.char_indices().peekable();
    while let Some((_, ch)) = chars.next() {
        match ch {
            '.' => {
                let mut key = String::new();
                while let Some((_, next)) = chars.peek().copied() {
                    if next == '.' || next == '[' {
                        break;
                    }
                    key.push(next);
                    chars.next();
                }
                if !key.is_empty() {
                    result.push(PathSegment::Key(key));
                }
            }
            '[' => {
                let mut token = String::new();
                for (_, next) in chars.by_ref() {
                    if next == ']' {
                        break;
                    }
                    token.push(next);
                }
                let token = token.trim().trim_matches('"').trim_matches('\'');
                if let Ok(index) = token.parse::<usize>() {
                    result.push(PathSegment::Index(index));
                } else if !token.is_empty() {
                    result.push(PathSegment::Key(token.to_string()));
                }
            }
            _ => break,
        }
    }
    result
}

fn json_path_get<'a>(value: &'a Value, path: &[PathSegment]) -> Option<&'a Value> {
    let mut current = value;
    for segment in path {
        current = match segment {
            PathSegment::Key(key) => current.get(key)?,
            PathSegment::Index(index) => current.get(*index)?,
        };
    }
    Some(current)
}

fn json_to_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn split_options(rest: &str) -> Vec<String> {
    rest.split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

fn choose_option(options: &[String]) -> Option<String> {
    if options.is_empty() {
        return Some(String::new());
    }
    let idx = pseudo_random(options.len());
    options.get(idx).cloned()
}

fn roll_dice(expr: &str) -> Option<String> {
    let lowered = expr.to_ascii_lowercase();
    let (count, sides) = if let Some((left, right)) = lowered.split_once('d') {
        let count = if left.trim().is_empty() {
            1
        } else {
            left.trim().parse::<usize>().ok()?
        };
        let sides = right.trim().parse::<usize>().ok()?;
        (count.max(1), sides.max(1))
    } else {
        (1, lowered.trim().parse::<usize>().ok()?.max(1))
    };

    let mut total = 0usize;
    for offset in 0..count {
        total += 1 + ((pseudo_random(sides) + offset) % sides);
    }
    Some(total.to_string())
}

fn pseudo_random(max: usize) -> usize {
    if max == 0 {
        return 0;
    }
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as usize)
        .unwrap_or_else(|_| Local::now().nanosecond() as usize);
    nanos % max
}

fn eval_calc(input: &str) -> Option<f64> {
    let mut parser = CalcParser::new(input);
    let value = parser.parse_expr()?;
    parser.skip_ws();
    if parser.is_done() {
        Some(value)
    } else {
        None
    }
}

fn format_number(value: f64) -> String {
    if (value.fract()).abs() < f64::EPSILON {
        format!("{}", value as i64)
    } else {
        let mut formatted = format!("{:.6}", value);
        while formatted.contains('.') && formatted.ends_with('0') {
            formatted.pop();
        }
        if formatted.ends_with('.') {
            formatted.pop();
        }
        formatted
    }
}

struct CalcParser<'a> {
    chars: Vec<char>,
    pos: usize,
    _source: &'a str,
}

impl<'a> CalcParser<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            chars: source.chars().collect(),
            pos: 0,
            _source: source,
        }
    }

    fn parse_expr(&mut self) -> Option<f64> {
        let mut value = self.parse_term()?;
        loop {
            self.skip_ws();
            match self.peek() {
                Some('+') => {
                    self.pos += 1;
                    value += self.parse_term()?;
                }
                Some('-') => {
                    self.pos += 1;
                    value -= self.parse_term()?;
                }
                _ => return Some(value),
            }
        }
    }

    fn parse_term(&mut self) -> Option<f64> {
        let mut value = self.parse_factor()?;
        loop {
            self.skip_ws();
            match self.peek() {
                Some('*') => {
                    self.pos += 1;
                    value *= self.parse_factor()?;
                }
                Some('/') => {
                    self.pos += 1;
                    let divisor = self.parse_factor()?;
                    if divisor == 0.0 {
                        return None;
                    }
                    value /= divisor;
                }
                _ => return Some(value),
            }
        }
    }

    fn parse_factor(&mut self) -> Option<f64> {
        self.skip_ws();
        match self.peek()? {
            '(' => {
                self.pos += 1;
                let value = self.parse_expr()?;
                self.skip_ws();
                if self.peek()? != ')' {
                    return None;
                }
                self.pos += 1;
                Some(value)
            }
            '-' => {
                self.pos += 1;
                self.parse_factor().map(|v| -v)
            }
            _ => self.parse_number(),
        }
    }

    fn parse_number(&mut self) -> Option<f64> {
        self.skip_ws();
        let start = self.pos;
        while matches!(self.peek(), Some(ch) if ch.is_ascii_digit() || ch == '.') {
            self.pos += 1;
        }
        if self.pos == start {
            return None;
        }
        self.chars[start..self.pos]
            .iter()
            .collect::<String>()
            .parse::<f64>()
            .ok()
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(ch) if ch.is_whitespace()) {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn is_done(&self) -> bool {
        self.pos >= self.chars.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> MacroContext {
        MacroContext {
            user_name: "Alice".to_string(),
            char_name: "Soyo".to_string(),
            input: "hello".to_string(),
            last_message_id: Some("m-1".to_string()),
            variables: HashMap::from([
                ("mood".to_string(), "warm".to_string()),
                (
                    "stats".to_string(),
                    r#"{"affection":7,"items":["tea","cake"]}"#.to_string(),
                ),
            ]),
        }
    }

    #[test]
    fn replaces_st_identity_and_variable_macros() {
        assert_eq!(
            apply_macros(
                "<user> meets {{char}}: {{input}} / {{getvar::mood}} / {{lastMessageId}}",
                &ctx()
            ),
            "Alice meets Soyo: hello / warm / m-1"
        );
    }

    #[test]
    fn evaluates_calc_macro() {
        assert_eq!(apply_macros("{{calc:(1 + 2) * 3 / 2}}", &ctx()), "4.5");
    }

    #[test]
    fn resolves_json_path_variables() {
        assert_eq!(
            apply_macros(
                "{{getvar::stats.affection}} / {{getvar::stats.items[1]}}",
                &ctx()
            ),
            "7 / cake"
        );
    }

    #[test]
    fn preserves_unknown_macros() {
        assert_eq!(apply_macros("{{unknown}}", &ctx()), "{{unknown}}");
    }
}
