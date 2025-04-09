use std::sync::LazyLock;

use regex::Regex;
use rust_decimal::{Decimal, prelude::Zero};

static DATE_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^(\d{4}-\d{2}-\d{2})(?:[ \t\n\r]|$)"#).expect("hard coded regex is valid")
});

static AMOUNT_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^(-?(?:\d{1,3}(?:,\d{3})*|\d+)(?:\.\d+)?)[ \t]*([A-Z]{3,})(?:[ \t\n\r]|$)"#)
        .expect("hard coded regex is valid")
});

static WHITESPACE_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^[ \t]+"#).expect("hard coded regex is valid"));

static OPTION_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^(option\s+"[^"]+"\s+"[^"]+")(?:[ \t\n\r]|$)"#)
        .expect("hard coded regex is valid")
});

static DIRECTIVE_OPEN_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^(open)(?:[ \t\n\r]|$)"#).expect("hard coded regex is valid"));

static DIRECTIVE_POST_TX_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^(\*)(?:[ \t\n\r]|$)"#).expect("hard coded regex is valid"));

static ACCOUNT_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^(Assets|Liabilities|Expenses|Income|Equity):([A-Z][A-Za-z]+)(?:[ \t\n\r]|$)"#)
        .expect("hard coded regex is valid")
});

static CURRENCY_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^([A-Z]+)(?:[ \t\n\r]|$)"#).expect("hard coded regex is valid"));

// we consume the newline at the end of a comment because it's part of the definition
static COMMENT_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^;.*(?:\r?\n|$)"#).expect("hard coded regex is valid"));

static TX_DESCRIPTION_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^("[^"]+")(?:[ \t\n\r]|$)"#).expect("hard coded regex is valid")
});

static AT_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^(@)(?:[ \t\n\r]|$)"#).expect("hard coded regex is valid"));

static NEWLINE_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^\r?\n"#).expect("hard coded regex is valid"));

// When something goes wrong we show (up to) this many characters of the remaining unparsed input
const MAX_ERROR_SAMPLE: usize = 10;

#[derive(Debug, PartialEq)]
pub struct TokenizeError(String);

impl std::fmt::Display for TokenizeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TokenizeError {}

#[derive(Debug, PartialEq)]
pub struct Amount {
    currency: String,
    amount: Decimal,
}

#[derive(Debug, PartialEq)]
pub enum Token {
    Date(String),
    Amount(Amount),
    DirectiveOpen,
    DirectivePostTx,
    Account(Account),
    Currency(String),
    At,
    Newline,
    OptionLine,
}

#[derive(Debug, PartialEq)]
pub enum Account {
    Equity(String),
    Liabilities(String),
    Assets(String),
    Income(String),
    Expenses(String),
}

pub struct Tokenizer {
    buffer: String,
    // The cursor represents a position between characters, not the character itself. For a string of
    // length n there are n+1 valid cursor positions.
    // - The cursor at position 0 refers to the start of the buffer (before the first character).
    // - A cursor at position n refers to the position after the n-1th character (zero-indexed) and before the nth character.
    // - A cursor at the end of the buffer (i.e., cursor == buffer.len()) is valid and refers to the position after the last character.
    // Characters before the cursor have been processed those after have not.
    cursor: usize,
}

impl Tokenizer {
    pub fn new(raw: String) -> Self {
        Tokenizer {
            buffer: raw,
            cursor: 0,
        }
    }

    pub fn next_token(&mut self) -> Result<Option<Token>, TokenizeError> {
        if self.cursor >= self.buffer.len() {
            Ok(None)
        } else if let Some(whitespace) = WHITESPACE_REGEX.find(&self.buffer[self.cursor..]) {
            self.cursor += whitespace.end();
            self.next_token()
        } else if let Some(option_line) =
            OPTION_REGEX.captures(&self.buffer[self.cursor..]).map(|c| {
                c.get(1).expect(
                    "if the entire regex matches then the first capture group will not be None",
                )
            })
        {
            self.cursor += option_line.end();
            Ok(Some(Token::OptionLine))
        } else if let Some(comment) = COMMENT_REGEX.find(&self.buffer[self.cursor..]) {
            // we ignore comments
            self.cursor += comment.end();
            self.next_token()
        } else if let Some(date) = DATE_REGEX.captures(&self.buffer[self.cursor..]).map(|c| {
            c.get(1)
                .expect("if the entire regex matches then the first capture group will not be None")
        }) {
            self.cursor += date.end();
            let date = date.as_str();
            Ok(Some(Token::Date(date.to_string())))
        } else if let Some(captures) = AMOUNT_REGEX.captures(&self.buffer[self.cursor..]) {
            let amount = captures
                .get(1)
                .expect("if there was a match there will be a 1 capture group");
            let currency = captures
                .get(2)
                .expect("if there was a match there will be a 1 capture group");
            let Ok(amount) = amount.as_str().replace(",", "").parse() else {
                // the regex accepts commas e.g. 9,000 which won't parse so we strip them out
                let (line, column) = self.current_line_column();
                return Err(TokenizeError(format!(
                    "decimal {} on line {}, column {} has too many digits",
                    amount.as_str(),
                    line,
                    column
                )));
            };
            self.cursor += currency.end();
            Ok(Some(Token::Amount(Amount {
                currency: currency.as_str().to_string(),
                amount,
            })))
        } else if let Some(directive_open) = DIRECTIVE_OPEN_REGEX
            .captures(&self.buffer[self.cursor..])
            .map(|c| {
                c.get(1).expect(
                    "if the entire regex matches then the first capture group will not be None",
                )
            })
        {
            self.cursor += directive_open.end();
            Ok(Some(Token::DirectiveOpen))
        } else if let Some(directive_post_tx) = DIRECTIVE_POST_TX_REGEX
            .captures(&self.buffer[self.cursor..])
            .map(|c| {
                c.get(1).expect(
                    "if the entire regex matches then the first capture group will not be None",
                )
            })
        {
            self.cursor += directive_post_tx.end();
            Ok(Some(Token::DirectivePostTx))
        } else if let Some(full_account) = ACCOUNT_REGEX.captures(&self.buffer[self.cursor..]) {
            let acct_type = full_account
                .get(1)
                .expect("if there was a match there will be a 1 capture group")
                .as_str();

            let acct_name = full_account
                .get(2)
                .expect("if there was a match there will be a 2 capture group");

            self.cursor += acct_name.end();
            let acct_name = acct_name.as_str();

            match acct_type {
                "Assets" => Ok(Some(Token::Account(Account::Assets(acct_name.to_string())))),
                "Equity" => Ok(Some(Token::Account(Account::Equity(acct_name.to_string())))),
                "Income" => Ok(Some(Token::Account(Account::Income(acct_name.to_string())))),
                "Expenses" => Ok(Some(Token::Account(Account::Expenses(
                    acct_name.to_string(),
                )))),
                "Liabilities" => Ok(Some(Token::Account(Account::Liabilities(
                    acct_name.to_string(),
                )))),
                _ => unreachable!("the regex forces one of the above"),
            }
        } else if let Some(currency) =
            CURRENCY_REGEX
                .captures(&self.buffer[self.cursor..])
                .map(|c| {
                    c.get(1).expect(
                        "if the entire regex matches then the first capture group will not be None",
                    )
                })
        {
            self.cursor += currency.end();
            Ok(Some(Token::Currency(currency.as_str().to_string())))
        } else if let Some(tx_description) = TX_DESCRIPTION_REGEX
            .captures(&self.buffer[self.cursor..])
            .map(|c| {
                c.get(1).expect(
                    "if the entire regex matches then the first capture group will not be None",
                )
            })
        {
            // we ignore tx descriptions TODO: don't ignore
            self.cursor += tx_description.end();
            self.next_token()
        } else if let Some(at) = AT_REGEX.captures(&self.buffer[self.cursor..]).map(|c| {
            c.get(1)
                .expect("if the entire regex matches then the first capture group will not be None")
        }) {
            self.cursor += at.end();
            Ok(Some(Token::At))
        } else if let Some(newline) = NEWLINE_REGEX.find(&self.buffer[self.cursor..]) {
            self.cursor += newline.end();
            Ok(Some(Token::Newline))
        } else {
            let end = self.cursor + MAX_ERROR_SAMPLE;
            let (endstr, end) = if end < self.buffer.len() {
                ("...", end)
            } else {
                ("", self.buffer.len())
            };
            let (line, column) = self.current_line_column();
            Err(TokenizeError(format!(
                "unexpected character sequence {}{} on line {}, column {}",
                &self.buffer[self.cursor..end],
                endstr,
                line,
                column
            )))
        }
    }

    /// Returns the current one-indexed line number and column of the cursor as a (line_number, column) tuple.
    fn current_line_column(&self) -> (usize, usize) {
        (self.current_line(), self.current_column())
    }

    /// Returns the current one-indexed line number of the cursor. If the cursor is at the start
    /// of the buffer then it's on line one. Every newline character represents an increment of one
    /// in the line count - this includes cases where the last character is a newline and the
    /// cursor increments to the end of the buffer.
    fn current_line(&self) -> usize {
        if self.cursor.is_zero() {
            // If the cursor is at zero i.e. the start of the buffer then below we would calculate
            // "".lines().count() as zero. However we want to count the start of the buffer as line
            // one.
            1
        } else {
            let lines = self.buffer[0..self.cursor].lines().count();

            // There's a nasty edge case here. If the string ends in a newline then the lines()
            // method doesn't return an empty line after the last newline. This means the line
            // count is one less than we expect (since the cursor is beyond the last newline we
            // want to count it as being on the next line) e.g.
            // "hello\n".lines().count() returns one whereas we expect this to count as two lines.
            // We check for this case and fix accordingly.
            if self.buffer[0..self.cursor].ends_with("\n") {
                lines + 1
            } else {
                lines
            }
        }
    }

    /// Returns the one-indexed column of the cursor. If the cursor is at the start of the buffer then
    /// it's at column one. Each increment of the cursor position increments the column number.
    /// When a newline character is crossed the column counter resets to one.
    fn current_column(&self) -> usize {
        if self.cursor.is_zero() {
            // If the cursor is at zero then `line_start` below would be zero and so the returned
            // value would also be zero. However if the cursor is at the start of the buffer then
            // it's by definition at column one. So we override for this edge case.
            1
        } else {
            // Find the start index of the current line.
            let line_start_index = self.buffer[..self.cursor].rfind('\n').unwrap_or(0); // or start of buffer
            // If the cursor has value n then the character before the cursor has index n-1
            // (think of the base case where the cursor has value one then the single character
            // before it has index zero). So if the cursor is immediately to the right of a newline
            // character the line_start_index will be one less than the cursor value. Subtraction
            // would then give a value of one which is the correct column value.
            self.cursor - line_start_index
        }
    }

    #[cfg(test)]
    fn set_cursor(&mut self, pos: usize) {
        self.cursor = pos;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let raw = r#"option "operating_currency" "GBP"

2023-02-01 open Equity:RetainedEarnings              GBP

;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;;
; An account entry

2023-02-03 * "Transaction description"
  Assets:AnAsset                                   12 USD @ 0.82 GBP
  Income:SomeIncome                                     -9,000.84 GBP"#;

        let mut tokenizer = Tokenizer::new(raw.to_string());

        assert_eq!(tokenizer.next_token(), Ok(Some(Token::OptionLine)),);

        assert_eq!(tokenizer.next_token(), Ok(Some(Token::Newline)),);

        assert_eq!(tokenizer.next_token(), Ok(Some(Token::Newline)),);

        assert_eq!(
            tokenizer.next_token(),
            Ok(Some(Token::Date("2023-02-01".to_string())))
        );

        assert_eq!(tokenizer.next_token(), Ok(Some(Token::DirectiveOpen)));

        assert_eq!(
            tokenizer.next_token(),
            Ok(Some(Token::Account(Account::Equity(
                "RetainedEarnings".to_string()
            ))))
        );

        assert_eq!(
            tokenizer.next_token(),
            Ok(Some(Token::Currency("GBP".to_string())))
        );

        assert_eq!(tokenizer.next_token(), Ok(Some(Token::Newline)),);

        assert_eq!(tokenizer.next_token(), Ok(Some(Token::Newline)),);

        assert_eq!(tokenizer.next_token(), Ok(Some(Token::Newline)),);

        assert_eq!(
            tokenizer.next_token(),
            Ok(Some(Token::Date("2023-02-03".to_string())))
        );

        assert_eq!(tokenizer.next_token(), Ok(Some(Token::DirectivePostTx)));

        assert_eq!(tokenizer.next_token(), Ok(Some(Token::Newline)),);

        assert_eq!(
            tokenizer.next_token(),
            Ok(Some(Token::Account(Account::Assets("AnAsset".to_string()))))
        );

        assert_eq!(
            tokenizer.next_token(),
            Ok(Some(Token::Amount(Amount {
                currency: "USD".to_string(),
                amount: "12".parse().expect("hard coded value is a valid decimal")
            })))
        );

        assert_eq!(tokenizer.next_token(), Ok(Some(Token::At)));

        assert_eq!(
            tokenizer.next_token(),
            Ok(Some(Token::Amount(Amount {
                currency: "GBP".to_string(),
                amount: "0.82".parse().expect("hard coded value is a valid decimal")
            })))
        );

        assert_eq!(tokenizer.next_token(), Ok(Some(Token::Newline)),);

        assert_eq!(
            tokenizer.next_token(),
            Ok(Some(Token::Account(Account::Income(
                "SomeIncome".to_string()
            ))))
        );

        assert_eq!(
            tokenizer.next_token(),
            Ok(Some(Token::Amount(Amount {
                currency: "GBP".to_string(),
                amount: "-9000.84"
                    .parse()
                    .expect("hard coded value is a valid decimal")
            })))
        );

        assert!(
            tokenizer
                .next_token()
                .expect("cursor is at the end of file so it should return with no error")
                .is_none()
        );
    }

    #[test]
    fn ensure_error_when_decimal_too_many_digits() {
        let mut tokenizer = Tokenizer::new("79228162514264337593543950336GBP".to_string());

        assert_eq!(
            tokenizer.next_token().unwrap_err(),
            TokenizeError(
                "decimal 79228162514264337593543950336 on line 1, column 1 has too many digits"
                    .to_string()
            )
        )
    }

    #[test]
    fn ensure_followed_by_whitespace() {
        // Ensures that tokens are followed by whitespace.
        let mut tokenizer = Tokenizer::new("792281USDopen".to_string());
        assert_eq!(
            tokenizer.next_token().unwrap_err(),
            TokenizeError(
                "unexpected character sequence 792281USDo... on line 1, column 1".to_string()
            )
        );

        let mut tokenizer = Tokenizer::new("2023-02-01open".to_string());
        assert_eq!(
            tokenizer.next_token().unwrap_err(),
            TokenizeError(
                "unexpected character sequence 2023-02-01... on line 1, column 1".to_string()
            )
        );

        let mut tokenizer = Tokenizer::new("openX".to_string());
        assert_eq!(
            tokenizer.next_token().unwrap_err(),
            TokenizeError("unexpected character sequence openX on line 1, column 1".to_string())
        );

        let mut tokenizer = Tokenizer::new(r#"option "operating_currency" "GBP"X"#.to_string());
        assert_eq!(
            tokenizer.next_token().unwrap_err(),
            TokenizeError(
                "unexpected character sequence option \"op... on line 1, column 1".to_string()
            )
        );

        let mut tokenizer = Tokenizer::new("@X".to_string());
        assert_eq!(
            tokenizer.next_token().unwrap_err(),
            TokenizeError("unexpected character sequence @X on line 1, column 1".to_string())
        );
    }

    #[test]
    fn account_name_capitalized() {
        let mut tokenizer = Tokenizer::new(r#"Assets:nOtCapitalized"#.to_string());
        assert_eq!(
            tokenizer.next_token().unwrap_err(),
            TokenizeError(
                "unexpected character sequence Assets:nOt... on line 1, column 1".to_string()
            )
        );
    }

    #[test]
    fn check_error_msg_simple() {
        // very basic check of error message
        let mut tokenizer = Tokenizer::new("@123sds".to_string());

        assert_eq!(
            tokenizer.next_token().unwrap_err(),
            TokenizeError("unexpected character sequence @123sds on line 1, column 1".to_string())
        )
    }

    #[test]
    fn ensure_error_msg_truncated() {
        // here we ensure that the code sample displayed in the error message is truncated properly
        // in cases where the remaining unparsed string is longer than MAX_ERROR_SAMPLE
        let mut tokenizer = Tokenizer::new("1.2345GBP \n @123sdsabcd".to_string());
        tokenizer
            .next_token()
            .expect("the 1.2345GBP should parse ok");

        tokenizer
            .next_token()
            .expect("the newline and whitespace should parse ok");

        assert_eq!(
            tokenizer.next_token().unwrap_err(),
            TokenizeError(
                "unexpected character sequence @123sdsabc... on line 2, column 2".to_string()
            )
        )
    }

    #[test]
    fn test_cursor_position() {
        let mut tokenizer = Tokenizer::new("".to_string());
        tokenizer.set_cursor(0);
        assert_eq!(tokenizer.current_line_column(), (1, 1));

        let mut tokenizer = Tokenizer::new("hello".to_string());
        tokenizer.set_cursor(0);
        assert_eq!(tokenizer.current_line_column(), (1, 1));

        let mut tokenizer = Tokenizer::new("hello\n\n".to_string());
        tokenizer.set_cursor(7);
        assert_eq!(tokenizer.current_line_column(), (3, 1));

        let mut tokenizer = Tokenizer::new("hello\n\n".to_string());
        tokenizer.set_cursor(6);
        assert_eq!(tokenizer.current_line_column(), (2, 1));

        let mut tokenizer = Tokenizer::new("hello\nworld".to_string());
        tokenizer.set_cursor(7);
        assert_eq!(tokenizer.current_line_column(), (2, 2));
    }
}
