//! Calculator tool for evaluating arithmetic expressions.

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
    optional_str, required_str,
};
use async_trait::async_trait;
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Debug, Clone, Serialize)]
struct CalculatorResponse {
    value: String,
    result: String,
}

pub struct CalculatorTool;

#[async_trait]
impl ToolSpec for CalculatorTool {
    fn name(&self) -> &'static str {
        "calculator"
    }

    fn description(&self) -> &'static str {
        "Evaluate a basic arithmetic expression."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "expression": { "type": "string" },
                "prefix": { "type": "string" },
                "suffix": { "type": "string" }
            },
            "required": ["expression"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::ReadOnly]
    }

    fn supports_parallel(&self) -> bool {
        true
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    async fn execute(&self, input: Value, _context: &ToolContext) -> Result<ToolResult, ToolError> {
        let expression = required_str(&input, "expression")?;
        let prefix = optional_str(&input, "prefix").unwrap_or("");
        let suffix = optional_str(&input, "suffix").unwrap_or("");

        let value = eval_expression(expression)
            .map_err(|e| ToolError::invalid_input(format!("Invalid expression: {e}")))?;
        if !value.is_finite() {
            return Err(ToolError::invalid_input(
                "Invalid expression: result is not finite".to_string(),
            ));
        }

        let rendered = format_value(value);
        let result = format!("{prefix}{rendered}{suffix}");

        ToolResult::json(&CalculatorResponse {
            value: rendered,
            result,
        })
        .map_err(|e| ToolError::execution_failed(e.to_string()))
    }
}

fn format_value(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{:.0}", value)
    } else {
        let rendered = format!("{value}");
        rendered
    }
}

fn eval_expression(expression: &str) -> std::result::Result<f64, String> {
    let mut parser = ExpressionParser::new(expression);
    let value = parser.parse_expression()?;
    parser.skip_whitespace();
    if !parser.is_eof() {
        return Err(format!("unexpected token at byte {}", parser.position()));
    }
    Ok(value)
}

struct ExpressionParser<'a> {
    input: &'a [u8],
    pos: usize,
}

impl<'a> ExpressionParser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input: input.as_bytes(),
            pos: 0,
        }
    }

    fn position(&self) -> usize {
        self.pos
    }

    fn is_eof(&self) -> bool {
        self.pos >= self.input.len()
    }

    fn skip_whitespace(&mut self) {
        while let Some(ch) = self.peek() {
            if ch.is_ascii_whitespace() {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.pos).copied()
    }

    fn consume(&mut self, ch: u8) -> bool {
        self.skip_whitespace();
        if self.peek() == Some(ch) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn parse_expression(&mut self) -> std::result::Result<f64, String> {
        let mut value = self.parse_term()?;
        loop {
            if self.consume(b'+') {
                value += self.parse_term()?;
            } else if self.consume(b'-') {
                value -= self.parse_term()?;
            } else {
                break;
            }
        }
        Ok(value)
    }

    fn parse_term(&mut self) -> std::result::Result<f64, String> {
        let mut value = self.parse_power()?;
        loop {
            if self.consume(b'*') {
                value *= self.parse_power()?;
            } else if self.consume(b'/') {
                let divisor = self.parse_power()?;
                if divisor == 0.0 {
                    return Err("division by zero".to_string());
                }
                value /= divisor;
            } else if self.consume(b'%') {
                let divisor = self.parse_power()?;
                if divisor == 0.0 {
                    return Err("modulo by zero".to_string());
                }
                value %= divisor;
            } else {
                break;
            }
        }
        Ok(value)
    }

    fn parse_power(&mut self) -> std::result::Result<f64, String> {
        let value = self.parse_unary()?;
        if self.consume(b'^') {
            let exponent = self.parse_power()?;
            Ok(value.powf(exponent))
        } else {
            Ok(value)
        }
    }

    fn parse_unary(&mut self) -> std::result::Result<f64, String> {
        if self.consume(b'+') {
            self.parse_unary()
        } else if self.consume(b'-') {
            Ok(-self.parse_unary()?)
        } else {
            self.parse_primary()
        }
    }

    fn parse_primary(&mut self) -> std::result::Result<f64, String> {
        self.skip_whitespace();
        if self.consume(b'(') {
            let value = self.parse_expression()?;
            if !self.consume(b')') {
                return Err("missing closing ')'".to_string());
            }
            return Ok(value);
        }
        self.parse_number()
    }

    fn parse_number(&mut self) -> std::result::Result<f64, String> {
        self.skip_whitespace();
        let start = self.pos;
        let mut saw_digit = false;
        let mut saw_dot = false;
        let mut saw_exp = false;

        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() {
                saw_digit = true;
                self.pos += 1;
            } else if ch == b'.' && !saw_dot && !saw_exp {
                saw_dot = true;
                self.pos += 1;
            } else if (ch == b'e' || ch == b'E') && saw_digit && !saw_exp {
                saw_exp = true;
                self.pos += 1;
                if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                    self.pos += 1;
                }
            } else {
                break;
            }
        }

        if start == self.pos || !saw_digit {
            return Err(format!("expected number at byte {}", start));
        }

        let number_text = std::str::from_utf8(&self.input[start..self.pos])
            .map_err(|_| format!("invalid UTF-8 near byte {}", start))?;
        number_text
            .parse::<f64>()
            .map_err(|_| format!("invalid number '{number_text}'"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_expression() {
        let value = eval_expression("2 + 2").expect("expression should parse");
        assert_eq!(format_value(value), "4");
    }

    #[test]
    fn handles_precedence_and_parentheses() {
        let value = eval_expression("2 + 3 * (4 - 1)").expect("expression should parse");
        assert_eq!(format_value(value), "11");
    }

    #[test]
    fn rejects_invalid_input() {
        let err = eval_expression("2 +").expect_err("invalid expression should fail");
        assert!(err.contains("expected number"));
    }
}
