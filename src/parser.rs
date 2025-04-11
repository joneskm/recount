use std::fmt::Display;

use crate::{
    accounts_doc::{Account, AccountsDocument, CurrencyConverter, Posting, Transaction},
    tokenizer::{Token, TokenizeError},
    types::AccountId,
};

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
    tokenizer: impl IntoIterator<Item = Result<Token, ParseError>>,
) -> Result<AccountsDocument, ParseError> {
    let mut tokenizer = tokenizer.into_iter();
    let mut accounts_doc = AccountsDocument::new();

    // transactions or open directive loop
    'tx_open_loop: while let Some(token) = tokenizer.next().transpose()? {
        if token == Token::Newline {
            continue;
        }

        let Token::Date(date) = token else {
            return Err(ParseError {
                msg: "expected date".to_string(),
                line: 0,
                column: 0,
            });
        };

        match tokenizer.next().transpose()? {
            Some(Token::DirectiveOpen) => {
                let Some(Token::Account(account)) = tokenizer.next().transpose()? else {
                    return Err(ParseError {
                        msg: "expected account".to_string(),
                        line: 0,
                        column: 0,
                    });
                };

                let Some(Token::Currency(currency)) = tokenizer.next().transpose()? else {
                    return Err(ParseError {
                        msg: "expected amount".to_string(),
                        line: 0,
                        column: 0,
                    });
                };

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
                    if token != Token::Newline {
                        return Err(ParseError {
                            msg: "expected newline".to_string(),
                            line: 0,
                            column: 0,
                        });
                    }
                } else {
                    // We've reached the end of the file. This is OK since we've parsed a complete account
                    // open directive.
                    break 'tx_open_loop;
                };
            }
            Some(Token::DirectivePostTx) => {
                if Some(Token::TxDescription) != tokenizer.next().transpose()? {
                    return Err(ParseError {
                        msg: "expected tx description".to_string(),
                        line: 0,
                        column: 0,
                    });
                };

                if Some(Token::Newline) != tokenizer.next().transpose()? {
                    return Err(ParseError {
                        msg: "expected newline".to_string(),
                        line: 0,
                        column: 0,
                    });
                };

                let mut transaction = Transaction::new(date.parse().unwrap(), "todo"); // TODO:unwrap + description

                'posts_loop: while let Some(token) = tokenizer.next().transpose()? {
                    if token == Token::Newline {
                        // We've reached the end of the postings. We'll add the transaction to the
                        // accounts document after the loop.
                        break 'posts_loop;
                    }
                    let Token::Account(account_id) = token else {
                        return Err(ParseError {
                            msg: "expected account".to_string(),
                            line: 0,
                            column: 0,
                        });
                    };

                    let Some(Token::Amount(amount)) = tokenizer.next().transpose()? else {
                        return Err(ParseError {
                            msg: "expected amount".to_string(),
                            line: 0,
                            column: 0,
                        });
                    };

                    match tokenizer.next().transpose()? {
                        Some(Token::Newline) => {
                            // This posting has no conversion, we can add it to the transaction
                            // and move on.

                            transaction
                                .add_posting(Posting {
                                    account_id,
                                    amount: amount.amount,
                                    currency: amount.currency,
                                    converter: None,
                                })
                                .unwrap(); // TODO: unwrap

                            continue;
                        }
                        Some(Token::At) => {
                            let Some(Token::Amount(conversion)) = tokenizer.next().transpose()?
                            else {
                                return Err(ParseError {
                                    msg: "expected amount".to_string(),
                                    line: 0,
                                    column: 0,
                                });
                            };
                            if let Some(token) = tokenizer.next().transpose()? {
                                if token != Token::Newline {
                                    return Err(ParseError {
                                        msg: "expected newline".to_string(),
                                        line: 0,
                                        column: 0,
                                    });
                                }
                                transaction
                                    .add_posting(Posting {
                                        account_id,
                                        amount: amount.amount,
                                        currency: amount.currency,
                                        converter: Some(CurrencyConverter {
                                            currency: conversion.currency,
                                            rate: conversion.amount,
                                        }),
                                    })
                                    .unwrap(); // TODO: unwrap

                                continue;
                            } else {
                                // We've reached the end of the file. This is OK since we've parsed
                                // a complete posting.
                                //
                                transaction
                                    .add_posting(Posting {
                                        account_id,
                                        amount: amount.amount,
                                        currency: amount.currency,
                                        converter: Some(CurrencyConverter {
                                            currency: conversion.currency,
                                            rate: conversion.amount,
                                        }),
                                    })
                                    .unwrap(); // TODO: unwrap
                                accounts_doc.add_transaction(transaction).unwrap(); //TODO: unwrap
                                break 'tx_open_loop;
                            };
                        }
                        None => {
                            // We've reached the end of the file. This is OK since we've parsed a
                            // complete posting.
                            transaction
                                .add_posting(Posting {
                                    account_id,
                                    amount: amount.amount,
                                    currency: amount.currency,
                                    converter: None,
                                })
                                .unwrap(); // TODO: unwrap
                            accounts_doc.add_transaction(transaction).unwrap(); //TODO: unwrap
                            break 'tx_open_loop;
                        }
                        _ => {
                            return Err(ParseError {
                                msg: "expected newline, end of file or @".to_string(),
                                line: 0,
                                column: 0,
                            });
                        }
                    }
                }
                accounts_doc.add_transaction(transaction).unwrap(); //TODO: unwrap
            }

            _ => {
                // `None` (end of file) or any other token (open or create transaction are covered by the match
                // branches above) is an error
                // (this is because we've parsed a date up to this point).
                return Err(ParseError {
                    msg: "expected either open or post transaction directive".to_string(),
                    line: 0,
                    column: 0,
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
    fn it_works() {
        let mut tokens = vec![];
        add_open_account_tokens(
            &mut tokens,
            "1912-01-12",
            AccountType::Asset,
            "account name",
            "GBP",
        );
        tokens.push(Ok(Token::Newline));
        add_open_account_tokens(
            &mut tokens,
            "1912-01-12",
            AccountType::Asset,
            "another account",
            "GBP",
        );
        tokens.push(Ok(Token::Newline));
        tokens.push(Ok(Token::Newline));
        add_tx_declaration_tokens(&mut tokens, "1912-01-12");
        tokens.push(Ok(Token::Newline));
        add_post_tokens(
            &mut tokens,
            "6.45".parse().expect("hard coded value will parse"),
            AccountType::Asset,
            "another account",
            "GBP",
        );
        tokens.push(Ok(Token::Newline));
        add_post_tokens(
            &mut tokens,
            "-6.45".parse().expect("hard coded value will parse"),
            AccountType::Asset,
            "account name",
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
                }
            ]
        )
    }

    fn add_open_account_tokens(
        tokens: &mut Vec<Result<Token, ParseError>>,
        date: impl Into<String>,
        account_type: AccountType,
        account_name: impl Into<String>,
        currency: impl Into<String>,
    ) {
        let mut open = vec![
            Ok(Token::Date(date.into())),
            Ok(Token::DirectiveOpen),
            Ok(Token::Account(crate::types::AccountId {
                type_: account_type,
                name: account_name.into(),
            })),
            Ok(Token::Currency(currency.into())),
        ];

        tokens.append(&mut open);
    }

    fn add_tx_declaration_tokens(
        tokens: &mut Vec<Result<Token, ParseError>>,
        date: impl Into<String>,
    ) {
        let mut tx_declare = vec![
            Ok(Token::Date(date.into())),
            Ok(Token::DirectivePostTx),
            Ok(Token::TxDescription),
        ];

        tokens.append(&mut tx_declare);
    }

    fn add_post_tokens(
        tokens: &mut Vec<Result<Token, ParseError>>,
        amount: Decimal,
        account_type: AccountType,
        account_name: impl Into<String>,
        currency: impl Into<String>,
    ) {
        let mut open = vec![
            Ok(Token::Account(crate::types::AccountId {
                type_: account_type,
                name: account_name.into(),
            })),
            Ok(Token::Amount(Amount {
                currency: currency.into(),
                amount,
            })),
        ];

        tokens.append(&mut open);
    }
}
