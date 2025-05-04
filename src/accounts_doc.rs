use date::Date;
use rust_decimal::Decimal;
use thiserror::Error;

use crate::types::{AccountId, Amount};

/// An account in the `AccountsDocument`.
#[derive(Debug)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub struct Account {
    pub(crate) id: AccountId,
    pub(crate) currency: String,
    pub(crate) opening_date: Date,
}

/// An iterator over accounts and balances
#[cfg_attr(test, derive(Debug))]
pub struct AccountBalances<'a> {
    accounts_doc: &'a AccountsDocument,
    position: usize,
}

impl<'a> Iterator for AccountBalances<'a> {
    type Item = (&'a AccountId, Amount);

    fn next(&mut self) -> Option<Self::Item> {
        let account = self.accounts_doc.accounts.get(self.position)?;
        let balance = self
            .accounts_doc
            .balance(&account.id)
            .expect("we know this account exists");
        self.position += 1;
        Some((
            &account.id,
            Amount {
                amount: balance,
                currency: account.currency.clone(),
            },
        ))
    }
}

/// Used by `Posting` to convert a posting's currency to a transaction's currency.
#[derive(Debug)]
pub struct CurrencyConverter {
    pub(crate) currency: String,
    pub(crate) rate: Decimal,
}

/// A transaction posting. The currency should match the currency of the account with id `account_id`
/// when calling `AccountsDocument::add_transaction`. A `converter` is required when the posting is
/// included in a transaction with a different currency to the posting's currency.
#[derive(Debug)]
pub struct Posting {
    pub(crate) account_id: AccountId,
    pub(crate) amount: Decimal,
    pub(crate) currency: String,
    pub(crate) converter: Option<CurrencyConverter>,
}

impl Posting {
    fn resolved_currency(&self) -> &String {
        self.converter
            .as_ref()
            .map_or(&self.currency, |c| &c.currency)
    }

    fn resolved_amount(&self) -> Decimal {
        self.converter
            .as_ref()
            .map_or(self.amount.clone(), |c| c.rate * self.amount)
    }
}

/// A transaction.
#[derive(Debug)]
pub struct Transaction {
    date: Date,
    _description: String,
    currency: Option<String>,
    postings: Vec<Posting>, // use a vec to preserve the read order
}

impl Transaction {
    pub fn new(date: Date, description: impl Into<String>) -> Transaction {
        Transaction {
            date,
            _description: description.into(),
            currency: None,
            postings: vec![],
        }
    }

    /// Adds a post to the transaction. If the transaction's currency is set then we check that the
    /// post resolves to the same currency (i.e. the posting either contains a conversion to the
    /// transaction currency or has no conversion but is already in the transaction currency). If
    /// the transaction's currency is not set (i.e. it has no postings) then we set it to the
    /// posting's resolved currency.
    pub fn add_posting(&mut self, posting: Posting) -> Result<(), AddPostingError> {
        let post_currency = posting.resolved_currency();
        match &self.currency {
            Some(c) => {
                if c == post_currency {
                    self.postings.push(posting);
                    Ok(())
                } else {
                    Err(AddPostingError::IncorrectCurrency)
                }
            }
            None => {
                self.currency = Some(post_currency.to_string());
                self.postings.push(posting);
                Ok(())
            }
        }
    }
}

/// The error returned by `Transaction::add_posting`.
#[derive(Error, Debug, PartialEq)]
pub enum AddPostingError {
    #[error("currency does not match")]
    IncorrectCurrency,
}

/// This is an in memory representation of a text accounts document. The document will preserve
/// the order which transactions and postings appear when constructed from a file.
#[cfg_attr(test, derive(Debug))]
pub struct AccountsDocument {
    pub(crate) accounts: Vec<Account>, // use vector to preserve the read order
    transactions: Vec<Transaction>,
}

impl AccountsDocument {
    pub fn new() -> AccountsDocument {
        AccountsDocument {
            accounts: vec![],
            transactions: vec![],
        }
    }

    /// Add an account to the document. Returns an error if an account with the same ID
    /// already exists.
    pub fn open_an_account(&mut self, account: Account) -> Result<(), OpenAccountError> {
        if self.accounts.iter().any(|a| a.id == account.id) {
            Err(OpenAccountError::AccountAlreadyExists)
        } else {
            self.accounts.push(account);
            Ok(())
        }
    }

    /// Add a transaction to the document. The transaction must:
    /// 1. Be balanced - the sum of all postings must equal zero.
    /// 2. Every account must be open and in the correct currency.
    /// If these conditions are not satisfied an error is returned.
    pub fn add_transaction(&mut self, transaction: Transaction) -> Result<(), AddTransactionError> {
        // TODO: what if an account is added later which is dated before a transaction (causing it
        // to fail)?

        let mut running_total = Decimal::ZERO;
        for posting in &transaction.postings {
            let Some(account) = self.find_account(&posting.account_id) else {
                return Err(AddTransactionError::AccountNotFound);
            };
            if transaction.date < account.opening_date {
                return Err(AddTransactionError::AccountNotOpen);
            }
            if account.currency != posting.currency {
                return Err(AddTransactionError::IncorrectCurrency);
            }
            running_total += posting.resolved_amount();
        }

        if running_total != Decimal::ZERO {
            return Err(AddTransactionError::NotBalanced);
        }

        self.transactions.push(transaction);

        Ok(())
    }

    fn find_account(&self, account_id: &AccountId) -> Option<&Account> {
        self.accounts.iter().find(|a| &a.id == account_id)
    }

    fn account_exists(&self, account: &AccountId) -> bool {
        self.accounts.iter().any(|a| &a.id == account)
    }

    /// Returns the balance of `account`. Returns `None` if the account doesn't exist.
    pub fn balance(&self, account: &AccountId) -> Option<Decimal> {
        if !self.account_exists(account) {
            return None;
        };

        Some(self.transactions.iter().fold(Decimal::ZERO, |s, t| {
            t.postings
                .iter()
                .filter(|&p| &p.account_id == account)
                .fold(s, |sp, p| sp + p.amount)
        }))
    }

    /// Returns an iterator over all accounts and balances.
    pub fn balances(&self) -> AccountBalances {
        AccountBalances {
            accounts_doc: self,
            position: 0,
        }
    }
}

/// The error returned by `AccountsDocument::add_transaction`.
#[derive(Error, Debug, PartialEq)]
pub enum AddTransactionError {
    #[error("account not found")]
    AccountNotFound,
    #[error("account not open")]
    AccountNotOpen,
    #[error("incorrect currency")]
    IncorrectCurrency,
    #[error("transaction is not balanced")]
    NotBalanced,
}

/// The error returned by `AccountsDocument::open_an_account`.
#[derive(Error, Debug, PartialEq)]
pub enum OpenAccountError {
    #[error("account already exists")]
    AccountAlreadyExists,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::AccountType;
    use date::date;
    use rust_decimal::dec;

    #[test]
    fn balance_works() {
        let accounts = accounts_doc();
        let sum = accounts.balance(&AccountId {
            name: "AccountA".to_string(),
            type_: AccountType::Income,
        });

        assert_eq!(sum, Some(Decimal::ONE));
    }

    #[test]
    fn balances_works() {
        let accounts = accounts_doc();
        let mut balances = accounts.balances();

        assert_eq!(
            balances.next(),
            Some((
                &AccountId {
                    name: "AccountA".to_string(),
                    type_: AccountType::Income,
                },
                Amount {
                    amount: Decimal::ONE,
                    currency: "GBP".to_string(),
                }
            ))
        );

        assert_eq!(
            balances.next(),
            Some((
                &AccountId {
                    name: "AccountB".to_string(),
                    type_: AccountType::Income,
                },
                Amount {
                    amount: Decimal::ONE,
                    currency: "USD".to_string(),
                }
            ))
        );

        assert_eq!(balances.next(), None);
    }

    #[test]
    fn add_posting_works() {
        // We create an transaction with no postings. Then add an initial posting. This can't fail
        // because the transaction has no currency. We then add four more postings covering the
        // four cases:
        // - correct currency
        // - incorrect currency
        // - correct resolved currency
        // - incorrect resolved currency
        let mut transaction = Transaction {
            date: date! {2012-05-13},
            _description: "a description".to_string(),
            currency: None,
            postings: vec![],
        };

        transaction
            .add_posting(Posting {
                account_id: AccountId {
                    name: "account 1".to_string(),
                    type_: AccountType::Asset,
                },
                amount: 100.into(),
                currency: "GBP".to_string(),
                converter: Some(CurrencyConverter {
                    currency: "USD".to_string(),
                    rate: dec!(1.5),
                }),
            })
            .expect("this can't fail beacuase the tx doesn't yet have a currency");

        assert_eq!(transaction.currency, Some("USD".to_string()));

        transaction
            .add_posting(Posting {
                account_id: AccountId {
                    name: "account 2".to_string(),
                    type_: AccountType::Asset,
                },
                amount: 50.into(),
                currency: "USD".to_string(),
                converter: None,
            })
            .expect("this is in the same currency as the tx so it won't fail");

        let err = transaction
            .add_posting(Posting {
                account_id: AccountId {
                    name: "account 3".to_string(),
                    type_: AccountType::Asset,
                },
                amount: 90.into(),
                currency: "GBP".to_string(),
                converter: None,
            })
            .unwrap_err();

        assert_eq!(err, AddPostingError::IncorrectCurrency);

        transaction
            .add_posting(Posting {
                account_id: AccountId {
                    name: "account 4".to_string(),
                    type_: AccountType::Asset,
                },
                amount: 10.into(),
                currency: "EUR".to_string(),
                converter: Some(CurrencyConverter {
                    currency: "USD".to_string(),
                    rate: dec!(1.7),
                }),
            })
            .expect("this can't fail beacuase the posting resolves to the same currency as the tx");

        let err = transaction
            .add_posting(Posting {
                account_id: AccountId {
                    name: "account 1".to_string(),
                    type_: AccountType::Asset,
                },
                amount: 100.into(),
                currency: "GBP".to_string(),
                converter: Some(CurrencyConverter {
                    currency: "EUR".to_string(),
                    rate: dec!(1.5),
                }),
            })
            .unwrap_err();

        assert_eq!(err, AddPostingError::IncorrectCurrency)
    }
    #[test]
    fn add_transaction_works() {
        // We test the happy path followed by a test for each error variant:
        // - AccountNotFound
        // - AccountNotOpen
        // - IncorrectCurrency
        // - NotBalanced
        accounts_doc()
            .add_transaction(Transaction {
                date: date! {2012-05-13},
                _description: "Another Tx".to_string(),
                currency: Some("GBP".to_string()),
                postings: vec![
                    Posting {
                        account_id: AccountId {
                            name: "AccountA".to_string(),
                            type_: AccountType::Income,
                        },
                        amount: 100.into(),
                        currency: "GBP".to_string(),
                        converter: None,
                    },
                    Posting {
                        account_id: AccountId {
                            name: "AccountB".to_string(),
                            type_: AccountType::Income,
                        },
                        amount: (-100_i8).into(),
                        currency: "USD".to_string(),
                        converter: Some(CurrencyConverter {
                            currency: "GBP".into(),
                            rate: 1.into(),
                        }),
                    },
                ],
            })
            .expect("won't return an error");

        let err = accounts_doc()
            .add_transaction(Transaction {
                date: date! {2012-05-13},
                _description: "Another Tx".to_string(),
                currency: Some("GBP".to_string()),
                postings: vec![
                    Posting {
                        account_id: AccountId {
                            name: "AccountA".to_string(),
                            type_: AccountType::Income,
                        },
                        amount: 100.into(),
                        currency: "GBP".to_string(),
                        converter: None,
                    },
                    Posting {
                        account_id: AccountId {
                            name: "AccountB".to_string(),
                            type_: AccountType::Asset, // There is no account named "AccountB" with
                                                       // type Asset (there is an "AccountB" with type Income).
                        },
                        amount: (-100_i8).into(),
                        currency: "USD".to_string(),
                        converter: Some(CurrencyConverter {
                            currency: "GBP".into(),
                            rate: 1.into(),
                        }),
                    },
                ],
            })
            .unwrap_err();
        assert_eq!(err, AddTransactionError::AccountNotFound);

        let err = accounts_doc()
            .add_transaction(Transaction {
                date: date! {2012-04-11}, // this is before the accounts were opened
                _description: "Another Tx".to_string(),
                currency: Some("GBP".to_string()),
                postings: vec![
                    Posting {
                        account_id: AccountId {
                            name: "AccountA".to_string(),
                            type_: AccountType::Income,
                        },
                        amount: 100.into(),
                        currency: "GBP".to_string(),
                        converter: None,
                    },
                    Posting {
                        account_id: AccountId {
                            name: "AccountB".to_string(),
                            type_: AccountType::Income,
                        },
                        amount: (-100_i8).into(),
                        currency: "USD".to_string(),
                        converter: Some(CurrencyConverter {
                            currency: "GBP".into(),
                            rate: 1.into(),
                        }),
                    },
                ],
            })
            .unwrap_err();
        assert_eq!(err, AddTransactionError::AccountNotOpen);

        let err = accounts_doc()
            .add_transaction(Transaction {
                date: date! {2012-05-13},
                _description: "Another Tx".to_string(),
                currency: Some("GBP".to_string()),
                postings: vec![
                    Posting {
                        account_id: AccountId {
                            name: "AccountA".to_string(),
                            type_: AccountType::Income,
                        },
                        amount: 100.into(),
                        currency: "GBP".to_string(),
                        converter: None,
                    },
                    Posting {
                        account_id: AccountId {
                            name: "AccountB".to_string(),
                            type_: AccountType::Income,
                        },
                        amount: (-100_i8).into(),
                        currency: "GBP".to_string(), // this doesn't match the account currency
                        converter: None,
                    },
                ],
            })
            .unwrap_err();
        assert_eq!(err, AddTransactionError::IncorrectCurrency);

        let err = accounts_doc()
            .add_transaction(Transaction {
                date: date! {2012-05-13},
                _description: "Another Tx".to_string(),
                currency: Some("GBP".to_string()),
                postings: vec![
                    Posting {
                        account_id: AccountId {
                            name: "AccountA".to_string(),
                            type_: AccountType::Income,
                        },
                        amount: 100.into(),
                        currency: "GBP".to_string(),
                        converter: None,
                    },
                    Posting {
                        account_id: AccountId {
                            name: "AccountB".to_string(),
                            type_: AccountType::Income,
                        },
                        amount: (-100_i8).into(),
                        currency: "USD".to_string(),
                        converter: Some(CurrencyConverter {
                            currency: "GBP".into(),
                            rate: dec!(1.1),
                        }),
                    },
                ],
            })
            .unwrap_err();
        assert_eq!(err, AddTransactionError::NotBalanced);
    }

    fn accounts_doc() -> AccountsDocument {
        AccountsDocument {
            accounts: vec![
                Account {
                    id: AccountId {
                        name: "AccountA".to_string(),
                        type_: AccountType::Income,
                    },
                    opening_date: date! {2012-04-12},
                    currency: "GBP".to_string(),
                },
                Account {
                    id: AccountId {
                        name: "AccountB".to_string(),
                        type_: AccountType::Income,
                    },
                    opening_date: date! {2012-04-12},
                    currency: "USD".to_string(),
                },
            ],
            transactions: vec![Transaction {
                date: date! {2012-04-21},
                _description: "transaction 1".to_string(),
                postings: vec![
                    Posting {
                        account_id: AccountId {
                            name: "AccountA".to_string(),
                            type_: AccountType::Income,
                        },
                        amount: Decimal::ONE,
                        currency: "GBP".to_string(),
                        converter: None,
                    },
                    Posting {
                        account_id: AccountId {
                            name: "AccountB".to_string(),
                            type_: AccountType::Income,
                        },
                        amount: Decimal::ONE,
                        currency: "USD".to_string(),
                        converter: Some(CurrencyConverter {
                            currency: "GBP".to_string(),
                            rate: Decimal::ONE,
                        }),
                    },
                ],
                currency: Some("GBP".to_string()),
            }],
        }
    }
}
