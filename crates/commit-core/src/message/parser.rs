use super::ast::{CommitMessage, Footer, FooterSeparator, Header, is_breaking_token};
use super::error::{ParseError, ParseErrorKind};

pub fn parse(message: &str) -> Result<CommitMessage<'_>, ParseError> {
    let message = message.trim_matches(['\n', '\r']);
    if message.trim().is_empty() {
        return Err(ParseError::new(
            ParseErrorKind::EmptyMessage,
            "generated commit message is empty",
        ));
    }

    let (header_line, rest) = split_first_line(message);
    let header = parse_header(header_line)?;
    let (body, footers) = parse_body_and_footers(rest)?;
    let breaking_footer = footers.iter().find(|footer| footer.is_breaking_change());
    let breaking = header.breaking || breaking_footer.is_some();
    let breaking_description = breaking_footer
        .map(|footer| footer.value)
        .or_else(|| header.breaking.then_some(header.description));

    Ok(CommitMessage {
        header,
        body,
        footers,
        breaking,
        breaking_description,
    })
}

fn split_first_line(message: &str) -> (&str, &str) {
    if let Some(index) = message.find('\n') {
        let header = message[..index].trim_end_matches('\r');
        (header, &message[index + 1..])
    } else {
        (message.trim_end_matches('\r'), "")
    }
}

fn parse_header(line: &str) -> Result<Header<'_>, ParseError> {
    if line.trim().is_empty() {
        return Err(ParseError::new(
            ParseErrorKind::EmptyMessage,
            "generated commit message is empty",
        ));
    }
    if line.starts_with(char::is_whitespace) {
        return Err(conventional_shape_error(ParseErrorKind::InvalidFormat));
    }

    let mut colon_index = None;
    let mut paren_depth = 0_i32;
    for (index, character) in line.char_indices() {
        match character {
            '(' => paren_depth += 1,
            ')' => {
                paren_depth -= 1;
                if paren_depth < 0 {
                    return Err(conventional_shape_error(ParseErrorKind::InvalidScope));
                }
            }
            ':' if paren_depth == 0 => {
                colon_index = Some(index);
                break;
            }
            _ => {}
        }
    }
    let colon_index =
        colon_index.ok_or_else(|| conventional_shape_error(ParseErrorKind::MissingColon))?;
    if paren_depth != 0 {
        return Err(conventional_shape_error(ParseErrorKind::InvalidScope));
    }
    let prefix = &line[..colon_index];
    let description_with_space = &line[colon_index + 1..];
    if !description_with_space.starts_with(' ') {
        return Err(conventional_shape_error(ParseErrorKind::MissingColon));
    }
    let description = description_with_space.trim_start();
    if description.is_empty() {
        return Err(ParseError::new(
            ParseErrorKind::MissingDescription,
            "commit subject must look like '<type>: <summary>'",
        ));
    }

    let (prefix, breaking) = if let Some(prefix) = prefix.strip_suffix('!') {
        (prefix, true)
    } else {
        (prefix, false)
    };
    if prefix.contains('!') {
        return Err(conventional_shape_error(ParseErrorKind::InvalidFormat));
    }

    let (ty, scope) = if let Some(scope_start) = prefix.find('(') {
        if !prefix.ends_with(')') {
            return Err(conventional_shape_error(ParseErrorKind::InvalidScope));
        }
        let ty = &prefix[..scope_start];
        let scope = &prefix[scope_start + 1..prefix.len() - 1];
        if scope.trim().is_empty() {
            return Err(conventional_shape_error(ParseErrorKind::InvalidScope));
        }
        (ty, Some(scope))
    } else {
        (prefix, None)
    };

    if ty.is_empty() {
        return Err(conventional_shape_error(ParseErrorKind::MissingType));
    }
    if ty.chars().any(is_invalid_type_char) {
        return Err(conventional_shape_error(ParseErrorKind::InvalidType));
    }

    Ok(Header {
        ty,
        scope,
        breaking,
        description,
    })
}

fn is_invalid_type_char(character: char) -> bool {
    character.is_whitespace() || matches!(character, '(' | ')' | ':' | '!' | '\n' | '\r')
}

fn parse_body_and_footers(rest: &str) -> Result<(Option<&str>, Vec<Footer<'_>>), ParseError> {
    let rest = rest.trim_end_matches(['\n', '\r']);
    if rest.is_empty() {
        return Ok((None, Vec::new()));
    }
    if !starts_with_blank_line(rest) {
        return Err(ParseError::new(
            ParseErrorKind::InvalidBody,
            "commit body must be separated from subject by a blank line",
        ));
    }

    let content = trim_leading_blank_lines(rest);
    if content.is_empty() {
        return Ok((None, Vec::new()));
    }

    let lines = LineSpans::new(content).collect::<Vec<_>>();
    let footer_start = find_footer_start(content, &lines);
    let footers = if let Some(start) = footer_start {
        parse_footers(&content[start..])?
    } else {
        Vec::new()
    };
    let body = match footer_start {
        Some(0) => None,
        Some(start) => trimmed_non_empty(&content[..start]),
        None => Some(content),
    };

    Ok((body, footers))
}

fn starts_with_blank_line(input: &str) -> bool {
    input.starts_with('\n') || input.starts_with("\r\n")
}

fn trim_leading_blank_lines(mut input: &str) -> &str {
    loop {
        if let Some(rest) = input.strip_prefix("\r\n") {
            input = rest;
        } else if let Some(rest) = input.strip_prefix('\n') {
            input = rest;
        } else {
            return input;
        }
    }
}

fn trimmed_non_empty(input: &str) -> Option<&str> {
    let trimmed = input.trim_matches(['\n', '\r']);
    (!trimmed.is_empty()).then_some(trimmed)
}

fn find_footer_start(content: &str, lines: &[LineSpan]) -> Option<usize> {
    let mut candidate = None;
    for (index, line) in lines.iter().enumerate() {
        let text = line.text(content);
        if text.trim().is_empty() {
            let Some(next) = next_non_blank_line(content, lines, index + 1) else {
                continue;
            };
            if footer_parts(next.text(content)).is_some()
                && footer_block_is_valid(content, lines, next.start)
                && (candidate.is_none() || next.start == 0)
            {
                candidate = Some(next.start);
            }
        }
    }
    lines
        .first()
        .filter(|line| footer_parts(line.text(content)).is_some())
        .map(|line| line.start)
        .filter(|start| footer_block_is_valid(content, lines, *start))
        .or(candidate)
}

fn next_non_blank_line<'a>(
    content: &str,
    lines: &'a [LineSpan],
    start: usize,
) -> Option<&'a LineSpan> {
    lines[start..]
        .iter()
        .find(|line| !line.text(content).trim().is_empty())
}

fn footer_block_is_valid(content: &str, lines: &[LineSpan], start: usize) -> bool {
    let Some(mut index) = lines.iter().position(|line| line.start == start) else {
        return false;
    };
    let mut saw_footer = false;

    while index < lines.len() {
        let line = lines[index].text(content);
        if line.trim().is_empty() {
            index += 1;
            continue;
        }
        if footer_parts(line).is_some() {
            saw_footer = true;
            index += 1;
            while index < lines.len() {
                let value_line = lines[index].text(content);
                if footer_parts(value_line).is_some() {
                    break;
                }
                index += 1;
            }
        } else {
            return false;
        }
    }

    saw_footer
}

fn parse_footers(input: &str) -> Result<Vec<Footer<'_>>, ParseError> {
    let lines = LineSpans::new(input).collect::<Vec<_>>();
    let mut footers = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        let line = lines[index].text(input);
        if line.trim().is_empty() {
            index += 1;
            continue;
        }
        let Some((token, separator, value_start)) = footer_parts(line) else {
            return Err(ParseError::new(
                ParseErrorKind::InvalidFooter,
                "commit footer is invalid",
            ));
        };
        let value_absolute_start = lines[index].start + value_start;
        let mut value_absolute_end = lines[index].end;
        index += 1;

        while index < lines.len() {
            let next_line = lines[index].text(input);
            if next_line.trim().is_empty() {
                value_absolute_end = lines[index].end;
                index += 1;
                continue;
            }
            if footer_parts(next_line).is_some() {
                break;
            }
            value_absolute_end = lines[index].end;
            index += 1;
        }

        let value = input[value_absolute_start..value_absolute_end].trim_matches(['\n', '\r']);
        if value.trim().is_empty() || matches!(separator, FooterSeparator::Hash) && value == "#" {
            return Err(ParseError::new(
                ParseErrorKind::InvalidFooter,
                "commit footer is invalid",
            ));
        }
        footers.push(Footer {
            token,
            separator,
            value,
        });
    }

    Ok(footers)
}

fn footer_parts(line: &str) -> Option<(&str, FooterSeparator, usize)> {
    if let Some(value_start) = breaking_footer_value_start(line, "BREAKING CHANGE") {
        return Some(("BREAKING CHANGE", FooterSeparator::Colon, value_start));
    }
    if let Some(value_start) = breaking_footer_value_start(line, "BREAKING-CHANGE") {
        return Some(("BREAKING-CHANGE", FooterSeparator::Colon, value_start));
    }

    if let Some(index) = line.find(':') {
        let token = &line[..index];
        let value_start = index + 1;
        if is_valid_footer_token(token) && line[value_start..].starts_with(' ') {
            return Some((token, FooterSeparator::Colon, value_start + 1));
        }
    }
    if let Some(index) = line.find(" #") {
        let token = &line[..index];
        if is_valid_footer_token(token) {
            return Some((token, FooterSeparator::Hash, index + 1));
        }
    }

    None
}

fn breaking_footer_value_start(line: &str, token: &str) -> Option<usize> {
    let rest = line.strip_prefix(token)?.strip_prefix(':')?;
    rest.starts_with(' ').then_some(line.len() - rest.len() + 1)
}

fn is_valid_footer_token(token: &str) -> bool {
    if token.is_empty() || token.trim() != token {
        return false;
    }
    if is_breaking_token(token) {
        return true;
    }
    token
        .chars()
        .all(|character| character.is_alphanumeric() || character == '-')
}

fn conventional_shape_error(kind: ParseErrorKind) -> ParseError {
    ParseError::new(kind, "commit subject must look like '<type>: <summary>'")
}

#[derive(Debug, Clone, Copy)]
struct LineSpan {
    start: usize,
    end: usize,
}

impl LineSpan {
    fn text(self, input: &str) -> &str {
        input[self.start..self.end].trim_end_matches('\r')
    }
}

struct LineSpans<'a> {
    input: &'a str,
    offset: usize,
}

impl<'a> LineSpans<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, offset: 0 }
    }
}

impl Iterator for LineSpans<'_> {
    type Item = LineSpan;

    fn next(&mut self) -> Option<Self::Item> {
        if self.offset >= self.input.len() {
            return None;
        }
        let start = self.offset;
        if let Some(relative_end) = self.input[start..].find('\n') {
            let end = start + relative_end;
            self.offset = end + 1;
            Some(LineSpan { start, end })
        } else {
            self.offset = self.input.len();
            Some(LineSpan {
                start,
                end: self.input.len(),
            })
        }
    }
}
