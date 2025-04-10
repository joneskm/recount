use date::Date;
use rust_decimal::Decimal;
use thiserror::Error;

#[derive(Debug, PartialEq, Eq)]
pub enum AccountType {
    Income,
    Expense,
    Assets,
    Equity,
    Liability,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Account {
    name: String,
    type_: AccountType,
    currency: String,
}

/// An iterator over accounts and balances
#[derive(Debug)]
pub struct AccountBalances<'a> {
    accounts_doc: &'a AccountsDocument,
    position: usize,
}

impl<'a> Iterator for AccountBalances<'a> {
    type Item = (&'a Account, Decimal);

    fn next(&mut self) -> Option<Self::Item> {
        let account = self.accounts_doc.accounts.get(self.position)?;
        let balance = self
            .accounts_doc
            .balance(account)
            .expect("we know this account exists");
        self.position += 1;
        Some((account, balance))
    }
}

#[derive(Debug)]
pub struct CurrencyConverter {
    _currency: String,
    _rate: Decimal,
}

#[derive(Debug)]
struct Posting {
    account: Account,
    amount: Decimal,
    _currency: String,
    _converter: Option<CurrencyConverter>,
}

#[derive(Debug)]
struct Transaction {
    _date: Date,
    _description: String,
    // use a vec to preserve the read order
    postings: Vec<Posting>,
}

#[derive(Debug)]
pub struct AccountsDocument {
    // we use vectors to preserve the read order
    accounts: Vec<Account>,
    transactions: Vec<Transaction>,
}

#[derive(Error, Debug, PartialEq)]
pub enum Error {
    #[error("account not found")]
    AccountNotFound,
}

impl AccountsDocument {
    pub fn balance(&self, account: &Account) -> Result<Decimal, Error> {
        if !self.accounts.iter().any(|a| a == account) {
            return Err(Error::AccountNotFound);
        };

        Ok(self.transactions.iter().fold(Decimal::ZERO, |s, t| {
            t.postings
                .iter()
                .filter(|&p| &p.account == account)
                .fold(s, |sp, p| sp + p.amount)
        }))
    }

    pub fn balances(&self) -> AccountBalances {
        AccountBalances {
            accounts_doc: self,
            position: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use date::date;

    #[test]
    fn balance_works() {
        let accounts = accounts_doc();
        let sum = accounts.balance(&Account {
            name: "AccountA".to_string(),
            type_: AccountType::Income,
            currency: "GBP".to_string(),
        });

        assert_eq!(sum, Ok(Decimal::ONE));
    }

    #[test]
    fn balancess_works() {
        let accounts = accounts_doc();
        let mut balances = accounts.balances();

        assert_eq!(
            balances.next(),
            Some((
                &Account {
                    name: "AccountA".to_string(),
                    type_: AccountType::Income,
                    currency: "GBP".to_string(),
                },
                Decimal::ONE
            ))
        );

        assert_eq!(
            balances.next(),
            Some((
                &Account {
                    name: "AccountB".to_string(),
                    type_: AccountType::Income,
                    currency: "USD".to_string(),
                },
                Decimal::ONE
            ))
        );

        assert_eq!(balances.next(), None);
    }

    fn accounts_doc() -> AccountsDocument {
        AccountsDocument {
            accounts: vec![
                Account {
                    name: "AccountA".to_string(),
                    type_: AccountType::Income,
                    currency: "GBP".to_string(),
                },
                Account {
                    name: "AccountB".to_string(),
                    type_: AccountType::Income,
                    currency: "USD".to_string(),
                },
            ],
            transactions: vec![Transaction {
                _date: date! { 2012-04-21 },
                _description: "transaction 1".to_string(),
                postings: vec![
                    Posting {
                        account: Account {
                            name: "AccountA".to_string(),
                            type_: AccountType::Income,
                            currency: "GBP".to_string(),
                        },
                        amount: Decimal::ONE,
                        _currency: "GBP".to_string(),
                        _converter: None,
                    },
                    Posting {
                        account: Account {
                            name: "AccountB".to_string(),
                            type_: AccountType::Income,
                            currency: "USD".to_string(),
                        },
                        amount: Decimal::ONE,
                        _currency: "USD".to_string(),
                        _converter: Some(CurrencyConverter {
                            _currency: "GBP".to_string(),
                            _rate: Decimal::ONE,
                        }),
                    },
                ],
            }],
        }
    }
}
