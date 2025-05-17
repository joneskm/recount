use std::fmt::Display;

use crate::{
    accounts_doc::{Account, AccountsDocument, ConversionPosting, Posting, RegularPosting},
    tokenizer::{Token, TokenKind, TokenizeError},
    types::AccountId,
};

macro_rules! expect_token {
    ($tokenizer:expr, $pattern:pat => $binding:expr, $err_msg:expr, $line:ident, $column:ident) => {{
        let Some(token) = $tokenizer.next().transpose()? else {
            return Err(ParseError {
                msg: $err_msg.to_string(),
                line: $line,
                column: $column,
            });
        };

        let $pattern = token.kind else {
            return Err(ParseError {
                msg: $err_msg.to_string(),
                line: token.line,
                column: token.column,
            });
        };

        ($binding, token.line, token.column)
    }};
}

#[derive(Debug, PartialEq)]
pub struct ParseError {
    msg: String,
    line: usize,
    column: usize,
}

impl Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.msg)
    }
}

impl From<TokenizeError> for ParseError {
    fn from(error: TokenizeError) -> Self {
        ParseError {
            msg: error.msg,
            line: error.line,
            column: error.column,
        }
    }
}

impl std::error::Error for ParseError {}

pub fn parse(
    tokenizer: impl IntoIterator<Item = Result<Token, TokenizeError>>,
) -> Result<AccountsDocument, ParseError> {
    let mut tokenizer = tokenizer.into_iter();
    let mut accounts_doc = AccountsDocument::new();

    //TODO: handle newlines at start of file
    let line = 0;
    let column = 0;
    let (_, _, _) = expect_token!(
        tokenizer,
        TokenKind::OptionLine => (),
        "expected option line",
        line,
        column
    );

    // transactions or open directive loop
    'tx_open_loop: while let Some(token) = tokenizer.next().transpose()? {
        if token.kind == TokenKind::Newline {
            continue;
        }

        let TokenKind::Date(date) = token.kind else {
            return Err(ParseError {
                msg: "expected date".to_string(),
                line: token.line,
                column: token.column,
            });
        };

        match tokenizer.next().transpose()? {
            Some(Token {
                kind: TokenKind::DirectiveOpen,
                line,
                column,
            }) => {
                let (account, line, column) = expect_token!(
                    tokenizer,
                    TokenKind::Account(account) => account,
                    "expected account",
                    line,
                    column
                );

                let (currency, _, _) = expect_token!(
                        tokenizer,
                        TokenKind::Currency(currency) => currency,
                        "expected amount",
                        line,
                        column
                );

                accounts_doc
                    .open_an_account(Account {
                        id: AccountId {
                            name: account.name,
                            type_: account.type_,
                        },
                        currency,
                        opening_date: date.parse().unwrap(), //TODO: unwrap
                    })
                    .map_err(|_| ParseError {
                        msg: "account already exists".to_string(),
                        line: 0,
                        column: 0,
                    })?;

                // The account opening is now complete. We either have a newline or we've reached
                // the end of the file. Any other token is an error.

                if let Some(token) = tokenizer.next().transpose()? {
                    if token.kind != TokenKind::Newline {
                        return Err(ParseError {
                            msg: "expected newline".to_string(),
                            line: token.line,
                            column: token.column,
                        });
                    }
                } else {
                    // We've reached the end of the file. This is OK since we've parsed a complete account
                    // open directive.
                    break 'tx_open_loop;
                };
            }
            Some(Token {
                kind: TokenKind::DirectivePostTx,
                line,
                column,
            }) => {
                let (_, line, column) = expect_token!(
                    tokenizer,
                    TokenKind::TxDescription => (),
                    "expected tx description",
                    line,
                    column
                );

                let (_, line, column) = expect_token!(
                    tokenizer,
                    TokenKind::Newline => (),
                    "expected newline",
                    line,
                    column
                );

                let mut postings = vec![];

                'posts_loop: while let Some(token) = tokenizer.next().transpose()? {
                    if token.kind == TokenKind::Newline {
                        // We've reached the end of the postings. We'll add the transaction to the
                        // accounts document after the loop.
                        break 'posts_loop;
                    }
                    let TokenKind::Account(account_id) = token.kind else {
                        return Err(ParseError {
                            msg: "expected account".to_string(),
                            line: token.line,
                            column: token.column,
                        });
                    };

                    let (amount, line, column) = expect_token!(
                        tokenizer,
                        TokenKind::Amount(amount) => amount,
                        "expected amount",
                        line,
                        column
                    );

                    match tokenizer.next().transpose()? {
                        Some(Token {
                            kind: TokenKind::Newline,
                            ..
                        }) => {
                            // This posting has no conversion, we can add it to the transaction
                            // and move on.

                            postings.push(Posting::Regular(RegularPosting {
                                account_id,
                                amount: amount.amount,
                                currency: amount.currency,
                            }));
                            continue;
                        }
                        Some(Token {
                            kind: TokenKind::At,
                            line,
                            column,
                        }) => {
                            let (conversion, _, _) = expect_token!(
                                tokenizer,
                                TokenKind::Amount(conversion) => conversion,
                                "expected amount",
                                line,
                                column
                            );

                            if let Some(token) = tokenizer.next().transpose()? {
                                if token.kind != TokenKind::Newline {
                                    return Err(ParseError {
                                        msg: "expected newline".to_string(),
                                        line: token.line,
                                        column: token.column,
                                    });
                                }
                                postings.push(Posting::Conversion(ConversionPosting {
                                    account_id,
                                    account_amount: amount.amount,
                                    account_currency: amount.currency,
                                    tx_currency: conversion.currency,
                                    rate: conversion.amount,
                                }));

                                continue;
                            } else {
                                // We've reached the end of the file. This is OK since we've parsed
                                // a complete posting.

                                postings.push(Posting::Conversion(ConversionPosting {
                                    account_id,
                                    account_amount: amount.amount,
                                    account_currency: amount.currency,
                                    tx_currency: conversion.currency,
                                    rate: conversion.amount,
                                }));

                                accounts_doc
                                    .add_transaction(date.parse().unwrap(), "todo", postings)
                                    .unwrap(); //TODO: unwraps + tx description
                                break 'tx_open_loop;
                            };
                        }
                        None => {
                            // We've reached the end of the file. This is OK since we've parsed a
                            // complete posting.

                            postings.push(Posting::Regular(RegularPosting {
                                account_id,
                                amount: amount.amount,
                                currency: amount.currency,
                            }));
                            accounts_doc
                                .add_transaction(date.parse().unwrap(), "todo", postings)
                                .unwrap(); //TODO: tx description + unwraps
                            break 'tx_open_loop;
                        }
                        _ => {
                            return Err(ParseError {
                                msg: "expected newline, end of file or @".to_string(),
                                line,
                                column,
                            });
                        }
                    }
                }
                accounts_doc
                    .add_transaction(date.parse().unwrap(), "todo", postings)
                    .unwrap(); //TODO: unwraps + description
            }
            _ => {
                // `None` (end of file) or any other token (open or create transaction are covered by the match
                // branches above) is an error
                // (this is because we've parsed a date up to this point).
                return Err(ParseError {
                    msg: "expected either open or post transaction directive".to_string(),
                    line,
                    column,
                });
            }
        }
    }

    Ok(accounts_doc)
}

#[cfg(test)]
mod tests {
    use date::date;
    use rust_decimal::Decimal;

    use crate::{types::AccountType, types::Amount};

    use super::*;

    #[test]
    fn happy_path() {
        let mut tokens = vec![];
        tokens.push(Ok(Token {
            kind: TokenKind::OptionLine,
            line: 0,
            column: 0,
        }));
        tokens.push(Ok(Token {
            kind: TokenKind::Newline,
            line: 0,
            column: 0,
        }));
        add_open_account_tokens(
            &mut tokens,
            "1912-01-12",
            AccountType::Asset,
            "account name",
            "GBP",
        );
        tokens.push(Ok(Token {
            kind: TokenKind::Newline,
            line: 0,
            column: 0,
        }));
        add_open_account_tokens(
            &mut tokens,
            "1912-01-12",
            AccountType::Asset,
            "another account",
            "GBP",
        );
        tokens.push(Ok(Token {
            kind: TokenKind::Newline,
            line: 0,
            column: 0,
        }));
        add_open_account_tokens(
            &mut tokens,
            "1912-01-12",
            AccountType::Asset,
            "yet another account",
            "EUR",
        );
        tokens.push(Ok(Token {
            kind: TokenKind::Newline,
            line: 0,
            column: 0,
        }));
        tokens.push(Ok(Token {
            kind: TokenKind::Newline,
            line: 0,
            column: 0,
        }));
        add_tx_declaration_tokens(&mut tokens, "1912-01-12");
        tokens.push(Ok(Token {
            kind: TokenKind::Newline,
            line: 0,
            column: 0,
        }));
        add_post_tokens(
            &mut tokens,
            "6.45".parse().expect("hard coded value will parse"),
            AccountType::Asset,
            "another account",
            "GBP",
        );
        tokens.push(Ok(Token {
            kind: TokenKind::Newline,
            line: 0,
            column: 0,
        }));
        add_post_tokens(
            &mut tokens,
            "-3.45".parse().expect("hard coded value will parse"),
            AccountType::Asset,
            "account name",
            "GBP",
        );
        tokens.push(Ok(Token {
            kind: TokenKind::Newline,
            line: 0,
            column: 0,
        }));
        add_conversion_post_tokens(
            &mut tokens,
            "-1.5".parse().expect("hard coded value will parse"),
            AccountType::Asset,
            "yet another account",
            "EUR",
            "2".parse().expect("hard coded value will parse"),
            "GBP",
        );

        let accts = parse(tokens).expect("the test token sequence is valid");

        assert_eq!(
            accts.accounts,
            vec![
                Account {
                    id: AccountId {
                        name: "account name".to_string(),
                        type_: crate::types::AccountType::Asset
                    },
                    currency: "GBP".to_string(),
                    opening_date: date! {1912-01-12}
                },
                Account {
                    id: AccountId {
                        name: "another account".to_string(),
                        type_: crate::types::AccountType::Asset
                    },
                    currency: "GBP".to_string(),
                    opening_date: date! {1912-01-12}
                },
                Account {
                    id: AccountId {
                        name: "yet another account".to_string(),
                        type_: crate::types::AccountType::Asset
                    },
                    currency: "EUR".to_string(),
                    opening_date: date! {1912-01-12}
                }
            ]
        );

        let mut balances = accts.balances();

        assert_eq!(
            balances.next(),
            Some((
                &AccountId {
                    name: "account name".to_string(),
                    type_: AccountType::Asset,
                },
                Amount {
                    amount: "-3.45".parse().expect("hard coded value will parse"),
                    currency: "GBP".to_string(),
                }
            ))
        );

        assert_eq!(
            balances.next(),
            Some((
                &AccountId {
                    name: "another account".to_string(),
                    type_: AccountType::Asset,
                },
                Amount {
                    amount: "6.45".parse().expect("hard coded value will parse"),
                    currency: "GBP".to_string(),
                }
            ))
        );

        assert_eq!(
            balances.next(),
            Some((
                &AccountId {
                    name: "yet another account".to_string(),
                    type_: AccountType::Asset,
                },
                Amount {
                    amount: "-1.5".parse().expect("hard coded value will parse"),
                    currency: "EUR".to_string(),
                }
            ))
        );

        assert_eq!(balances.next(), None);
    }

    fn add_open_account_tokens(
        tokens: &mut Vec<Result<Token, TokenizeError>>,
        date: impl Into<String>,
        account_type: AccountType,
        account_name: impl Into<String>,
        currency: impl Into<String>,
    ) {
        let mut open = vec![
            Ok(Token {
                kind: TokenKind::Date(date.into()),
                line: 0,
                column: 0,
            }),
            Ok(Token {
                kind: TokenKind::DirectiveOpen,
                line: 0,
                column: 0,
            }),
            Ok(Token {
                kind: TokenKind::Account(crate::types::AccountId {
                    type_: account_type,
                    name: account_name.into(),
                }),
                line: 0,
                column: 0,
            }),
            Ok(Token {
                kind: TokenKind::Currency(currency.into()),
                line: 0,
                column: 0,
            }),
        ];

        tokens.append(&mut open);
    }

    fn add_tx_declaration_tokens(
        tokens: &mut Vec<Result<Token, TokenizeError>>,
        date: impl Into<String>,
    ) {
        let mut tx_declare = vec![
            Ok(Token {
                kind: TokenKind::Date(date.into()),
                line: 0,
                column: 0,
            }),
            Ok(Token {
                kind: TokenKind::DirectivePostTx,
                line: 0,
                column: 0,
            }),
            Ok(Token {
                kind: TokenKind::TxDescription,
                line: 0,
                column: 0,
            }),
        ];

        tokens.append(&mut tx_declare);
    }

    fn add_post_tokens(
        tokens: &mut Vec<Result<Token, TokenizeError>>,
        amount: Decimal,
        account_type: AccountType,
        account_name: impl Into<String>,
        currency: impl Into<String>,
    ) {
        let mut open = vec![
            Ok(Token {
                kind: TokenKind::Account(crate::types::AccountId {
                    type_: account_type,
                    name: account_name.into(),
                }),
                line: 0,
                column: 0,
            }),
            Ok(Token {
                kind: TokenKind::Amount(Amount {
                    currency: currency.into(),
                    amount,
                }),
                line: 0,
                column: 0,
            }),
        ];

        tokens.append(&mut open);
    }

    fn add_conversion_post_tokens(
        tokens: &mut Vec<Result<Token, TokenizeError>>,
        amount: Decimal,
        account_type: AccountType,
        account_name: impl Into<String>,
        currency: impl Into<String>,
        conversion_amount: Decimal,
        conversion_currency: impl Into<String>,
    ) {
        let mut open = vec![
            Ok(Token {
                kind: TokenKind::Account(crate::types::AccountId {
                    type_: account_type,
                    name: account_name.into(),
                }),
                line: 0,
                column: 0,
            }),
            Ok(Token {
                kind: TokenKind::Amount(Amount {
                    currency: currency.into(),
                    amount,
                }),
                line: 0,
                column: 0,
            }),
            Ok(Token {
                kind: TokenKind::At,
                line: 0,
                column: 0,
            }),
            Ok(Token {
                kind: TokenKind::Amount(Amount {
                    currency: conversion_currency.into(),
                    amount: conversion_amount,
                }),
                line: 0,
                column: 0,
            }),
        ];

        tokens.append(&mut open);
    }
}
