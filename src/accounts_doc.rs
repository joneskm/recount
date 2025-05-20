use date::Date;
use rust_decimal::Decimal;
use rust_decimal::dec;
use thiserror::Error;

use crate::types::{AccountId, Amount};

// Transactions can be out of balance by a maximum of this amount (this is inline with Beancount)
const TOLERANCE: Decimal = dec!(0.005);

/// Represents an account in the [`AccountsDocument`].
#[derive(Debug)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub struct Account {
    pub(crate) id: AccountId,
    pub(crate) currency: String,
    pub(crate) opening_date: Date,
}

/// An iterator over accounts and balances returned by [`AccountsDocument::balances`].
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

/// Represents a regular beancount posting, where the account currency and the transaction currency
/// are the same. Regular postings take the form:
/// ```beancount
/// Assets:BankChecking     1000.00 GBP
/// ```
#[derive(Debug)]
pub struct RegularPosting {
    pub(crate) account_id: AccountId,
    pub(crate) amount: Decimal,
    pub(crate) currency: String,
}

/// Represents a posting to an account with a different currency to the transaction currency.
/// Conversion postings take the form:
/// ```beancount
/// Assets:BankCheckingEUR 100 EUR @ 0.8 GBP
/// ```
#[derive(Debug)]
pub struct ConversionPosting {
    pub(crate) account_id: AccountId,
    pub(crate) account_amount: Decimal,
    pub(crate) account_currency: String,
    pub(crate) rate: Decimal,
    pub(crate) tx_currency: String,
}

/// Represents the three different types of posting.
#[derive(Debug)]
pub enum Posting {
    Auto(AccountId),
    Regular(RegularPosting),
    Conversion(ConversionPosting),
}

/// Posting information for [`Posting::Regular`] and [`Posting::Conversion`]
struct PostingInfo {
    account_currency: String,
    tx_amount: Decimal,
    tx_currency: String,
}

impl Posting {
    /// Returns the [`AccountId`] unless the posting is an auto-posting in which case [`None`]
    /// is returned.
    pub fn account_id(&self) -> &AccountId {
        match self {
            Posting::Auto(id) => id,
            Posting::Regular(posting) => &posting.account_id,
            Posting::Conversion(posting) => &posting.account_id,
        }
    }

    /// Returns the account amount unless the posting is an auto-posting in which case [`None`]
    /// is returned.
    pub fn account_amount(&self) -> Option<Decimal> {
        match self {
            Posting::Auto(_) => None,
            Posting::Regular(posting) => Some(posting.amount),
            Posting::Conversion(posting) => Some(posting.account_amount),
        }
    }

    /// Returns  [`Some`] containing a [`PostingInfo`] for [`Posting::Regular`] and
    /// [`Posting::Conversion`], Returns [`None`] for a [`Posting::Auto`].
    fn info(&self) -> Option<PostingInfo> {
        match self {
            Posting::Auto(_) => None,
            Posting::Regular(posting) => Some(PostingInfo {
                account_currency: posting.currency.clone(),
                tx_amount: posting.amount,
                tx_currency: posting.currency.clone(),
            }),
            Posting::Conversion(posting) => {
                let tx_amount = posting.account_amount * posting.rate;

                Some(PostingInfo {
                    account_currency: posting.account_currency.clone(),
                    tx_amount,
                    tx_currency: posting.tx_currency.clone(),
                })
            }
        }
    }
}

/// A [`Transaction`] is a collection of [`Posting`]s with some metadata. The following conditions
/// are guaranteed to be true for all [`Transaction`] instances:
/// 1. All regular postings have the same currency.
/// 2. All conversion postings have the same conversion currency and the same currency as regular
///    postings.
/// 3. There is at most one auto posting.
/// 4. The account id in regular postings corresponds to an open account, with the same currency.
/// 5. The account id in a conversion posting corresponds to an open account with the same (pre
///    conversion) currency.
/// 6. The account id in an auto-posting corresponds to an open account, If the transaction
///    contains other postings then the currency of the account will be the same as the currency of
///    all regular postings and the converted to currency of all conversion postings.
/// 7. For transactions with no auto-posting the sum of all amounts (converted amounts in the case
///    of conversion postings) will be zero i.e. the transaction will be balanced.
///
/// It isn't possible to create a [`Transaction`] directly however the
/// [`AccountsDocument::add_transaction`] method provides an indirect method of creating a
/// [`Transaction`]. This method guarantees that all the above requirements are satisfied before
/// creating and adding the transaction to the accounts document.
///
/// The order in which [`Posting`]s are passed to the [`AccountsDocument::add_transaction`] is
/// preserved. This is useful when writing an [`AccountsDocument`] to a file and a particular order
/// is required.
#[derive(Debug)]
pub struct Transaction {
    _date: Date,
    _description: String,
    balance: Decimal, // this can only be non zero if the transaction contains an auto-posting
    postings: Vec<Posting>, // use a vec to preserve the order
}

impl Transaction {
    /// For a given `account` returns the sum of all postings to that account in the account
    /// currency. If there's an auto-posting to `account` then the posting amount is given by the
    /// amount required to balance the transaction.
    pub fn balance(&self, account: &AccountId) -> Option<Decimal> {
        let filtered: Vec<&Posting> = self
            .postings
            .iter()
            .filter(|&p| p.account_id() == account)
            .collect();

        if filtered.is_empty() {
            return None;
        }

        Some(filtered.iter().fold(Decimal::ZERO, |s, p| {
            s + p.account_amount().unwrap_or_else(|| -self.balance) // NOTE: if there's an auto
            // posting it's account_currency is guaranteed to be the same as the
            // transaction_currency.
        }))
    }
}

/// This is an in memory representation of a beancount accounts document. The document will preserve
/// the order which transactions and postings appear when constructed from a file.
#[cfg_attr(test, derive(Debug))]
pub struct AccountsDocument {
    pub(crate) accounts: Vec<Account>, // use vector to preserve the read order
    transactions: Vec<Transaction>,
}

#[allow(clippy::new_without_default)] // `new` is more idiomatic than `default` for initializing an empty `AccountsDocument`
impl AccountsDocument {
    pub fn new() -> AccountsDocument {
        AccountsDocument {
            accounts: vec![],
            transactions: vec![],
        }
    }

    /// Add an [`Account`] to the document. Returns an [`OpenAccountError`] if an account with the
    /// same [`AccountId`] already exists.
    pub fn open_an_account(&mut self, account: Account) -> Result<(), OpenAccountError> {
        if self.accounts.iter().any(|a| a.id == account.id) {
            Err(OpenAccountError::AccountAlreadyExists)
        } else {
            self.accounts.push(account);
            Ok(())
        }
    }

    /// Adds a [`Transaction`] to the document if the [`Transaction`] defined by the arguments is
    /// valid.
    pub fn add_transaction(
        &mut self,
        date: Date,
        description: impl Into<String>,
        postings: Vec<Posting>,
    ) -> Result<(), AddTransactionError> {
        let mut running_total = Decimal::ZERO;
        let mut auto_posting: Option<(&Posting, &Account)> = None;
        let mut currency: Option<String> = None;
        for posting in &postings {
            let Some(account) = self.find_account(posting.account_id()) else {
                return Err(AddTransactionError::AccountNotFound);
            };
            if date < account.opening_date {
                return Err(AddTransactionError::AccountNotOpen);
            }

            if let Some(post_info) = posting.info() {
                // We now know this posting isn't an auto-posting
                if account.currency != post_info.account_currency {
                    return Err(AddTransactionError::IncorrectAccountCurrency);
                }

                match &mut currency {
                    Some(c) if c == &post_info.tx_currency => c,
                    Some(_) => return Err(AddTransactionError::IncorrectTransactionCurrency),
                    None => currency.insert(post_info.tx_currency),
                };

                running_total += post_info.tx_amount;
            } else {
                if auto_posting.is_some() {
                    return Err(AddTransactionError::MoreThanOneAutoPosting);
                }

                let _ = auto_posting.insert((posting, account));
            }
        }

        if let Some((_, account)) = auto_posting {
            // We have an auto-posting, we must check that the account posted to has the same
            // currency as the transaction currency. If there is no transaction currency (which can
            // happen if this is the only posting), then the transaction currency **is** the
            // posting currency so all is good.
            if currency.is_some_and(|c| c != account.currency) {
                return Err(AddTransactionError::IncorrectTransactionCurrency);
            }
        } else {
            // If there is no auto-posting then the tx must balance
            if running_total.abs() > TOLERANCE {
                return Err(AddTransactionError::NotBalanced);
            }
        };

        self.transactions.push(Transaction {
            _date: date,
            _description: description.into(),
            balance: running_total,
            postings,
        });

        Ok(())
    }

    fn find_account(&self, account_id: &AccountId) -> Option<&Account> {
        self.accounts.iter().find(|a| &a.id == account_id)
    }

    fn account_exists(&self, account: &AccountId) -> bool {
        self.accounts.iter().any(|a| &a.id == account)
    }

    /// Returns the balance of `account`it it exists otherwise returns [`None`].
    pub fn balance(&self, account: &AccountId) -> Option<Decimal> {
        if !self.account_exists(account) {
            return None;
        };

        Some(self.transactions.iter().fold(Decimal::ZERO, |s, t| {
            s + t.balance(account).unwrap_or(Decimal::ZERO)
        }))
    }

    /// Returns an [`AccountBalances`] iterator over all accounts.
    pub fn balances(&self) -> AccountBalances {
        AccountBalances {
            accounts_doc: self,
            position: 0,
        }
    }
}

/// The error returned by [`AccountsDocument::add_transaction`].
#[derive(Error, Debug, PartialEq)]
pub enum AddTransactionError {
    #[error("account not found")]
    AccountNotFound,
    #[error("account not open")]
    AccountNotOpen,
    #[error("the postings have different transaction currencies")]
    IncorrectTransactionCurrency,
    #[error("the account currency is incorect")]
    IncorrectAccountCurrency,
    #[error("transaction is not balanced")]
    NotBalanced,
    #[error("only one auto posting is allowed per transaction")]
    MoreThanOneAutoPosting,
}

/// The error returned by [`AccountsDocument::open_an_account`].
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

        assert_eq!(sum, Some(100.into()));
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
                    amount: 100.into(),
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
                    amount: (-50).into(),
                    currency: "USD".to_string(),
                }
            ))
        );

        assert_eq!(
            balances.next(),
            Some((
                &AccountId {
                    name: "AccountC".to_string(),
                    type_: AccountType::Income,
                },
                Amount {
                    amount: (0).into(),
                    currency: "GBP".to_string(),
                }
            ))
        );

        assert_eq!(
            balances.next(),
            Some((
                &AccountId {
                    name: "AccountD".to_string(),
                    type_: AccountType::Income,
                },
                Amount {
                    amount: (-50).into(),
                    currency: "GBP".to_string(),
                }
            ))
        );

        assert_eq!(balances.next(), None);
    }

    #[test]
    fn open_an_account_works() {
        accounts_doc()
            .open_an_account(Account {
                id: AccountId {
                    name: "AccountA".to_string(),
                    type_: AccountType::Asset,
                },
                currency: "GBP".to_string(),
                opening_date: date! {2012-01-04},
            })
            .expect("there is no acount with the same AccountId so this won't fail");

        let err = accounts_doc()
            .open_an_account(Account {
                id: AccountId {
                    name: "AccountA".to_string(),
                    type_: AccountType::Income,
                },
                currency: "GBP".to_string(),
                opening_date: date! {2012-01-04},
            })
            .unwrap_err();

        assert_eq!(err, OpenAccountError::AccountAlreadyExists)
    }

    #[test]
    fn add_transaction_works() {
        // We test two happy paths:
        // - One regular posting and one conversion posting. The tx is balanced so this should
        // work.
        // - Same as above (except it's not balanced) plus an auto-posting (which should "balance"
        // the tx).
        // Followed by a test for each error variant:
        // - AccountNotFound
        // - AccountNotOpen
        // - IncorrectTransactionCurrency
        // - IncorrectAccountCurrency
        // - NotBalanced
        // - MoreThanOneAutoPosting

        accounts_doc()
            .add_transaction(
                date! {2012-05-13},
                "Another Tx",
                vec![
                    Posting::Regular(RegularPosting {
                        account_id: AccountId {
                            name: "AccountA".to_string(),
                            type_: AccountType::Income,
                        },
                        amount: 100.into(),
                        currency: "GBP".to_string(),
                    }),
                    Posting::Conversion(ConversionPosting {
                        account_id: AccountId {
                            name: "AccountB".to_string(),
                            type_: AccountType::Income,
                        },
                        account_amount: (-100_i8).into(),
                        account_currency: "USD".to_string(),
                        tx_currency: "GBP".into(),
                        rate: 1.into(),
                    }),
                ],
            )
            .expect("won't return an error");

        let mut doc = accounts_doc();
        doc.add_transaction(
            date! {2012-05-13},
            "Another Tx",
            vec![
                Posting::Regular(RegularPosting {
                    account_id: AccountId {
                        name: "AccountA".to_string(),
                        type_: AccountType::Income,
                    },
                    amount: 100.into(),
                    currency: "GBP".to_string(),
                }),
                Posting::Conversion(ConversionPosting {
                    account_id: AccountId {
                        name: "AccountB".to_string(),
                        type_: AccountType::Income,
                    },
                    account_amount: (-50_i8).into(),
                    account_currency: "USD".to_string(),
                    tx_currency: "GBP".into(),
                    rate: 1.into(),
                }),
                Posting::Auto(AccountId {
                    name: "AccountD".to_string(),
                    type_: AccountType::Income,
                }),
            ],
        )
        .expect("won't return an error");
        assert_eq!(
            doc.balance(&AccountId {
                name: "AccountD".to_string(),
                type_: AccountType::Income,
            }),
            Some((-100).into()) // -100 because the balance was -50 before we added this tx
        );

        let err = accounts_doc()
            .add_transaction(
                date! {2012-05-13},
                "Another Tx",
                vec![
                    Posting::Regular(RegularPosting {
                        account_id: AccountId {
                            name: "AccountA".to_string(),
                            type_: AccountType::Income,
                        },
                        amount: 100.into(),
                        currency: "GBP".to_string(),
                    }),
                    Posting::Conversion(ConversionPosting {
                        account_id: AccountId {
                            name: "AccountB".to_string(),
                            type_: AccountType::Asset, // There is no account named "AccountB" with
                                                       // type Asset (there is an "AccountB" with type Income).
                        },
                        account_amount: (-100_i8).into(),
                        account_currency: "USD".to_string(),
                        tx_currency: "GBP".into(),
                        rate: 1.into(),
                    }),
                ],
            )
            .unwrap_err();
        assert_eq!(err, AddTransactionError::AccountNotFound);

        let err = accounts_doc()
            .add_transaction(
                date! {2012-04-11}, // this is before the accounts were opened
                "Another Tx",
                vec![
                    Posting::Regular(RegularPosting {
                        account_id: AccountId {
                            name: "AccountA".to_string(),
                            type_: AccountType::Income,
                        },
                        amount: 100.into(),
                        currency: "GBP".to_string(),
                    }),
                    Posting::Conversion(ConversionPosting {
                        account_id: AccountId {
                            name: "AccountB".to_string(),
                            type_: AccountType::Income,
                        },
                        account_amount: (-100_i8).into(),
                        account_currency: "USD".to_string(),
                        tx_currency: "GBP".into(),
                        rate: 1.into(),
                    }),
                ],
            )
            .unwrap_err();
        assert_eq!(err, AddTransactionError::AccountNotOpen);

        let err = accounts_doc()
            .add_transaction(
                date! {2012-05-13},
                "Another Tx",
                vec![
                    Posting::Regular(RegularPosting {
                        account_id: AccountId {
                            name: "AccountA".to_string(),
                            type_: AccountType::Income,
                        },
                        amount: 100.into(),
                        currency: "GBP".to_string(),
                    }),
                    Posting::Conversion(ConversionPosting {
                        account_id: AccountId {
                            name: "AccountB".to_string(),
                            type_: AccountType::Income,
                        },
                        account_amount: (-100_i8).into(),
                        account_currency: "USD".to_string(),
                        tx_currency: "EUR".into(),
                        rate: 1.into(),
                    }),
                ],
            )
            .unwrap_err();
        assert_eq!(err, AddTransactionError::IncorrectTransactionCurrency);

        let err = accounts_doc()
            .add_transaction(
                date! {2012-05-13},
                "Another Tx",
                vec![
                    Posting::Regular(RegularPosting {
                        account_id: AccountId {
                            name: "AccountA".to_string(),
                            type_: AccountType::Income,
                        },
                        amount: 100.into(),
                        currency: "GBP".to_string(),
                    }),
                    Posting::Regular(RegularPosting {
                        account_id: AccountId {
                            name: "AccountB".to_string(),
                            type_: AccountType::Income,
                        },
                        amount: (-100_i8).into(),
                        currency: "GBP".to_string(), // this doesn't match the account currency
                    }),
                ],
            )
            .unwrap_err();
        assert_eq!(err, AddTransactionError::IncorrectAccountCurrency);

        let err = accounts_doc()
            .add_transaction(
                date! {2012-05-13},
                "Another Tx",
                vec![
                    Posting::Regular(RegularPosting {
                        account_id: AccountId {
                            name: "AccountA".to_string(),
                            type_: AccountType::Income,
                        },
                        amount: 100.into(),
                        currency: "GBP".to_string(),
                    }),
                    Posting::Conversion(ConversionPosting {
                        account_id: AccountId {
                            name: "AccountB".to_string(),
                            type_: AccountType::Income,
                        },
                        account_amount: (-100_i8).into(),
                        account_currency: "USD".to_string(),
                        tx_currency: "GBP".into(),
                        rate: dec!(1.1),
                    }),
                ],
            )
            .unwrap_err();
        assert_eq!(err, AddTransactionError::NotBalanced);

        let err = accounts_doc()
            .add_transaction(
                date! {2012-05-13},
                "Another Tx",
                vec![
                    Posting::Regular(RegularPosting {
                        account_id: AccountId {
                            name: "AccountA".to_string(),
                            type_: AccountType::Income,
                        },
                        amount: 100.into(),
                        currency: "GBP".to_string(),
                    }),
                    Posting::Auto(AccountId {
                        name: "AccountC".to_string(),
                        type_: AccountType::Income,
                    }),
                    Posting::Auto(AccountId {
                        name: "AccountD".to_string(),
                        type_: AccountType::Income,
                    }),
                ],
            )
            .unwrap_err();
        assert_eq!(err, AddTransactionError::MoreThanOneAutoPosting);
    }

    #[test]
    fn auto_posting_incorrect_currency() {
        // An auto-posting account must have the same currency as the transaction currency

        let err = accounts_doc()
            .add_transaction(
                date! {2012-05-13},
                "Another Tx",
                vec![
                    Posting::Regular(RegularPosting {
                        account_id: AccountId {
                            name: "AccountA".to_string(),
                            type_: AccountType::Income,
                        },
                        amount: 100.into(),
                        currency: "GBP".to_string(),
                    }),
                    Posting::Auto(AccountId {
                        name: "AccountB".to_string(),
                        type_: AccountType::Income,
                    }),
                ],
            )
            .unwrap_err();
        assert_eq!(err, AddTransactionError::IncorrectTransactionCurrency);
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
                Account {
                    id: AccountId {
                        name: "AccountC".to_string(),
                        type_: AccountType::Income,
                    },
                    opening_date: date! {2012-04-12},
                    currency: "GBP".to_string(),
                },
                Account {
                    id: AccountId {
                        name: "AccountD".to_string(),
                        type_: AccountType::Income,
                    },
                    opening_date: date! {2012-04-12},
                    currency: "GBP".to_string(),
                },
            ],
            transactions: vec![Transaction {
                _date: date! {2012-04-21},
                _description: "transaction 1".to_string(),
                balance: (50).into(),
                postings: vec![
                    Posting::Regular(RegularPosting {
                        account_id: AccountId {
                            name: "AccountA".to_string(),
                            type_: AccountType::Income,
                        },
                        amount: 100.into(),
                        currency: "GBP".to_string(),
                    }),
                    Posting::Conversion(ConversionPosting {
                        account_id: AccountId {
                            name: "AccountB".to_string(),
                            type_: AccountType::Income,
                        },
                        account_amount: (-50).into(),
                        account_currency: "USD".to_string(),
                        rate: Decimal::ONE,
                        tx_currency: "GBP".to_string(),
                    }),
                    Posting::Auto(AccountId {
                        name: "AccountD".to_string(),
                        type_: AccountType::Income,
                    }),
                ],
            }],
        }
    }
}
