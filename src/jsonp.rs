use anyhow::{anyhow, Context};
use serde_json::Value;

pub fn parse_jsonp(input: &str) -> anyhow::Result<Value> {
    let trimmed = input.trim();
    let start = trimmed
        .find('(')
        .ok_or_else(|| anyhow!("JSONP response has no opening parenthesis"))?;
    let end = find_matching_paren(trimmed, start)
        .ok_or_else(|| anyhow!("JSONP response has no matching closing parenthesis"))?;
    let json = trimmed[start + 1..end].trim();
    serde_json::from_str(json).context("failed to parse JSONP payload")
}

fn find_matching_paren(input: &str, open_byte_index: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut quote = '\0';

    for (index, ch) in input
        .char_indices()
        .skip_while(|(index, _)| *index < open_byte_index)
    {
        if escaped {
            escaped = false;
            continue;
        }

        if in_string {
            match ch {
                '\\' => escaped = true,
                value if value == quote => in_string = false,
                _ => {}
            }
            continue;
        }

        match ch {
            '"' | '\'' => {
                in_string = true;
                quote = ch;
            }
            '(' => depth += 1,
            ')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }

    None
}

pub fn parse_ptui_cb_args(input: &str) -> anyhow::Result<Vec<String>> {
    let trimmed = input.trim();
    let start = trimmed
        .find('(')
        .ok_or_else(|| anyhow!("ptuiCB response has no opening parenthesis"))?;
    let end = trimmed
        .rfind(')')
        .ok_or_else(|| anyhow!("ptuiCB response has no closing parenthesis"))?;

    let args = &trimmed[start + 1..end];
    let mut values = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    let mut escaped = false;

    for ch in args.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }

        match ch {
            '\\' if in_quote => escaped = true,
            '\'' => in_quote = !in_quote,
            ',' if !in_quote => {
                values.push(current.trim().trim_matches('\'').to_owned());
                current.clear();
            }
            _ => current.push(ch),
        }
    }

    if !current.is_empty() || args.ends_with(',') {
        values.push(current.trim().trim_matches('\'').to_owned());
    }

    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_jsonp() {
        let value = parse_jsonp("jQuery_123({\"iRet\":\"0\",\"nickName\":\"abc\"});").unwrap();
        assert_eq!(value["iRet"], "0");
    }

    #[test]
    fn parses_jsonp_with_extra_parenthesized_suffix() {
        let value = parse_jsonp("jQuery_123({\"iRet\":\"0\",\"nickName\":\"a)b\"}); window.foo();")
            .unwrap();
        assert_eq!(value["nickName"], "a)b");
    }

    #[test]
    fn parses_ptui_cb() {
        let args = parse_ptui_cb_args(
            "ptuiCB('0','0','https://example.com/a?x=1','0','登录成功','nick');",
        )
        .unwrap();
        assert_eq!(args[2], "https://example.com/a?x=1");
    }
}
